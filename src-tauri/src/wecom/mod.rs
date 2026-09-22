pub mod api;
pub mod types;

pub const BASE_URL: &str = "https://qyapi.weixin.qq.com";
/// 距 access_token 过期不足这么多秒就提前刷新。
pub const TOKEN_SAFETY_MARGIN_SECS: i64 = 300;
