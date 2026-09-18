use std::collections::HashMap;
use std::io;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use tokio::sync::{Mutex, RwLock};

use crate::ilink::client::http_client;
use crate::ilink::types::Credentials;
use crate::server::ServerHandle;
use crate::store::{Config, Store};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum LoginStatus {
    Wait,
    Scanned,
    Confirmed,
    Expired,
    Error { message: String },
}

pub struct LoginSession {
    pub qrcode: String,
    pub svg: String,
    pub status: LoginStatus,
}

pub struct AppState {
    pub store: Store,
    pub http: reqwest::Client,
    pub creds: RwLock<Option<Credentials>>,
    pub config: RwLock<Config>,
    pub login: RwLock<Option<LoginSession>>,
    pub server: Mutex<Option<ServerHandle>>,
    /// 各来源 IP 最近一次记录鉴权失败日志的时间，用于限流（None 表示来源未知）。
    pub auth_fail_log: Mutex<HashMap<Option<IpAddr>, Instant>>,
    /// 上次发送因微信侧 token 过期（ret=-14）而清除了凭据；重新登录后复位。
    pub token_expired: RwLock<bool>,
}

impl AppState {
    pub fn new(store: Store) -> Arc<Self> {
        let creds = store.load_credentials();
        let config = store.load_config();
        Arc::new(Self {
            store,
            http: http_client(),
            creds: RwLock::new(creds),
            config: RwLock::new(config),
            login: RwLock::new(None),
            server: Mutex::new(None),
            auth_fail_log: Mutex::new(HashMap::new()),
            token_expired: RwLock::new(false),
        })
    }

    /// 同时更新内存与磁盘；None 表示登出/失效。
    pub async fn set_credentials(&self, creds: Option<Credentials>) -> io::Result<()> {
        match &creds {
            Some(c) => self.store.save_credentials(c)?,
            None => self.store.clear_credentials()?,
        }
        *self.creds.write().await = creds;
        Ok(())
    }

    pub async fn is_logged_in(&self) -> bool {
        self.creds.read().await.is_some()
    }
}
