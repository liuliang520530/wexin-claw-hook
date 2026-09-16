pub mod middleware;
pub mod routes;

use std::io;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::sync::oneshot;

use crate::state::AppState;

/// 绑定 0.0.0.0:port 并在后台运行；返回句柄用于停止。
pub async fn start(state: Arc<AppState>, port: u16) -> io::Result<ServerHandle> {
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    let port = listener.local_addr()?.port();
    let (tx, rx) = oneshot::channel::<()>();
    let app = routes::router(state).into_make_service_with_connect_info::<SocketAddr>();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = rx.await;
            })
            .await;
    });
    Ok(ServerHandle::new(tx, port))
}

/// 运行中的 webhook 服务句柄；drop 或 stop 都会触发优雅关闭。
pub struct ServerHandle {
    shutdown: Option<oneshot::Sender<()>>,
    pub port: u16,
}

impl ServerHandle {
    pub fn new(shutdown: oneshot::Sender<()>, port: u16) -> Self {
        Self { shutdown: Some(shutdown), port }
    }

    pub fn stop(mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

impl Drop for ServerHandle {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;
    use crate::store::Store;

    #[tokio::test]
    async fn starts_on_random_port_serves_health_and_stops() {
        let d = tempfile::tempdir().unwrap();
        let state = AppState::new(Store::new(d.path().to_path_buf()));
        let handle = start(state, 0).await.unwrap();
        assert_ne!(handle.port, 0);
        let url = format!("http://127.0.0.1:{}/health", handle.port);

        // 测试机配置了系统代理，reqwest 默认会走代理（目标不可达时代理返回 503 而非报错），
        // 因此强制直连。
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let v: serde_json::Value = client.get(&url).send().await.unwrap().json().await.unwrap();
        assert_eq!(v["ok"], true);

        handle.stop();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(client.get(&url).send().await.is_err(), "停止后应拒绝连接");
    }

    #[tokio::test]
    async fn port_in_use_is_an_error() {
        let d = tempfile::tempdir().unwrap();
        let state = AppState::new(Store::new(d.path().to_path_buf()));
        let first = start(state.clone(), 0).await.unwrap();
        assert!(start(state, first.port).await.is_err());
    }
}
