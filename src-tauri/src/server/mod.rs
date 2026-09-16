use tokio::sync::oneshot;

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
