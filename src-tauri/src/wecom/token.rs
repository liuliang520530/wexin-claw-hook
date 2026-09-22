use crate::state::AppState;
use crate::wecom::api::ApiError;
use crate::wecom::types::{describe_error, is_credential_error, is_rate_limit, CachedToken, WecomApp, WecomFailure};
use crate::wecom::TOKEN_SAFETY_MARGIN_SECS;

pub fn now_secs() -> i64 {
    chrono::Utc::now().timestamp()
}

/// 缓存 token 仍在安全边际内则返回它。
fn cached_valid(app: &WecomApp, now: i64) -> Option<String> {
    app.token
        .as_ref()
        .filter(|t| now < t.expires_at - TOKEN_SAFETY_MARGIN_SECS)
        .map(|t| t.access_token.clone())
}

/// 取可用的 access_token。`stale` = 刚被上游拒绝的 token：传入时强制刷新，除非别的请求已经换掉了它。
/// 刷新持全局 wecom_refresh 锁并双检，并发的多条请求只打一次 gettoken。
pub async fn ensure_token(
    state: &AppState,
    name: &str,
    stale: Option<&str>,
) -> Result<String, WecomFailure> {
    let now = now_secs();
    let app = state
        .find_wecom_app(name)
        .await
        .ok_or_else(|| WecomFailure::UnknownApp(name.to_string()))?;
    if stale.is_none() {
        if let Some(t) = cached_valid(&app, now) {
            return Ok(t);
        }
    }

    let _refresh = state.wecom_refresh.lock().await;
    let app = state
        .find_wecom_app(name)
        .await
        .ok_or_else(|| WecomFailure::UnknownApp(name.to_string()))?;
    if let Some(t) = cached_valid(&app, now) {
        if stale != Some(t.as_str()) {
            return Ok(t);
        }
    }

    match state.wecom_api.get_token(&app.corpid, &app.secret).await {
        Ok(tok) => {
            let cached = CachedToken {
                access_token: tok.access_token.clone(),
                expires_at: now + tok.expires_in,
            };
            state
                .set_wecom_token(name, cached)
                .await
                .map_err(WecomFailure::Upstream)?;
            Ok(tok.access_token)
        }
        Err(ApiError::Code { code, msg }) if is_credential_error(code) => {
            let reason = describe_error(code, &msg, app.agentid);
            let _ = state.mark_wecom_invalid(name, &reason).await;
            Err(WecomFailure::InvalidCredentials(reason))
        }
        Err(ApiError::Code { code, .. }) if is_rate_limit(code) => Err(WecomFailure::RateLimited),
        Err(e) => Err(WecomFailure::Upstream(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use std::sync::Arc;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn state_with_app(base: &str, token: Option<CachedToken>) -> (tempfile::TempDir, Arc<AppState>) {
        let d = tempfile::tempdir().unwrap();
        let s = AppState::new_with_wecom_base(Store::new(d.path().to_path_buf()), base);
        s.add_wecom_app(WecomApp {
            name: "a".into(),
            corpid: "ww1".into(),
            agentid: 1,
            secret: "sec".into(),
            token,
            invalid: None,
        })
        .await
        .unwrap();
        (d, s)
    }

    fn token_mock() -> wiremock::MockBuilder {
        Mock::given(method("GET")).and(path("/cgi-bin/gettoken"))
    }

    fn token_ok() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "errcode": 0, "access_token": "fresh", "expires_in": 7200
        }))
    }

    fn cached(token: &str, secs_left: i64) -> CachedToken {
        CachedToken { access_token: token.into(), expires_at: now_secs() + secs_left }
    }

    #[tokio::test]
    async fn valid_cache_skips_gettoken() {
        let server = MockServer::start().await;
        token_mock().respond_with(token_ok()).expect(0).mount(&server).await;
        let (_d, s) = state_with_app(&server.uri(), Some(cached("old", 3600))).await;
        assert_eq!(ensure_token(&s, "a", None).await.unwrap(), "old");
    }

    #[tokio::test]
    async fn refreshes_inside_safety_margin_but_not_outside() {
        let server = MockServer::start().await;
        token_mock().respond_with(token_ok()).expect(0).mount(&server).await;
        let (_d, s) = state_with_app(&server.uri(), Some(cached("old", 310))).await;
        assert_eq!(ensure_token(&s, "a", None).await.unwrap(), "old", "距过期 310 秒不刷新");

        let server = MockServer::start().await;
        token_mock().respond_with(token_ok()).expect(1).mount(&server).await;
        let (_d, s) = state_with_app(&server.uri(), Some(cached("old", 299))).await;
        assert_eq!(ensure_token(&s, "a", None).await.unwrap(), "fresh", "299 秒要刷新");
        let stored = s.find_wecom_app("a").await.unwrap().token.unwrap();
        assert_eq!(stored.access_token, "fresh");
        assert!(stored.expires_at >= now_secs() + 7000, "expires_at = now + expires_in");
        assert_eq!(s.store.load_wecom_apps()[0].token.as_ref().unwrap().access_token, "fresh", "应落盘");
    }

    #[tokio::test]
    async fn concurrent_requests_refresh_once() {
        let server = MockServer::start().await;
        token_mock().respond_with(token_ok()).expect(1).mount(&server).await;
        let (_d, s) = state_with_app(&server.uri(), None).await;
        let handles: Vec<_> = (0..10)
            .map(|_| {
                let s = s.clone();
                tokio::spawn(async move { ensure_token(&s, "a", None).await.unwrap() })
            })
            .collect();
        for h in handles {
            assert_eq!(h.await.unwrap(), "fresh");
        }
    }

    #[tokio::test]
    async fn stale_token_forces_refresh_unless_already_replaced() {
        let server = MockServer::start().await;
        token_mock().respond_with(token_ok()).expect(1).mount(&server).await;
        let (_d, s) = state_with_app(&server.uri(), Some(cached("old", 3600))).await;
        assert_eq!(ensure_token(&s, "a", Some("old")).await.unwrap(), "fresh", "缓存虽有效但就是它被拒绝了");
        // 缓存已是 fresh，再拿着 old 来：不应再打 gettoken（expect(1)）
        assert_eq!(ensure_token(&s, "a", Some("old")).await.unwrap(), "fresh");
    }

    #[tokio::test]
    async fn credential_error_marks_invalid_and_clears_token() {
        let server = MockServer::start().await;
        token_mock()
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "errcode": 40001, "errmsg": "invalid credential"
            })))
            .mount(&server)
            .await;
        let (_d, s) = state_with_app(&server.uri(), None).await;
        match ensure_token(&s, "a", None).await {
            Err(WecomFailure::InvalidCredentials(r)) => assert!(r.contains("secret") && r.contains("40001")),
            other => panic!("unexpected {other:?}"),
        }
        let stored = s.store.load_wecom_apps().remove(0);
        assert!(stored.invalid.is_some());
        assert!(stored.token.is_none());
    }

    #[tokio::test]
    async fn rate_limit_and_busy_do_not_mark_invalid() {
        let server = MockServer::start().await;
        token_mock()
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"errcode": 45009, "errmsg": "freq"})))
            .mount(&server)
            .await;
        let (_d, s) = state_with_app(&server.uri(), None).await;
        assert_eq!(ensure_token(&s, "a", None).await, Err(WecomFailure::RateLimited));
        assert!(s.find_wecom_app("a").await.unwrap().invalid.is_none());

        let server = MockServer::start().await;
        token_mock()
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"errcode": -1, "errmsg": "system busy"})))
            .mount(&server)
            .await;
        let (_d, s) = state_with_app(&server.uri(), None).await;
        assert!(matches!(ensure_token(&s, "a", None).await, Err(WecomFailure::Upstream(_))));
        assert!(s.find_wecom_app("a").await.unwrap().invalid.is_none());
    }

    #[tokio::test]
    async fn unknown_app_is_reported() {
        let d = tempfile::tempdir().unwrap();
        let s = AppState::new(Store::new(d.path().to_path_buf()));
        assert_eq!(ensure_token(&s, "zz", None).await, Err(WecomFailure::UnknownApp("zz".into())));
    }
}
