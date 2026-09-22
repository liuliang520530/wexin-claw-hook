use std::collections::HashMap;
use std::io;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use tokio::sync::{Mutex, RwLock};

use crate::ilink::client::http_client;
use crate::ilink::types::Credentials;
use crate::server::ServerHandle;
use crate::store::{Account, Config, Store};
use crate::wecom::api::WecomApi;
use crate::wecom::types::WecomApp;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum LoginStatus {
    Wait,
    Scanned,
    /// 扫码确认：refreshed = 该 user_id 已存在，仅刷新了凭据。
    Confirmed { user_id: String, refreshed: bool },
    Expired,
    Error { message: String },
}

pub struct LoginSession {
    pub qrcode: String,
    pub svg: String,
    pub status: LoginStatus,
}

/// 锁顺序：accounts -> config -> wecom_apps，且企微方法内不嵌套持锁；wecom_refresh 内只再拿 wecom_apps。
pub struct AppState {
    pub store: Store,
    pub http: reqwest::Client,
    pub accounts: RwLock<Vec<Account>>,
    pub config: RwLock<Config>,
    pub login: RwLock<Option<LoginSession>>,
    pub server: Mutex<Option<ServerHandle>>,
    /// 各来源 IP 最近一次记录鉴权失败日志的时间，用于限流（None 表示来源未知）。
    pub auth_fail_log: Mutex<HashMap<Option<IpAddr>, Instant>>,
    pub wecom_api: WecomApi,
    pub wecom_apps: RwLock<Vec<WecomApp>>,
    /// 刷新 access_token 的全局锁；刷新两小时才一次，不按应用分锁。
    pub wecom_refresh: Mutex<()>,
}

impl AppState {
    pub fn new(store: Store) -> Arc<Self> {
        Self::build(store, crate::wecom::BASE_URL, http_client())
    }

    /// 测试用：企业微信 API 指向 mock 服务，并强制直连（测试机可能配置了系统代理，
    /// 走代理访问 127.0.0.1 的 wiremock 会偶发连接失败）。
    #[cfg(test)]
    pub fn new_with_wecom_base(store: Store, wecom_base_url: &str) -> Arc<Self> {
        let http = reqwest::Client::builder()
            .timeout(crate::ilink::client::SEND_TIMEOUT)
            .no_proxy()
            .build()
            .expect("reqwest client");
        Self::build(store, wecom_base_url, http)
    }

    fn build(store: Store, wecom_base_url: &str, http: reqwest::Client) -> Arc<Self> {
        let accounts = store.load_accounts();
        let config = store.load_config();
        let wecom_apps = store.load_wecom_apps();
        Arc::new(Self {
            store,
            wecom_api: WecomApi::new(http.clone(), wecom_base_url),
            http,
            accounts: RwLock::new(accounts),
            config: RwLock::new(config),
            login: RwLock::new(None),
            server: Mutex::new(None),
            auth_fail_log: Mutex::new(HashMap::new()),
            wecom_apps: RwLock::new(wecom_apps),
            wecom_refresh: Mutex::new(()),
        })
    }

    /// 扫码确认：同一 user_id 已存在则刷新凭据并清除失效标记（返回 true），否则新增账号（返回 false）。
    pub async fn upsert_account(&self, creds: Credentials) -> io::Result<bool> {
        let mut accounts = self.accounts.write().await;
        let refreshed = match accounts.iter_mut().find(|a| a.creds.ilink_user_id == creds.ilink_user_id) {
            Some(a) => {
                a.creds = creds;
                a.token_expired = false;
                true
            }
            None => {
                let name = format!("微信账号 {}", accounts.len() + 1);
                accounts.push(Account { name, creds, token_expired: false });
                false
            }
        };
        self.store.save_accounts(&accounts)?;
        Ok(refreshed)
    }

    /// 发送遇到 ret=-14：标记该账号失效，保留记录等待重新扫码。
    pub async fn mark_expired(&self, user_id: &str) -> io::Result<()> {
        let mut accounts = self.accounts.write().await;
        if let Some(a) = accounts.iter_mut().find(|a| a.user_id() == user_id) {
            a.token_expired = true;
            self.store.save_accounts(&accounts)?;
        }
        Ok(())
    }

    /// 返回 false 表示账号不存在。
    pub async fn rename_account(&self, user_id: &str, name: &str) -> io::Result<bool> {
        let mut accounts = self.accounts.write().await;
        let Some(a) = accounts.iter_mut().find(|a| a.user_id() == user_id) else {
            return Ok(false);
        };
        a.name = name.to_string();
        self.store.save_accounts(&accounts)?;
        Ok(true)
    }

    pub async fn remove_account(&self, user_id: &str) -> io::Result<()> {
        {
            let mut accounts = self.accounts.write().await;
            accounts.retain(|a| a.user_id() != user_id);
            self.store.save_accounts(&accounts)?;
        }
        let mut cfg = self.config.write().await;
        if cfg.default_account.as_deref() == Some(user_id) {
            cfg.default_account = None;
            self.store.save_config(&cfg)?;
        }
        Ok(())
    }

    pub async fn find_account(&self, user_id: &str) -> Option<Account> {
        self.accounts.read().await.iter().find(|a| a.user_id() == user_id).cloned()
    }

    /// 默认账号：配置指定的那个；没有或找不到则用列表第一个。
    pub async fn default_account(&self) -> Option<Account> {
        let accounts = self.accounts.read().await;
        let want = self.config.read().await.default_account.clone();
        want.and_then(|u| accounts.iter().find(|a| a.user_id() == u).cloned())
            .or_else(|| accounts.first().cloned())
    }

    /// 至少有一个凭据未失效的账号。
    pub async fn is_logged_in(&self) -> bool {
        self.accounts.read().await.iter().any(|a| !a.token_expired)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn creds(user: &str) -> Credentials {
        Credentials {
            bot_token: format!("t-{user}"),
            ilink_bot_id: "b@im.bot".into(),
            ilink_user_id: user.into(),
            baseurl: "https://x".into(),
        }
    }

    #[tokio::test]
    async fn upsert_dedupes_by_user_id_and_clears_expired() {
        let d = tempfile::tempdir().unwrap();
        let s = AppState::new(Store::new(d.path().to_path_buf()));
        assert!(!s.is_logged_in().await);
        assert!(!s.upsert_account(creds("a@im.wechat")).await.unwrap(), "首次为新增");
        assert!(!s.upsert_account(creds("b@im.wechat")).await.unwrap());
        s.mark_expired("a@im.wechat").await.unwrap();
        assert!(s.find_account("a@im.wechat").await.unwrap().token_expired);

        let mut fresh = creds("a@im.wechat");
        fresh.bot_token = "t-new".into();
        assert!(s.upsert_account(fresh).await.unwrap(), "同 user_id 应为刷新");
        let a = s.find_account("a@im.wechat").await.unwrap();
        assert_eq!(a.creds.bot_token, "t-new");
        assert!(!a.token_expired);
        assert_eq!(s.accounts.read().await.len(), 2);
        assert_eq!(s.store.load_accounts().len(), 2, "应已落盘");
    }

    #[tokio::test]
    async fn default_account_prefers_config_then_first_and_remove_clears_default() {
        let d = tempfile::tempdir().unwrap();
        let s = AppState::new(Store::new(d.path().to_path_buf()));
        assert!(s.default_account().await.is_none());
        s.upsert_account(creds("a@im.wechat")).await.unwrap();
        s.upsert_account(creds("b@im.wechat")).await.unwrap();
        assert_eq!(s.default_account().await.unwrap().user_id(), "a@im.wechat");

        s.config.write().await.default_account = Some("b@im.wechat".into());
        assert_eq!(s.default_account().await.unwrap().user_id(), "b@im.wechat");

        s.remove_account("b@im.wechat").await.unwrap();
        assert!(s.config.read().await.default_account.is_none(), "删除默认账号应清空配置");
        assert_eq!(s.default_account().await.unwrap().user_id(), "a@im.wechat");
        assert!(s.rename_account("a@im.wechat", "老板").await.unwrap());
        assert!(!s.rename_account("zz@im.wechat", "x").await.unwrap());
        assert_eq!(s.find_account("a@im.wechat").await.unwrap().name, "老板");
    }
}
