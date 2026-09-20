use crate::ilink::client::{IlinkClient, SendError};
use crate::state::AppState;
use crate::store::{Account, LogEntry};

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
            SendFailure::NotLoggedIn => "尚未扫码登录任何账号".to_string(),
            SendFailure::TokenExpired => "该账号登录已失效，请重新扫码".to_string(),
            SendFailure::RateLimited => "微信侧拒绝发送（ret=-2）：若是首次给该用户推送，需对方先在微信里给机器人发一条消息建立会话；否则为频率限制（约 7 条/5 分钟），稍后重试".to_string(),
            SendFailure::Upstream(m) => m.clone(),
        }
    }
}

/// webhook 入口。收件人 `to` 缺省 = 默认账号自己；`to` 是某个已登录账号时用它自己的凭据，
/// 否则用默认账号的凭据发给 `to`。
pub async fn send_text(
    state: &AppState,
    to: Option<&str>,
    text: &str,
) -> Result<String, SendFailure> {
    let to = to.map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);
    let account = match &to {
        Some(t) => match state.find_account(t).await {
            Some(a) => Some(a),
            None => state.default_account().await,
        },
        None => state.default_account().await,
    };
    let Some(account) = account else {
        let _ = state.store.append_log(LogEntry::now(
            false,
            Some(SendFailure::NotLoggedIn.code()),
            "",
            to.as_deref().unwrap_or(""),
            text,
        ));
        return Err(SendFailure::NotLoggedIn);
    };
    let to = to.unwrap_or_else(|| account.user_id().to_string());
    send_with_account(state, &account, &to, text).await
}

/// 用指定账号的凭据给任意 ID 发；发消息页的交叉测试也走这里。
/// 每次调用都写日志；ret=-14 时标记该账号失效（账号保留）。
pub async fn send_with_account(
    state: &AppState,
    account: &Account,
    to: &str,
    text: &str,
) -> Result<String, SendFailure> {
    let from = account.user_id().to_string();
    if account.token_expired {
        let _ = state.store.append_log(LogEntry::now(
            false,
            Some(SendFailure::TokenExpired.code()),
            &from,
            to,
            text,
        ));
        return Err(SendFailure::TokenExpired);
    }

    let client = IlinkClient::new(state.http.clone(), account.creds.clone());
    let outcome = match client.send_text(to, text, None).await {
        Ok(_) => Ok(to.to_string()),
        Err(SendError::RateLimited) => Err(SendFailure::RateLimited),
        Err(SendError::TokenExpired) => Err(SendFailure::TokenExpired),
        Err(SendError::Upstream(m)) => Err(SendFailure::Upstream(m)),
    };

    let _ = state.store.append_log(LogEntry::now(
        outcome.is_ok(),
        outcome.as_ref().err().map(SendFailure::code),
        &from,
        to,
        text,
    ));

    if matches!(outcome, Err(SendFailure::TokenExpired)) {
        let _ = state.mark_expired(&from).await;
    }
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ilink::types::Credentials;
    use crate::store::Store;
    use std::sync::Arc;
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn creds(base: &str, user: &str) -> Credentials {
        Credentials {
            bot_token: format!("tok-{user}"),
            ilink_bot_id: "bot@im.bot".into(),
            ilink_user_id: user.into(),
            baseurl: base.into(),
        }
    }

    async fn state_with(base: &str, users: &[&str]) -> (tempfile::TempDir, Arc<AppState>) {
        let d = tempfile::tempdir().unwrap();
        let state = AppState::new(Store::new(d.path().to_path_buf()));
        for u in users {
            state.upsert_account(creds(base, u)).await.unwrap();
        }
        (d, state)
    }

    /// 先 and 匹配器、expect 次数，最后再 respond_with。
    fn ok_mock() -> wiremock::MockBuilder {
        Mock::given(method("POST")).and(path("/ilink/bot/sendmessage"))
    }

    fn ok_resp() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(serde_json::json!({"ret": 0}))
    }

    #[tokio::test]
    async fn no_accounts_fails_and_logs() {
        let d = tempfile::tempdir().unwrap();
        let state = AppState::new(Store::new(d.path().to_path_buf()));
        assert_eq!(send_text(&state, None, "hi").await, Err(SendFailure::NotLoggedIn));
        let logs = state.store.load_logs();
        assert_eq!(logs[0].code.as_deref(), Some("not_logged_in"));
        assert!(!logs[0].ok);
        assert_eq!(logs[0].from, "");
    }

    #[tokio::test]
    async fn no_to_sends_default_account_to_itself() {
        let server = MockServer::start().await;
        ok_mock()
            .and(header("Authorization", "Bearer tok-a@im.wechat"))
            .and(body_partial_json(serde_json::json!({"msg": {"to_user_id": "a@im.wechat"}})))
            .respond_with(ok_resp())
            .expect(1)
            .mount(&server)
            .await;
        let (_d, state) = state_with(&server.uri(), &["a@im.wechat", "b@im.wechat"]).await;
        assert_eq!(send_text(&state, None, "hi").await.unwrap(), "a@im.wechat");
        let logs = state.store.load_logs();
        assert!(logs[0].ok);
        assert_eq!(logs[0].from, "a@im.wechat");
        assert_eq!(logs[0].to, "a@im.wechat");
        assert_eq!(logs[0].text, "hi");
    }

    #[tokio::test]
    async fn to_matching_account_uses_that_accounts_own_creds() {
        let server = MockServer::start().await;
        ok_mock()
            .and(header("Authorization", "Bearer tok-b@im.wechat"))
            .and(body_partial_json(serde_json::json!({"msg": {"to_user_id": "b@im.wechat"}})))
            .respond_with(ok_resp())
            .expect(1)
            .mount(&server)
            .await;
        let (_d, state) = state_with(&server.uri(), &["a@im.wechat", "b@im.wechat"]).await;
        assert_eq!(send_text(&state, Some("b@im.wechat"), "hi").await.unwrap(), "b@im.wechat");
        assert_eq!(state.store.load_logs()[0].from, "b@im.wechat");
    }

    #[tokio::test]
    async fn config_default_account_and_unknown_to_use_default_creds() {
        let server = MockServer::start().await;
        ok_mock()
            .and(header("Authorization", "Bearer tok-b@im.wechat"))
            .and(body_partial_json(serde_json::json!({"msg": {"to_user_id": "b@im.wechat"}})))
            .respond_with(ok_resp())
            .expect(2)
            .mount(&server)
            .await;
        ok_mock()
            .and(header("Authorization", "Bearer tok-b@im.wechat"))
            .and(body_partial_json(serde_json::json!({"msg": {"to_user_id": "x@im.wechat"}})))
            .respond_with(ok_resp())
            .expect(1)
            .mount(&server)
            .await;
        let (_d, state) = state_with(&server.uri(), &["a@im.wechat", "b@im.wechat"]).await;
        state.config.write().await.default_account = Some("b@im.wechat".into());
        assert_eq!(send_text(&state, None, "1").await.unwrap(), "b@im.wechat");
        assert_eq!(send_text(&state, Some("  "), "2").await.unwrap(), "b@im.wechat");
        // 不是已登录账号的 ID：用默认账号（b）的凭据发给它
        assert_eq!(send_text(&state, Some("x@im.wechat"), "3").await.unwrap(), "x@im.wechat");
    }

    #[tokio::test]
    async fn cross_send_with_explicit_account() {
        let server = MockServer::start().await;
        ok_mock()
            .and(header("Authorization", "Bearer tok-a@im.wechat"))
            .and(body_partial_json(serde_json::json!({"msg": {"to_user_id": "b@im.wechat"}})))
            .respond_with(ok_resp())
            .expect(1)
            .mount(&server)
            .await;
        let (_d, state) = state_with(&server.uri(), &["a@im.wechat", "b@im.wechat"]).await;
        let a = state.find_account("a@im.wechat").await.unwrap();
        assert_eq!(send_with_account(&state, &a, "b@im.wechat", "x").await.unwrap(), "b@im.wechat");
        let l = &state.store.load_logs()[0];
        assert_eq!((l.from.as_str(), l.to.as_str()), ("a@im.wechat", "b@im.wechat"));
    }

    #[tokio::test]
    async fn token_expired_marks_only_that_account_and_skips_http_afterwards() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ret": -14})))
            .expect(1)
            .mount(&server)
            .await;
        let (_d, state) = state_with(&server.uri(), &["a@im.wechat", "b@im.wechat"]).await;
        assert_eq!(send_text(&state, None, "hi").await, Err(SendFailure::TokenExpired));
        let a = state.find_account("a@im.wechat").await.expect("账号应保留");
        assert!(a.token_expired);
        assert!(state.store.load_accounts()[0].token_expired, "失效标记应落盘");
        assert!(!state.find_account("b@im.wechat").await.unwrap().token_expired);
        assert!(state.is_logged_in().await, "还有 b 未失效");
        assert_eq!(state.store.load_logs()[0].code.as_deref(), Some("token_expired"));
        // 再发一次：不再请求上游（mock expect(1)），直接 TokenExpired 并记日志
        assert_eq!(send_text(&state, Some("a@im.wechat"), "again").await, Err(SendFailure::TokenExpired));
        assert_eq!(state.store.load_logs()[0].text, "again");
    }

    #[tokio::test]
    async fn rate_limited_and_upstream_keep_account_valid() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ret": -2})))
            .mount(&server)
            .await;
        let (_d, state) = state_with(&server.uri(), &["a@im.wechat"]).await;
        assert_eq!(send_text(&state, None, "hi").await, Err(SendFailure::RateLimited));
        assert!(state.is_logged_in().await);

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(502))
            .mount(&server)
            .await;
        let (_d, state) = state_with(&server.uri(), &["a@im.wechat"]).await;
        assert!(matches!(send_text(&state, None, "hi").await, Err(SendFailure::Upstream(_))));
        assert!(state.is_logged_in().await);
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
