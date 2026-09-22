use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use super::middleware::require_api_key;
use crate::ilink::types::is_ilink_user_id;
use crate::sender::{send_text, SendFailure};
use crate::state::AppState;
use crate::wecom::sender::{send_text as wecom_send_text, WecomFailure};
use crate::wecom::types::normalize_to;

pub const MAX_BODY_BYTES: usize = 1024 * 1024;

#[derive(Debug, Deserialize)]
pub struct SendReq {
    pub to: Option<String>,
    pub text: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WecomSendReq {
    pub to: Option<String>,
    pub text: Option<String>,
    pub app: Option<String>,
}

pub fn wecom_failure_status(f: &WecomFailure) -> StatusCode {
    match f {
        WecomFailure::UnknownApp(_) | WecomFailure::UnknownRecipient(_) => StatusCode::BAD_REQUEST,
        WecomFailure::NotConfigured | WecomFailure::InvalidCredentials(_) => StatusCode::SERVICE_UNAVAILABLE,
        WecomFailure::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        WecomFailure::Upstream(_) => StatusCode::BAD_GATEWAY,
    }
}

pub fn failure_status(f: &SendFailure) -> StatusCode {
    match f {
        SendFailure::UnknownRecipient(_) => StatusCode::BAD_REQUEST,
        SendFailure::NotLoggedIn | SendFailure::TokenExpired => StatusCode::SERVICE_UNAVAILABLE,
        SendFailure::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        SendFailure::Upstream(_) => StatusCode::BAD_GATEWAY,
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    let protected = Router::new()
        .route("/send", post(send))
        .route("/wecom/send", post(wecom_send))
        .layer(axum::middleware::from_fn_with_state(state.clone(), require_api_key));
    Router::new()
        .route("/health", get(health))
        .merge(protected)
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(state)
}

fn bad_request(msg: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "ok": false, "code": "bad_request", "error": msg })),
    )
}

async fn send(
    State(state): State<Arc<AppState>>,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    // 不依赖 Content-Type：直接按 JSON 解析原始字节
    let req: SendReq = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => return bad_request(&format!("invalid JSON body: {e}")),
    };
    let text = match req.text.as_deref().map(str::trim) {
        Some(t) if !t.is_empty() => t.to_string(),
        _ => return bad_request("text is required"),
    };
    if let Some(to) = req.to.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        if !is_ilink_user_id(to) {
            return bad_request("to 必须是 iLink 用户 ID（形如 xxx@im.wechat），不是微信号/wxid");
        }
    }
    match send_text(&state, req.to.as_deref(), &text).await {
        Ok(to) => (StatusCode::OK, Json(json!({ "ok": true, "to": to }))),
        Err(f) => (
            failure_status(&f),
            Json(json!({ "ok": false, "code": f.code(), "error": f.message() })),
        ),
    }
}

async fn wecom_send(
    State(state): State<Arc<AppState>>,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let req: WecomSendReq = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => return bad_request(&format!("invalid JSON body: {e}")),
    };
    let text = match req.text.as_deref().map(str::trim) {
        Some(t) if !t.is_empty() => t.to_string(),
        _ => return bad_request("text is required"),
    };
    let to = match normalize_to(req.to.as_deref()) {
        Ok(t) => t,
        Err(m) => return bad_request(&m),
    };
    match wecom_send_text(&state, req.app.as_deref(), &to, &text).await {
        Ok(r) => (
            StatusCode::OK,
            Json(json!({
                "ok": true,
                "app": r.app,
                "to": r.to,
                "msgid": r.msgid,
                "invalid_users": r.invalid_users,
            })),
        ),
        Err(f) => (
            wecom_failure_status(&f),
            Json(json!({ "ok": false, "code": f.code(), "error": f.message() })),
        ),
    }
}

async fn health(State(state): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({
        "ok": true,
        "logged_in": state.is_logged_in().await,
        "accounts": state.accounts.read().await.len(),
        "wecom_apps": state.wecom_apps.read().await.len(),
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ilink::types::Credentials;
    use crate::store::Store;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn state_with(base: Option<&str>) -> (tempfile::TempDir, Arc<AppState>) {
        let d = tempfile::tempdir().unwrap();
        let state = AppState::new(Store::new(d.path().to_path_buf()));
        if let Some(b) = base {
            state
                .upsert_account(Credentials {
                    bot_token: "t".into(),
                    ilink_bot_id: "bot@im.bot".into(),
                    ilink_user_id: "me@im.wechat".into(),
                    baseurl: b.into(),
                })
                .await
                .unwrap();
        }
        (d, state)
    }

    async fn post_send(state: &Arc<AppState>, key: Option<&str>, body: &str) -> (StatusCode, serde_json::Value) {
        let mut req = Request::post("/send").header("content-type", "application/json");
        if let Some(k) = key {
            req = req.header("x-api-key", k);
        }
        let r = router(state.clone())
            .oneshot(req.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = r.status();
        let bytes = r.into_body().collect().await.unwrap().to_bytes();
        let v = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, v)
    }

    async fn mock_ret(ret: i64) -> MockServer {
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "ret": ret })))
            .mount(&s)
            .await;
        s
    }

    #[tokio::test]
    async fn health_is_public_and_reports_login() {
        let (_d, state) = state_with(None).await;
        let r = router(state.clone())
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let v: serde_json::Value =
            serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["logged_in"], false);
        assert_eq!(v["accounts"], 0);
        assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn send_requires_key() {
        let (_d, state) = state_with(None).await;
        let (s, v) = post_send(&state, None, r#"{"text":"hi"}"#).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        assert_eq!(v["code"], "unauthorized");
    }

    #[tokio::test]
    async fn bad_request_on_missing_text_or_invalid_json() {
        let (_d, state) = state_with(None).await;
        let k = state.config.read().await.api_key.clone();
        let (s, v) = post_send(&state, Some(&k), r#"{"to":"x"}"#).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert_eq!(v["code"], "bad_request");
        let (s, v) = post_send(&state, Some(&k), r#"{"text":"   "}"#).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert_eq!(v["code"], "bad_request");
        let (s, v) = post_send(&state, Some(&k), "not json").await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert_eq!(v["code"], "bad_request");
    }

    #[tokio::test]
    async fn rejects_non_ilink_recipient_id() {
        let (_d, state) = state_with(None).await;
        let k = state.config.read().await.api_key.clone();
        let (s, v) = post_send(&state, Some(&k), r#"{"to":"liuliangzheng","text":"hi"}"#).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert_eq!(v["code"], "bad_request");
        assert!(v["error"].as_str().unwrap().contains("im.wechat"));
    }

    #[tokio::test]
    async fn unknown_recipient_is_400() {
        let m = mock_ret(0).await;
        let (_d, state) = state_with(Some(&m.uri())).await;
        let k = state.config.read().await.api_key.clone();
        let (s, v) = post_send(&state, Some(&k), r#"{"to":"other@im.wechat","text":"hi"}"#).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert_eq!(v["ok"], false);
        assert_eq!(v["code"], "unknown_recipient");
        assert!(v["error"].as_str().unwrap().contains("other@im.wechat"));
    }

    #[tokio::test]
    async fn not_logged_in_is_503() {
        let (_d, state) = state_with(None).await;
        let k = state.config.read().await.api_key.clone();
        let (s, v) = post_send(&state, Some(&k), r#"{"text":"hi"}"#).await;
        assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(v["code"], "not_logged_in");
    }

    #[tokio::test]
    async fn send_accepts_json_body_without_content_type() {
        let (_d, state) = state_with(None).await;
        let k = state.config.read().await.api_key.clone();
        // 故意不带 content-type：body 仍应被解析，走到 not_logged_in 而非 400
        let req = Request::post("/send")
            .header("x-api-key", k)
            .body(Body::from(r#"{"text":"hi"}"#))
            .unwrap();
        let r = router(state.clone()).oneshot(req).await.unwrap();
        assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
        let v: serde_json::Value =
            serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(v["code"], "not_logged_in");
    }

    #[tokio::test]
    async fn success_returns_200_ok_true() {
        let m = mock_ret(0).await;
        let (_d, state) = state_with(Some(&m.uri())).await;
        let k = state.config.read().await.api_key.clone();
        let (s, v) = post_send(&state, Some(&k), r#"{"text":"hi"}"#).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v["ok"], true);
        assert_eq!(v["to"], "me@im.wechat");
    }

    #[tokio::test]
    async fn upstream_codes_map_to_http() {
        for (ret, status, code) in [
            (-2, StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
            (-14, StatusCode::SERVICE_UNAVAILABLE, "token_expired"),
            (-99, StatusCode::BAD_GATEWAY, "upstream_error"),
        ] {
            let m = mock_ret(ret).await;
            let (_d, state) = state_with(Some(&m.uri())).await;
            let k = state.config.read().await.api_key.clone();
            let (s, v) = post_send(&state, Some(&k), r#"{"text":"hi"}"#).await;
            assert_eq!(s, status, "ret={ret}");
            assert_eq!(v["ok"], false);
            assert_eq!(v["code"], code);
            assert!(v["error"].as_str().map(|e| !e.is_empty()).unwrap_or(false));
        }
    }

    async fn wecom_state(base: &str) -> (tempfile::TempDir, Arc<AppState>) {
        use crate::wecom::types::{CachedToken, WecomApp};
        let d = tempfile::tempdir().unwrap();
        let state = AppState::new_with_wecom_base(Store::new(d.path().to_path_buf()), base);
        state
            .add_wecom_app(WecomApp {
                name: "运维".into(),
                corpid: "ww1".into(),
                agentid: 1000002,
                secret: "s".into(),
                token: Some(CachedToken {
                    access_token: "tok".into(),
                    expires_at: crate::wecom::token::now_secs() + 3600,
                }),
                invalid: None,
            })
            .await
            .unwrap();
        (d, state)
    }

    async fn post_wecom(state: &Arc<AppState>, key: Option<&str>, body: &str) -> (StatusCode, serde_json::Value) {
        let mut req = Request::post("/wecom/send");
        if let Some(k) = key {
            req = req.header("x-api-key", k);
        }
        let r = router(state.clone())
            .oneshot(req.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = r.status();
        let bytes = r.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null))
    }

    async fn wecom_mock(code: i64) -> MockServer {
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "errcode": code, "errmsg": "e", "msgid": "m1", "invaliduser": "lisi"
            })))
            .mount(&s)
            .await;
        s
    }

    #[tokio::test]
    async fn wecom_send_requires_key() {
        let (_d, state) = state_with(None).await;
        let (s, v) = post_wecom(&state, None, r#"{"text":"hi"}"#).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        assert_eq!(v["code"], "unauthorized");
    }

    #[tokio::test]
    async fn wecom_send_validates_text_and_to_shape() {
        let (_d, state) = state_with(None).await;
        let k = state.config.read().await.api_key.clone();
        for body in [r#"{"to":"a"}"#, r#"{"text":" "}"#, "nope", r#"{"text":"hi","to":"a||b"}"#, r#"{"text":"hi","to":"@all|a"}"#, r#"{"text":"hi","to":"a b"}"#] {
            let (s, v) = post_wecom(&state, Some(&k), body).await;
            assert_eq!(s, StatusCode::BAD_REQUEST, "{body}");
            assert_eq!(v["code"], "bad_request", "{body}");
        }
    }

    #[tokio::test]
    async fn wecom_send_not_configured_is_503() {
        let (_d, state) = state_with(None).await;
        let k = state.config.read().await.api_key.clone();
        let (s, v) = post_wecom(&state, Some(&k), r#"{"text":"hi"}"#).await;
        assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(v["code"], "not_configured");
    }

    #[tokio::test]
    async fn wecom_send_success_body() {
        let m = wecom_mock(0).await;
        let (_d, state) = wecom_state(&m.uri()).await;
        let k = state.config.read().await.api_key.clone();
        let (s, v) = post_wecom(&state, Some(&k), r#"{"text":"hi","to":" zhangsan | lisi "}"#).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v["ok"], true);
        assert_eq!(v["app"], "运维");
        assert_eq!(v["to"], "zhangsan|lisi");
        assert_eq!(v["msgid"], "m1");
        assert_eq!(v["invalid_users"], serde_json::json!(["lisi"]));

        let (s, v) = post_wecom(&state, Some(&k), r#"{"text":"hi"}"#).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v["to"], "@all", "缺省 to = @all");
    }

    #[tokio::test]
    async fn wecom_failures_map_to_http_status() {
        for (code, status, expect) in [
            (81013, StatusCode::BAD_REQUEST, "unknown_recipient"),
            (45009, StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
            (40056, StatusCode::SERVICE_UNAVAILABLE, "invalid_credentials"),
            (-1, StatusCode::BAD_GATEWAY, "upstream_error"),
        ] {
            let m = wecom_mock(code).await;
            let (_d, state) = wecom_state(&m.uri()).await;
            let k = state.config.read().await.api_key.clone();
            let (s, v) = post_wecom(&state, Some(&k), r#"{"text":"hi"}"#).await;
            assert_eq!(s, status, "errcode={code}");
            assert_eq!(v["ok"], false);
            assert_eq!(v["code"], expect);
            assert!(v["error"].as_str().map(|e| !e.is_empty()).unwrap_or(false));
        }
        let m = wecom_mock(0).await;
        let (_d, state) = wecom_state(&m.uri()).await;
        let k = state.config.read().await.api_key.clone();
        let (s, v) = post_wecom(&state, Some(&k), r#"{"text":"hi","app":"nope"}"#).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert_eq!(v["code"], "unknown_app");
    }

    #[tokio::test]
    async fn health_reports_wecom_apps() {
        let m = wecom_mock(0).await;
        let (_d, state) = wecom_state(&m.uri()).await;
        let r = router(state.clone())
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let v: serde_json::Value =
            serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(v["wecom_apps"], 1);
        assert_eq!(v["accounts"], 0);
    }
}
