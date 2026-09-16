use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::ilink::types::Credentials;

pub const DEFAULT_PORT: u16 = 9720;
pub const MAX_LOGS: usize = 500;
pub const LOG_TEXT_CHARS: usize = 200;

const CREDENTIALS_FILE: &str = "credentials.json";
const CONFIG_FILE: &str = "config.json";
const LOGS_FILE: &str = "logs.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recipient {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub port: u16,
    pub api_key: String,
    pub default_recipient: Option<String>,
    pub recipients: Vec<Recipient>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogEntry {
    pub ts: String,
    pub ok: bool,
    pub code: Option<String>,
    pub to: String,
    pub text: String,
}

impl LogEntry {
    pub fn now(ok: bool, code: Option<&str>, to: &str, text: &str) -> Self {
        Self {
            ts: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            ok,
            code: code.map(str::to_string),
            to: to.to_string(),
            text: log_text(text),
        }
    }
}

pub fn log_text(s: &str) -> String {
    s.chars().take(LOG_TEXT_CHARS).collect()
}

/// 32 字节随机 -> base64url 无填充（43 字符）。
pub fn generate_api_key() -> String {
    use base64::Engine;
    let bytes: [u8; 32] = rand::random();
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

pub struct Store {
    dir: PathBuf,
    /// 串行化读-改-写（日志追加、配置保存），避免 webhook 与 UI 并发写坏文件。
    io_lock: Mutex<()>,
}

impl Store {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, io_lock: Mutex::new(()) }
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn read<T: serde::de::DeserializeOwned>(&self, name: &str) -> Option<T> {
        let data = fs::read(self.path(name)).ok()?;
        serde_json::from_slice(&data).ok()
    }

    fn write<T: Serialize>(&self, name: &str, value: &T) -> io::Result<()> {
        fs::create_dir_all(&self.dir)?;
        let tmp = self.path(&format!("{name}.tmp"));
        fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
        fs::rename(&tmp, self.path(name))
    }

    pub fn load_credentials(&self) -> Option<Credentials> {
        self.read(CREDENTIALS_FILE)
    }

    pub fn save_credentials(&self, c: &Credentials) -> io::Result<()> {
        let _g = self.io_lock.lock().unwrap();
        self.write(CREDENTIALS_FILE, c)
    }

    pub fn clear_credentials(&self) -> io::Result<()> {
        let _g = self.io_lock.lock().unwrap();
        match fs::remove_file(self.path(CREDENTIALS_FILE)) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }

    pub fn load_config(&self) -> Config {
        if let Some(c) = self.read::<Config>(CONFIG_FILE) {
            return c;
        }
        let c = Config {
            port: DEFAULT_PORT,
            api_key: generate_api_key(),
            default_recipient: None,
            recipients: Vec::new(),
        };
        let _ = self.save_config(&c);
        c
    }

    pub fn save_config(&self, c: &Config) -> io::Result<()> {
        let _g = self.io_lock.lock().unwrap();
        self.write(CONFIG_FILE, c)
    }

    pub fn load_logs(&self) -> Vec<LogEntry> {
        self.read(LOGS_FILE).unwrap_or_default()
    }

    pub fn append_log(&self, entry: LogEntry) -> io::Result<()> {
        let _g = self.io_lock.lock().unwrap();
        let mut logs = self.load_logs();
        logs.insert(0, entry);
        logs.truncate(MAX_LOGS);
        self.write(LOGS_FILE, &logs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> (tempfile::TempDir, Store) {
        let d = tempfile::tempdir().unwrap();
        let s = Store::new(d.path().to_path_buf());
        (d, s)
    }

    #[test]
    fn credentials_roundtrip_and_clear() {
        let (_d, s) = tmp();
        assert!(s.load_credentials().is_none());
        let c = Credentials {
            bot_token: "t".into(),
            ilink_bot_id: "b".into(),
            ilink_user_id: "u".into(),
            baseurl: "https://x".into(),
        };
        s.save_credentials(&c).unwrap();
        assert_eq!(s.load_credentials(), Some(c));
        s.clear_credentials().unwrap();
        assert!(s.load_credentials().is_none());
        s.clear_credentials().unwrap(); // 幂等
    }

    #[test]
    fn config_defaults_generate_key_and_persist() {
        let (_d, s) = tmp();
        let c1 = s.load_config();
        assert_eq!(c1.port, DEFAULT_PORT);
        assert!(c1.api_key.len() >= 40);
        assert!(c1.recipients.is_empty());
        let c2 = s.load_config();
        assert_eq!(c1.api_key, c2.api_key, "第二次读取应复用已落盘的 key");

        let mut c3 = c2.clone();
        c3.port = 9999;
        c3.recipients.push(Recipient { id: "u@im.wechat".into(), name: "我".into() });
        c3.default_recipient = Some("u@im.wechat".into());
        s.save_config(&c3).unwrap();
        let c4 = s.load_config();
        assert_eq!(c4.port, 9999);
        assert_eq!(c4.recipients.len(), 1);
        assert_eq!(c4.default_recipient.as_deref(), Some("u@im.wechat"));
    }

    #[test]
    fn api_keys_are_urlsafe_and_unique() {
        let a = generate_api_key();
        let b = generate_api_key();
        assert_ne!(a, b);
        assert!(a.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'));
    }

    #[test]
    fn logs_ring_buffer_keeps_latest_first() {
        let (_d, s) = tmp();
        for i in 0..(MAX_LOGS + 10) {
            s.append_log(LogEntry {
                ts: format!("t{i}"),
                ok: true,
                code: None,
                to: "u".into(),
                text: format!("m{i}"),
            })
            .unwrap();
        }
        let logs = s.load_logs();
        assert_eq!(logs.len(), MAX_LOGS);
        assert_eq!(logs[0].text, format!("m{}", MAX_LOGS + 9));
        assert_eq!(logs.last().unwrap().text, "m10");
    }

    #[test]
    fn log_text_truncates_by_chars() {
        let long: String = "中".repeat(300);
        assert_eq!(log_text(&long).chars().count(), LOG_TEXT_CHARS);
        assert_eq!(log_text("短"), "短");
    }
}
