use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::ilink::types::Credentials;

pub const DEFAULT_PORT: u16 = 9720;
pub const MAX_LOGS: usize = 500;
pub const LOG_TEXT_CHARS: usize = 200;

const ACCOUNTS_FILE: &str = "accounts.json";
/// v1 的单账号凭据文件；首次启动时自动迁移为 accounts.json 里的第一个账号。
const LEGACY_CREDENTIALS_FILE: &str = "credentials.json";
const CONFIG_FILE: &str = "config.json";
const LOGS_FILE: &str = "logs.json";

/// 一次扫码 = 一个账号：扫码者的微信用户 ID + 属于该用户的 bot 凭据。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Account {
    pub name: String,
    pub creds: Credentials,
    /// 上次发送返回 ret=-14：凭据已失效，需该账号重新扫码；账号记录保留。
    #[serde(default)]
    pub token_expired: bool,
}

impl Account {
    pub fn user_id(&self) -> &str {
        &self.creds.ilink_user_id
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub port: u16,
    pub api_key: String,
    /// 默认账号的 user_id；None 表示用列表里的第一个。
    #[serde(default)]
    pub default_account: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogEntry {
    pub ts: String,
    pub ok: bool,
    pub code: Option<String>,
    /// 用哪个账号（user_id）发的；鉴权失败等无账号的记录为空。
    #[serde(default)]
    pub from: String,
    pub to: String,
    pub text: String,
}

impl LogEntry {
    pub fn now(ok: bool, code: Option<&str>, from: &str, to: &str, text: &str) -> Self {
        Self {
            ts: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            ok,
            code: code.map(str::to_string),
            from: from.to_string(),
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

    /// 读账号列表；没有 accounts.json 但有 v1 的 credentials.json 时自动迁移。
    pub fn load_accounts(&self) -> Vec<Account> {
        if let Some(a) = self.read::<Vec<Account>>(ACCOUNTS_FILE) {
            return a;
        }
        if let Some(creds) = self.read::<Credentials>(LEGACY_CREDENTIALS_FILE) {
            let accounts = vec![Account { name: "我的微信".into(), creds, token_expired: false }];
            if self.save_accounts(&accounts).is_ok() {
                let _ = fs::rename(
                    self.path(LEGACY_CREDENTIALS_FILE),
                    self.path("credentials.json.migrated"),
                );
            }
            return accounts;
        }
        Vec::new()
    }

    pub fn save_accounts(&self, accounts: &[Account]) -> io::Result<()> {
        let _g = self.io_lock.lock().unwrap();
        self.write(ACCOUNTS_FILE, &accounts)
    }

    pub fn load_config(&self) -> Config {
        if let Some(c) = self.read::<Config>(CONFIG_FILE) {
            return c;
        }
        let c = Config { port: DEFAULT_PORT, api_key: generate_api_key(), default_account: None };
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

    pub fn clear_logs(&self) -> io::Result<()> {
        let _g = self.io_lock.lock().unwrap();
        self.write(LOGS_FILE, &Vec::<LogEntry>::new())
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

    fn creds(user: &str) -> Credentials {
        Credentials {
            bot_token: format!("t-{user}"),
            ilink_bot_id: "b@im.bot".into(),
            ilink_user_id: user.into(),
            baseurl: "https://x".into(),
        }
    }

    #[test]
    fn accounts_roundtrip() {
        let (_d, s) = tmp();
        assert!(s.load_accounts().is_empty());
        let a = vec![
            Account { name: "A".into(), creds: creds("a@im.wechat"), token_expired: false },
            Account { name: "B".into(), creds: creds("b@im.wechat"), token_expired: true },
        ];
        s.save_accounts(&a).unwrap();
        assert_eq!(s.load_accounts(), a);
    }

    #[test]
    fn migrates_v1_credentials_into_first_account() {
        let (d, s) = tmp();
        std::fs::write(
            d.path().join("credentials.json"),
            serde_json::to_vec(&creds("me@im.wechat")).unwrap(),
        )
        .unwrap();
        let a = s.load_accounts();
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].name, "我的微信");
        assert_eq!(a[0].user_id(), "me@im.wechat");
        assert!(!a[0].token_expired);
        assert!(d.path().join("accounts.json").exists());
        assert!(!d.path().join("credentials.json").exists(), "旧文件应被重命名");
        assert!(d.path().join("credentials.json.migrated").exists());
        assert_eq!(s.load_accounts(), a, "第二次直接读 accounts.json");
    }

    #[test]
    fn config_defaults_generate_key_and_persist() {
        let (_d, s) = tmp();
        let c1 = s.load_config();
        assert_eq!(c1.port, DEFAULT_PORT);
        assert!(c1.api_key.len() >= 40);
        assert!(c1.default_account.is_none());
        let c2 = s.load_config();
        assert_eq!(c1.api_key, c2.api_key, "第二次读取应复用已落盘的 key");

        let mut c3 = c2.clone();
        c3.port = 9999;
        c3.default_account = Some("u@im.wechat".into());
        s.save_config(&c3).unwrap();
        let c4 = s.load_config();
        assert_eq!(c4.port, 9999);
        assert_eq!(c4.default_account.as_deref(), Some("u@im.wechat"));
    }

    #[test]
    fn config_ignores_v1_recipient_fields() {
        let (d, s) = tmp();
        std::fs::write(
            d.path().join("config.json"),
            br#"{"port":9720,"api_key":"k","default_recipient":"x","recipients":[{"id":"x","name":"x"}]}"#,
        )
        .unwrap();
        let c = s.load_config();
        assert_eq!(c.api_key, "k");
        assert!(c.default_account.is_none());
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
                from: "u".into(),
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
    fn clear_logs_empties_the_ring() {
        let (_d, s) = tmp();
        s.append_log(LogEntry::now(true, None, "u", "u", "a")).unwrap();
        s.append_log(LogEntry::now(false, Some("x"), "u", "u", "b")).unwrap();
        assert_eq!(s.load_logs().len(), 2);
        s.clear_logs().unwrap();
        assert!(s.load_logs().is_empty());
        s.append_log(LogEntry::now(true, None, "u", "u", "c")).unwrap();
        assert_eq!(s.load_logs().len(), 1, "清空后可继续追加");
    }

    #[test]
    fn old_logs_without_from_still_load() {
        let (d, s) = tmp();
        std::fs::write(
            d.path().join("logs.json"),
            br#"[{"ts":"t","ok":true,"code":null,"to":"u","text":"x"}]"#,
        )
        .unwrap();
        let logs = s.load_logs();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].from, "");
    }

    #[test]
    fn log_text_truncates_by_chars() {
        let long: String = "中".repeat(300);
        assert_eq!(log_text(&long).chars().count(), LOG_TEXT_CHARS);
        assert_eq!(log_text("短"), "短");
    }
}
