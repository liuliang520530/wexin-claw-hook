# 微信 ClawBot Webhook 工具 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 做一个 Tauri 2 桌面应用，扫码登录微信 ClawBot（iLink 协议）后，内嵌一个带 API key 鉴权的局域网 HTTP webhook，让 curl/脚本/CI 一行请求就能给微信发文本消息。

**Architecture:** Rust 后端分三层——`ilink/`（协议客户端，纯 HTTP/JSON，自实现登录 + 发消息）、`server/`（axum webhook，只依赖 `sender`/`store`）、`commands.rs`（Tauri 命令，唯一粘合前端的地方）。前端是 SvelteKit SPA（adapter-static）+ Tailwind 4，四个页面：登录 / 收件人 / 设置 / 日志。数据全部落 app data 目录下的三个 JSON 文件。

**Tech Stack:** Tauri 2.11、axum 0.8、tokio 1、reqwest 0.13（native-tls）、serde、rand 0.10、base64 0.23、subtle 2、qrcode 0.14（svg）、chrono、uuid、thiserror；测试用 wiremock 0.6、tempfile、tower。前端 Svelte 5 + SvelteKit 2 + Tailwind 4 + Vite，pnpm。

**Spec:** `docs/superpowers/specs/2026-09-15-weixin-clawbot-webhook-design.md`

## Global Constraints

- Tauri 必须是 **2.x**（`tauri = "2"`，`@tauri-apps/cli ^2`，`@tauri-apps/api ^2`）。crates.io/npm 上的 3.0.0-alpha 一律不用。
- iLink 协议常量（来自 spec §2，逐字）：基础域名 `https://ilinkai.weixin.qq.com`；`channel_version` = `2.4.6`；`iLink-App-Id` = `bot`；`iLink-App-ClientVersion` = 版本号编码整数（2.4.6 → `132102`）；请求头 `AuthorizationType: ilink_bot_token`、`Authorization: Bearer <bot_token>`、`X-WECHAT-UIN` = base64(随机 uint32 的十进制字符串)。
- 发消息：`POST {baseurl}/ilink/bot/sendmessage`，`message_type: 2`、`message_state: 2`、文本 item `type: 1`；`from_user_id` 填 `ilink_bot_id`；`context_token` 可选，None 时不序列化该字段。
- 错误码：`ret=-2` → 限频；`ret=-14` → token 失效。上游调用超时 **15s**。
- Webhook：默认端口 **9720**，监听 **0.0.0.0**；鉴权头 `X-API-Key` 或 `Authorization: Bearer`；`/health` 不鉴权、永远 200；请求体上限 1MB。HTTP 映射见 spec §6（200 / 400 bad_request / 401 unauthorized / 503 not_logged_in / 429 rate_limited / 503 token_expired / 502 upstream_error）。
- 存储：`credentials.json`、`config.json`、`logs.json`，日志环形 **500** 条，日志文本截断 **200** 字符（按字符不是字节），日志不含 key/token。
- reqwest 用 `default-features = false, features = ["native-tls", "json", "http2", "charset"]`（避免 aws-lc 在 Windows 上要 CMake）。
- 所有 Rust 测试是模块内 `#[cfg(test)]` 单元测试，在 `src-tauri/` 下跑 `cargo test`。**跑 cargo 前先在项目根执行一次 `pnpm build`**（Tauri 的 codegen 要求 `build/` 目录存在）。
- v1 不做：收消息、媒体发送、多账号、签名、mac/linux 构建、自动重试、DPAPI。
- 提交信息用中文，每个 task 结尾提交。

## File Structure

```
.github/workflows/release.yml     # tag 触发，windows-latest，NSIS + 便携 exe 上传 Release
README.md                         # 使用说明（扫码、curl 示例、防火墙提示）
package.json / vite.config.js / svelte.config.js / tsconfig.json   # 脚手架生成，仅小改
src/app.css                       # @import "tailwindcss"
src/app.html                      # 脚手架生成
src/routes/+layout.ts             # ssr=false（脚手架生成）
src/routes/+layout.svelte         # 引入 app.css，渲染 children
src/routes/+page.svelte           # 应用外壳：左侧导航 + 当前页面
src/lib/api.ts                    # 所有 invoke 的类型化封装（前端唯一调后端的地方）
src/lib/pages/Login.svelte        # 二维码 + 登录状态轮询 / 已登录信息 + 退出
src/lib/pages/Recipients.svelte   # 收件人增删改 + 设默认
src/lib/pages/Settings.svelte     # 端口、API key 查看/重置、服务开关、curl 示例
src/lib/pages/Logs.svelte         # 日志表
src-tauri/Cargo.toml
src-tauri/tauri.conf.json         # productName、identifier、窗口、bundle nsis
src-tauri/capabilities/default.json
src-tauri/src/main.rs             # 调 lib::run()
src-tauri/src/lib.rs              # 模块声明、Tauri Builder、setup 里加载状态 + 启动 server
src-tauri/src/ilink/mod.rs        # pub mod types/client/auth + 常量
src-tauri/src/ilink/types.rs      # 协议类型、头构造、版本编码、UIN 生成
src-tauri/src/ilink/client.rs     # IlinkClient::send_text / send_items，SendError
src-tauri/src/ilink/auth.rs       # fetch_qrcode / poll_qrcode_status / qrcode_svg
src-tauri/src/store.rs            # Store：三个 JSON 文件读写、api key 生成、环形日志
src-tauri/src/state.rs            # AppState（Arc 共享）、LoginSession/LoginStatus
src-tauri/src/sender.rs           # send_text(state, to, text)：收件人解析 + 调协议 + 错误映射 + 写日志 + -14 清凭据
src-tauri/src/server/mod.rs       # start(state, port) -> ServerHandle，graceful shutdown
src-tauri/src/server/middleware.rs# require_api_key
src-tauri/src/server/routes.rs    # router(state)：POST /send、GET /health
src-tauri/src/commands.rs         # 全部 #[tauri::command]
```

---

### Task 1: 脚手架 + 依赖 + 空壳可编译

**Files:**
- Create: 整个脚手架（由 create-tauri-app 生成）、`src/app.css`、`src/routes/+layout.svelte`
- Modify: `package.json`、`vite.config.js`、`src-tauri/Cargo.toml`、`src-tauri/tauri.conf.json`、`src-tauri/capabilities/default.json`、`src-tauri/src/lib.rs`、`src-tauri/src/main.rs`、`src/routes/+page.svelte`

**Interfaces:**
- Produces: crate 名 `weixin-clawbot-webhook`，lib 名 `weixin_clawbot_webhook_lib`，`pub fn run()`；后续 task 在 `lib.rs` 里追加 `pub mod` 声明。

- [ ] **Step 1: 生成脚手架（Tauri 2、Svelte TS、pnpm）**

在项目根目录（已有 `docs/` 和 `.git`，所以要 `-f`）：

```bash
pnpm create tauri-app@latest . -t svelte-ts -m pnpm -y -f --identifier com.liuli.weixin-clawbot-webhook --tauri-version 2
```

生成后确认 `src-tauri/Cargo.toml` 里 `tauri = { version = "2", ...}`（不是 3）。

- [ ] **Step 2: 统一项目名（目录名有拼写 weibhook，产物名要正确）**

`package.json`：`"name": "weixin-clawbot-webhook"`。

`src-tauri/Cargo.toml` 的 `[package]` 与 `[lib]`：

```toml
[package]
name = "weixin-clawbot-webhook"
version = "0.1.0"
description = "微信 ClawBot Webhook 工具"
edition = "2021"

[lib]
name = "weixin_clawbot_webhook_lib"
crate-type = ["staticlib", "cdylib", "rlib"]
```

`src-tauri/src/main.rs` 整个替换为：

```rust
// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    weixin_clawbot_webhook_lib::run()
}
```

`src-tauri/tauri.conf.json` 整个替换为：

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "weixin-clawbot-webhook",
  "version": "0.1.0",
  "identifier": "com.liuli.weixin-clawbot-webhook",
  "build": {
    "beforeDevCommand": "pnpm dev",
    "devUrl": "http://localhost:1420",
    "beforeBuildCommand": "pnpm build",
    "frontendDist": "../build"
  },
  "app": {
    "windows": [
      {
        "title": "微信 ClawBot Webhook",
        "width": 960,
        "height": 640,
        "minWidth": 800,
        "minHeight": 520
      }
    ],
    "security": {
      "csp": null
    }
  },
  "bundle": {
    "active": true,
    "targets": ["nsis"],
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ]
  }
}
```

- [ ] **Step 3: 去掉模板自带的 opener 插件，写最小 lib.rs**

`src-tauri/capabilities/default.json` 替换为：

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "default capability",
  "windows": ["main"],
  "permissions": ["core:default"]
}
```

`src-tauri/src/lib.rs` 整个替换为：

```rust
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

`package.json` 的 `dependencies` 里删掉 `@tauri-apps/plugin-opener`。

- [ ] **Step 4: 写 Cargo 依赖**

`src-tauri/Cargo.toml` 的依赖部分替换为：

```toml
[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
tauri = { version = "2", features = [] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
axum = "0.8"
reqwest = { version = "0.13", default-features = false, features = ["native-tls", "json", "http2", "charset"] }
rand = "0.10"
base64 = "0.23"
subtle = "2"
qrcode = { version = "0.14", default-features = false, features = ["std", "svg"] }
chrono = "0.4"
uuid = { version = "1", features = ["v4"] }
thiserror = "2"

[dev-dependencies]
wiremock = "0.6"
tempfile = "3"
tower = { version = "0.5", features = ["util"] }
http-body-util = "0.1"
```

- [ ] **Step 5: 加 Tailwind 4 与布局文件**

```bash
pnpm install
pnpm add -D tailwindcss @tailwindcss/vite
```

`vite.config.js` 里加插件（保留模板其余内容）：

```js
import tailwindcss from "@tailwindcss/vite";
// ...
  plugins: [tailwindcss(), sveltekit()],
```

新建 `src/app.css`：

```css
@import "tailwindcss";
```

新建 `src/routes/+layout.svelte`：

```svelte
<script lang="ts">
  import "../app.css";
  let { children } = $props();
</script>

{@render children()}
```

`src/routes/+page.svelte` 先替换成占位：

```svelte
<main class="p-6">
  <h1 class="text-xl font-semibold">weixin-clawbot-webhook</h1>
</main>
```

- [ ] **Step 6: 验证前端与 Rust 都能编译**

```bash
pnpm check
pnpm build
cd src-tauri && cargo test
```

Expected: `pnpm check` 0 errors；`pnpm build` 生成 `build/`；`cargo test` 编译通过，`running 0 tests`。

- [ ] **Step 7: 提交**

```bash
git add -A
git commit -m "chore: Tauri 2 + SvelteKit + Tailwind 脚手架与依赖"
```

---

### Task 2: iLink 协议类型与请求头构造

**Files:**
- Create: `src-tauri/src/ilink/mod.rs`、`src-tauri/src/ilink/types.rs`
- Modify: `src-tauri/src/lib.rs`（加 `pub mod ilink;`）

**Interfaces:**
- Produces:
  - `ilink::DEFAULT_BASE_URL: &str`、`ilink::CHANNEL_VERSION: &str`、`ilink::ILINK_APP_ID: &str`
  - `types::Credentials { bot_token, ilink_bot_id, ilink_user_id, baseurl: String }`（Debug/Clone/PartialEq/Serialize/Deserialize）
  - `types::encode_client_version(&str) -> u32`
  - `types::random_uin() -> String`
  - `types::request_headers(token: Option<&str>) -> Vec<(&'static str, String)>`
  - `types::MessageItem::text(impl Into<String>) -> MessageItem`
  - `types::SendMessageRequest::new(from, to, client_id, items, context_token)` / `::text(from, to, client_id, text, context_token)`
  - `types::SendMessageResponse { ret: i64, errmsg: String }`
  - `types::QrCodeResponse { qrcode, qrcode_img_content }`、`types::QrStatusResponse { status, bot_token, ilink_bot_id, ilink_user_id, baseurl }`

- [ ] **Step 1: 写失败测试**

新建 `src-tauri/src/ilink/mod.rs`：

```rust
pub mod types;

pub const DEFAULT_BASE_URL: &str = "https://ilinkai.weixin.qq.com";
pub const CHANNEL_VERSION: &str = "2.4.6";
pub const ILINK_APP_ID: &str = "bot";
```

新建 `src-tauri/src/ilink/types.rs`，先只放测试模块：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn client_version_encodes_like_official_package() {
        assert_eq!(encode_client_version("2.4.6"), 132102);
        assert_eq!(encode_client_version("1.0.0"), 0x0001_0000);
    }

    #[test]
    fn uin_is_base64_of_decimal_u32() {
        let uin = random_uin();
        let raw = base64::engine::general_purpose::STANDARD.decode(&uin).unwrap();
        let s = String::from_utf8(raw).unwrap();
        let n: u64 = s.parse().unwrap();
        assert!(n <= u32::MAX as u64);
    }

    #[test]
    fn headers_include_auth_and_protocol_fields() {
        let h = request_headers(Some("tok"));
        let get = |k: &str| h.iter().find(|(n, _)| *n == k).map(|(_, v)| v.clone());
        assert_eq!(get("Content-Type").as_deref(), Some("application/json"));
        assert_eq!(get("AuthorizationType").as_deref(), Some("ilink_bot_token"));
        assert_eq!(get("Authorization").as_deref(), Some("Bearer tok"));
        assert_eq!(get("iLink-App-Id").as_deref(), Some("bot"));
        assert_eq!(get("iLink-App-ClientVersion").as_deref(), Some("132102"));
        assert!(get("X-WECHAT-UIN").is_some());

        let anon = request_headers(None);
        assert!(anon.iter().all(|(n, _)| *n != "Authorization"));
    }

    #[test]
    fn send_request_serializes_and_omits_none_context_token() {
        let req = SendMessageRequest::text("bot@im.bot", "u@im.wechat", "cid-1", "你好", None);
        let v = serde_json::to_value(&req).unwrap();
        assert_eq!(v["msg"]["from_user_id"], "bot@im.bot");
        assert_eq!(v["msg"]["to_user_id"], "u@im.wechat");
        assert_eq!(v["msg"]["client_id"], "cid-1");
        assert_eq!(v["msg"]["message_type"], 2);
        assert_eq!(v["msg"]["message_state"], 2);
        assert_eq!(v["msg"]["item_list"][0]["type"], 1);
        assert_eq!(v["msg"]["item_list"][0]["text_item"]["text"], "你好");
        assert!(v["msg"].get("context_token").is_none());
        assert_eq!(v["base_info"]["channel_version"], "2.4.6");

        let req2 = SendMessageRequest::text("b", "u", "c", "t", Some("ctx"));
        let v2 = serde_json::to_value(&req2).unwrap();
        assert_eq!(v2["msg"]["context_token"], "ctx");
    }

    #[test]
    fn responses_deserialize_with_missing_fields() {
        let r: SendMessageResponse = serde_json::from_str("{}").unwrap();
        assert_eq!(r.ret, 0);
        let r: SendMessageResponse =
            serde_json::from_str(r#"{"ret":-14,"errmsg":"expired"}"#).unwrap();
        assert_eq!(r.ret, -14);
        let q: QrStatusResponse = serde_json::from_str(r#"{"status":"wait"}"#).unwrap();
        assert_eq!(q.status, "wait");
        assert_eq!(q.bot_token, "");
    }
}
```

`src-tauri/src/lib.rs` 顶部加：

```rust
pub mod ilink;
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test ilink::types`
Expected: 编译失败，`encode_client_version` 等未定义。

- [ ] **Step 3: 实现 types.rs**

在 `src-tauri/src/ilink/types.rs` 顶部（测试模块之前）写：

```rust
use serde::{Deserialize, Serialize};

use super::{CHANNEL_VERSION, ILINK_APP_ID};

/// 扫码登录后拿到并持久化的凭据。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Credentials {
    pub bot_token: String,
    pub ilink_bot_id: String,
    pub ilink_user_id: String,
    pub baseurl: String,
}

/// "2.4.6" -> 0x00020406，与官方包 iLink-App-ClientVersion 一致。
pub fn encode_client_version(version: &str) -> u32 {
    let mut parts = version.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    let major = parts.next().unwrap_or(0) & 0xff;
    let minor = parts.next().unwrap_or(0) & 0xff;
    let patch = parts.next().unwrap_or(0) & 0xff;
    (major << 16) | (minor << 8) | patch
}

/// X-WECHAT-UIN：随机 uint32 -> 十进制字符串 -> base64。
pub fn random_uin() -> String {
    use base64::Engine;
    let n: u32 = rand::random();
    base64::engine::general_purpose::STANDARD.encode(n.to_string())
}

/// 每次请求都重新生成（UIN 需随机）。token 为 None 时用于扫码阶段的匿名请求。
pub fn request_headers(token: Option<&str>) -> Vec<(&'static str, String)> {
    let mut h = vec![
        ("Content-Type", "application/json".to_string()),
        ("X-WECHAT-UIN", random_uin()),
        ("iLink-App-Id", ILINK_APP_ID.to_string()),
        (
            "iLink-App-ClientVersion",
            encode_client_version(CHANNEL_VERSION).to_string(),
        ),
    ];
    if let Some(t) = token {
        h.push(("AuthorizationType", "ilink_bot_token".to_string()));
        h.push(("Authorization", format!("Bearer {t}")));
    }
    h
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BaseInfo {
    pub channel_version: String,
    pub bot_agent: String,
}

pub fn base_info() -> BaseInfo {
    BaseInfo {
        channel_version: CHANNEL_VERSION.to_string(),
        bot_agent: format!("weixin-clawbot-webhook/{}", env!("CARGO_PKG_VERSION")),
    }
}

pub const MESSAGE_TYPE_BOT: u8 = 2;
pub const MESSAGE_STATE_FINISH: u8 = 2;
pub const ITEM_TYPE_TEXT: u8 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextItem {
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageItem {
    #[serde(rename = "type")]
    pub item_type: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_item: Option<TextItem>,
}

impl MessageItem {
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            item_type: ITEM_TYPE_TEXT,
            text_item: Some(TextItem { text: text.into() }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMsg {
    pub from_user_id: String,
    pub to_user_id: String,
    pub client_id: String,
    pub message_type: u8,
    pub message_state: u8,
    pub item_list: Vec<MessageItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageRequest {
    pub msg: SendMsg,
    pub base_info: BaseInfo,
}

impl SendMessageRequest {
    pub fn new(
        from: &str,
        to: &str,
        client_id: &str,
        items: Vec<MessageItem>,
        context_token: Option<&str>,
    ) -> Self {
        Self {
            msg: SendMsg {
                from_user_id: from.to_string(),
                to_user_id: to.to_string(),
                client_id: client_id.to_string(),
                message_type: MESSAGE_TYPE_BOT,
                message_state: MESSAGE_STATE_FINISH,
                item_list: items,
                context_token: context_token.map(str::to_string),
            },
            base_info: base_info(),
        }
    }

    pub fn text(
        from: &str,
        to: &str,
        client_id: &str,
        text: &str,
        context_token: Option<&str>,
    ) -> Self {
        Self::new(from, to, client_id, vec![MessageItem::text(text)], context_token)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SendMessageResponse {
    #[serde(default)]
    pub ret: i64,
    #[serde(default)]
    pub errmsg: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QrCodeResponse {
    pub qrcode: String,
    pub qrcode_img_content: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct QrStatusResponse {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub bot_token: String,
    #[serde(default)]
    pub ilink_bot_id: String,
    #[serde(default)]
    pub ilink_user_id: String,
    #[serde(default)]
    pub baseurl: String,
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cd src-tauri && cargo test ilink::types`
Expected: 5 passed。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src
git commit -m "feat(ilink): 协议类型、请求头与版本编码"
```

---

### Task 3: iLink 发消息客户端

**Files:**
- Create: `src-tauri/src/ilink/client.rs`
- Modify: `src-tauri/src/ilink/mod.rs`（加 `pub mod client;`）

**Interfaces:**
- Consumes: Task 2 的 `types::*`
- Produces:
  - `client::http_client() -> reqwest::Client`（15s 超时）
  - `client::SendError { RateLimited, TokenExpired, Upstream(String) }`（thiserror，Debug/Clone/PartialEq）
  - `client::IlinkClient::new(http: reqwest::Client, creds: Credentials) -> IlinkClient`
  - `IlinkClient::send_text(&self, to: &str, text: &str, context_token: Option<&str>) -> Result<String, SendError>`（Ok 返回 client_id）
  - `IlinkClient::send_items(&self, to: &str, items: Vec<MessageItem>, context_token: Option<&str>) -> Result<String, SendError>`

- [ ] **Step 1: 写失败测试（wiremock 模拟 iLink）**

新建 `src-tauri/src/ilink/client.rs`，先写测试模块：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_partial_json, header, header_exists, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn creds(base: &str) -> Credentials {
        Credentials {
            bot_token: "tok".into(),
            ilink_bot_id: "bot@im.bot".into(),
            ilink_user_id: "me@im.wechat".into(),
            baseurl: base.into(),
        }
    }

    #[tokio::test]
    async fn sends_text_with_protocol_headers_and_body() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/ilink/bot/sendmessage"))
            .and(header("Authorization", "Bearer tok"))
            .and(header("AuthorizationType", "ilink_bot_token"))
            .and(header("iLink-App-Id", "bot"))
            .and(header("iLink-App-ClientVersion", "132102"))
            .and(header_exists("X-WECHAT-UIN"))
            .and(body_partial_json(serde_json::json!({
                "msg": {
                    "from_user_id": "bot@im.bot",
                    "to_user_id": "u@im.wechat",
                    "message_type": 2,
                    "message_state": 2,
                    "item_list": [{"type": 1, "text_item": {"text": "hi"}}]
                },
                "base_info": {"channel_version": "2.4.6"}
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ret": 0})))
            .expect(1)
            .mount(&server)
            .await;

        let c = IlinkClient::new(http_client(), creds(&server.uri()));
        let id = c.send_text("u@im.wechat", "hi", None).await.unwrap();
        assert!(!id.is_empty());
    }

    #[tokio::test]
    async fn empty_body_counts_as_success() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let c = IlinkClient::new(http_client(), creds(&server.uri()));
        assert!(c.send_text("u", "hi", None).await.is_ok());
    }

    #[tokio::test]
    async fn maps_ret_codes() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ret": -2})))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        let c = IlinkClient::new(http_client(), creds(&server.uri()));
        assert_eq!(c.send_text("u", "hi", None).await, Err(SendError::RateLimited));

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ret": -14})))
            .mount(&server)
            .await;
        let c = IlinkClient::new(http_client(), creds(&server.uri()));
        assert_eq!(c.send_text("u", "hi", None).await, Err(SendError::TokenExpired));

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"ret": -99, "errmsg": "boom"})),
            )
            .mount(&server)
            .await;
        let c = IlinkClient::new(http_client(), creds(&server.uri()));
        match c.send_text("u", "hi", None).await {
            Err(SendError::Upstream(m)) => assert!(m.contains("-99") && m.contains("boom")),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[tokio::test]
    async fn non_2xx_and_bad_json_are_upstream_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500).set_body_string("oops"))
            .mount(&server)
            .await;
        let c = IlinkClient::new(http_client(), creds(&server.uri()));
        match c.send_text("u", "hi", None).await {
            Err(SendError::Upstream(m)) => assert!(m.contains("500")),
            other => panic!("unexpected {other:?}"),
        }

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>"))
            .mount(&server)
            .await;
        let c = IlinkClient::new(http_client(), creds(&server.uri()));
        assert!(matches!(c.send_text("u", "hi", None).await, Err(SendError::Upstream(_))));
    }

    #[tokio::test]
    async fn connection_refused_is_upstream_error() {
        let c = IlinkClient::new(http_client(), creds("http://127.0.0.1:1"));
        assert!(matches!(c.send_text("u", "hi", None).await, Err(SendError::Upstream(_))));
    }
}
```

`src-tauri/src/ilink/mod.rs` 加一行 `pub mod client;`。

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test ilink::client`
Expected: 编译失败，`IlinkClient`/`http_client`/`SendError` 未定义。

- [ ] **Step 3: 实现 client.rs**

在测试模块之前写：

```rust
use std::time::Duration;

use super::types::{request_headers, Credentials, MessageItem, SendMessageRequest, SendMessageResponse};

pub const SEND_TIMEOUT: Duration = Duration::from_secs(15);

pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(SEND_TIMEOUT)
        .build()
        .expect("reqwest client")
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SendError {
    #[error("rate limited (ret=-2)")]
    RateLimited,
    #[error("token expired (ret=-14)")]
    TokenExpired,
    #[error("upstream error: {0}")]
    Upstream(String),
}

pub struct IlinkClient {
    http: reqwest::Client,
    creds: Credentials,
}

impl IlinkClient {
    pub fn new(http: reqwest::Client, creds: Credentials) -> Self {
        Self { http, creds }
    }

    pub async fn send_text(
        &self,
        to: &str,
        text: &str,
        context_token: Option<&str>,
    ) -> Result<String, SendError> {
        self.send_items(to, vec![MessageItem::text(text)], context_token).await
    }

    /// 通用入口，以后加图片/文件 item 也走这里。返回 client_id。
    pub async fn send_items(
        &self,
        to: &str,
        items: Vec<MessageItem>,
        context_token: Option<&str>,
    ) -> Result<String, SendError> {
        let client_id = uuid::Uuid::new_v4().to_string();
        let req = SendMessageRequest::new(&self.creds.ilink_bot_id, to, &client_id, items, context_token);
        let url = format!("{}/ilink/bot/sendmessage", self.creds.baseurl.trim_end_matches('/'));

        let mut builder = self.http.post(&url);
        for (k, v) in request_headers(Some(&self.creds.bot_token)) {
            builder = builder.header(k, v);
        }
        let resp = builder
            .json(&req)
            .send()
            .await
            .map_err(|e| SendError::Upstream(e.to_string()))?;

        let status = resp.status();
        let body = resp.text().await.map_err(|e| SendError::Upstream(e.to_string()))?;
        if !status.is_success() {
            return Err(SendError::Upstream(format!("HTTP {}: {}", status.as_u16(), truncate(&body, 200))));
        }
        if body.trim().is_empty() {
            return Ok(client_id);
        }
        let parsed: SendMessageResponse = serde_json::from_str(&body)
            .map_err(|_| SendError::Upstream(format!("invalid JSON: {}", truncate(&body, 200))))?;
        match parsed.ret {
            0 => Ok(client_id),
            -2 => Err(SendError::RateLimited),
            -14 => Err(SendError::TokenExpired),
            other => Err(SendError::Upstream(format!("ret={other} {}", parsed.errmsg))),
        }
    }
}

fn truncate(s: &str, max_chars: usize) -> String {
    s.chars().take(max_chars).collect()
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cd src-tauri && cargo test ilink::client`
Expected: 5 passed。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src
git commit -m "feat(ilink): sendmessage 客户端与错误码映射"
```

---

### Task 4: 扫码登录（取二维码、轮询状态、SVG 渲染）

**Files:**
- Create: `src-tauri/src/ilink/auth.rs`
- Modify: `src-tauri/src/ilink/mod.rs`（加 `pub mod auth;`）

**Interfaces:**
- Consumes: Task 2 `types::{Credentials, QrCodeResponse, QrStatusResponse, request_headers}`
- Produces:
  - `auth::AuthError { Network(String), BadResponse(String), QrEncode(String) }`（thiserror，Debug/Clone/PartialEq）
  - `auth::QrCode { qrcode: String, content: String }`
  - `auth::QrStatus { Wait, Scanned, Confirmed(Credentials), Expired, Unknown(String) }`（Debug/Clone/PartialEq）
  - `auth::fetch_qrcode(http: &reqwest::Client, base_url: &str) -> Result<QrCode, AuthError>`
  - `auth::poll_qrcode_status(http: &reqwest::Client, base_url: &str, qrcode: &str) -> Result<QrStatus, AuthError>`（单次调用，服务端可能 hold 最长约 35s，请求超时设 40s）
  - `auth::qrcode_svg(content: &str) -> Result<String, AuthError>`

- [ ] **Step 1: 写失败测试**

新建 `src-tauri/src/ilink/auth.rs`，先写测试：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn fetch_qrcode_hits_get_bot_qrcode_with_bot_type_3() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ilink/bot/get_bot_qrcode"))
            .and(query_param("bot_type", "3"))
            .and(header("iLink-App-Id", "bot"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "qrcode": "qrc_1",
                "qrcode_img_content": "https://liteapp.weixin.qq.com/q/abc"
            })))
            .expect(1)
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        let qr = fetch_qrcode(&http, &server.uri()).await.unwrap();
        assert_eq!(qr.qrcode, "qrc_1");
        assert_eq!(qr.content, "https://liteapp.weixin.qq.com/q/abc");
    }

    #[tokio::test]
    async fn poll_maps_statuses() {
        let server = MockServer::start().await;
        let http = reqwest::Client::new();

        for (status, expected) in [
            ("wait", QrStatus::Wait),
            ("scaned", QrStatus::Scanned),
            ("expired", QrStatus::Expired),
            ("weird", QrStatus::Unknown("weird".into())),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .and(path("/ilink/bot/get_qrcode_status"))
                .and(query_param("qrcode", "qrc_1"))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(serde_json::json!({ "status": status })),
                )
                .mount(&server)
                .await;
            assert_eq!(poll_qrcode_status(&http, &server.uri(), "qrc_1").await.unwrap(), expected);
        }

        Mock::given(method("GET"))
            .and(path("/ilink/bot/get_qrcode_status"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "confirmed",
                "bot_token": "t",
                "ilink_bot_id": "b@im.bot",
                "ilink_user_id": "u@im.wechat",
                "baseurl": "https://other.weixin.qq.com"
            })))
            .mount(&server)
            .await;
        let got = poll_qrcode_status(&http, &server.uri(), "qrc_1").await.unwrap();
        assert_eq!(
            got,
            QrStatus::Confirmed(Credentials {
                bot_token: "t".into(),
                ilink_bot_id: "b@im.bot".into(),
                ilink_user_id: "u@im.wechat".into(),
                baseurl: "https://other.weixin.qq.com".into(),
            })
        );
    }

    #[tokio::test]
    async fn confirmed_without_baseurl_falls_back_to_requested_base() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "confirmed", "bot_token": "t", "ilink_bot_id": "b", "ilink_user_id": "u"
            })))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        match poll_qrcode_status(&http, &server.uri(), "q").await.unwrap() {
            QrStatus::Confirmed(c) => assert_eq!(c.baseurl, server.uri()),
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn http_error_is_bad_response() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let http = reqwest::Client::new();
        assert!(matches!(
            fetch_qrcode(&http, &server.uri()).await,
            Err(AuthError::BadResponse(_))
        ));
    }

    #[test]
    fn svg_contains_svg_tag() {
        let svg = qrcode_svg("https://liteapp.weixin.qq.com/q/abc").unwrap();
        assert!(svg.starts_with("<?xml") || svg.starts_with("<svg"));
        assert!(svg.contains("<svg"));
    }
}
```

`src-tauri/src/ilink/mod.rs` 加 `pub mod auth;`。

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test ilink::auth`
Expected: 编译失败。

- [ ] **Step 3: 实现 auth.rs**

测试模块之前写：

```rust
use std::time::Duration;

use super::types::{request_headers, Credentials, QrCodeResponse, QrStatusResponse};

/// get_qrcode_status 是长轮询，服务端可能 hold ~35s。
pub const POLL_TIMEOUT: Duration = Duration::from_secs(40);

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AuthError {
    #[error("network error: {0}")]
    Network(String),
    #[error("bad response: {0}")]
    BadResponse(String),
    #[error("qrcode encode error: {0}")]
    QrEncode(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct QrCode {
    pub qrcode: String,
    /// 二维码内容（URL），用 qrcode_svg 渲染。
    pub content: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum QrStatus {
    Wait,
    Scanned,
    Confirmed(Credentials),
    Expired,
    Unknown(String),
}

async fn get_json<T: serde::de::DeserializeOwned>(
    http: &reqwest::Client,
    url: &str,
    timeout: Option<Duration>,
) -> Result<T, AuthError> {
    let mut b = http.get(url);
    for (k, v) in request_headers(None) {
        b = b.header(k, v);
    }
    if let Some(t) = timeout {
        b = b.timeout(t);
    }
    let resp = b.send().await.map_err(|e| AuthError::Network(e.to_string()))?;
    let status = resp.status();
    let body = resp.text().await.map_err(|e| AuthError::Network(e.to_string()))?;
    if !status.is_success() {
        return Err(AuthError::BadResponse(format!("HTTP {}", status.as_u16())));
    }
    serde_json::from_str(&body)
        .map_err(|e| AuthError::BadResponse(format!("{e}: {}", body.chars().take(200).collect::<String>())))
}

pub async fn fetch_qrcode(http: &reqwest::Client, base_url: &str) -> Result<QrCode, AuthError> {
    let url = format!("{}/ilink/bot/get_bot_qrcode?bot_type=3", base_url.trim_end_matches('/'));
    let r: QrCodeResponse = get_json(http, &url, None).await?;
    Ok(QrCode { qrcode: r.qrcode, content: r.qrcode_img_content })
}

pub async fn poll_qrcode_status(
    http: &reqwest::Client,
    base_url: &str,
    qrcode: &str,
) -> Result<QrStatus, AuthError> {
    let base = base_url.trim_end_matches('/');
    let url = format!("{base}/ilink/bot/get_qrcode_status?qrcode={qrcode}");
    let r: QrStatusResponse = get_json(http, &url, Some(POLL_TIMEOUT)).await?;
    Ok(match r.status.as_str() {
        "wait" => QrStatus::Wait,
        "scaned" | "scanned" => QrStatus::Scanned,
        "expired" => QrStatus::Expired,
        "confirmed" => QrStatus::Confirmed(Credentials {
            bot_token: r.bot_token,
            ilink_bot_id: r.ilink_bot_id,
            ilink_user_id: r.ilink_user_id,
            baseurl: if r.baseurl.is_empty() { base.to_string() } else { r.baseurl },
        }),
        other => QrStatus::Unknown(other.to_string()),
    })
}

pub fn qrcode_svg(content: &str) -> Result<String, AuthError> {
    use qrcode::render::svg;
    let code = qrcode::QrCode::new(content.as_bytes()).map_err(|e| AuthError::QrEncode(e.to_string()))?;
    Ok(code
        .render::<svg::Color>()
        .min_dimensions(240, 240)
        .dark_color(svg::Color("#111827"))
        .light_color(svg::Color("#ffffff"))
        .build())
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cd src-tauri && cargo test ilink::auth`
Expected: 5 passed。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src
git commit -m "feat(ilink): 扫码登录流程与二维码 SVG"
```

---

### Task 5: 本地存储（凭据 / 配置 / 环形日志）

**Files:**
- Create: `src-tauri/src/store.rs`
- Modify: `src-tauri/src/lib.rs`（加 `pub mod store;`）

**Interfaces:**
- Consumes: Task 2 `types::Credentials`
- Produces:
  - `store::DEFAULT_PORT: u16 = 9720`、`store::MAX_LOGS: usize = 500`、`store::LOG_TEXT_CHARS: usize = 200`
  - `store::Recipient { id: String, name: String }`
  - `store::Config { port: u16, api_key: String, default_recipient: Option<String>, recipients: Vec<Recipient> }`
  - `store::LogEntry { ts: String, ok: bool, code: Option<String>, to: String, text: String }`
  - `store::generate_api_key() -> String`
  - `store::Store::new(dir: PathBuf) -> Store`
  - `Store::load_credentials(&self) -> Option<Credentials>` / `save_credentials(&self, &Credentials) -> io::Result<()>` / `clear_credentials(&self) -> io::Result<()>`
  - `Store::load_config(&self) -> Config`（文件不存在则生成默认配置 + 新 key 并落盘）/ `save_config(&self, &Config) -> io::Result<()>`
  - `Store::append_log(&self, LogEntry) -> io::Result<()>` / `load_logs(&self) -> Vec<LogEntry>`（最新在前）
  - `store::log_text(&str) -> String`（截断到 200 字符）

- [ ] **Step 1: 写失败测试**

新建 `src-tauri/src/store.rs`，测试模块：

```rust
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
```

`src-tauri/src/lib.rs` 加 `pub mod store;`。

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test store::`
Expected: 编译失败。

- [ ] **Step 3: 实现 store.rs**

测试模块之前写：

```rust
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
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cd src-tauri && cargo test store::`
Expected: 5 passed。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src
git commit -m "feat(store): 凭据、配置与环形日志的 JSON 存储"
```

---

### Task 6: 共享状态与发送服务（收件人解析 + 错误映射 + 日志 + -14 清凭据）

**Files:**
- Create: `src-tauri/src/state.rs`、`src-tauri/src/sender.rs`、`src-tauri/src/server/mod.rs`（本 task 只放 `ServerHandle`，`start()` 在 Task 8 补）
- Modify: `src-tauri/src/lib.rs`（加 `pub mod state; pub mod sender; pub mod server;`）

**Interfaces:**
- Consumes: Task 3 `client::{http_client, IlinkClient, SendError}`，Task 5 `store::{Store, Config, LogEntry}`
- Produces:
  - `state::LoginStatus { Wait, Scanned, Confirmed, Expired, Error { message } }`（Serialize，`#[serde(tag="state", rename_all="snake_case")]`）
  - `state::LoginSession { qrcode: String, svg: String, status: LoginStatus }`
  - `state::AppState { store: Store, http: reqwest::Client, creds: RwLock<Option<Credentials>>, config: RwLock<Config>, login: RwLock<Option<LoginSession>>, server: Mutex<Option<ServerHandle>> }`（tokio 锁）
  - `AppState::new(store: Store) -> Arc<AppState>`、`AppState::set_credentials(&self, Option<Credentials>) -> io::Result<()>`、`AppState::is_logged_in(&self) -> bool`
  - `server::ServerHandle { port: u16, .. }`，`ServerHandle::stop(self)`
  - `sender::SendFailure { NotLoggedIn, TokenExpired, RateLimited, Upstream(String) }`，`SendFailure::code(&self) -> &'static str`、`SendFailure::message(&self) -> String`
  - `sender::send_text(state: &AppState, to: Option<&str>, text: &str) -> Result<String, SendFailure>`（Ok 返回实际收件人 id）

- [ ] **Step 1: 写 state.rs 与 server/mod.rs（无逻辑，直接实现）**

新建 `src-tauri/src/server/mod.rs`：

```rust
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
```

新建 `src-tauri/src/state.rs`：

```rust
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
```

`src-tauri/src/lib.rs` 加：

```rust
pub mod sender;
pub mod server;
pub mod state;
```

- [ ] **Step 2: 写 sender.rs 失败测试**

新建 `src-tauri/src/sender.rs`，测试模块：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ilink::types::Credentials;
    use crate::store::Store;
    use std::sync::Arc;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn logged_in_state(base: &str) -> (tempfile::TempDir, Arc<AppState>) {
        let d = tempfile::tempdir().unwrap();
        let state = AppState::new(Store::new(d.path().to_path_buf()));
        state
            .set_credentials(Some(Credentials {
                bot_token: "t".into(),
                ilink_bot_id: "bot@im.bot".into(),
                ilink_user_id: "me@im.wechat".into(),
                baseurl: base.into(),
            }))
            .await
            .unwrap();
        (d, state)
    }

    fn ok_mock() -> Mock {
        Mock::given(method("POST"))
            .and(path("/ilink/bot/sendmessage"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ret": 0})))
    }

    #[tokio::test]
    async fn not_logged_in_fails_and_logs() {
        let d = tempfile::tempdir().unwrap();
        let state = AppState::new(Store::new(d.path().to_path_buf()));
        assert_eq!(send_text(&state, None, "hi").await, Err(SendFailure::NotLoggedIn));
        let logs = state.store.load_logs();
        assert_eq!(logs[0].code.as_deref(), Some("not_logged_in"));
        assert!(!logs[0].ok);
    }

    #[tokio::test]
    async fn defaults_to_login_user_when_no_to_and_no_default() {
        let server = MockServer::start().await;
        ok_mock()
            .and(body_partial_json(serde_json::json!({"msg": {"to_user_id": "me@im.wechat"}})))
            .expect(1)
            .mount(&server)
            .await;
        let (_d, state) = logged_in_state(&server.uri()).await;
        assert_eq!(send_text(&state, None, "hi").await.unwrap(), "me@im.wechat");
        let logs = state.store.load_logs();
        assert!(logs[0].ok);
        assert_eq!(logs[0].to, "me@im.wechat");
        assert_eq!(logs[0].text, "hi");
    }

    #[tokio::test]
    async fn uses_config_default_then_explicit_to() {
        let server = MockServer::start().await;
        ok_mock()
            .and(body_partial_json(serde_json::json!({"msg": {"to_user_id": "cfg@im.wechat"}})))
            .expect(1)
            .mount(&server)
            .await;
        ok_mock()
            .and(body_partial_json(serde_json::json!({"msg": {"to_user_id": "x@im.wechat"}})))
            .expect(1)
            .mount(&server)
            .await;
        let (_d, state) = logged_in_state(&server.uri()).await;
        state.config.write().await.default_recipient = Some("cfg@im.wechat".into());
        assert_eq!(send_text(&state, None, "a").await.unwrap(), "cfg@im.wechat");
        assert_eq!(send_text(&state, Some("x@im.wechat"), "b").await.unwrap(), "x@im.wechat");
        assert_eq!(send_text(&state, Some("  "), "c").await.unwrap(), "cfg@im.wechat");
    }

    #[tokio::test]
    async fn token_expired_clears_credentials() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ret": -14})))
            .mount(&server)
            .await;
        let (_d, state) = logged_in_state(&server.uri()).await;
        assert_eq!(send_text(&state, None, "hi").await, Err(SendFailure::TokenExpired));
        assert!(!state.is_logged_in().await);
        assert!(state.store.load_credentials().is_none());
        assert_eq!(state.store.load_logs()[0].code.as_deref(), Some("token_expired"));
    }

    #[tokio::test]
    async fn rate_limited_and_upstream_keep_credentials() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ret": -2})))
            .mount(&server)
            .await;
        let (_d, state) = logged_in_state(&server.uri()).await;
        assert_eq!(send_text(&state, None, "hi").await, Err(SendFailure::RateLimited));
        assert!(state.is_logged_in().await);

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(502))
            .mount(&server)
            .await;
        let (_d, state) = logged_in_state(&server.uri()).await;
        assert!(matches!(send_text(&state, None, "hi").await, Err(SendFailure::Upstream(_))));
        assert_eq!(state.store.load_logs()[0].code.as_deref(), Some("upstream_error"));
    }

    #[test]
    fn codes_are_stable() {
        assert_eq!(SendFailure::NotLoggedIn.code(), "not_logged_in");
        assert_eq!(SendFailure::TokenExpired.code(), "token_expired");
        assert_eq!(SendFailure::RateLimited.code(), "rate_limited");
        assert_eq!(SendFailure::Upstream("x".into()).code(), "upstream_error");
    }
}
```

- [ ] **Step 3: 跑测试确认失败**

Run: `cd src-tauri && cargo test sender::`
Expected: 编译失败，`send_text`/`SendFailure` 未定义。

- [ ] **Step 4: 实现 sender.rs**

测试模块之前写：

```rust
use crate::ilink::client::{IlinkClient, SendError};
use crate::state::AppState;
use crate::store::LogEntry;

#[derive(Debug, Clone, PartialEq)]
pub enum SendFailure {
    NotLoggedIn,
    TokenExpired,
    RateLimited,
    Upstream(String),
}

impl SendFailure {
    pub fn code(&self) -> &'static str {
        match self {
            SendFailure::NotLoggedIn => "not_logged_in",
            SendFailure::TokenExpired => "token_expired",
            SendFailure::RateLimited => "rate_limited",
            SendFailure::Upstream(_) => "upstream_error",
        }
    }

    pub fn message(&self) -> String {
        match self {
            SendFailure::NotLoggedIn => "尚未扫码登录".to_string(),
            SendFailure::TokenExpired => "登录已失效，请重新扫码".to_string(),
            SendFailure::RateLimited => "触发频率限制（约 7 条/5 分钟），稍后重试".to_string(),
            SendFailure::Upstream(m) => m.clone(),
        }
    }
}

/// webhook 与 UI 共用的发送入口：解析收件人 -> 调 iLink -> 写日志 -> -14 时清凭据。
pub async fn send_text(
    state: &AppState,
    to: Option<&str>,
    text: &str,
) -> Result<String, SendFailure> {
    let creds = state.creds.read().await.clone();
    let Some(creds) = creds else {
        let _ = state.store.append_log(LogEntry::now(
            false,
            Some(SendFailure::NotLoggedIn.code()),
            to.unwrap_or(""),
            text,
        ));
        return Err(SendFailure::NotLoggedIn);
    };

    let to = match to.map(str::trim).filter(|s| !s.is_empty()) {
        Some(t) => t.to_string(),
        None => state
            .config
            .read()
            .await
            .default_recipient
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| creds.ilink_user_id.clone()),
    };

    let client = IlinkClient::new(state.http.clone(), creds);
    let outcome = match client.send_text(&to, text, None).await {
        Ok(_) => Ok(to.clone()),
        Err(SendError::RateLimited) => Err(SendFailure::RateLimited),
        Err(SendError::TokenExpired) => Err(SendFailure::TokenExpired),
        Err(SendError::Upstream(m)) => Err(SendFailure::Upstream(m)),
    };

    let _ = state.store.append_log(LogEntry::now(
        outcome.is_ok(),
        outcome.as_ref().err().map(SendFailure::code),
        &to,
        text,
    ));

    if matches!(outcome, Err(SendFailure::TokenExpired)) {
        let _ = state.set_credentials(None).await;
    }
    outcome
}
```

- [ ] **Step 5: 跑测试确认通过**

Run: `cd src-tauri && cargo test sender:: && cargo test`
Expected: sender 6 passed；全量测试全部通过。

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src
git commit -m "feat: 共享状态与发送服务（收件人解析、错误映射、日志）"
```

---

### Task 7: API key 鉴权中间件

**Files:**
- Create: `src-tauri/src/server/middleware.rs`
- Modify: `src-tauri/src/server/mod.rs`（加 `pub mod middleware;`）

**Interfaces:**
- Consumes: Task 6 `state::AppState`、Task 5 `store::LogEntry`
- Produces:
  - `middleware::extract_key(req: &axum::extract::Request) -> Option<String>`
  - `middleware::key_matches(provided: &str, expected: &str) -> bool`（常数时间）
  - `middleware::require_api_key(State<Arc<AppState>>, Request, Next) -> Response`（用于 `axum::middleware::from_fn_with_state`）
  - `middleware::unauthorized() -> Response`（401 + `{"ok":false,"code":"unauthorized"}`）

- [ ] **Step 1: 写失败测试**

新建 `src-tauri/src/server/middleware.rs`，测试模块：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::{Request, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use http_body_util::BodyExt;
    use std::net::SocketAddr;
    use tower::ServiceExt;

    fn app() -> (tempfile::TempDir, Arc<AppState>, Router) {
        let d = tempfile::tempdir().unwrap();
        let state = AppState::new(Store::new(d.path().to_path_buf()));
        let router = Router::new()
            .route("/p", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn_with_state(state.clone(), require_api_key))
            .with_state(state.clone());
        (d, state, router)
    }

    async fn key(state: &AppState) -> String {
        state.config.read().await.api_key.clone()
    }

    #[tokio::test]
    async fn accepts_x_api_key_and_bearer() {
        let (_d, state, router) = app();
        let k = key(&state).await;

        let r = router
            .clone()
            .oneshot(Request::get("/p").header("x-api-key", &k).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);

        let r = router
            .oneshot(
                Request::get("/p")
                    .header("authorization", format!("Bearer {k}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn rejects_missing_or_wrong_key_with_json_and_logs_ip() {
        let (_d, state, router) = app();

        let r = router
            .clone()
            .oneshot(Request::get("/p").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
        let body = r.into_body().collect().await.unwrap().to_bytes();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["ok"], false);
        assert_eq!(v["code"], "unauthorized");

        let mut req = Request::get("/p").header("x-api-key", "wrong").body(Body::empty()).unwrap();
        req.extensions_mut()
            .insert(ConnectInfo(SocketAddr::from(([192, 168, 1, 5], 40000))));
        let r = router.oneshot(req).await.unwrap();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);

        let logs = state.store.load_logs();
        assert_eq!(logs[0].code.as_deref(), Some("unauthorized"));
        assert!(logs[0].text.contains("192.168.1.5"));
        assert!(!logs[0].text.contains("wrong"), "日志不能记录 key");
    }

    #[test]
    fn key_matches_is_exact() {
        assert!(key_matches("abc", "abc"));
        assert!(!key_matches("abc", "abd"));
        assert!(!key_matches("ab", "abc"));
        assert!(!key_matches("", "abc"));
    }
}
```

`src-tauri/src/server/mod.rs` 顶部加 `pub mod middleware;`。

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test server::middleware`
Expected: 编译失败。

- [ ] **Step 3: 实现 middleware.rs**

测试模块之前写：

```rust
use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use subtle::ConstantTimeEq;

use crate::state::AppState;
use crate::store::LogEntry;

pub fn extract_key(req: &Request) -> Option<String> {
    if let Some(v) = req.headers().get("x-api-key").and_then(|v| v.to_str().ok()) {
        let v = v.trim();
        if !v.is_empty() {
            return Some(v.to_string());
        }
    }
    let auth = req.headers().get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = auth.trim().split_once(' ')?;
    if scheme.eq_ignore_ascii_case("bearer") && !token.trim().is_empty() {
        Some(token.trim().to_string())
    } else {
        None
    }
}

/// 常数时间比较；长度不同直接判否（subtle 对不同长度返回 false）。
pub fn key_matches(provided: &str, expected: &str) -> bool {
    provided.as_bytes().ct_eq(expected.as_bytes()).into()
}

pub fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({ "ok": false, "code": "unauthorized" })),
    )
        .into_response()
}

pub async fn require_api_key(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Response {
    let expected = state.config.read().await.api_key.clone();
    let ok = extract_key(&req)
        .map(|k| key_matches(&k, &expected))
        .unwrap_or(false);
    if ok {
        return next.run(req).await;
    }
    let ip = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let _ = state.store.append_log(LogEntry::now(
        false,
        Some("unauthorized"),
        "",
        &format!("鉴权失败，来源 {ip}"),
    ));
    unauthorized()
}
```

- [ ] **Step 4: 跑测试确认通过**

Run: `cd src-tauri && cargo test server::middleware`
Expected: 3 passed。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src
git commit -m "feat(server): API key 鉴权中间件"
```

---

### Task 8: Webhook 路由与服务启停

**Files:**
- Create: `src-tauri/src/server/routes.rs`
- Modify: `src-tauri/src/server/mod.rs`（加 `pub mod routes;` 与 `start()`）

**Interfaces:**
- Consumes: Task 6 `sender::{send_text, SendFailure}`、`state::AppState`；Task 7 `middleware::require_api_key`
- Produces:
  - `routes::router(state: Arc<AppState>) -> axum::Router`
  - `routes::SendReq { to: Option<String>, text: Option<String> }`
  - `routes::failure_status(&SendFailure) -> StatusCode`（TokenExpired/NotLoggedIn → 503，RateLimited → 429，Upstream → 502）
  - `server::start(state: Arc<AppState>, port: u16) -> io::Result<ServerHandle>`（绑定 `0.0.0.0:port`；`port=0` 时随机端口，`handle.port` 为实际端口）

- [ ] **Step 1: 写 routes 失败测试**

新建 `src-tauri/src/server/routes.rs`，测试模块：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ilink::types::Credentials;
    use crate::store::Store;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn state_with(base: Option<&str>) -> (tempfile::TempDir, Arc<AppState>) {
        let d = tempfile::tempdir().unwrap();
        let state = AppState::new(Store::new(d.path().to_path_buf()));
        if let Some(b) = base {
            state
                .set_credentials(Some(Credentials {
                    bot_token: "t".into(),
                    ilink_bot_id: "bot@im.bot".into(),
                    ilink_user_id: "me@im.wechat".into(),
                    baseurl: b.into(),
                }))
                .await
                .unwrap();
        }
        (d, state)
    }

    async fn post_send(state: &Arc<AppState>, key: Option<&str>, body: &str) -> (StatusCode, serde_json::Value) {
        let mut req = Request::post("/send").header("content-type", "application/json");
        if let Some(k) = key {
            req = req.header("x-api-key", k);
        }
        let r = router(state.clone())
            .oneshot(req.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = r.status();
        let bytes = r.into_body().collect().await.unwrap().to_bytes();
        let v = serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, v)
    }

    async fn mock_ret(ret: i64) -> MockServer {
        let s = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({ "ret": ret })))
            .mount(&s)
            .await;
        s
    }

    #[tokio::test]
    async fn health_is_public_and_reports_login() {
        let (_d, state) = state_with(None).await;
        let r = router(state.clone())
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::OK);
        let v: serde_json::Value =
            serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap();
        assert_eq!(v["ok"], true);
        assert_eq!(v["logged_in"], false);
        assert_eq!(v["version"], env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn send_requires_key() {
        let (_d, state) = state_with(None).await;
        let (s, v) = post_send(&state, None, r#"{"text":"hi"}"#).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
        assert_eq!(v["code"], "unauthorized");
    }

    #[tokio::test]
    async fn bad_request_on_missing_text_or_invalid_json() {
        let (_d, state) = state_with(None).await;
        let k = state.config.read().await.api_key.clone();
        let (s, v) = post_send(&state, Some(&k), r#"{"to":"x"}"#).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert_eq!(v["code"], "bad_request");
        let (s, v) = post_send(&state, Some(&k), r#"{"text":"   "}"#).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert_eq!(v["code"], "bad_request");
        let (s, v) = post_send(&state, Some(&k), "not json").await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert_eq!(v["code"], "bad_request");
    }

    #[tokio::test]
    async fn not_logged_in_is_503() {
        let (_d, state) = state_with(None).await;
        let k = state.config.read().await.api_key.clone();
        let (s, v) = post_send(&state, Some(&k), r#"{"text":"hi"}"#).await;
        assert_eq!(s, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(v["code"], "not_logged_in");
    }

    #[tokio::test]
    async fn success_returns_200_ok_true() {
        let m = mock_ret(0).await;
        let (_d, state) = state_with(Some(&m.uri())).await;
        let k = state.config.read().await.api_key.clone();
        let (s, v) = post_send(&state, Some(&k), r#"{"text":"hi"}"#).await;
        assert_eq!(s, StatusCode::OK);
        assert_eq!(v["ok"], true);
        assert_eq!(v["to"], "me@im.wechat");
    }

    #[tokio::test]
    async fn upstream_codes_map_to_http() {
        for (ret, status, code) in [
            (-2, StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
            (-14, StatusCode::SERVICE_UNAVAILABLE, "token_expired"),
            (-99, StatusCode::BAD_GATEWAY, "upstream_error"),
        ] {
            let m = mock_ret(ret).await;
            let (_d, state) = state_with(Some(&m.uri())).await;
            let k = state.config.read().await.api_key.clone();
            let (s, v) = post_send(&state, Some(&k), r#"{"text":"hi"}"#).await;
            assert_eq!(s, status, "ret={ret}");
            assert_eq!(v["ok"], false);
            assert_eq!(v["code"], code);
            assert!(v["error"].as_str().map(|e| !e.is_empty()).unwrap_or(false));
        }
    }
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cd src-tauri && cargo test server::routes`
Expected: 编译失败，`router` 未定义。

- [ ] **Step 3: 实现 routes.rs**

测试模块之前写：

```rust
use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::{DefaultBodyLimit, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use super::middleware::require_api_key;
use crate::sender::{send_text, SendFailure};
use crate::state::AppState;

pub const MAX_BODY_BYTES: usize = 1024 * 1024;

#[derive(Debug, Deserialize)]
pub struct SendReq {
    pub to: Option<String>,
    pub text: Option<String>,
}

pub fn failure_status(f: &SendFailure) -> StatusCode {
    match f {
        SendFailure::NotLoggedIn | SendFailure::TokenExpired => StatusCode::SERVICE_UNAVAILABLE,
        SendFailure::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        SendFailure::Upstream(_) => StatusCode::BAD_GATEWAY,
    }
}

pub fn router(state: Arc<AppState>) -> Router {
    let protected = Router::new()
        .route("/send", post(send))
        .layer(axum::middleware::from_fn_with_state(state.clone(), require_api_key));
    Router::new()
        .route("/health", get(health))
        .merge(protected)
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
        .with_state(state)
}

fn bad_request(msg: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "ok": false, "code": "bad_request", "error": msg })),
    )
}

async fn send(
    State(state): State<Arc<AppState>>,
    body: Result<Json<SendReq>, JsonRejection>,
) -> (StatusCode, Json<Value>) {
    let Json(req) = match body {
        Ok(b) => b,
        Err(e) => return bad_request(&format!("invalid JSON body: {e}")),
    };
    let text = match req.text.as_deref().map(str::trim) {
        Some(t) if !t.is_empty() => t.to_string(),
        _ => return bad_request("text is required"),
    };
    match send_text(&state, req.to.as_deref(), &text).await {
        Ok(to) => (StatusCode::OK, Json(json!({ "ok": true, "to": to }))),
        Err(f) => (
            failure_status(&f),
            Json(json!({ "ok": false, "code": f.code(), "error": f.message() })),
        ),
    }
}

async fn health(State(state): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({
        "ok": true,
        "logged_in": state.is_logged_in().await,
        "version": env!("CARGO_PKG_VERSION"),
    }))
}
```

- [ ] **Step 4: 跑 routes 测试确认通过**

Run: `cd src-tauri && cargo test server::routes`
Expected: 6 passed。

- [ ] **Step 5: 写 server::start 的失败测试**

在 `src-tauri/src/server/mod.rs` 末尾加：

```rust
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

        let v: serde_json::Value = reqwest::get(&url).await.unwrap().json().await.unwrap();
        assert_eq!(v["ok"], true);

        handle.stop();
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(reqwest::get(&url).await.is_err(), "停止后应拒绝连接");
    }

    #[tokio::test]
    async fn port_in_use_is_an_error() {
        let d = tempfile::tempdir().unwrap();
        let state = AppState::new(Store::new(d.path().to_path_buf()));
        let first = start(state.clone(), 0).await.unwrap();
        assert!(start(state, first.port).await.is_err());
    }
}
```

- [ ] **Step 6: 实现 start()**

`src-tauri/src/server/mod.rs` 顶部改为：

```rust
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
```

（`ServerHandle` 定义保持 Task 6 的内容不变。）

- [ ] **Step 7: 跑全部测试**

Run: `cd src-tauri && cargo test`
Expected: 全部通过（含 server 2 个）。

- [ ] **Step 8: 提交**

```bash
git add src-tauri/src
git commit -m "feat(server): /send 与 /health 路由、服务启停"
```

---

### Task 9: Tauri 命令与应用装配（后端完成）

**Files:**
- Create: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`（完整替换）

**Interfaces:**
- Consumes: Task 4 `auth::*`、Task 6 `state::*`、`sender`、Task 8 `server::start`、Task 5 `store::*`
- Produces（前端 invoke 名 = 函数名；Rust 参数 snake_case 在 JS 侧自动变 camelCase）：
  - `get_status() -> StatusInfo { logged_in: bool, bot_id: Option<String>, user_id: Option<String>, server_running: bool, port: u16, version: String }`
  - `login_start() -> Result<LoginView { svg: String, status: LoginStatus }, String>`
  - `login_status() -> Option<LoginView>`
  - `logout() -> Result<(), String>`
  - `get_config() -> Config`
  - `save_recipients(recipients: Vec<Recipient>, default_recipient: Option<String>) -> Result<Config, String>`
  - `set_port(port: u16) -> Result<StatusInfo, String>`（保存并重启服务）
  - `reset_api_key() -> Result<String, String>`
  - `server_start() -> Result<StatusInfo, String>` / `server_stop() -> Result<StatusInfo, String>`
  - `list_logs() -> Vec<LogEntry>`
  - `commands::start_server(&Arc<AppState>) -> Result<u16, String>`（lib.rs 启动时也用）

- [ ] **Step 1: 写 commands.rs**

```rust
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
```

- [ ] **Step 2: 替换 lib.rs**

```rust
pub mod commands;
pub mod ilink;
pub mod sender;
pub mod server;
pub mod state;
pub mod store;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            // Windows 上是 %APPDATA%\com.liuli.weixin-clawbot-webhook\
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let state = state::AppState::new(store::Store::new(dir));
            app.manage(state.clone());
            tauri::async_runtime::spawn(async move {
                if let Err(e) = commands::start_server(&state).await {
                    eprintln!("webhook 服务启动失败：{e}");
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::login_start,
            commands::login_status,
            commands::logout,
            commands::get_config,
            commands::save_recipients,
            commands::set_port,
            commands::reset_api_key,
            commands::server_start,
            commands::server_stop,
            commands::list_logs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

- [ ] **Step 3: 编译 + 全量测试**

Run: `cd src-tauri && cargo test`
Expected: 编译通过，全部测试 passed（无 warning 最好；有 unused 警告就删掉对应 import）。

- [ ] **Step 4: 手工验证：应用能起、webhook 能通**

在项目根：`pnpm tauri dev`。窗口弹出（内容还是占位）。另开终端：

```bash
curl -s http://127.0.0.1:9720/health
curl -s -X POST http://127.0.0.1:9720/send -H "Content-Type: application/json" -d '{"text":"hi"}'
```

Expected：第一条 `{"ok":true,"logged_in":false,"version":"0.1.0"}`；第二条 HTTP 401 `{"ok":false,"code":"unauthorized"}`。Windows 首次会弹防火墙提示，允许即可。关闭窗口结束。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src
git commit -m "feat: Tauri 命令与应用装配，启动时拉起 webhook 服务"
```

---

### Task 10: 前端 API 封装 + 应用外壳 + 占位页面

**Files:**
- Create: `src/lib/api.ts`、`src/lib/pages/Login.svelte`、`src/lib/pages/Recipients.svelte`、`src/lib/pages/Settings.svelte`、`src/lib/pages/Logs.svelte`（本 task 为占位，后续 task 逐个替换）
- Modify: `src/routes/+page.svelte`

**Interfaces:**
- Consumes: Task 9 全部命令
- Produces: `api` 对象与 TS 类型（`StatusInfo`、`LoginView`、`LoginStatus`、`Config`、`Recipient`、`LogEntry`）；页面 props 约定：`Login`/`Settings` 接收 `{ status: StatusInfo | null; onchange: () => void }`，`Recipients` 接收 `{ status: StatusInfo | null }`，`Logs` 无 props。

- [ ] **Step 1: 写 api.ts**

```ts
import { invoke } from "@tauri-apps/api/core";

export type LoginStatus =
  | { state: "wait" }
  | { state: "scanned" }
  | { state: "confirmed" }
  | { state: "expired" }
  | { state: "error"; message: string };

export interface LoginView {
  svg: string;
  status: LoginStatus;
}

export interface StatusInfo {
  logged_in: boolean;
  bot_id: string | null;
  user_id: string | null;
  server_running: boolean;
  port: number;
  version: string;
}

export interface Recipient {
  id: string;
  name: string;
}

export interface Config {
  port: number;
  api_key: string;
  default_recipient: string | null;
  recipients: Recipient[];
}

export interface LogEntry {
  ts: string;
  ok: boolean;
  code: string | null;
  to: string;
  text: string;
}

export const api = {
  getStatus: () => invoke<StatusInfo>("get_status"),
  loginStart: () => invoke<LoginView>("login_start"),
  loginStatus: () => invoke<LoginView | null>("login_status"),
  logout: () => invoke<void>("logout"),
  getConfig: () => invoke<Config>("get_config"),
  saveRecipients: (recipients: Recipient[], defaultRecipient: string | null) =>
    invoke<Config>("save_recipients", { recipients, defaultRecipient }),
  setPort: (port: number) => invoke<StatusInfo>("set_port", { port }),
  resetApiKey: () => invoke<string>("reset_api_key"),
  serverStart: () => invoke<StatusInfo>("server_start"),
  serverStop: () => invoke<StatusInfo>("server_stop"),
  listLogs: () => invoke<LogEntry[]>("list_logs"),
};
```

- [ ] **Step 2: 写四个占位页面**

`src/lib/pages/Login.svelte`：

```svelte
<script lang="ts">
  import type { StatusInfo } from "$lib/api";
  let { status, onchange }: { status: StatusInfo | null; onchange: () => void } = $props();
</script>

<h2 class="text-lg font-semibold">登录</h2>
```

`src/lib/pages/Settings.svelte`：

```svelte
<script lang="ts">
  import type { StatusInfo } from "$lib/api";
  let { status, onchange }: { status: StatusInfo | null; onchange: () => void } = $props();
</script>

<h2 class="text-lg font-semibold">设置</h2>
```

`src/lib/pages/Recipients.svelte`：

```svelte
<script lang="ts">
  import type { StatusInfo } from "$lib/api";
  let { status }: { status: StatusInfo | null } = $props();
</script>

<h2 class="text-lg font-semibold">收件人</h2>
```

`src/lib/pages/Logs.svelte`：

```svelte
<h2 class="text-lg font-semibold">日志</h2>
```

- [ ] **Step 3: 写应用外壳 +page.svelte**

```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import { api, type StatusInfo } from "$lib/api";
  import Login from "$lib/pages/Login.svelte";
  import Recipients from "$lib/pages/Recipients.svelte";
  import Settings from "$lib/pages/Settings.svelte";
  import Logs from "$lib/pages/Logs.svelte";

  type Page = "login" | "recipients" | "settings" | "logs";
  let page = $state<Page>("login");
  let status = $state<StatusInfo | null>(null);

  async function refresh() {
    try {
      status = await api.getStatus();
    } catch (e) {
      console.error(e);
    }
  }

  onMount(() => {
    refresh();
    const t = setInterval(refresh, 3000);
    return () => clearInterval(t);
  });

  const nav: { id: Page; label: string }[] = [
    { id: "login", label: "登录" },
    { id: "recipients", label: "收件人" },
    { id: "settings", label: "设置" },
    { id: "logs", label: "日志" },
  ];
</script>

<div class="flex h-screen bg-slate-50 text-slate-800">
  <aside class="flex w-52 shrink-0 flex-col bg-slate-900 text-slate-200">
    <div class="border-b border-slate-800 px-5 py-5">
      <div class="text-base font-semibold text-white">ClawBot Webhook</div>
      <div class="mt-1 text-xs text-slate-400">v{status?.version ?? "-"}</div>
    </div>
    <nav class="flex-1 py-3">
      {#each nav as item (item.id)}
        <button
          class="w-full px-5 py-2.5 text-left text-sm transition hover:bg-slate-800 {page === item.id
            ? 'bg-slate-800 font-medium text-white'
            : ''}"
          onclick={() => (page = item.id)}
        >
          {item.label}
        </button>
      {/each}
    </nav>
    <div class="space-y-1.5 border-t border-slate-800 px-5 py-4 text-xs">
      <div class="flex items-center gap-2">
        <span class="h-2 w-2 rounded-full {status?.logged_in ? 'bg-emerald-400' : 'bg-rose-400'}"></span>
        {status?.logged_in ? "已登录" : "未登录"}
      </div>
      <div class="flex items-center gap-2">
        <span class="h-2 w-2 rounded-full {status?.server_running ? 'bg-emerald-400' : 'bg-slate-500'}"></span>
        {status?.server_running ? `服务运行中 :${status.port}` : "服务已停止"}
      </div>
    </div>
  </aside>

  <main class="flex-1 overflow-auto p-8">
    {#if page === "login"}
      <Login {status} onchange={refresh} />
    {:else if page === "recipients"}
      <Recipients {status} />
    {:else if page === "settings"}
      <Settings {status} onchange={refresh} />
    {:else}
      <Logs />
    {/if}
  </main>
</div>
```

- [ ] **Step 4: 类型检查 + 手工看一眼**

```bash
pnpm check
pnpm tauri dev
```

Expected：`pnpm check` 0 errors（占位页面里未使用的 props 只会是 warning）；窗口左侧深色导航、底部两个状态点，点击导航切换标题；未登录红点、服务运行绿点 `:9720`。

- [ ] **Step 5: 提交**

```bash
git add src
git commit -m "feat(ui): 前端 API 封装、应用外壳与导航"
```

---

### Task 11: 登录页（二维码 + 状态轮询 + 退出）

**Files:**
- Modify: `src/lib/pages/Login.svelte`（整个替换）

**Interfaces:**
- Consumes: `api.loginStart / loginStatus / logout`，props `{ status, onchange }`

- [ ] **Step 1: 写 Login.svelte**

```svelte
<script lang="ts">
  import { onDestroy } from "svelte";
  import { api, type LoginView, type StatusInfo } from "$lib/api";

  let { status, onchange }: { status: StatusInfo | null; onchange: () => void } = $props();

  let view = $state<LoginView | null>(null);
  let busy = $state(false);
  let error = $state("");
  let timer: ReturnType<typeof setInterval> | null = null;

  const label: Record<string, string> = {
    wait: "请用微信「扫一扫」",
    scanned: "已扫码，请在手机上点确认",
    confirmed: "登录成功",
    expired: "二维码已过期，请刷新",
    error: "登录出错",
  };

  function stopPolling() {
    if (timer) {
      clearInterval(timer);
      timer = null;
    }
  }

  async function poll() {
    try {
      const v = await api.loginStatus();
      if (!v) return;
      view = v;
      const s = v.status.state;
      if (s === "confirmed") {
        stopPolling();
        onchange();
      } else if (s === "expired" || s === "error") {
        stopPolling();
      }
    } catch (e) {
      error = String(e);
      stopPolling();
    }
  }

  async function start() {
    busy = true;
    error = "";
    try {
      view = await api.loginStart();
      stopPolling();
      timer = setInterval(poll, 1500);
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function logout() {
    stopPolling();
    view = null;
    await api.logout();
    onchange();
  }

  onDestroy(stopPolling);
</script>

<h2 class="text-lg font-semibold">登录</h2>
<p class="mt-1 text-sm text-slate-500">用微信扫码授权后，本机才能通过 ClawBot 发消息。</p>

{#if status?.logged_in}
  <div class="mt-6 max-w-md rounded-xl border border-emerald-200 bg-emerald-50 p-5">
    <div class="flex items-center gap-2 text-emerald-700">
      <span class="h-2.5 w-2.5 rounded-full bg-emerald-500"></span>
      <span class="font-medium">已登录</span>
    </div>
    <dl class="mt-3 space-y-1 text-sm">
      <div class="flex gap-3"><dt class="w-20 text-slate-500">Bot ID</dt><dd class="font-mono">{status.bot_id}</dd></div>
      <div class="flex gap-3"><dt class="w-20 text-slate-500">扫码账号</dt><dd class="font-mono">{status.user_id}</dd></div>
    </dl>
    <p class="mt-3 text-xs text-slate-500">未指定收件人时，消息默认发给扫码账号。</p>
    <button class="mt-4 rounded-md border border-slate-300 bg-white px-3 py-1.5 text-sm hover:bg-slate-100" onclick={logout}>
      退出登录
    </button>
  </div>
{:else}
  <div class="mt-6 max-w-md rounded-xl border border-slate-200 bg-white p-6">
    {#if view}
      <div class="flex flex-col items-center">
        <div class="rounded-lg border border-slate-200 p-2 [&>svg]:h-60 [&>svg]:w-60">
          {@html view.svg}
        </div>
        <p class="mt-4 text-sm {view.status.state === 'error' || view.status.state === 'expired' ? 'text-rose-600' : 'text-slate-700'}">
          {label[view.status.state]}
          {#if view.status.state === "error"}：{view.status.message}{/if}
        </p>
        {#if view.status.state === "expired" || view.status.state === "error"}
          <button class="mt-3 rounded-md bg-slate-900 px-4 py-1.5 text-sm text-white hover:bg-slate-700" onclick={start} disabled={busy}>
            刷新二维码
          </button>
        {/if}
      </div>
    {:else}
      <p class="text-sm text-slate-600">点击下方按钮获取二维码。二维码约 5 分钟内有效。</p>
      <button class="mt-4 rounded-md bg-slate-900 px-4 py-2 text-sm text-white hover:bg-slate-700 disabled:opacity-50" onclick={start} disabled={busy}>
        {busy ? "获取中…" : "扫码登录"}
      </button>
    {/if}
    {#if error}
      <p class="mt-3 text-sm text-rose-600">{error}</p>
    {/if}
  </div>
{/if}
```

- [ ] **Step 2: 类型检查 + 真机验证**

```bash
pnpm check
pnpm tauri dev
```

Expected：点“扫码登录”出现二维码；用微信扫 → 文案变“已扫码”→ 手机确认 → “登录成功”，侧栏变绿“已登录”，页面切成已登录卡片，显示 Bot ID 与账号；`%APPDATA%\com.liuli.weixin-clawbot-webhook\credentials.json` 生成。点“退出登录”回到扫码入口，文件被删。

若扫码后手机提示需要配对码/重定向等本项目未处理的状态，页面会停在“请用微信扫一扫”——记录下来作为后续迭代，v1 不处理。

- [ ] **Step 3: 提交**

```bash
git add src
git commit -m "feat(ui): 扫码登录页"
```

---

### Task 12: 收件人页

**Files:**
- Modify: `src/lib/pages/Recipients.svelte`（整个替换）

**Interfaces:**
- Consumes: `api.getConfig / saveRecipients`，props `{ status }`

- [ ] **Step 1: 写 Recipients.svelte**

```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import { api, type Recipient, type StatusInfo } from "$lib/api";

  let { status }: { status: StatusInfo | null } = $props();

  let recipients = $state<Recipient[]>([]);
  let defaultRecipient = $state<string | null>(null);
  let newId = $state("");
  let newName = $state("");
  let message = $state("");
  let error = $state("");

  async function load() {
    const cfg = await api.getConfig();
    recipients = cfg.recipients;
    defaultRecipient = cfg.default_recipient;
  }

  async function save() {
    error = "";
    try {
      const cfg = await api.saveRecipients(recipients, defaultRecipient);
      recipients = cfg.recipients;
      defaultRecipient = cfg.default_recipient;
      message = "已保存";
      setTimeout(() => (message = ""), 1500);
    } catch (e) {
      error = String(e);
    }
  }

  async function add() {
    const id = newId.trim();
    if (!id) {
      error = "请输入收件人 ID";
      return;
    }
    if (recipients.some((r) => r.id === id)) {
      error = "该 ID 已存在";
      return;
    }
    recipients = [...recipients, { id, name: newName.trim() || id }];
    newId = "";
    newName = "";
    await save();
  }

  async function remove(id: string) {
    recipients = recipients.filter((r) => r.id !== id);
    if (defaultRecipient === id) defaultRecipient = null;
    await save();
  }

  async function setDefault(id: string | null) {
    defaultRecipient = id;
    await save();
  }

  onMount(load);
</script>

<h2 class="text-lg font-semibold">收件人</h2>
<p class="mt-1 text-sm text-slate-500">
  iLink 没有好友列表接口，收件人 ID（形如 <code class="rounded bg-slate-100 px-1">xxx@im.wechat</code>）需要手动录入。
  webhook 请求不带 <code class="rounded bg-slate-100 px-1">to</code> 时发给默认收件人。
</p>

<div class="mt-6 max-w-2xl overflow-hidden rounded-xl border border-slate-200 bg-white">
  <table class="w-full text-sm">
    <thead class="bg-slate-50 text-left text-xs uppercase text-slate-500">
      <tr>
        <th class="px-4 py-2.5">默认</th>
        <th class="px-4 py-2.5">备注</th>
        <th class="px-4 py-2.5">ID</th>
        <th class="px-4 py-2.5"></th>
      </tr>
    </thead>
    <tbody>
      <tr class="border-t border-slate-100">
        <td class="px-4 py-2.5">
          <input type="radio" name="default" checked={defaultRecipient === null} onchange={() => setDefault(null)} />
        </td>
        <td class="px-4 py-2.5">扫码账号（自己）</td>
        <td class="px-4 py-2.5 font-mono text-slate-600">{status?.user_id ?? "未登录"}</td>
        <td></td>
      </tr>
      {#each recipients as r (r.id)}
        <tr class="border-t border-slate-100">
          <td class="px-4 py-2.5">
            <input type="radio" name="default" checked={defaultRecipient === r.id} onchange={() => setDefault(r.id)} />
          </td>
          <td class="px-4 py-2.5">{r.name}</td>
          <td class="px-4 py-2.5 font-mono text-slate-600">{r.id}</td>
          <td class="px-4 py-2.5 text-right">
            <button class="text-xs text-rose-600 hover:underline" onclick={() => remove(r.id)}>删除</button>
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
</div>

<form class="mt-4 flex max-w-2xl items-end gap-3" onsubmit={(e) => { e.preventDefault(); add(); }}>
  <label class="flex-1 text-xs text-slate-500">
    收件人 ID
    <input class="mt-1 w-full rounded-md border border-slate-300 px-3 py-1.5 font-mono text-sm" placeholder="xxx@im.wechat" bind:value={newId} />
  </label>
  <label class="w-40 text-xs text-slate-500">
    备注
    <input class="mt-1 w-full rounded-md border border-slate-300 px-3 py-1.5 text-sm" placeholder="可选" bind:value={newName} />
  </label>
  <button type="submit" class="rounded-md bg-slate-900 px-4 py-2 text-sm text-white hover:bg-slate-700">添加</button>
</form>

{#if message}<p class="mt-3 text-sm text-emerald-600">{message}</p>{/if}
{#if error}<p class="mt-3 text-sm text-rose-600">{error}</p>{/if}
```

- [ ] **Step 2: 验证**

```bash
pnpm check
pnpm tauri dev
```

Expected：添加一条 `test@im.wechat` 出现在表里；选它为默认后 `config.json` 里 `default_recipient` 变为该 ID；删除后回到“扫码账号”默认；重开应用数据仍在。

- [ ] **Step 3: 提交**

```bash
git add src
git commit -m "feat(ui): 收件人管理页"
```

---

### Task 13: 设置页（端口、API key、服务开关、curl 示例）

**Files:**
- Modify: `src/lib/pages/Settings.svelte`（整个替换）

**Interfaces:**
- Consumes: `api.getConfig / setPort / resetApiKey / serverStart / serverStop`，props `{ status, onchange }`

- [ ] **Step 1: 写 Settings.svelte**

```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import { api, type StatusInfo } from "$lib/api";

  let { status, onchange }: { status: StatusInfo | null; onchange: () => void } = $props();

  let apiKey = $state("");
  let port = $state(9720);
  let showKey = $state(false);
  let confirmReset = $state(false);
  let busy = $state(false);
  let message = $state("");
  let error = $state("");

  async function load() {
    const cfg = await api.getConfig();
    apiKey = cfg.api_key;
    port = cfg.port;
  }

  function flash(m: string) {
    message = m;
    setTimeout(() => (message = ""), 1500);
  }

  async function run(fn: () => Promise<unknown>, ok: string) {
    busy = true;
    error = "";
    try {
      await fn();
      flash(ok);
      onchange();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function savePort() {
    if (!Number.isInteger(port) || port < 1 || port > 65535) {
      error = "端口必须是 1-65535 的整数";
      return;
    }
    await run(() => api.setPort(port), `已保存并在 :${port} 重启服务`);
  }

  async function resetKey() {
    if (!confirmReset) {
      confirmReset = true;
      return;
    }
    confirmReset = false;
    await run(async () => (apiKey = await api.resetApiKey()), "API key 已重置，旧 key 立即失效");
  }

  async function toggleServer() {
    if (status?.server_running) await run(() => api.serverStop(), "服务已停止");
    else await run(() => api.serverStart(), "服务已启动");
  }

  async function copy(text: string) {
    await navigator.clipboard.writeText(text);
    flash("已复制");
  }

  const masked = $derived(showKey ? apiKey : apiKey.replace(/./g, "•"));
  const curl = $derived(
    `curl -X POST http://<本机IP>:${status?.port ?? port}/send -H "X-API-Key: ${apiKey}" -H "Content-Type: application/json" -d '{"text":"你好"}'`,
  );

  onMount(load);
</script>

<h2 class="text-lg font-semibold">设置</h2>

<section class="mt-6 max-w-2xl rounded-xl border border-slate-200 bg-white p-5">
  <h3 class="font-medium">Webhook 服务</h3>
  <div class="mt-3 flex items-center gap-3">
    <span class="h-2.5 w-2.5 rounded-full {status?.server_running ? 'bg-emerald-500' : 'bg-slate-400'}"></span>
    <span class="text-sm">{status?.server_running ? `运行中，监听 0.0.0.0:${status.port}` : "已停止"}</span>
    <button class="ml-auto rounded-md border border-slate-300 px-3 py-1.5 text-sm hover:bg-slate-100 disabled:opacity-50" onclick={toggleServer} disabled={busy}>
      {status?.server_running ? "停止" : "启动"}
    </button>
  </div>
  <div class="mt-4 flex items-end gap-3">
    <label class="text-xs text-slate-500">
      端口
      <input type="number" min="1" max="65535" class="mt-1 w-32 rounded-md border border-slate-300 px-3 py-1.5 text-sm" bind:value={port} />
    </label>
    <button class="rounded-md bg-slate-900 px-4 py-2 text-sm text-white hover:bg-slate-700 disabled:opacity-50" onclick={savePort} disabled={busy}>
      保存并重启
    </button>
  </div>
</section>

<section class="mt-4 max-w-2xl rounded-xl border border-slate-200 bg-white p-5">
  <h3 class="font-medium">API key</h3>
  <p class="mt-1 text-xs text-slate-500">调用 /send 时放在 <code>X-API-Key</code> 头或 <code>Authorization: Bearer</code>。</p>
  <div class="mt-3 flex items-center gap-2">
    <code class="flex-1 truncate rounded-md bg-slate-100 px-3 py-2 font-mono text-sm">{masked}</code>
    <button class="rounded-md border border-slate-300 px-3 py-1.5 text-sm hover:bg-slate-100" onclick={() => (showKey = !showKey)}>{showKey ? "隐藏" : "显示"}</button>
    <button class="rounded-md border border-slate-300 px-3 py-1.5 text-sm hover:bg-slate-100" onclick={() => copy(apiKey)}>复制</button>
    <button class="rounded-md border px-3 py-1.5 text-sm {confirmReset ? 'border-rose-400 bg-rose-50 text-rose-700' : 'border-slate-300 hover:bg-slate-100'}" onclick={resetKey} disabled={busy}>
      {confirmReset ? "确认重置？" : "重置"}
    </button>
  </div>
</section>

<section class="mt-4 max-w-2xl rounded-xl border border-slate-200 bg-white p-5">
  <h3 class="font-medium">调用示例</h3>
  <p class="mt-1 text-xs text-slate-500">把 &lt;本机IP&gt; 换成这台电脑的局域网 IP（`ipconfig` 查看）。</p>
  <pre class="mt-3 overflow-x-auto rounded-md bg-slate-900 p-3 text-xs text-slate-100">{curl}</pre>
  <button class="mt-2 rounded-md border border-slate-300 px-3 py-1.5 text-sm hover:bg-slate-100" onclick={() => copy(curl)}>复制命令</button>
  <table class="mt-4 w-full text-xs">
    <tbody class="text-slate-600">
      <tr><td class="py-1 pr-3 font-mono">200</td><td>已发送</td></tr>
      <tr><td class="py-1 pr-3 font-mono">400</td><td>缺 text 或 JSON 非法</td></tr>
      <tr><td class="py-1 pr-3 font-mono">401</td><td>API key 错误</td></tr>
      <tr><td class="py-1 pr-3 font-mono">429</td><td>触发频率限制（约 7 条/5 分钟）</td></tr>
      <tr><td class="py-1 pr-3 font-mono">503</td><td>未登录 / 登录已失效（重新扫码）</td></tr>
      <tr><td class="py-1 pr-3 font-mono">502</td><td>微信服务端或网络错误</td></tr>
    </tbody>
  </table>
</section>

{#if message}<p class="mt-3 text-sm text-emerald-600">{message}</p>{/if}
{#if error}<p class="mt-3 text-sm text-rose-600">{error}</p>{/if}
```

- [ ] **Step 2: 验证（含端到端发一条真消息）**

```bash
pnpm check
pnpm tauri dev
```

Expected：
1. 显示/复制 key 正常；“重置”需点两次，重置后旧 key 调用返回 401。
2. 改端口为 9721 保存 → 侧栏显示 `:9721`，`curl http://127.0.0.1:9721/health` 通；改回 9720。
3. 停止 → `curl` 连接被拒；启动 → 恢复。
4. 已登录状态下，复制示例命令、把 `<本机IP>` 换成 `127.0.0.1` 在终端执行 → 返回 `{"ok":true,"to":"…@im.wechat"}`，**手机微信收到“你好”**。从局域网另一台机器用真实 IP 再执行一次，同样收到。

- [ ] **Step 3: 提交**

```bash
git add src
git commit -m "feat(ui): 设置页（端口、API key、服务开关、调用示例）"
```

---

### Task 14: 日志页

**Files:**
- Modify: `src/lib/pages/Logs.svelte`（整个替换）

**Interfaces:**
- Consumes: `api.listLogs`

- [ ] **Step 1: 写 Logs.svelte**

```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import { api, type LogEntry } from "$lib/api";

  let logs = $state<LogEntry[]>([]);

  async function refresh() {
    try {
      logs = await api.listLogs();
    } catch (e) {
      console.error(e);
    }
  }

  onMount(() => {
    refresh();
    const t = setInterval(refresh, 3000);
    return () => clearInterval(t);
  });
</script>

<div class="flex items-center justify-between">
  <div>
    <h2 class="text-lg font-semibold">日志</h2>
    <p class="mt-1 text-sm text-slate-500">最近 500 条 webhook 调用与鉴权记录，每 3 秒自动刷新。</p>
  </div>
  <button class="rounded-md border border-slate-300 px-3 py-1.5 text-sm hover:bg-slate-100" onclick={refresh}>刷新</button>
</div>

<div class="mt-4 overflow-hidden rounded-xl border border-slate-200 bg-white">
  {#if logs.length === 0}
    <p class="p-6 text-sm text-slate-500">还没有记录。</p>
  {:else}
    <table class="w-full text-sm">
      <thead class="bg-slate-50 text-left text-xs uppercase text-slate-500">
        <tr>
          <th class="px-4 py-2.5">时间</th>
          <th class="px-4 py-2.5">结果</th>
          <th class="px-4 py-2.5">收件人</th>
          <th class="px-4 py-2.5">内容</th>
        </tr>
      </thead>
      <tbody>
        {#each logs as l, i (i)}
          <tr class="border-t border-slate-100 align-top">
            <td class="whitespace-nowrap px-4 py-2 font-mono text-xs text-slate-500">{l.ts}</td>
            <td class="whitespace-nowrap px-4 py-2">
              {#if l.ok}
                <span class="rounded bg-emerald-100 px-2 py-0.5 text-xs text-emerald-700">成功</span>
              {:else}
                <span class="rounded bg-rose-100 px-2 py-0.5 text-xs text-rose-700">{l.code ?? "失败"}</span>
              {/if}
            </td>
            <td class="px-4 py-2 font-mono text-xs text-slate-600">{l.to || "-"}</td>
            <td class="px-4 py-2 break-all text-slate-700">{l.text}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>
```

- [ ] **Step 2: 验证**

```bash
pnpm check
pnpm tauri dev
```

Expected：之前的成功发送、401 鉴权失败（含来源 IP）都在表里，最新在最上；用错 key 再 curl 一次，3 秒内出现新的一行。

- [ ] **Step 3: 提交**

```bash
git add src
git commit -m "feat(ui): 日志页"
```

---

### Task 15: GitHub Actions 发布 + README

**Files:**
- Create: `.github/workflows/release.yml`、`README.md`

**Interfaces:**
- Consumes: `src-tauri/tauri.conf.json` 的 `version`（Task 1 设为 0.1.0）；产物名 `weixin-clawbot-webhook_0.1.0_x64-setup.exe`（NSIS）与 `src-tauri/target/release/weixin-clawbot-webhook.exe`

- [ ] **Step 1: 写 release.yml**

```yaml
name: release

on:
  push:
    tags: ["v*"]

permissions:
  contents: write

jobs:
  build-windows:
    runs-on: windows-latest
    steps:
      - uses: actions/checkout@v4

      - name: 校验 tag 与 tauri.conf.json 版本一致
        shell: pwsh
        run: |
          $v = (Get-Content src-tauri/tauri.conf.json | ConvertFrom-Json).version
          if ("v$v" -ne "${{ github.ref_name }}") {
            Write-Error "tag ${{ github.ref_name }} 与 tauri.conf.json 版本 v$v 不一致"
            exit 1
          }

      - uses: pnpm/action-setup@v4
        with:
          version: 11

      - uses: actions/setup-node@v4
        with:
          node-version: 22
          cache: pnpm

      - uses: dtolnay/rust-toolchain@stable

      - uses: Swatinem/rust-cache@v2
        with:
          workspaces: src-tauri

      - run: pnpm install --frozen-lockfile

      - name: 构建并发布安装包
        uses: tauri-apps/tauri-action@v1
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
        with:
          tagName: v__VERSION__
          releaseName: "weixin-clawbot-webhook v__VERSION__"
          releaseBody: "附件说明：`*-setup.exe` 为安装包；`*-portable.exe` 为免安装单文件（依赖系统自带 WebView2）。未签名，SmartScreen 提示时选「更多信息 → 仍要运行」。"
          releaseDraft: false
          prerelease: false

      - name: 准备便携 exe
        shell: pwsh
        run: Copy-Item src-tauri/target/release/weixin-clawbot-webhook.exe "weixin-clawbot-webhook-${{ github.ref_name }}-portable.exe"

      - name: 上传便携 exe 到同一 Release
        uses: softprops/action-gh-release@v2
        with:
          tag_name: ${{ github.ref_name }}
          files: weixin-clawbot-webhook-*-portable.exe
```

- [ ] **Step 2: 写 README.md**

````markdown
# weixin-clawbot-webhook

把微信 ClawBot（腾讯官方 iLink 协议）包成一个本地 webhook：扫码登录一次，之后局域网内任何脚本 / curl / CI 一行请求就能给你的微信发消息。Tauri 2 + Rust，单 exe。

## 安装

到 [Releases](../../releases) 下载：

- `*-setup.exe`：安装版
- `*-portable.exe`：免安装单文件（Win10/11 自带 WebView2 即可运行）

未做代码签名，首次运行 SmartScreen 会提示「未知发布者」，点「更多信息 → 仍要运行」。首次启动 Windows 防火墙会询问是否允许监听，请勾选专用网络。

## 使用

1. **登录**：打开应用 → 登录页 → 扫码登录 → 手机微信确认。
2. **收件人**（可选）：默认发给扫码的账号；要发给别人，在收件人页录入其 ID（`xxx@im.wechat`）。iLink 没有好友列表接口，ID 需要你自己拿到。
3. **设置**：查看/复制 API key，按需改端口（默认 9720）。
4. **调用**：

```bash
curl -X POST http://<本机IP>:9720/send \
  -H "X-API-Key: <你的key>" \
  -H "Content-Type: application/json" \
  -d '{"text":"部署完成 ✅","to":"可选，收件人ID"}'
```

也可以用 `Authorization: Bearer <key>`。

## 接口

| 方法 | 路径 | 说明 |
|---|---|---|
| `POST /send` | `{"text": "...", "to": "可选"}` | 发文本消息，需鉴权 |
| `GET /health` | — | `{"ok":true,"logged_in":bool,"version":"..."}`，不鉴权 |

| HTTP | code | 含义 |
|---|---|---|
| 200 | — | 已发送 |
| 400 | `bad_request` | 缺 `text` 或 JSON 非法 |
| 401 | `unauthorized` | API key 错误 |
| 429 | `rate_limited` | 微信侧频率限制（约 7 条/5 分钟），稍后重试 |
| 503 | `not_logged_in` / `token_expired` | 未登录或登录失效，回应用重新扫码 |
| 502 | `upstream_error` | 微信服务端或网络错误，body 里有原始信息 |

失败响应统一为 `{"ok":false,"code":"...","error":"..."}`。

## 数据目录

`%APPDATA%\com.liuli.weixin-clawbot-webhook\`：`credentials.json`（登录凭据，明文，请勿外传）、`config.json`（端口/API key/收件人）、`logs.json`（最近 500 条）。

## 开发

```bash
pnpm install
pnpm tauri dev          # 开发运行
cd src-tauri && cargo test   # 先在根目录 pnpm build 一次
```

发版：把 `src-tauri/tauri.conf.json` 里的 `version` 改好，打 tag `vX.Y.Z` 推上去，GitHub Actions 自动构建并发布 Release。

## 说明与限制

- 只做「主动发送」，不接收消息；只支持文本。
- bot_token 有效期数天到数周，失效后 `/send` 返回 503 `token_expired`，重新扫码即可。
- 协议为腾讯官方 iLink Bot API（`ilinkai.weixin.qq.com`），非逆向；协议演进可能导致失效。
````

- [ ] **Step 3: 本地验证构建产物**

```bash
pnpm tauri build
```

Expected：`src-tauri/target/release/bundle/nsis/weixin-clawbot-webhook_0.1.0_x64-setup.exe` 与 `src-tauri/target/release/weixin-clawbot-webhook.exe` 生成；双击后者能直接运行，功能与 dev 一致。

- [ ] **Step 4: 提交、推送、打 tag 触发发布**

```bash
git add .github README.md
git commit -m "ci: GitHub Actions 发布流程与 README"
git remote add origin <你的仓库地址>
git push -u origin master
git tag v0.1.0
git push origin v0.1.0
```

Expected：Actions 里 `release` 工作流成功；Release `v0.1.0` 下有 `*-setup.exe` 与 `*-portable.exe` 两个附件；下载便携版运行正常。
