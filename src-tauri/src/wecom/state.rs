use crate::state::AppState;
use crate::wecom::types::{CachedToken, WecomApp};

impl AppState {
    pub async fn find_wecom_app(&self, name: &str) -> Option<WecomApp> {
        self.wecom_apps.read().await.iter().find(|a| a.name == name).cloned()
    }

    /// 默认应用：配置指定的那个；没有或找不到则用列表第一个。
    pub async fn default_wecom_app(&self) -> Option<WecomApp> {
        let want = self.config.read().await.default_wecom_app.clone();
        let apps = self.wecom_apps.read().await;
        want.and_then(|n| apps.iter().find(|a| a.name == n).cloned())
            .or_else(|| apps.first().cloned())
    }

    pub async fn wecom_app_exists(&self, corpid: &str, agentid: u32) -> bool {
        self.wecom_apps
            .read()
            .await
            .iter()
            .any(|a| a.corpid == corpid && a.agentid == agentid)
    }

    /// 唯一性：备注名精确唯一，(corpid, agentid) 组合唯一；`skip` 为修改时排除的自身名字。
    fn check_unique(apps: &[WecomApp], candidate: &WecomApp, skip: Option<&str>) -> Result<(), String> {
        for a in apps.iter().filter(|a| Some(a.name.as_str()) != skip) {
            if a.name == candidate.name {
                return Err(format!("备注名「{}」已存在", candidate.name));
            }
            if a.corpid == candidate.corpid && a.agentid == candidate.agentid {
                return Err(format!(
                    "corpid {} 的应用 {} 已添加（备注「{}」）",
                    candidate.corpid, candidate.agentid, a.name
                ));
            }
        }
        Ok(())
    }

    pub async fn add_wecom_app(&self, app: WecomApp) -> Result<(), String> {
        let mut apps = self.wecom_apps.write().await;
        Self::check_unique(&apps, &app, None)?;
        apps.push(app);
        self.store.save_wecom_apps(&apps).map_err(|e| e.to_string())
    }

    /// 用 `new` 整体替换名为 `name` 的应用；改名时同步默认配置。
    pub async fn replace_wecom_app(&self, name: &str, new: WecomApp) -> Result<(), String> {
        let new_name = new.name.clone();
        {
            let mut apps = self.wecom_apps.write().await;
            let Some(idx) = apps.iter().position(|a| a.name == name) else {
                return Err("应用不存在".to_string());
            };
            Self::check_unique(&apps, &new, Some(name))?;
            apps[idx] = new;
            self.store.save_wecom_apps(&apps).map_err(|e| e.to_string())?;
        }
        if new_name != name {
            let mut cfg = self.config.write().await;
            if cfg.default_wecom_app.as_deref() == Some(name) {
                cfg.default_wecom_app = Some(new_name);
                self.store.save_config(&cfg).map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    pub async fn remove_wecom_app(&self, name: &str) -> Result<(), String> {
        {
            let mut apps = self.wecom_apps.write().await;
            apps.retain(|a| a.name != name);
            self.store.save_wecom_apps(&apps).map_err(|e| e.to_string())?;
        }
        let mut cfg = self.config.write().await;
        if cfg.default_wecom_app.as_deref() == Some(name) {
            cfg.default_wecom_app = None;
            self.store.save_config(&cfg).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    async fn update_wecom_app(
        &self,
        name: &str,
        f: impl FnOnce(&mut WecomApp),
    ) -> Result<(), String> {
        let mut apps = self.wecom_apps.write().await;
        let Some(a) = apps.iter_mut().find(|a| a.name == name) else {
            return Err("应用不存在".to_string());
        };
        f(a);
        self.store.save_wecom_apps(&apps).map_err(|e| e.to_string())
    }

    pub async fn set_wecom_token(&self, name: &str, token: CachedToken) -> Result<(), String> {
        self.update_wecom_app(name, |a| a.token = Some(token)).await
    }

    /// 凭据无效：记录原因、清掉 token；账号记录保留等待用户修正。
    pub async fn mark_wecom_invalid(&self, name: &str, reason: &str) -> Result<(), String> {
        self.update_wecom_app(name, |a| {
            a.invalid = Some(reason.to_string());
            a.token = None;
        })
        .await
    }

    pub async fn set_default_wecom_app(&self, name: &str) -> Result<(), String> {
        if self.find_wecom_app(name).await.is_none() {
            return Err("应用不存在".to_string());
        }
        let mut cfg = self.config.write().await;
        cfg.default_wecom_app = Some(name.to_string());
        self.store.save_config(&cfg).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    pub fn app(name: &str, corpid: &str, agentid: u32) -> WecomApp {
        WecomApp {
            name: name.into(),
            corpid: corpid.into(),
            agentid,
            secret: format!("sec-{name}"),
            token: None,
            invalid: None,
        }
    }

    fn state() -> (tempfile::TempDir, std::sync::Arc<AppState>) {
        let d = tempfile::tempdir().unwrap();
        let s = AppState::new(Store::new(d.path().to_path_buf()));
        (d, s)
    }

    #[tokio::test]
    async fn add_enforces_unique_name_and_corpid_agentid() {
        let (_d, s) = state();
        s.add_wecom_app(app("a", "ww1", 1)).await.unwrap();
        assert!(s.add_wecom_app(app("a", "ww2", 2)).await.unwrap_err().contains("备注"));
        assert!(s.add_wecom_app(app("b", "ww1", 1)).await.unwrap_err().contains("已添加"));
        s.add_wecom_app(app("b", "ww1", 2)).await.unwrap();
        assert_eq!(s.wecom_apps.read().await.len(), 2);
        assert_eq!(s.store.load_wecom_apps().len(), 2, "应已落盘");
        assert!(s.wecom_app_exists("ww1", 1).await);
        assert!(!s.wecom_app_exists("ww1", 3).await);
    }

    #[tokio::test]
    async fn default_prefers_config_then_first_and_remove_clears_default() {
        let (_d, s) = state();
        assert!(s.default_wecom_app().await.is_none());
        s.add_wecom_app(app("a", "ww1", 1)).await.unwrap();
        s.add_wecom_app(app("b", "ww1", 2)).await.unwrap();
        assert_eq!(s.default_wecom_app().await.unwrap().name, "a");
        s.set_default_wecom_app("b").await.unwrap();
        assert_eq!(s.default_wecom_app().await.unwrap().name, "b");
        assert!(s.set_default_wecom_app("zz").await.is_err());
        s.remove_wecom_app("b").await.unwrap();
        assert!(s.config.read().await.default_wecom_app.is_none());
        assert_eq!(s.default_wecom_app().await.unwrap().name, "a");
        assert!(s.find_wecom_app("b").await.is_none());
    }

    #[tokio::test]
    async fn replace_renames_syncs_default_and_checks_uniqueness_excluding_self() {
        let (_d, s) = state();
        s.add_wecom_app(app("a", "ww1", 1)).await.unwrap();
        s.add_wecom_app(app("b", "ww1", 2)).await.unwrap();
        s.set_default_wecom_app("a").await.unwrap();

        // 同名同凭据替换自身：不算冲突
        let mut same = app("a", "ww1", 1);
        same.secret = "new".into();
        s.replace_wecom_app("a", same).await.unwrap();
        assert_eq!(s.find_wecom_app("a").await.unwrap().secret, "new");

        // 改名：默认配置跟着改
        s.replace_wecom_app("a", app("ops", "ww1", 1)).await.unwrap();
        assert!(s.find_wecom_app("a").await.is_none());
        assert_eq!(s.config.read().await.default_wecom_app.as_deref(), Some("ops"));

        // 与别的应用冲突
        assert!(s.replace_wecom_app("ops", app("b", "ww1", 1)).await.is_err());
        assert!(s.replace_wecom_app("ops", app("ops", "ww1", 2)).await.is_err());
        assert!(s.replace_wecom_app("nope", app("x", "ww9", 9)).await.unwrap_err().contains("不存在"));
    }

    #[tokio::test]
    async fn token_and_invalid_are_persisted() {
        let (_d, s) = state();
        s.add_wecom_app(app("a", "ww1", 1)).await.unwrap();
        s.set_wecom_token("a", CachedToken { access_token: "t".into(), expires_at: 99 }).await.unwrap();
        assert_eq!(s.store.load_wecom_apps()[0].token.as_ref().unwrap().access_token, "t");
        s.mark_wecom_invalid("a", "secret 不合法（40001）").await.unwrap();
        let a = s.find_wecom_app("a").await.unwrap();
        assert_eq!(a.invalid.as_deref(), Some("secret 不合法（40001）"));
        assert!(a.token.is_none(), "标失效时应清掉 token");
        assert!(s.set_wecom_token("zz", CachedToken { access_token: "t".into(), expires_at: 1 }).await.is_err());
    }
}
