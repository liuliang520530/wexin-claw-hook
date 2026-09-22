use serde::Serialize;

use crate::state::AppState;
use crate::store::LogEntry;
use crate::wecom::api::ApiError;
use crate::wecom::token::ensure_token;
use crate::wecom::types::{
    describe_error, is_credential_error, is_rate_limit, is_recipient_error, is_token_stale, WecomApp,
};
pub use crate::wecom::types::WecomFailure;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct WecomSendOk {
    pub app: String,
    pub to: String,
    pub msgid: String,
    /// 未送达的收件人（不在可见范围 / 无许可）；非空仍算成功
    pub invalid_users: Vec<String>,
}

/// webhook 与发消息页共用入口。`app` 缺省 = 默认应用；`to` 须已由 normalize_to 规范化。每次调用恰好写一条日志。
pub async fn send_text(
    state: &AppState,
    app: Option<&str>,
    to: &str,
    text: &str,
) -> Result<WecomSendOk, WecomFailure> {
    let app_name = app.map(str::trim).filter(|s| !s.is_empty());
    let chosen = match app_name {
        Some(n) => state.find_wecom_app(n).await,
        None => state.default_wecom_app().await,
    };
    let Some(app) = chosen else {
        let has_apps = !state.wecom_apps.read().await.is_empty();
        let failure = match app_name {
            Some(n) if has_apps => WecomFailure::UnknownApp(n.to_string()),
            _ => WecomFailure::NotConfigured,
        };
        let _ = state
            .store
            .append_log(LogEntry::wecom(false, Some(failure.code()), "", to, text));
        return Err(failure);
    };

    let outcome = send_with(state, &app, to, text).await;
    let _ = state.store.append_log(LogEntry::wecom(
        outcome.is_ok(),
        outcome.as_ref().err().map(WecomFailure::code),
        &app.name,
        to,
        text,
    ));
    if let Err(WecomFailure::InvalidCredentials(reason)) = &outcome {
        let _ = state.mark_wecom_invalid(&app.name, reason).await;
    }
    outcome
}

async fn send_with(state: &AppState, app: &WecomApp, to: &str, text: &str) -> Result<WecomSendOk, WecomFailure> {
    if let Some(reason) = &app.invalid {
        return Err(WecomFailure::InvalidCredentials(reason.clone()));
    }
    let token = ensure_token(state, &app.name, None).await?;
    let first = state.wecom_api.send_text(&token, app.agentid, to, text).await;
    let result = match first {
        // token 被提前作废或刚过期：强制刷新，重发一次
        Err(ApiError::Code { code, .. }) if is_token_stale(code) => {
            let token = ensure_token(state, &app.name, Some(&token)).await?;
            state.wecom_api.send_text(&token, app.agentid, to, text).await
        }
        other => other,
    };
    match result {
        Ok(r) => Ok(WecomSendOk {
            app: app.name.clone(),
            to: to.to_string(),
            msgid: r.msgid,
            invalid_users: r.invalid_users,
        }),
        Err(ApiError::Code { code, msg }) => Err(map_send_code(code, &msg, to, app.agentid)),
        Err(ApiError::Http(m)) => Err(WecomFailure::Upstream(m)),
    }
}

fn map_send_code(code: i64, msg: &str, to: &str, agentid: u32) -> WecomFailure {
    if is_token_stale(code) {
        WecomFailure::Upstream(format!("access_token 刷新后仍被拒绝（errcode={code} {msg}）"))
    } else if is_recipient_error(code) {
        WecomFailure::UnknownRecipient(to.to_string())
    } else if is_rate_limit(code) {
        WecomFailure::RateLimited
    } else if is_credential_error(code) {
        WecomFailure::InvalidCredentials(describe_error(code, msg, agentid))
    } else {
        WecomFailure::Upstream(format!("errcode={code} {msg}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use crate::wecom::types::CachedToken;
    use std::sync::Arc;
    use wiremock::matchers::{body_partial_json, method, path, query_param};
    use wiremock::{Mock, MockGuard, MockServer, ResponseTemplate};

    fn app(name: &str, agentid: u32, token: Option<&str>) -> WecomApp {
        WecomApp {
            name: name.into(),
            corpid: "ww1".into(),
            agentid,
            secret: format!("sec-{name}"),
            token: token.map(|t| CachedToken {
                access_token: t.into(),
                expires_at: crate::wecom::token::now_secs() + 3600,
            }),
            invalid: None,
        }
    }

    async fn state_with(base: &str, apps: Vec<WecomApp>) -> (tempfile::TempDir, Arc<AppState>) {
        let d = tempfile::tempdir().unwrap();
        let s = AppState::new_with_wecom_base(Store::new(d.path().to_path_buf()), base);
        for a in apps {
            s.add_wecom_app(a).await.unwrap();
        }
        (d, s)
    }

    fn send_mock() -> wiremock::MockBuilder {
        Mock::given(method("POST")).and(path("/cgi-bin/message/send"))
    }

    fn code_resp(code: i64) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(serde_json::json!({"errcode": code, "errmsg": "e", "msgid": "m1"}))
    }

    async fn mount_gettoken(server: &MockServer, times: u64) -> MockGuard {
        Mock::given(method("GET"))
            .and(path("/cgi-bin/gettoken"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "errcode": 0, "access_token": "fresh", "expires_in": 7200
            })))
            .expect(times)
            .mount_as_scoped(server)
            .await
    }

    #[tokio::test]
    async fn no_apps_is_not_configured_even_with_app_name() {
        let d = tempfile::tempdir().unwrap();
        let s = AppState::new(Store::new(d.path().to_path_buf()));
        assert_eq!(send_text(&s, None, "@all", "hi").await, Err(WecomFailure::NotConfigured));
        assert_eq!(send_text(&s, Some("x"), "@all", "hi").await, Err(WecomFailure::NotConfigured));
        let l = &s.store.load_logs()[0];
        assert_eq!((l.channel.as_str(), l.from.as_str(), l.code.as_deref()), ("wecom", "", Some("not_configured")));
    }

    #[tokio::test]
    async fn unknown_app_when_apps_exist() {
        let server = MockServer::start().await;
        let (_d, s) = state_with(&server.uri(), vec![app("a", 1, Some("tok"))]).await;
        assert_eq!(send_text(&s, Some("zz"), "@all", "hi").await, Err(WecomFailure::UnknownApp("zz".into())));
        assert_eq!(s.store.load_logs()[0].code.as_deref(), Some("unknown_app"));
    }

    #[tokio::test]
    async fn default_app_prefers_config_then_first() {
        let server = MockServer::start().await;
        send_mock()
            .and(query_param("access_token", "tok-b"))
            .and(body_partial_json(serde_json::json!({"agentid": 2, "touser": "@all", "text": {"content": "hi"}})))
            .respond_with(code_resp(0))
            .expect(1)
            .mount(&server)
            .await;
        send_mock()
            .and(query_param("access_token", "tok-a"))
            .respond_with(code_resp(0))
            .expect(1)
            .mount(&server)
            .await;
        let (_d, s) = state_with(&server.uri(), vec![app("a", 1, Some("tok-a")), app("b", 2, Some("tok-b"))]).await;
        assert_eq!(send_text(&s, None, "@all", "hi").await.unwrap().app, "a");
        s.set_default_wecom_app("b").await.unwrap();
        let ok = send_text(&s, Some("  "), "@all", "hi").await.unwrap();
        assert_eq!(ok.app, "b");
        assert_eq!(ok.msgid, "m1");
        let l = &s.store.load_logs()[0];
        assert!(l.ok);
        assert_eq!((l.channel.as_str(), l.from.as_str(), l.to.as_str(), l.text.as_str()), ("wecom", "b", "@all", "hi"));
    }

    #[tokio::test]
    async fn invalid_app_is_rejected_without_upstream_call() {
        let server = MockServer::start().await;
        send_mock().respond_with(code_resp(0)).expect(0).mount(&server).await;
        let (_d, s) = state_with(&server.uri(), vec![app("a", 1, Some("tok"))]).await;
        s.mark_wecom_invalid("a", "secret 不合法（40001）").await.unwrap();
        match send_text(&s, None, "@all", "hi").await {
            Err(WecomFailure::InvalidCredentials(r)) => assert!(r.contains("40001")),
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(s.store.load_logs()[0].code.as_deref(), Some("invalid_credentials"));
    }

    #[tokio::test]
    async fn stale_token_refreshes_and_retries_once() {
        let server = MockServer::start().await;
        let _g = mount_gettoken(&server, 1).await;
        send_mock()
            .and(query_param("access_token", "tok"))
            .respond_with(code_resp(40014))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        send_mock()
            .and(query_param("access_token", "fresh"))
            .respond_with(code_resp(0))
            .expect(1)
            .mount(&server)
            .await;
        let (_d, s) = state_with(&server.uri(), vec![app("a", 1, Some("tok"))]).await;
        assert!(send_text(&s, None, "@all", "hi").await.is_ok());
        assert_eq!(s.find_wecom_app("a").await.unwrap().token.unwrap().access_token, "fresh");
    }

    #[tokio::test]
    async fn two_stale_rejections_give_up_as_upstream() {
        let server = MockServer::start().await;
        let _g = mount_gettoken(&server, 1).await;
        send_mock().respond_with(code_resp(42001)).expect(2).mount(&server).await;
        let (_d, s) = state_with(&server.uri(), vec![app("a", 1, Some("tok"))]).await;
        match send_text(&s, None, "@all", "hi").await {
            Err(WecomFailure::Upstream(m)) => assert!(m.contains("42001")),
            other => panic!("unexpected {other:?}"),
        }
        assert!(s.find_wecom_app("a").await.unwrap().invalid.is_none());
    }

    #[tokio::test]
    async fn send_error_codes_map_to_failures() {
        for (code, expect_code, marks_invalid) in [
            (81013, "unknown_recipient", false),
            (60111, "unknown_recipient", false),
            (45009, "rate_limited", false),
            (40056, "invalid_credentials", true),
            (301002, "invalid_credentials", true),
            (-1, "upstream_error", false),
            (60020, "upstream_error", false),
        ] {
            let server = MockServer::start().await;
            send_mock().respond_with(code_resp(code)).mount(&server).await;
            let (_d, s) = state_with(&server.uri(), vec![app("a", 1000002, Some("tok"))]).await;
            let err = send_text(&s, None, "zhangsan", "hi").await.unwrap_err();
            assert_eq!(err.code(), expect_code, "errcode={code}");
            assert_eq!(s.store.load_logs()[0].code.as_deref(), Some(expect_code));
            assert_eq!(s.find_wecom_app("a").await.unwrap().invalid.is_some(), marks_invalid, "errcode={code}");
            if code == 81013 {
                assert!(err.message().contains("zhangsan"));
            }
        }
    }

    #[tokio::test]
    async fn invalid_users_are_returned_on_success() {
        let server = MockServer::start().await;
        send_mock()
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "errcode": 0, "msgid": "m", "invaliduser": "lisi"
            })))
            .mount(&server)
            .await;
        let (_d, s) = state_with(&server.uri(), vec![app("a", 1, Some("tok"))]).await;
        let ok = send_text(&s, None, "zhangsan|lisi", "hi").await.unwrap();
        assert_eq!(ok.invalid_users, vec!["lisi"]);
        assert!(s.store.load_logs()[0].ok);
    }
}
