use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use subtle::ConstantTimeEq;

use crate::state::AppState;
use crate::store::LogEntry;

pub fn extract_key(req: &Request) -> Option<String> {
    if let Some(v) = req.headers().get("x-api-key").and_then(|v| v.to_str().ok()) {
        let v = v.trim();
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    let auth = req.headers().get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = auth.trim().split_once(' ')?;
    if scheme.eq_ignore_ascii_case("bearer") && !token.trim().is_empty() {
        Some(token.trim().to_string())
    } else {
        None
    }
}

/// 常数时间比较；长度不同直接判否（subtle 对不同长度返回 false）。
pub fn key_matches(provided: &str, expected: &str) -> bool {
    provided.as_bytes().ct_eq(expected.as_bytes()).into()
}

pub fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "ok": false, "code": "unauthorized" })),
    )
        .into_response()
}

pub async fn require_api_key(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    let expected = state.config.read().await.api_key.clone();
    let ok = extract_key(&req)
        .map(|k| key_matches(&k, &expected))
        .unwrap_or(false);
    if ok {
        return next.run(req).await;
    }
    let ip = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let _ = state.store.append_log(LogEntry::now(
        false,
        Some("unauthorized"),
        "",
        &format!("鉴权失败，来源 {ip}"),
    ));
    unauthorized()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::{Request, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use http_body_util::BodyExt;
    use std::net::SocketAddr;
    use tower::ServiceExt;

    fn app() -> (tempfile::TempDir, Arc<AppState>, Router) {
        let d = tempfile::tempdir().unwrap();
        let state = AppState::new(Store::new(d.path().to_path_buf()));
        let router = Router::new()
            .route("/p", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(state.clone(), require_api_key))
            .with_state(state.clone());
        (d, state, router)
    }

    async fn key(state: &AppState) -> String {
        state.config.read().await.api_key.clone()
    }

    #[tokio::test]
    async fn accepts_x_api_key_and_bearer() {
        let (_d, state, router) = app();
        let k = key(&state).await;

        let r = router
            .clone()
            .oneshot(Request::get("/p").header("x-api-key", &k).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);

        let r = router
            .oneshot(
                Request::get("/p")
                    .header("authorization", format!("Bearer {k}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn rejects_missing_or_wrong_key_with_json_and_logs_ip() {
        let (_d, state, router) = app();

        let r = router
            .clone()
            .oneshot(Request::get("/p").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
        let body = r.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["ok"], false);
        assert_eq!(v["code"], "unauthorized");

        let mut req = Request::get("/p").header("x-api-key", "wrong").body(Body::empty()).unwrap();
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([192, 168, 1, 5], 40000))));
        let r = router.oneshot(req).await.unwrap();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);

        let logs = state.store.load_logs();
        assert_eq!(logs[0].code.as_deref(), Some("unauthorized"));
        assert!(logs[0].text.contains("192.168.1.5"));
        assert!(!logs[0].text.contains("wrong"), "日志不能记录 key");
    }

    #[test]
    fn key_matches_is_exact() {
        assert!(key_matches("abc", "abc"));
        assert!(!key_matches("abc", "abd"));
        assert!(!key_matches("ab", "abc"));
        assert!(!key_matches("", "abc"));
    }
}
