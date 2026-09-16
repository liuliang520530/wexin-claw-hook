use serde::{Deserialize, Serialize};

use super::{CHANNEL_VERSION, ILINK_APP_ID};

/// 扫码登录后拿到并持久化的凭据。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Credentials {
    pub bot_token: String,
    pub ilink_bot_id: String,
    pub ilink_user_id: String,
    pub baseurl: String,
}

/// "2.4.6" -> 0x00020406，与官方包 iLink-App-ClientVersion 一致。
pub fn encode_client_version(version: &str) -> u32 {
    let mut parts = version.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    let major = parts.next().unwrap_or(0) & 0xff;
    let minor = parts.next().unwrap_or(0) & 0xff;
    let patch = parts.next().unwrap_or(0) & 0xff;
    (major << 16) | (minor << 8) | patch
}

/// X-WECHAT-UIN：随机 uint32 -> 十进制字符串 -> base64。
pub fn random_uin() -> String {
    use base64::Engine;
    let n: u32 = rand::random();
    base64::engine::general_purpose::STANDARD.encode(n.to_string())
}

/// 每次请求都重新生成（UIN 需随机）。token 为 None 时用于扫码阶段的匿名请求。
pub fn request_headers(token: Option<&str>) -> Vec<(&'static str, String)> {
    let mut h = vec![
        ("Content-Type", "application/json".to_string()),
        ("X-WECHAT-UIN", random_uin()),
        ("iLink-App-Id", ILINK_APP_ID.to_string()),
        (
            "iLink-App-ClientVersion",
            encode_client_version(CHANNEL_VERSION).to_string(),
        ),
    ];
    if let Some(t) = token {
        h.push(("AuthorizationType", "ilink_bot_token".to_string()));
        h.push(("Authorization", format!("Bearer {t}")));
    }
    h
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseInfo {
    pub channel_version: String,
    pub bot_agent: String,
}

pub fn base_info() -> BaseInfo {
    BaseInfo {
        channel_version: CHANNEL_VERSION.to_string(),
        bot_agent: format!("weixin-clawbot-webhook/{}", env!("CARGO_PKG_VERSION")),
    }
}

pub const MESSAGE_TYPE_BOT: u8 = 2;
pub const MESSAGE_STATE_FINISH: u8 = 2;
pub const ITEM_TYPE_TEXT: u8 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextItem {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageItem {
    #[serde(rename = "type")]
    pub item_type: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_item: Option<TextItem>,
}

impl MessageItem {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            item_type: ITEM_TYPE_TEXT,
            text_item: Some(TextItem { text: text.into() }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMsg {
    pub from_user_id: String,
    pub to_user_id: String,
    pub client_id: String,
    pub message_type: u8,
    pub message_state: u8,
    pub item_list: Vec<MessageItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageRequest {
    pub msg: SendMsg,
    pub base_info: BaseInfo,
}

impl SendMessageRequest {
    pub fn new(
        from: &str,
        to: &str,
        client_id: &str,
        items: Vec<MessageItem>,
        context_token: Option<&str>,
    ) -> Self {
        Self {
            msg: SendMsg {
                from_user_id: from.to_string(),
                to_user_id: to.to_string(),
                client_id: client_id.to_string(),
                message_type: MESSAGE_TYPE_BOT,
                message_state: MESSAGE_STATE_FINISH,
                item_list: items,
                context_token: context_token.map(str::to_string),
            },
            base_info: base_info(),
        }
    }

    pub fn text(
        from: &str,
        to: &str,
        client_id: &str,
        text: &str,
        context_token: Option<&str>,
    ) -> Self {
        Self::new(from, to, client_id, vec![MessageItem::text(text)], context_token)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SendMessageResponse {
    #[serde(default)]
    pub ret: i64,
    #[serde(default)]
    pub errmsg: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QrCodeResponse {
    pub qrcode: String,
    pub qrcode_img_content: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct QrStatusResponse {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub bot_token: String,
    #[serde(default)]
    pub ilink_bot_id: String,
    #[serde(default)]
    pub ilink_user_id: String,
    #[serde(default)]
    pub baseurl: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn client_version_encodes_like_official_package() {
        assert_eq!(encode_client_version("2.4.6"), 132102);
        assert_eq!(encode_client_version("1.0.0"), 0x0001_0000);
    }

    #[test]
    fn uin_is_base64_of_decimal_u32() {
        let uin = random_uin();
        let raw = base64::engine::general_purpose::STANDARD.decode(&uin).unwrap();
        let s = String::from_utf8(raw).unwrap();
        let n: u64 = s.parse().unwrap();
        assert!(n <= u32::MAX as u64);
    }

    #[test]
    fn headers_include_auth_and_protocol_fields() {
        let h = request_headers(Some("tok"));
        let get = |k: &str| h.iter().find(|(n, _)| *n == k).map(|(_, v)| v.clone());
        assert_eq!(get("Content-Type").as_deref(), Some("application/json"));
        assert_eq!(get("AuthorizationType").as_deref(), Some("ilink_bot_token"));
        assert_eq!(get("Authorization").as_deref(), Some("Bearer tok"));
        assert_eq!(get("iLink-App-Id").as_deref(), Some("bot"));
        assert_eq!(get("iLink-App-ClientVersion").as_deref(), Some("132102"));
        assert!(get("X-WECHAT-UIN").is_some());

        let anon = request_headers(None);
        assert!(anon.iter().all(|(n, _)| *n != "Authorization"));
    }

    #[test]
    fn send_request_serializes_and_omits_none_context_token() {
        let req = SendMessageRequest::text("bot@im.bot", "u@im.wechat", "cid-1", "你好", None);
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["msg"]["from_user_id"], "bot@im.bot");
        assert_eq!(v["msg"]["to_user_id"], "u@im.wechat");
        assert_eq!(v["msg"]["client_id"], "cid-1");
        assert_eq!(v["msg"]["message_type"], 2);
        assert_eq!(v["msg"]["message_state"], 2);
        assert_eq!(v["msg"]["item_list"][0]["type"], 1);
        assert_eq!(v["msg"]["item_list"][0]["text_item"]["text"], "你好");
        assert!(v["msg"].get("context_token").is_none());
        assert_eq!(v["base_info"]["channel_version"], "2.4.6");

        let req2 = SendMessageRequest::text("b", "u", "c", "t", Some("ctx"));
        let v2 = serde_json::to_value(&req2).unwrap();
        assert_eq!(v2["msg"]["context_token"], "ctx");
    }

    #[test]
    fn responses_deserialize_with_missing_fields() {
        let r: SendMessageResponse = serde_json::from_str("{}").unwrap();
        assert_eq!(r.ret, 0);
        let r: SendMessageResponse =
            serde_json::from_str(r#"{"ret":-14,"errmsg":"expired"}"#).unwrap();
        assert_eq!(r.ret, -14);
        let q: QrStatusResponse = serde_json::from_str(r#"{"status":"wait"}"#).unwrap();
        assert_eq!(q.status, "wait");
        assert_eq!(q.bot_token, "");
    }
}
