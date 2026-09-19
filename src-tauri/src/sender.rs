use crate::ilink::client::{IlinkClient, SendError};
use crate::state::AppState;
use crate::store::LogEntry;

#[derive(Debug, Clone, PartialEq)]
pub enum SendFailure {
    NotLoggedIn,
    TokenExpired,
    RateLimited,
    Upstream(String),
}

impl SendFailure {
    pub fn code(&self) -> &'static str {
        match self {
            SendFailure::NotLoggedIn => "not_logged_in",
            SendFailure::TokenExpired => "token_expired",
            SendFailure::RateLimited => "rate_limited",
            SendFailure::Upstream(_) => "upstream_error",
        }
    }

    pub fn message(&self) -> String {
        match self {
            SendFailure::NotLoggedIn => "尚未扫码登录".to_string(),
            SendFailure::TokenExpired => "登录已失效，请重新扫码".to_string(),
            SendFailure::RateLimited => "微信侧拒绝发送（ret=-2）：若是首次给该用户推送，需对方先在微信里给机器人发一条消息建立会话；否则为频率限制（约 7 条/5 分钟），稍后重试".to_string(),
            SendFailure::Upstream(m) => m.clone(),
        }
    }
}

/// webhook 与 UI 共用的发送入口：解析收件人 -> 调 iLink -> 写日志 -> -14 时清凭据。
pub async fn send_text(
    state: &AppState,
    to: Option<&str>,
    text: &str,
) -> Result<String, SendFailure> {
    let creds = state.creds.read().await.clone();
    let Some(creds) = creds else {
        let _ = state.store.append_log(LogEntry::now(
            false,
            Some(SendFailure::NotLoggedIn.code()),
            to.unwrap_or(""),
            text,
        ));
        return Err(SendFailure::NotLoggedIn);
    };

    let to = match to.map(str::trim).filter(|s| !s.is_empty()) {
        Some(t) => t.to_string(),
        None => state
            .config
            .read()
            .await
            .default_recipient
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| creds.ilink_user_id.clone()),
    };

    let client = IlinkClient::new(state.http.clone(), creds);
    let outcome = match client.send_text(&to, text, None).await {
        Ok(_) => Ok(to.clone()),
        Err(SendError::RateLimited) => Err(SendFailure::RateLimited),
        Err(SendError::TokenExpired) => Err(SendFailure::TokenExpired),
        Err(SendError::Upstream(m)) => Err(SendFailure::Upstream(m)),
    };

    let _ = state.store.append_log(LogEntry::now(
        outcome.is_ok(),
        outcome.as_ref().err().map(SendFailure::code),
        &to,
        text,
    ));

    if matches!(outcome, Err(SendFailure::TokenExpired)) {
        let _ = state.set_credentials(None).await;
        *state.token_expired.write().await = true;
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ilink::types::Credentials;
    use crate::store::Store;
    use std::sync::Arc;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn logged_in_state(base: &str) -> (tempfile::TempDir, Arc<AppState>) {
        let d = tempfile::tempdir().unwrap();
        let state = AppState::new(Store::new(d.path().to_path_buf()));
        state
            .set_credentials(Some(Credentials {
                bot_token: "t".into(),
                ilink_bot_id: "bot@im.bot".into(),
                ilink_user_id: "me@im.wechat".into(),
                baseurl: base.into(),
            }))
            .await
            .unwrap();
        (d, state)
    }

    /// ret=0 基础模板；先 and 匹配器、expect 次数，最后再 respond_with。
    fn ok_mock() -> wiremock::MockBuilder {
        Mock::given(method("POST")).and(path("/ilink/bot/sendmessage"))
    }

    #[tokio::test]
    async fn not_logged_in_fails_and_logs() {
        let d = tempfile::tempdir().unwrap();
        let state = AppState::new(Store::new(d.path().to_path_buf()));
        assert_eq!(send_text(&state, None, "hi").await, Err(SendFailure::NotLoggedIn));
        let logs = state.store.load_logs();
        assert_eq!(logs[0].code.as_deref(), Some("not_logged_in"));
        assert!(!logs[0].ok);
    }

    #[tokio::test]
    async fn defaults_to_login_user_when_no_to_and_no_default() {
        let server = MockServer::start().await;
        ok_mock()
            .and(body_partial_json(serde_json::json!({"msg": {"to_user_id": "me@im.wechat"}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ret": 0})))
            .expect(1)
            .mount(&server)
            .await;
        let (_d, state) = logged_in_state(&server.uri()).await;
        assert_eq!(send_text(&state, None, "hi").await.unwrap(), "me@im.wechat");
        let logs = state.store.load_logs();
        assert!(logs[0].ok);
        assert_eq!(logs[0].to, "me@im.wechat");
        assert_eq!(logs[0].text, "hi");
    }

    #[tokio::test]
    async fn uses_config_default_then_explicit_to() {
        let server = MockServer::start().await;
        ok_mock()
            .and(body_partial_json(serde_json::json!({"msg": {"to_user_id": "cfg@im.wechat"}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ret": 0})))
            .expect(2)
            .mount(&server)
            .await;
        ok_mock()
            .and(body_partial_json(serde_json::json!({"msg": {"to_user_id": "x@im.wechat"}})))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ret": 0})))
            .expect(1)
            .mount(&server)
            .await;
        let (_d, state) = logged_in_state(&server.uri()).await;
        state.config.write().await.default_recipient = Some("cfg@im.wechat".into());
        assert_eq!(send_text(&state, None, "a").await.unwrap(), "cfg@im.wechat");
        assert_eq!(send_text(&state, Some("x@im.wechat"), "b").await.unwrap(), "x@im.wechat");
        assert_eq!(send_text(&state, Some("  "), "c").await.unwrap(), "cfg@im.wechat");
    }

    #[tokio::test]
    async fn token_expired_clears_credentials() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ret": -14})))
            .mount(&server)
            .await;
        let (_d, state) = logged_in_state(&server.uri()).await;
        assert_eq!(send_text(&state, None, "hi").await, Err(SendFailure::TokenExpired));
        assert!(!state.is_logged_in().await);
        assert!(state.store.load_credentials().is_none());
        assert!(*state.token_expired.read().await, "应标记 token 已失效");
        assert_eq!(state.store.load_logs()[0].code.as_deref(), Some("token_expired"));
    }

    #[tokio::test]
    async fn rate_limited_and_upstream_keep_credentials() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ret": -2})))
            .mount(&server)
            .await;
        let (_d, state) = logged_in_state(&server.uri()).await;
        assert_eq!(send_text(&state, None, "hi").await, Err(SendFailure::RateLimited));
        assert!(state.is_logged_in().await);

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(502))
            .mount(&server)
            .await;
        let (_d, state) = logged_in_state(&server.uri()).await;
        assert!(matches!(send_text(&state, None, "hi").await, Err(SendFailure::Upstream(_))));
        assert_eq!(state.store.load_logs()[0].code.as_deref(), Some("upstream_error"));
    }

    #[test]
    fn codes_are_stable() {
        assert_eq!(SendFailure::NotLoggedIn.code(), "not_logged_in");
        assert_eq!(SendFailure::TokenExpired.code(), "token_expired");
        assert_eq!(SendFailure::RateLimited.code(), "rate_limited");
        assert_eq!(SendFailure::Upstream("x".into()).code(), "upstream_error");
    }
}
