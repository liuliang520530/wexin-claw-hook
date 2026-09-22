use serde::{Deserialize, Serialize};

/// 一个企业微信自建应用 = 一条发送通道。`name` 是备注名，webhook `app` 字段与日志 `from` 都用它。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WecomApp {
    pub name: String,
    pub corpid: String,
    pub agentid: u32,
    /// 明文存储，与 bot_token 同等对待；不通过 list 命令返回前端。
    pub secret: String,
    #[serde(default)]
    pub token: Option<CachedToken>,
    /// Some(原因) = 凭据无效：发送直接拒绝、UI 提示；编辑凭据并重新验证通过后清除。
    #[serde(default)]
    pub invalid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CachedToken {
    pub access_token: String,
    /// Unix 秒 = 获取时刻 + expires_in
    pub expires_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WecomFailure {
    NotConfigured,
    UnknownApp(String),
    InvalidCredentials(String),
    UnknownRecipient(String),
    RateLimited,
    Upstream(String),
}

impl WecomFailure {
    pub fn code(&self) -> &'static str {
        match self {
            WecomFailure::NotConfigured => "not_configured",
            WecomFailure::UnknownApp(_) => "unknown_app",
            WecomFailure::InvalidCredentials(_) => "invalid_credentials",
            WecomFailure::UnknownRecipient(_) => "unknown_recipient",
            WecomFailure::RateLimited => "rate_limited",
            WecomFailure::Upstream(_) => "upstream_error",
        }
    }

    pub fn message(&self) -> String {
        match self {
            WecomFailure::NotConfigured => "尚未添加任何企业微信应用，请到「账号 → 企业微信」添加".to_string(),
            WecomFailure::UnknownApp(n) => format!("应用「{n}」不存在，app 字段须为账号页里的应用备注名"),
            WecomFailure::InvalidCredentials(r) => format!("应用凭据无效：{r}，请到账号页检查 corpid / agentid / secret"),
            WecomFailure::UnknownRecipient(to) => format!("收件人 {to} 全部无效或不在应用可见范围内"),
            WecomFailure::RateLimited => "企业微信频率限制（每应用对同一成员 30 次/分钟、1000 次/小时），稍后重试".to_string(),
            WecomFailure::Upstream(m) => m.clone(),
        }
    }
}

// ---- errcode 分类（spec §2 表）----

/// access_token 不合法 / 已过期：刷新后重试一次。
pub fn is_token_stale(code: i64) -> bool {
    matches!(code, 40014 | 42001)
}

/// 配置本身错了：标失效，重试无意义。
pub fn is_credential_error(code: i64) -> bool {
    matches!(code, 40001 | 40091 | 40013 | 40056 | 301002)
}

/// 收件人全部无效 / 无权限。
pub fn is_recipient_error(code: i64) -> bool {
    matches!(code, 40003 | 60111 | 60011 | 81013 | 82001)
}

pub fn is_rate_limit(code: i64) -> bool {
    matches!(code, 45009 | 45033)
}

/// 用户可读的错误文案；同时用于 `invalid` 字段与添加应用时的报错。
pub fn describe_error(code: i64, msg: &str, agentid: u32) -> String {
    match code {
        40013 => "corpid 不合法（40013）".to_string(),
        40001 | 40091 => format!("secret 不合法（{code}）"),
        40056 => "agentid 不合法（40056）".to_string(),
        301002 => format!("该 secret 无权限操作 agentid {agentid}（301002），secret 与应用不匹配"),
        60020 => format!("企业微信拒绝了本机 IP（60020）：{msg}"),
        _ => format!("errcode={code} {msg}"),
    }
}

/// `to` 规范化：空/缺省 → "@all"；否则按 `|` 拆分校验后重新拼接。Err 为 400 文案。
/// routes 层与 wecom_send_test 命令共用这一处实现。
pub fn normalize_to(raw: Option<&str>) -> Result<String, String> {
    let s = raw.map(str::trim).unwrap_or("");
    if s.is_empty() || s == "@all" {
        return Ok("@all".to_string());
    }
    let mut parts = Vec::new();
    for p in s.split('|') {
        let p = p.trim();
        if p.is_empty() {
            return Err("to 含空的收件人".to_string());
        }
        if p == "@all" {
            return Err("@all 不能与 UserID 混用".to_string());
        }
        if p.chars().any(char::is_whitespace) {
            return Err(format!("UserID「{p}」不能含空格"));
        }
        parts.push(p);
    }
    Ok(parts.join("|"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_to_defaults_to_all_and_joins_users() {
        assert_eq!(normalize_to(None).unwrap(), "@all");
        assert_eq!(normalize_to(Some("  ")).unwrap(), "@all");
        assert_eq!(normalize_to(Some("@all")).unwrap(), "@all");
        assert_eq!(normalize_to(Some(" a | b ")).unwrap(), "a|b");
        assert_eq!(normalize_to(Some("zhangsan")).unwrap(), "zhangsan");
    }

    #[test]
    fn normalize_to_rejects_bad_shapes() {
        assert!(normalize_to(Some("a||b")).unwrap_err().contains("空"));
        assert!(normalize_to(Some("a|")).is_err());
        assert!(normalize_to(Some("@all|a")).unwrap_err().contains("@all"));
        assert!(normalize_to(Some("zhang san")).unwrap_err().contains("空格"));
    }

    #[test]
    fn errcode_classes_do_not_overlap() {
        for c in [40014, 42001] {
            assert!(is_token_stale(c) && !is_credential_error(c));
        }
        for c in [40001, 40091, 40013, 40056, 301002] {
            assert!(is_credential_error(c) && !is_recipient_error(c));
        }
        for c in [40003, 60111, 60011, 81013, 82001] {
            assert!(is_recipient_error(c));
        }
        assert!(is_rate_limit(45009) && is_rate_limit(45033));
        assert!(!is_rate_limit(-1) && !is_credential_error(-1) && !is_recipient_error(-1));
    }

    #[test]
    fn describe_error_mentions_code_and_field() {
        assert!(describe_error(40013, "", 1).contains("corpid"));
        assert!(describe_error(40091, "", 1).contains("40091"));
        assert!(describe_error(301002, "", 1000002).contains("1000002"));
        assert!(describe_error(60020, "from ip 1.2.3.4", 1).contains("1.2.3.4"));
        assert_eq!(describe_error(-1, "system busy", 1), "errcode=-1 system busy");
    }

    #[test]
    fn failure_codes_are_stable() {
        assert_eq!(WecomFailure::NotConfigured.code(), "not_configured");
        assert_eq!(WecomFailure::UnknownApp("x".into()).code(), "unknown_app");
        assert_eq!(WecomFailure::InvalidCredentials("x".into()).code(), "invalid_credentials");
        assert_eq!(WecomFailure::UnknownRecipient("x".into()).code(), "unknown_recipient");
        assert_eq!(WecomFailure::RateLimited.code(), "rate_limited");
        assert_eq!(WecomFailure::Upstream("x".into()).code(), "upstream_error");
        assert!(WecomFailure::UnknownApp("ops".into()).message().contains("ops"));
    }

    #[test]
    fn app_deserializes_without_optional_fields() {
        let a: WecomApp = serde_json::from_str(
            r#"{"name":"n","corpid":"ww1","agentid":1000002,"secret":"s"}"#,
        )
        .unwrap();
        assert!(a.token.is_none());
        assert!(a.invalid.is_none());
    }
}
