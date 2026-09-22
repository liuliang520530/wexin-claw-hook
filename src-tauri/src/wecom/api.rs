use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ApiError {
    /// 企业微信返回 errcode != 0
    #[error("errcode={code} {msg}")]
    Code { code: i64, msg: String },
    /// 网络错误、非 2xx、非 JSON
    #[error("{0}")]
    Http(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub access_token: String,
    pub expires_in: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SendResult {
    pub msgid: String,
    /// invaliduser ∪ unlicenseduser，按 | 拆分
    pub invalid_users: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AgentInfo {
    pub agentid: u32,
    pub name: String,
    pub close: bool,
}

/// 三个接口共用的响应外壳；不同接口只用到其中一部分字段。
#[derive(Debug, Default, Deserialize)]
struct Raw {
    #[serde(default)]
    errcode: i64,
    #[serde(default)]
    errmsg: String,
    #[serde(default)]
    access_token: String,
    #[serde(default)]
    expires_in: i64,
    #[serde(default)]
    msgid: String,
    #[serde(default)]
    invaliduser: String,
    #[serde(default)]
    unlicenseduser: String,
    #[serde(default)]
    agentid: u32,
    #[serde(default)]
    name: String,
    #[serde(default)]
    close: i64,
}

pub struct WecomApi {
    http: reqwest::Client,
    base_url: String,
}

impl WecomApi {
    pub fn new(http: reqwest::Client, base_url: impl Into<String>) -> Self {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        Self { http, base_url }
    }

    pub async fn get_token(&self, corpid: &str, secret: &str) -> Result<Token, ApiError> {
        let url = self.url("/cgi-bin/gettoken", &[("corpid", corpid), ("corpsecret", secret)])?;
        let raw = self.call(self.http.get(url)).await?;
        Ok(Token { access_token: raw.access_token, expires_in: raw.expires_in })
    }

    pub async fn send_text(
        &self,
        token: &str,
        agentid: u32,
        touser: &str,
        content: &str,
    ) -> Result<SendResult, ApiError> {
        let body = json!({
            "touser": touser,
            "msgtype": "text",
            "agentid": agentid,
            "text": { "content": content },
        });
        let url = self.url("/cgi-bin/message/send", &[("access_token", token)])?;
        let raw = self.call(self.http.post(url).json(&body)).await?;
        let mut invalid_users = split_users(&raw.invaliduser);
        invalid_users.extend(split_users(&raw.unlicenseduser));
        Ok(SendResult { msgid: raw.msgid, invalid_users })
    }

    pub async fn get_agent(&self, token: &str, agentid: u32) -> Result<AgentInfo, ApiError> {
        let agentid_s = agentid.to_string();
        let url = self.url(
            "/cgi-bin/agent/get",
            &[("access_token", token), ("agentid", agentid_s.as_str())],
        )?;
        let raw = self.call(self.http.get(url)).await?;
        Ok(AgentInfo { agentid: raw.agentid, name: raw.name, close: raw.close == 1 })
    }

    /// 本项目 reqwest 未启用 `query` feature，用 Url::parse_with_params 拼查询串（自动百分号编码）。
    fn url(&self, path: &str, params: &[(&str, &str)]) -> Result<reqwest::Url, ApiError> {
        reqwest::Url::parse_with_params(&format!("{}{}", self.base_url, path), params)
            .map_err(|e| ApiError::Http(format!("invalid url: {e}")))
    }

    /// secret / access_token 都在 URL 查询串里：reqwest 错误一律先 without_url() 再转字符串。
    async fn call(&self, req: reqwest::RequestBuilder) -> Result<Raw, ApiError> {
        let resp = req
            .send()
            .await
            .map_err(|e| ApiError::Http(e.without_url().to_string()))?;
        let status = resp.status();
        let body = resp
            .text()
            .await
            .map_err(|e| ApiError::Http(e.without_url().to_string()))?;
        if !status.is_success() {
            return Err(ApiError::Http(format!("HTTP {}: {}", status.as_u16(), truncate(&body, 200))));
        }
        let raw: Raw = serde_json::from_str(&body)
            .map_err(|_| ApiError::Http(format!("invalid JSON: {}", truncate(&body, 200))))?;
        if raw.errcode != 0 {
            return Err(ApiError::Code { code: raw.errcode, msg: raw.errmsg });
        }
        Ok(raw)
    }
}

fn split_users(s: &str) -> Vec<String> {
    s.split('|')
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(str::to_string)
        .collect()
}

fn truncate(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ilink::client::http_client;
    use wiremock::matchers::{body_partial_json, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn api(base: &str) -> WecomApi {
        WecomApi::new(http_client(), base)
    }

    #[tokio::test]
    async fn get_token_sends_corpid_secret_and_parses_expiry() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/cgi-bin/gettoken"))
            .and(query_param("corpid", "ww1"))
            .and(query_param("corpsecret", "sec"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "errcode": 0, "errmsg": "ok", "access_token": "tok", "expires_in": 7200
            })))
            .expect(1)
            .mount(&server)
            .await;
        let t = api(&server.uri()).get_token("ww1", "sec").await.unwrap();
        assert_eq!(t, Token { access_token: "tok".into(), expires_in: 7200 });
    }

    #[tokio::test]
    async fn nonzero_errcode_is_code_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "errcode": 40001, "errmsg": "invalid credential"
            })))
            .mount(&server)
            .await;
        assert_eq!(
            api(&server.uri()).get_token("ww1", "bad").await,
            Err(ApiError::Code { code: 40001, msg: "invalid credential".into() })
        );
    }

    #[tokio::test]
    async fn non_2xx_and_bad_json_are_http_errors() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500).set_body_string("oops"))
            .mount(&server)
            .await;
        match api(&server.uri()).get_token("ww1", "s").await {
            Err(ApiError::Http(m)) => assert!(m.contains("500")),
            other => panic!("unexpected {other:?}"),
        }

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>"))
            .mount(&server)
            .await;
        assert!(matches!(api(&server.uri()).get_token("ww1", "s").await, Err(ApiError::Http(_))));
    }

    #[tokio::test]
    async fn connection_error_message_does_not_leak_secret() {
        let secret = "SuperSecretValue123";
        match api("http://127.0.0.1:1").get_token("ww1", secret).await {
            Err(ApiError::Http(m)) => assert!(!m.contains(secret), "错误文案泄露了 secret：{m}"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn send_text_posts_expected_body_and_merges_invalid_users() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/cgi-bin/message/send"))
            .and(query_param("access_token", "tok"))
            .and(body_partial_json(json!({
                "touser": "a|b",
                "msgtype": "text",
                "agentid": 1000002,
                "text": { "content": "hi" }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "errcode": 0, "errmsg": "ok",
                "invaliduser": "a", "unlicenseduser": "c|d", "msgid": "m1"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let r = api(&server.uri()).send_text("tok", 1000002, "a|b", "hi").await.unwrap();
        assert_eq!(r.msgid, "m1");
        assert_eq!(r.invalid_users, vec!["a", "c", "d"]);
    }

    #[tokio::test]
    async fn send_text_success_without_invalid_fields() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"errcode": 0, "msgid": "m"})))
            .mount(&server)
            .await;
        let r = api(&server.uri()).send_text("tok", 1, "@all", "hi").await.unwrap();
        assert!(r.invalid_users.is_empty());
    }

    #[tokio::test]
    async fn get_agent_parses_name_and_close() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/cgi-bin/agent/get"))
            .and(query_param("access_token", "tok"))
            .and(query_param("agentid", "1000002"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "errcode": 0, "agentid": 1000002, "name": "运维告警", "close": 1
            })))
            .mount(&server)
            .await;
        let a = api(&server.uri()).get_agent("tok", 1000002).await.unwrap();
        assert_eq!(a, AgentInfo { agentid: 1000002, name: "运维告警".into(), close: true });
    }
}
