use std::time::Duration;

use super::types::{request_headers, Credentials, QrCodeResponse, QrStatusResponse};

/// get_qrcode_status 是长轮询，服务端可能 hold ~35s。
pub const POLL_TIMEOUT: Duration = Duration::from_secs(40);

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AuthError {
    #[error("network error: {0}")]
    Network(String),
    #[error("bad response: {0}")]
    BadResponse(String),
    #[error("qrcode encode error: {0}")]
    QrEncode(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct QrCode {
    pub qrcode: String,
    /// 二维码内容（URL），用 qrcode_svg 渲染。
    pub content: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum QrStatus {
    Wait,
    Scanned,
    Confirmed(Credentials),
    Expired,
    Unknown(String),
}

async fn get_json<T: serde::de::DeserializeOwned>(
    http: &reqwest::Client,
    url: &str,
    timeout: Option<Duration>,
) -> Result<T, AuthError> {
    let mut b = http.get(url);
    for (k, v) in request_headers(None) {
        b = b.header(k, v);
    }
    if let Some(t) = timeout {
        b = b.timeout(t);
    }
    let resp = b.send().await.map_err(|e| AuthError::Network(e.to_string()))?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| AuthError::Network(e.to_string()))?;
    if !status.is_success() {
        return Err(AuthError::BadResponse(format!("HTTP {}", status.as_u16())));
    }
    serde_json::from_str(&body)
        .map_err(|e| AuthError::BadResponse(format!("{e}: {}", body.chars().take(200).collect::<String>())))
}

pub async fn fetch_qrcode(http: &reqwest::Client, base_url: &str) -> Result<QrCode, AuthError> {
    let url = format!("{}/ilink/bot/get_bot_qrcode?bot_type=3", base_url.trim_end_matches('/'));
    let r: QrCodeResponse = get_json(http, &url, None).await?;
    Ok(QrCode { qrcode: r.qrcode, content: r.qrcode_img_content })
}

pub async fn poll_qrcode_status(
    http: &reqwest::Client,
    base_url: &str,
    qrcode: &str,
) -> Result<QrStatus, AuthError> {
    let base = base_url.trim_end_matches('/');
    let url = format!("{base}/ilink/bot/get_qrcode_status?qrcode={qrcode}");
    let r: QrStatusResponse = get_json(http, &url, Some(POLL_TIMEOUT)).await?;
    Ok(match r.status.as_str() {
        "wait" => QrStatus::Wait,
        "scaned" | "scanned" => QrStatus::Scanned,
        "expired" => QrStatus::Expired,
        "confirmed" => {
            if r.bot_token.is_empty() {
                return Err(AuthError::BadResponse("confirmed without bot_token".into()));
            }
            QrStatus::Confirmed(Credentials {
                bot_token: r.bot_token,
                ilink_bot_id: r.ilink_bot_id,
                ilink_user_id: r.ilink_user_id,
                baseurl: if r.baseurl.is_empty() { base.to_string() } else { r.baseurl },
            })
        }
        other => QrStatus::Unknown(other.to_string()),
    })
}

pub fn qrcode_svg(content: &str) -> Result<String, AuthError> {
    use qrcode::render::svg;
    let code = qrcode::QrCode::new(content.as_bytes()).map_err(|e| AuthError::QrEncode(e.to_string()))?;
    Ok(code
        .render::<svg::Color>()
        .min_dimensions(240, 240)
        .dark_color(svg::Color("#111827"))
        .light_color(svg::Color("#ffffff"))
        .build())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn fetch_qrcode_hits_get_bot_qrcode_with_bot_type_3() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ilink/bot/get_bot_qrcode"))
            .and(query_param("bot_type", "3"))
            .and(header("iLink-App-Id", "bot"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "qrcode": "qrc_1",
                "qrcode_img_content": "https://liteapp.weixin.qq.com/q/abc"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let qr = fetch_qrcode(&http, &server.uri()).await.unwrap();
        assert_eq!(qr.qrcode, "qrc_1");
        assert_eq!(qr.content, "https://liteapp.weixin.qq.com/q/abc");
    }

    #[tokio::test]
    async fn poll_maps_statuses() {
        let server = MockServer::start().await;
        let http = reqwest::Client::new();

        for (status, expected) in [
            ("wait", QrStatus::Wait),
            ("scaned", QrStatus::Scanned),
            ("expired", QrStatus::Expired),
            ("weird", QrStatus::Unknown("weird".into())),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/ilink/bot/get_qrcode_status"))
                .and(query_param("qrcode", "qrc_1"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(serde_json::json!({ "status": status })),
                )
                .mount(&server)
                .await;
            assert_eq!(poll_qrcode_status(&http, &server.uri(), "qrc_1").await.unwrap(), expected);
        }

        Mock::given(method("GET"))
            .and(path("/ilink/bot/get_qrcode_status"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "confirmed",
                "bot_token": "t",
                "ilink_bot_id": "b@im.bot",
                "ilink_user_id": "u@im.wechat",
                "baseurl": "https://other.weixin.qq.com"
            })))
            .mount(&server)
            .await;
        let got = poll_qrcode_status(&http, &server.uri(), "qrc_1").await.unwrap();
        assert_eq!(
            got,
            QrStatus::Confirmed(Credentials {
                bot_token: "t".into(),
                ilink_bot_id: "b@im.bot".into(),
                ilink_user_id: "u@im.wechat".into(),
                baseurl: "https://other.weixin.qq.com".into(),
            })
        );
    }

    #[tokio::test]
    async fn confirmed_without_baseurl_falls_back_to_requested_base() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "confirmed", "bot_token": "t", "ilink_bot_id": "b", "ilink_user_id": "u"
            })))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        match poll_qrcode_status(&http, &server.uri(), "q").await.unwrap() {
            QrStatus::Confirmed(c) => assert_eq!(c.baseurl, server.uri()),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn confirmed_without_bot_token_is_bad_response() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "confirmed", "ilink_bot_id": "b", "ilink_user_id": "u"
            })))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        assert_eq!(
            poll_qrcode_status(&http, &server.uri(), "q").await,
            Err(AuthError::BadResponse("confirmed without bot_token".into()))
        );
    }

    #[tokio::test]
    async fn http_error_is_bad_response() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        assert!(matches!(
            fetch_qrcode(&http, &server.uri()).await,
            Err(AuthError::BadResponse(_))
        ));
    }

    #[test]
    fn svg_contains_svg_tag() {
        let svg = qrcode_svg("https://liteapp.weixin.qq.com/q/abc").unwrap();
        assert!(svg.starts_with("<?xml") || svg.starts_with("<svg"));
        assert!(svg.contains("<svg"));
    }
}
