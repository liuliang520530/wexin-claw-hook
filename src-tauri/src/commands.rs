use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::State;

use crate::ilink::auth::{self, AuthError, QrStatus};
use crate::ilink::types::is_ilink_user_id;
use crate::ilink::DEFAULT_BASE_URL;
use crate::server;
use crate::state::{AppState, LoginSession, LoginStatus};
use crate::store::{Config, LogEntry, Recipient};

#[derive(Debug, Clone, Serialize)]
pub struct StatusInfo {
    pub logged_in: bool,
    pub bot_id: Option<String>,
    pub user_id: Option<String>,
    pub server_running: bool,
    pub port: u16,
    pub version: String,
    /// 上次发送因 token 过期被登出（需重新扫码）
    pub token_expired: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LoginView {
    pub svg: String,
    pub status: LoginStatus,
}

async fn status(state: &AppState) -> StatusInfo {
    let creds = state.creds.read().await.clone();
    let cfg_port = state.config.read().await.port;
    let token_expired = *state.token_expired.read().await;
    let server = state.server.lock().await;
    StatusInfo {
        logged_in: creds.is_some(),
        bot_id: creds.as_ref().map(|c| c.ilink_bot_id.clone()),
        user_id: creds.as_ref().map(|c| c.ilink_user_id.clone()),
        server_running: server.is_some(),
        port: server.as_ref().map(|h| h.port).unwrap_or(cfg_port),
        version: env!("CARGO_PKG_VERSION").to_string(),
        token_expired,
    }
}

pub async fn stop_server(state: &AppState) {
    if let Some(h) = state.server.lock().await.take() {
        h.stop();
    }
}

/// 按指定端口尝试绑定；刚关闭的端口可能要几百毫秒才释放，所以带重试。
async fn bind_with_retry(state: &Arc<AppState>, port: u16) -> Result<server::ServerHandle, String> {
    let mut last = String::new();
    for _ in 0..20 {
        match server::start(state.clone(), port).await {
            Ok(h) => return Ok(h),
            Err(e) => {
                last = e.to_string();
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
    Err(format!("端口 {port} 启动失败：{last}"))
}

/// 先停旧的再按配置端口启动。
pub async fn start_server(state: &Arc<AppState>) -> Result<u16, String> {
    stop_server(state).await;
    let port = state.config.read().await.port;
    let h = bind_with_retry(state, port).await?;
    let p = h.port;
    *state.server.lock().await = Some(h);
    Ok(p)
}

/// 切换端口：先绑定成功再写入配置；绑定失败则恢复原配置端口的服务并返回错误。
pub async fn apply_port(state: &Arc<AppState>, port: u16) -> Result<u16, String> {
    if port == 0 {
        return Err("端口必须在 1-65535 之间".into());
    }
    stop_server(state).await;
    match bind_with_retry(state, port).await {
        Ok(h) => {
            let p = h.port;
            *state.server.lock().await = Some(h);
            let mut cfg = state.config.write().await;
            cfg.port = port;
            state.store.save_config(&cfg).map_err(|e| e.to_string())?;
            Ok(p)
        }
        Err(e) => {
            if let Err(r) = start_server(state).await {
                return Err(format!("{e}；恢复原端口失败：{r}"));
            }
            Err(e)
        }
    }
}

// ---------- 登录 ----------

/// 只在当前会话仍是同一个二维码时更新状态；返回 false 表示会话已被替换，轮询应退出。
async fn set_login_status(state: &AppState, qrcode: &str, status: LoginStatus) -> bool {
    let mut g = state.login.write().await;
    match g.as_mut() {
        Some(s) if s.qrcode == qrcode => {
            s.status = status;
            true
        }
        _ => false,
    }
}

/// 当前登录会话是否仍是这个二维码；false 表示已被新的 login_start 替换或已登出。
async fn is_current_session(state: &AppState, qrcode: &str) -> bool {
    matches!(state.login.read().await.as_ref(), Some(s) if s.qrcode == qrcode)
}

async fn poll_login(state: Arc<AppState>, qrcode: String) {
    let mut net_errors = 0u32;
    loop {
        if !is_current_session(&state, &qrcode).await {
            return;
        }
        let polled = auth::poll_qrcode_status(&state.http, DEFAULT_BASE_URL, &qrcode).await;
        if polled.is_ok() {
            net_errors = 0;
        }
        let next = match polled {
            Ok(QrStatus::Wait) => Some(LoginStatus::Wait),
            Ok(QrStatus::Scanned) => Some(LoginStatus::Scanned),
            Ok(QrStatus::Unknown(_)) => None,
            Ok(QrStatus::Expired) => {
                set_login_status(&state, &qrcode, LoginStatus::Expired).await;
                return;
            }
            Ok(QrStatus::Confirmed(creds)) => {
                // 长轮询期间会话可能已被替换：不是当前会话就不保存凭据
                if !is_current_session(&state, &qrcode).await {
                    return;
                }
                let st = match state.set_credentials(Some(creds)).await {
                    Ok(()) => {
                        *state.token_expired.write().await = false;
                        LoginStatus::Confirmed
                    }
                    Err(e) => LoginStatus::Error { message: format!("保存凭据失败：{e}") },
                };
                set_login_status(&state, &qrcode, st).await;
                return;
            }
            Err(AuthError::Network(m)) => {
                net_errors += 1;
                if net_errors >= 5 {
                    set_login_status(&state, &qrcode, LoginStatus::Error { message: m }).await;
                    return;
                }
                None
            }
            Err(e) => {
                set_login_status(&state, &qrcode, LoginStatus::Error { message: e.to_string() }).await;
                return;
            }
        };
        if let Some(s) = next {
            if !set_login_status(&state, &qrcode, s).await {
                return;
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

#[tauri::command]
pub async fn login_start(state: State<'_, Arc<AppState>>) -> Result<LoginView, String> {
    let qr = auth::fetch_qrcode(&state.http, DEFAULT_BASE_URL)
        .await
        .map_err(|e| e.to_string())?;
    let svg = auth::qrcode_svg(&qr.content).map_err(|e| e.to_string())?;
    *state.login.write().await = Some(LoginSession {
        qrcode: qr.qrcode.clone(),
        svg: svg.clone(),
        status: LoginStatus::Wait,
    });
    *state.token_expired.write().await = false;
    tauri::async_runtime::spawn(poll_login(state.inner().clone(), qr.qrcode));
    Ok(LoginView { svg, status: LoginStatus::Wait })
}

#[tauri::command]
pub async fn login_status(state: State<'_, Arc<AppState>>) -> Result<Option<LoginView>, String> {
    Ok(state.login.read().await.as_ref().map(|s| LoginView {
        svg: s.svg.clone(),
        status: s.status.clone(),
    }))
}

#[tauri::command]
pub async fn logout(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.set_credentials(None).await.map_err(|e| e.to_string())?;
    *state.login.write().await = None;
    *state.token_expired.write().await = false;
    Ok(())
}

// ---------- 状态 / 配置 ----------

#[tauri::command]
pub async fn get_status(state: State<'_, Arc<AppState>>) -> Result<StatusInfo, String> {
    Ok(status(&state).await)
}

#[tauri::command]
pub async fn get_config(state: State<'_, Arc<AppState>>) -> Result<Config, String> {
    Ok(state.config.read().await.clone())
}

#[tauri::command]
pub async fn save_recipients(
    state: State<'_, Arc<AppState>>,
    recipients: Vec<Recipient>,
    default_recipient: Option<String>,
) -> Result<Config, String> {
    let recipients: Vec<Recipient> = recipients
        .into_iter()
        .map(|r| Recipient { id: r.id.trim().to_string(), name: r.name.trim().to_string() })
        .filter(|r| !r.id.is_empty())
        .collect();
    let mut cfg = state.config.write().await;
    validate_new_recipients(&cfg.recipients, &recipients)?;
    cfg.recipients = recipients;
    let known: Vec<String> = cfg.recipients.iter().map(|r| r.id.clone()).collect();
    cfg.default_recipient = default_recipient
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && known.contains(s));
    state.store.save_config(&cfg).map_err(|e| e.to_string())?;
    Ok(cfg.clone())
}

#[tauri::command]
pub async fn set_port(state: State<'_, Arc<AppState>>, port: u16) -> Result<StatusInfo, String> {
    apply_port(state.inner(), port).await?;
    Ok(status(&state).await)
}

#[tauri::command]
pub async fn reset_api_key(state: State<'_, Arc<AppState>>) -> Result<String, String> {
    let mut cfg = state.config.write().await;
    cfg.api_key = crate::store::generate_api_key();
    state.store.save_config(&cfg).map_err(|e| e.to_string())?;
    Ok(cfg.api_key.clone())
}

#[tauri::command]
pub async fn server_start(state: State<'_, Arc<AppState>>) -> Result<StatusInfo, String> {
    start_server(state.inner()).await?;
    Ok(status(&state).await)
}

#[tauri::command]
pub async fn server_stop(state: State<'_, Arc<AppState>>) -> Result<StatusInfo, String> {
    stop_server(&state).await;
    Ok(status(&state).await)
}

#[tauri::command]
pub async fn list_logs(state: State<'_, Arc<AppState>>) -> Result<Vec<LogEntry>, String> {
    Ok(state.store.load_logs())
}

/// 发消息页用：直接走 sender（不经 webhook/鉴权），日志与 webhook 发送完全一致。
#[tauri::command]
pub async fn send_test(
    state: State<'_, Arc<AppState>>,
    to: String,
    text: String,
) -> Result<String, String> {
    if !is_ilink_user_id(to.trim()) {
        return Err("收件人 ID 必须是 iLink 用户 ID（形如 xxx@im.wechat），不是微信号/wxid".into());
    }
    crate::sender::send_text(&state, Some(&to), &text)
        .await
        .map_err(|f| f.message())
}

/// 只校验新增的收件人 ID；已存在的记录（含历史上录入的无效 ID）允许保留或删除，
/// 避免一条无效记录卡住整个列表的保存。
pub fn validate_new_recipients(existing: &[Recipient], incoming: &[Recipient]) -> Result<(), String> {
    let is_new = |r: &Recipient| !existing.iter().any(|e| e.id == r.id);
    if let Some(bad) = incoming.iter().filter(|r| is_new(r)).find(|r| !is_ilink_user_id(&r.id)) {
        return Err(format!(
            "收件人 ID「{}」不是 iLink 用户 ID（形如 xxx@im.wechat），不是微信号/wxid；该 ID 只能从对方发给机器人的消息中获得",
            bad.id
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn recipient_validation_only_applies_to_new_ids() {
        use crate::store::Recipient;
        let r = |id: &str| Recipient { id: id.into(), name: id.into() };
        let old_bad = vec![r("liuliangzheng")];
        // 旧的无效记录：保留可以，删除也可以
        assert!(super::validate_new_recipients(&old_bad, &old_bad).is_ok());
        assert!(super::validate_new_recipients(&old_bad, &[]).is_ok());
        // 新增合法 ID 可以
        assert!(super::validate_new_recipients(&old_bad, &[r("liuliangzheng"), r("o9cq8abc@im.wechat")]).is_ok());
        // 新增微信号被拒
        let err = super::validate_new_recipients(&[], &[r("wxid_abc")]).unwrap_err();
        assert!(err.contains("wxid_abc") && err.contains("im.wechat"));
    }

    use super::*;
    use crate::store::Store;

    #[tokio::test]
    async fn apply_port_keeps_config_and_restores_server_when_bind_fails() {
        let d = tempfile::tempdir().unwrap();
        let state = AppState::new(Store::new(d.path().to_path_buf()));
        state.config.write().await.port = 0;
        start_server(&state).await.unwrap();

        // 占住一个端口，使新端口绑定必然失败
        let busy = std::net::TcpListener::bind("0.0.0.0:0").unwrap();
        let p = busy.local_addr().unwrap().port();

        assert!(apply_port(&state, p).await.is_err());
        assert_eq!(state.config.read().await.port, 0, "绑定失败不应写入配置");
        assert!(state.server.lock().await.is_some(), "应恢复原端口的服务");
        drop(busy);
    }
}
