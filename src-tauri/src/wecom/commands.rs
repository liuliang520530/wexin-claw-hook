use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::state::AppState;
use crate::wecom::api::{AgentInfo, ApiError};
use crate::wecom::sender::{send_text, WecomSendOk};
use crate::wecom::token::now_secs;
use crate::wecom::types::{describe_error, normalize_to, CachedToken, WecomApp};

#[derive(Debug, Clone, Serialize)]
pub struct WecomAppView {
    pub name: String,
    pub corpid: String,
    pub agentid: u32,
    pub invalid: Option<String>,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct WecomAddResult {
    pub apps: Vec<WecomAppView>,
    /// 应用已停用等非致命提示
    pub notice: Option<String>,
}

pub async fn wecom_views(state: &AppState) -> Vec<WecomAppView> {
    let default = state.default_wecom_app().await.map(|a| a.name);
    state
        .wecom_apps
        .read()
        .await
        .iter()
        .map(|a| WecomAppView {
            name: a.name.clone(),
            corpid: a.corpid.clone(),
            agentid: a.agentid,
            invalid: a.invalid.clone(),
            is_default: default.as_deref() == Some(a.name.as_str()),
        })
        .collect()
}

fn verify_error(e: ApiError, agentid: u32) -> String {
    match e {
        ApiError::Code { code, msg } => describe_error(code, &msg, agentid),
        ApiError::Http(m) => format!("无法连接企业微信：{m}"),
    }
}

/// gettoken + agent/get：三项凭据任一填错都在这里报出具体错误码。成功返回可直接缓存的 token 与应用信息。
pub async fn verify_app(
    state: &AppState,
    corpid: &str,
    agentid: u32,
    secret: &str,
) -> Result<(CachedToken, AgentInfo), String> {
    let now = now_secs();
    let tok = state
        .wecom_api
        .get_token(corpid, secret)
        .await
        .map_err(|e| verify_error(e, agentid))?;
    let agent = state
        .wecom_api
        .get_agent(&tok.access_token, agentid)
        .await
        .map_err(|e| verify_error(e, agentid))?;
    Ok((CachedToken { access_token: tok.access_token, expires_at: now + tok.expires_in }, agent))
}

fn closed_notice(agent: &AgentInfo) -> Option<String> {
    agent.close.then(|| "应用已停用，发送会失败；请到企业微信管理后台启用".to_string())
}

fn non_empty(s: Option<String>) -> Option<String> {
    s.map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

#[tauri::command]
pub async fn wecom_list_apps(state: State<'_, Arc<AppState>>) -> Result<Vec<WecomAppView>, String> {
    Ok(wecom_views(&state).await)
}

#[tauri::command]
pub async fn wecom_add_app(
    state: State<'_, Arc<AppState>>,
    name: Option<String>,
    corpid: String,
    agentid: u32,
    secret: String,
) -> Result<WecomAddResult, String> {
    let corpid = corpid.trim().to_string();
    let secret = secret.trim().to_string();
    if corpid.is_empty() {
        return Err("corpid 不能为空".into());
    }
    if agentid == 0 {
        return Err("agentid 必须是正整数".into());
    }
    if secret.is_empty() {
        return Err("secret 不能为空".into());
    }
    if state.wecom_app_exists(&corpid, agentid).await {
        return Err(format!("corpid {corpid} 的应用 {agentid} 已添加"));
    }
    let (token, agent) = verify_app(&state, &corpid, agentid, &secret).await?;
    let name = match non_empty(name) {
        Some(n) => n,
        None => {
            let n = agent.name.trim().to_string();
            if n.is_empty() {
                return Err("无法获取应用名称，请手动填写备注".into());
            }
            n
        }
    };
    state
        .add_wecom_app(WecomApp { name, corpid, agentid, secret, token: Some(token), invalid: None })
        .await?;
    Ok(WecomAddResult { apps: wecom_views(&state).await, notice: closed_notice(&agent) })
}

#[tauri::command]
pub async fn wecom_update_app(
    state: State<'_, Arc<AppState>>,
    name: String,
    new_name: Option<String>,
    corpid: Option<String>,
    agentid: Option<u32>,
    secret: Option<String>,
) -> Result<WecomAddResult, String> {
    let Some(existing) = state.find_wecom_app(&name).await else {
        return Err("应用不存在".into());
    };
    let mut app = existing.clone();
    if let Some(n) = non_empty(new_name) {
        app.name = n;
    }
    if let Some(c) = non_empty(corpid) {
        app.corpid = c;
    }
    if let Some(a) = agentid.filter(|a| *a != 0) {
        app.agentid = a;
    }
    if let Some(s) = non_empty(secret) {
        app.secret = s;
    }
    let creds_changed =
        app.corpid != existing.corpid || app.agentid != existing.agentid || app.secret != existing.secret;
    let mut notice = None;
    if creds_changed {
        let (token, agent) = verify_app(&state, &app.corpid, app.agentid, &app.secret).await?;
        app.token = Some(token);
        app.invalid = None;
        notice = closed_notice(&agent);
    }
    state.replace_wecom_app(&name, app).await?;
    Ok(WecomAddResult { apps: wecom_views(&state).await, notice })
}

#[tauri::command]
pub async fn wecom_remove_app(state: State<'_, Arc<AppState>>, name: String) -> Result<Vec<WecomAppView>, String> {
    state.remove_wecom_app(&name).await?;
    Ok(wecom_views(&state).await)
}

#[tauri::command]
pub async fn wecom_set_default(state: State<'_, Arc<AppState>>, name: String) -> Result<Vec<WecomAppView>, String> {
    state.set_default_wecom_app(&name).await?;
    Ok(wecom_views(&state).await)
}

/// 发消息页：与 webhook 同一发送入口与日志；`to` 走同一份 normalize_to。
#[tauri::command]
pub async fn wecom_send_test(
    state: State<'_, Arc<AppState>>,
    name: String,
    to: String,
    text: String,
) -> Result<WecomSendOk, String> {
    let to = normalize_to(Some(&to))?;
    let text = text.trim();
    if text.is_empty() {
        return Err("消息内容不能为空".into());
    }
    send_text(&state, Some(name.trim()), &to, text)
        .await
        .map_err(|f| f.message())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn ok_json(v: serde_json::Value) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(v)
    }

    async fn state(base: &str) -> (tempfile::TempDir, Arc<AppState>) {
        let d = tempfile::tempdir().unwrap();
        let s = AppState::new_with_wecom_base(Store::new(d.path().to_path_buf()), base);
        (d, s)
    }

    #[tokio::test]
    async fn verify_app_returns_token_and_agent() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/cgi-bin/gettoken"))
            .and(query_param("corpid", "ww1"))
            .and(query_param("corpsecret", "sec"))
            .respond_with(ok_json(serde_json::json!({"errcode": 0, "access_token": "tok", "expires_in": 7200})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/cgi-bin/agent/get"))
            .and(query_param("access_token", "tok"))
            .and(query_param("agentid", "1000002"))
            .respond_with(ok_json(serde_json::json!({"errcode": 0, "agentid": 1000002, "name": "运维告警", "close": 0})))
            .expect(1)
            .mount(&server)
            .await;
        let (_d, s) = state(&server.uri()).await;
        let (tok, agent) = verify_app(&s, "ww1", 1000002, "sec").await.unwrap();
        assert_eq!(tok.access_token, "tok");
        assert!(tok.expires_at > now_secs() + 7000);
        assert_eq!(agent.name, "运维告警");
        assert!(!agent.close);
    }

    #[tokio::test]
    async fn verify_app_reports_corpid_secret_and_agentid_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/cgi-bin/gettoken"))
            .respond_with(ok_json(serde_json::json!({"errcode": 40013, "errmsg": "invalid corpid"})))
            .mount(&server)
            .await;
        let (_d, s) = state(&server.uri()).await;
        let e = verify_app(&s, "bad", 1, "sec").await.unwrap_err();
        assert!(e.contains("corpid") && e.contains("40013"), "{e}");

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/cgi-bin/gettoken"))
            .respond_with(ok_json(serde_json::json!({"errcode": 0, "access_token": "tok", "expires_in": 7200})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/cgi-bin/agent/get"))
            .respond_with(ok_json(serde_json::json!({"errcode": 40056, "errmsg": "invalid agentid"})))
            .mount(&server)
            .await;
        let (_d, s) = state(&server.uri()).await;
        let e = verify_app(&s, "ww1", 9, "sec").await.unwrap_err();
        assert!(e.contains("agentid") && e.contains("40056"), "{e}");

        let (_d, s) = state("http://127.0.0.1:1").await;
        let e = verify_app(&s, "ww1", 1, "TopSecret").await.unwrap_err();
        assert!(e.contains("无法连接") && !e.contains("TopSecret"), "{e}");
    }

    #[tokio::test]
    async fn views_hide_secret_and_mark_default() {
        let (_d, s) = state("http://127.0.0.1:1").await;
        s.add_wecom_app(WecomApp {
            name: "a".into(),
            corpid: "ww1".into(),
            agentid: 1,
            secret: "hidden".into(),
            token: None,
            invalid: Some("x".into()),
        })
        .await
        .unwrap();
        let v = wecom_views(&s).await;
        assert_eq!(v.len(), 1);
        assert!(v[0].is_default);
        assert_eq!(v[0].invalid.as_deref(), Some("x"));
        assert!(!serde_json::to_string(&v).unwrap().contains("hidden"));
    }
}
