use std::time::Duration;

use super::types::{request_headers, Credentials, MessageItem, SendMessageRequest, SendMessageResponse};

pub const SEND_TIMEOUT: Duration = Duration::from_secs(15);

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(SEND_TIMEOUT)
        .build()
        .expect("reqwest client")
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SendError {
    #[error("rate limited (ret=-2)")]
    RateLimited,
    #[error("token expired (ret=-14)")]
    TokenExpired,
    #[error("upstream error: {0}")]
    Upstream(String),
}

pub struct IlinkClient {
    http: reqwest::Client,
    creds: Credentials,
}

impl IlinkClient {
    pub fn new(http: reqwest::Client, creds: Credentials) -> Self {
        Self { http, creds }
    }

    pub async fn send_text(
        &self,
        to: &str,
        text: &str,
        context_token: Option<&str>,
    ) -> Result<String, SendError> {
        self.send_items(to, vec![MessageItem::text(text)], context_token).await
    }

    /// 通用入口，以后加图片/文件 item 也走这里。返回 client_id。
    pub async fn send_items(
        &self,
        to: &str,
        items: Vec<MessageItem>,
        context_token: Option<&str>,
    ) -> Result<String, SendError> {
        let client_id = uuid::Uuid::new_v4().to_string();
        let req = SendMessageRequest::new(&self.creds.ilink_bot_id, to, &client_id, items, context_token);
        let url = format!("{}/ilink/bot/sendmessage", self.creds.baseurl.trim_end_matches('/'));

        let mut builder = self.http.post(&url);
        for (k, v) in request_headers(Some(&self.creds.bot_token)) {
            builder = builder.header(k, v);
        }
        let resp = builder
            .json(&req)
            .send()
            .await
            .map_err(|e| SendError::Upstream(e.to_string()))?;

        let status = resp.status();
        let body = resp.text().await.map_err(|e| SendError::Upstream(e.to_string()))?;
        if !status.is_success() {
            return Err(SendError::Upstream(format!("HTTP {}: {}", status.as_u16(), truncate(&body, 200))));
        }
        if body.trim().is_empty() {
            return Ok(client_id);
        }
        let parsed: SendMessageResponse = serde_json::from_str(&body)
            .map_err(|_| SendError::Upstream(format!("invalid JSON: {}", truncate(&body, 200))))?;
        match parsed.ret {
            0 => Ok(client_id),
            -2 => Err(SendError::RateLimited),
            -14 => Err(SendError::TokenExpired),
            other => Err(SendError::Upstream(format!("ret={other} {}", parsed.errmsg))),
        }
    }
}

fn truncate(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_partial_json, header, header_exists, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn creds(base: &str) -> Credentials {
        Credentials {
            bot_token: "tok".into(),
            ilink_bot_id: "bot@im.bot".into(),
            ilink_user_id: "me@im.wechat".into(),
            baseurl: base.into(),
        }
    }

    #[tokio::test]
    async fn sends_text_with_protocol_headers_and_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/ilink/bot/sendmessage"))
            .and(header("Authorization", "Bearer tok"))
            .and(header("AuthorizationType", "ilink_bot_token"))
            .and(header("iLink-App-Id", "bot"))
            .and(header("iLink-App-ClientVersion", "132102"))
            .and(header_exists("X-WECHAT-UIN"))
            .and(body_partial_json(serde_json::json!({
                "msg": {
                    "from_user_id": "bot@im.bot",
                    "to_user_id": "u@im.wechat",
                    "message_type": 2,
                    "message_state": 2,
                    "item_list": [{"type": 1, "text_item": {"text": "hi"}}]
                },
                "base_info": {"channel_version": "2.4.6"}
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ret": 0})))
            .expect(1)
            .mount(&server)
            .await;

        let c = IlinkClient::new(http_client(), creds(&server.uri()));
        let id = c.send_text("u@im.wechat", "hi", None).await.unwrap();
        assert!(!id.is_empty());
    }

    #[tokio::test]
    async fn empty_body_counts_as_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let c = IlinkClient::new(http_client(), creds(&server.uri()));
        assert!(c.send_text("u", "hi", None).await.is_ok());
    }

    #[tokio::test]
    async fn maps_ret_codes() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ret": -2})))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        let c = IlinkClient::new(http_client(), creds(&server.uri()));
        assert_eq!(c.send_text("u", "hi", None).await, Err(SendError::RateLimited));

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ret": -14})))
            .mount(&server)
            .await;
        let c = IlinkClient::new(http_client(), creds(&server.uri()));
        assert_eq!(c.send_text("u", "hi", None).await, Err(SendError::TokenExpired));

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"ret": -99, "errmsg": "boom"})),
            )
            .mount(&server)
            .await;
        let c = IlinkClient::new(http_client(), creds(&server.uri()));
        match c.send_text("u", "hi", None).await {
            Err(SendError::Upstream(m)) => assert!(m.contains("-99") && m.contains("boom")),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn non_2xx_and_bad_json_are_upstream_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500).set_body_string("oops"))
            .mount(&server)
            .await;
        let c = IlinkClient::new(http_client(), creds(&server.uri()));
        match c.send_text("u", "hi", None).await {
            Err(SendError::Upstream(m)) => assert!(m.contains("500")),
            other => panic!("unexpected {other:?}"),
        }

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>"))
            .mount(&server)
            .await;
        let c = IlinkClient::new(http_client(), creds(&server.uri()));
        assert!(matches!(c.send_text("u", "hi", None).await, Err(SendError::Upstream(_))));
    }

    #[tokio::test]
    async fn connection_refused_is_upstream_error() {
        let c = IlinkClient::new(http_client(), creds("http://127.0.0.1:1"));
        assert!(matches!(c.send_text("u", "hi", None).await, Err(SendError::Upstream(_))));
    }
}
