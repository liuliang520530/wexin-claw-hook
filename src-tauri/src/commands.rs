use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::State;

use crate::ilink::auth::{self, AuthError, QrStatus};
use crate::ilink::DEFAULT_BASE_URL;
use crate::server;
use crate::state::{AppState, LoginSession, LoginStatus};
use crate::store::{Config, LogEntry};

#[derive(Debug, Clone, Serialize)]
pub struct StatusInfo {
    /// 至少有一个凭据有效的账号
    pub logged_in: bool,
    pub account_count: usize,
    pub expired_count: usize,
    pub default_user_id: Option<String>,
    pub server_running: bool,
    pub port: u16,
    pub wecom_app_count: usize,
    pub wecom_invalid_count: usize,
    pub default_wecom_app: Option<String>,
    pub version: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountView {
    pub name: String,
    pub user_id: String,
    pub bot_id: String,
    pub token_expired: bool,
    pub is_default: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LoginView {
    pub svg: String,
    pub status: LoginStatus,
}

async fn account_views(state: &AppState) -> Vec<AccountView> {
    let default = state.default_account().await.map(|a| a.user_id().to_string());
    state
        .accounts
        .read()
        .await
        .iter()
        .map(|a| AccountView {
            name: a.name.clone(),
            user_id: a.user_id().to_string(),
            bot_id: a.creds.ilink_bot_id.clone(),
            token_expired: a.token_expired,
            is_default: default.as_deref() == Some(a.user_id()),
        })
        .collect()
}

async fn status(state: &AppState) -> StatusInfo {
    let default_user_id = state.default_account().await.map(|a| a.user_id().to_string());
    let (account_count, expired_count) = {
        let accounts = state.accounts.read().await;
        (accounts.len(), accounts.iter().filter(|a| a.token_expired).count())
    };
    let default_wecom_app = state.default_wecom_app().await.map(|a| a.name);
    let (wecom_app_count, wecom_invalid_count) = {
        let apps = state.wecom_apps.read().await;
        (apps.len(), apps.iter().filter(|a| a.invalid.is_some()).count())
    };
    let cfg_port = state.config.read().await.port;
    let server = state.server.lock().await;
    StatusInfo {
        logged_in: account_count > expired_count,
        account_count,
        expired_count,
        default_user_id,
        server_running: server.is_some(),
        port: server.as_ref().map(|h| h.port).unwrap_or(cfg_port),
        wecom_app_count,
        wecom_invalid_count,
        default_wecom_app,
        version: env!("CARGO_PKG_VERSION").to_string(),
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

/// 当前登录会话是否仍是这个二维码；false 表示已被新的 login_start 替换。
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
                let user_id = creds.ilink_user_id.clone();
                let st = match state.upsert_account(creds).await {
                    Ok(refreshed) => LoginStatus::Confirmed { user_id, refreshed },
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

// ---------- 账号 ----------

#[tauri::command]
pub async fn list_accounts(state: State<'_, Arc<AppState>>) -> Result<Vec<AccountView>, String> {
    Ok(account_views(&state).await)
}

#[tauri::command]
pub async fn rename_account(
    state: State<'_, Arc<AppState>>,
    user_id: String,
    name: String,
) -> Result<Vec<AccountView>, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("备注不能为空".into());
    }
    if !state.rename_account(&user_id, name).await.map_err(|e| e.to_string())? {
        return Err("账号不存在".into());
    }
    Ok(account_views(&state).await)
}

#[tauri::command]
pub async fn remove_account(
    state: State<'_, Arc<AppState>>,
    user_id: String,
) -> Result<Vec<AccountView>, String> {
    state.remove_account(&user_id).await.map_err(|e| e.to_string())?;
    Ok(account_views(&state).await)
}

#[tauri::command]
pub async fn set_default_account(
    state: State<'_, Arc<AppState>>,
    user_id: String,
) -> Result<Vec<AccountView>, String> {
    if state.find_account(&user_id).await.is_none() {
        return Err("账号不存在".into());
    }
    {
        let mut cfg = state.config.write().await;
        cfg.default_account = Some(user_id);
        state.store.save_config(&cfg).map_err(|e| e.to_string())?;
    }
    Ok(account_views(&state).await)
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

#[tauri::command]
pub async fn clear_logs(state: State<'_, Arc<AppState>>) -> Result<(), String> {
    state.store.clear_logs().map_err(|e| e.to_string())
}

/// 发消息页：用账号自己的凭据发给它自己，不经 webhook/鉴权；日志与 webhook 一致。
#[tauri::command]
pub async fn send_test(
    state: State<'_, Arc<AppState>>,
    user_id: String,
    text: String,
) -> Result<String, String> {
    let Some(account) = state.find_account(user_id.trim()).await else {
        return Err("账号不存在，请先扫码接入".into());
    };
    crate::sender::send_as(&state, &account, &text)
        .await
        .map_err(|f| f.message())
}

#[cfg(test)]
mod tests {
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
