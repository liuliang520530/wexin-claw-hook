use std::io;
use std::sync::Arc;

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
