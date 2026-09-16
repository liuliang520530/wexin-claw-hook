use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::State;

use crate::ilink::auth::{self, AuthError, QrStatus};
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
}

#[derive(Debug, Clone, Serialize)]
pub struct LoginView {
    pub svg: String,
    pub status: LoginStatus,
}

async fn status(state: &AppState) -> StatusInfo {
    let creds = state.creds.read().await.clone();
    let cfg_port = state.config.read().await.port;
    let server = state.server.lock().await;
    StatusInfo {
        logged_in: creds.is_some(),
        bot_id: creds.as_ref().map(|c| c.ilink_bot_id.clone()),
        user_id: creds.as_ref().map(|c| c.ilink_user_id.clone()),
        server_running: server.is_some(),
        port: server.as_ref().map(|h| h.port).unwrap_or(cfg_port),
        version: env!("CARGO_PKG_VERSION").to_string(),
    }
}

pub async fn stop_server(state: &AppState) {
    if let Some(h) = state.server.lock().await.take() {
        h.stop();
    }
}

/// 先停旧的再按配置端口启动；刚关闭的端口可能要几百毫秒才释放，所以带重试。
pub async fn start_server(state: &Arc<AppState>) -> Result<u16, String> {
    stop_server(state).await;
    let port = state.config.read().await.port;
    let mut last = String::new();
    for _ in 0..20 {
        match server::start(state.clone(), port).await {
            Ok(h) => {
                let p = h.port;
                *state.server.lock().await = Some(h);
                return Ok(p);
            }
            Err(e) => {
                last = e.to_string();
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
    Err(format!("端口 {port} 启动失败：{last}"))
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

async fn poll_login(state: Arc<AppState>, qrcode: String) {
    let mut net_errors = 0u32;
    loop {
        let next = match auth::poll_qrcode_status(&state.http, DEFAULT_BASE_URL, &qrcode).await {
            Ok(QrStatus::Wait) => Some(LoginStatus::Wait),
            Ok(QrStatus::Scanned) => Some(LoginStatus::Scanned),
            Ok(QrStatus::Unknown(_)) => None,
            Ok(QrStatus::Expired) => {
                set_login_status(&state, &qrcode, LoginStatus::Expired).await;
                return;
            }
            Ok(QrStatus::Confirmed(creds)) => {
                let st = match state.set_credentials(Some(creds)).await {
                    Ok(()) => LoginStatus::Confirmed,
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
    tokio::spawn(poll_login(state.inner().clone(), qr.qrcode));
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
    let mut cfg = state.config.write().await;
    cfg.recipients = recipients
        .into_iter()
        .map(|r| Recipient { id: r.id.trim().to_string(), name: r.name.trim().to_string() })
        .filter(|r| !r.id.is_empty())
        .collect();
    let known: Vec<String> = cfg.recipients.iter().map(|r| r.id.clone()).collect();
    cfg.default_recipient = default_recipient
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && known.contains(s));
    state.store.save_config(&cfg).map_err(|e| e.to_string())?;
    Ok(cfg.clone())
}

#[tauri::command]
pub async fn set_port(state: State<'_, Arc<AppState>>, port: u16) -> Result<StatusInfo, String> {
    if port == 0 {
        return Err("端口必须在 1-65535 之间".into());
    }
    {
        let mut cfg = state.config.write().await;
        cfg.port = port;
        state.store.save_config(&cfg).map_err(|e| e.to_string())?;
    }
    start_server(state.inner()).await?;
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
