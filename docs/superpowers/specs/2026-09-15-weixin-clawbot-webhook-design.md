# 微信 ClawBot Webhook 工具 — 设计文档

日期：2026-09-15
状态：待审阅

## 1. 背景与目标

腾讯官方 iLink Bot API（微信 ClawBot / 智联协议，域名 `https://ilinkai.weixin.qq.com`）是纯 HTTP/JSON 协议，支持扫码登录后通过 `sendmessage` 主动给微信用户发消息。社区现有项目（wxclawbot-cli、weixin-clawbot-gui、wx-clawbot 等）**均不提供 HTTP webhook 入口**。

本项目做一个 **Tauri 2 桌面应用**：内嵌 webhook HTTP 服务，供局域网内的脚本 / curl / CI 告警调用，实现"一行 curl 发微信消息"；同时提供图形界面完成扫码登录、收件人管理、API key 管理与发送日志查看。

**成功标准：**

1. 扫码登录一次后，局域网内 `curl -H "X-API-Key: xxx" -d '{"text":"hi"}' http://<ip>:9720/send` 可送达消息
2. 单 exe 产物，通过 GitHub Actions 打包发布到 Release
3. token 失效（-14）时 UI 明确提示重新扫码，webhook 调用方收到明确错误码

## 2. 协议事实（已查证）

以下细节已对照 `lroolle/wxclawbot-cli`（TypeScript）与 `fastclaw-ai/weclaw`（Go）两个独立实现源码交叉验证，非仅凭 AI 会话总结：

- **认证**：扫码获得 `bot_token`，业务请求带头 `Authorization: Bearer <token>`、`AuthorizationType: ilink_bot_token`、`X-WECHAT-UIN`（随机 uint32 → 十进制字符串 → base64）、`iLink-App-Id: bot`、`iLink-App-ClientVersion`（版本号编码为整数，如 2.4.6 → 0x00020406）
- **登录流程**：`GET /ilink/bot/get_bot_qrcode?bot_type=3` 获取二维码 → 轮询 `GET /ilink/bot/get_qrcode_status?qrcode=...`（状态：wait / scaned / confirmed / expired）→ confirmed 时返回 `bot_token`、`ilink_bot_id`、`ilink_user_id`、`baseurl`（后续请求用 baseurl，可能非默认域名）
- **发消息**：`POST {baseurl}/ilink/bot/sendmessage`，body 含 `msg`（`from_user_id` 填 botId、`to_user_id`、`client_id` 唯一值、`message_type: 2`、`message_state: 2`、`item_list`、`context_token` 可选）与 `base_info`（`channel_version`）
- **附加字段**：`base_info.bot_agent`（客户端标识，如 `weixin-clawbot-webhook/0.1.0`）为参考实现 wxclawbot-cli 携带的附加字段，服务端宽容，非协议必需
- **无好友列表接口**：收件人 ID（`xxx@im.wechat`）只能来自被动收到的消息或事先录入
- **错误码**：`ret=-2` 限频（约 7 条/5 分钟，服务端硬限制）、`ret=-14` token 失效（有效期数天到数周，不保证永久）
- **协议风险**：服务端行为随版本演进，`context_token` 是否必需在不同实现中表现不一（yao 强制、wxclawbot-cli 可省略）；本项目将其作为可选参数透传，实测为准

## 3. 需求（已与用户确认）

| 维度 | 决定 |
|---|---|
| 调用方 | 脚本 / curl / CI 告警 |
| 部署 | 本机 Windows，监听局域网（0.0.0.0）+ API key 鉴权 |
| 收件人 | 默认发给自己（扫码账号），支持多收件人管理 |
| 消息方向 | 只主动发，不做接收（无需长轮询 getupdates） |
| UI | Tauri 2 单文件应用，界面好看、效率高、跨平台 |
| 打包 | push tag → GitHub Actions 构建 → 发布到 Release |

## 4. 技术栈

| 层 | 选型 | 理由 |
|---|---|---|
| 桌面框架 | Tauri 2 | 单 exe，系统 WebView2，跨平台 |
| HTTP 服务 | axum + tokio | 嵌入式常驻服务的标准选择 |
| HTTP 客户端 | reqwest | 调 iLink API |
| 前端 | Svelte 5 + Tailwind | 包小、开发体验好 |
| 二维码 | Rust `qrcode` crate 生成 SVG | 无 JS 依赖 |
| 存储 | JSON 文件（app data 目录） | 数据量小，免 SQLite |
| 协议实现 | Rust 自实现（仅登录 + 发送两个接口，约 200 行） | 官方 npm 包是 Node 生态，复用需挂 Node sidecar，破坏单文件形态 |

被否决的备选：Electron + 官方 npm 包（150MB+ 包体、300MB+ 内存、非单文件）；纯后台服务 + Web 管理页（无桌面形态，托盘/自启体验差）。

## 5. 模块划分

```
weixin-clawbot-webhook/
├── src-tauri/                  # Rust 后端
│   └── src/
│       ├── main.rs             # Tauri 入口，启动时加载存储、恢复 HTTP 服务
│       ├── ilink/              # iLink 协议客户端（不感知 server 与 UI）
│       │   ├── mod.rs
│       │   ├── types.rs        # 协议请求/响应类型
│       │   ├── auth.rs         # 扫码登录：get_bot_qrcode → get_qrcode_status 轮询
│       │   └── client.rs       # sendmessage 发送（-2/-14 错误识别）
│       ├── server/             # 内嵌 HTTP 服务（只依赖 ilink 与 store）
│       │   ├── mod.rs          # axum 启动/停止，端口管理
│       │   ├── routes.rs       # POST /send、GET /health
│       │   └── middleware.rs   # API key 鉴权（常数时间比较）
│       ├── store.rs            # JSON 持久化（credentials/config/logs）
│       └── commands.rs         # Tauri commands，前端 ↔ 后端唯一粘合层
└── src/                        # Svelte 前端
    └── lib/pages/              # 登录页 / 收件人 / 设置 / 日志
```

**数据流（发送）：**

```
curl → POST /send (X-API-Key) → 鉴权中间件 → routes
  → 解析 to/text（to 缺省用默认收件人）→ ilink/client.rs 组装请求
  → 成功/失败写日志 → 返回 JSON
```

**数据流（登录）：**

```
UI 点"扫码登录" → command login_start → auth.rs 取二维码 → 生成 SVG 返回渲染
  → 前端定时调 login_status → auth.rs 轮询 get_qrcode_status
  → confirmed 时持久化 credentials → UI 显示成功
```

## 6. Webhook API

| 方法 | 路径 | 鉴权 | 说明 |
|---|---|---|---|
| `POST /send` | body `{"to": "可选", "text": "必填"}` | 必须 | 发消息 |
| `GET /health` | — | 无 | 服务探针，永远 200 |

鉴权：接受 `X-API-Key: <key>` 或 `Authorization: Bearer <key>`；key 首次启动自动生成（32 字节随机 base64url），设置页可查看/重置；比对用常数时间比较。`/health` 只暴露 `logged_in` 布尔值与版本号，无敏感信息。

**错误映射（调用方只看 HTTP 状态码分支，body 给细节）：**

| 情况 | HTTP | code |
|---|---|---|
| 成功 | 200 | —（`{"ok":true}`） |
| 缺 text / JSON 非法 | 400 | `bad_request` |
| key 错误 | 401 | `unauthorized` |
| 未登录 | 503 | `not_logged_in` |
| iLink ret=-2 | 429 | `rate_limited` |
| iLink ret=-14 | 503 | `token_expired`（同时本地标记失效，UI 提示重新扫码） |
| 上游其他错误 / 网络异常 / 超时 | 502 | `upstream_error` |

**设计决策：**

- 限频**不排队重试**，直接 429——CI 场景要快速明确的失败，重试策略留给调用方
- v1 只支持文本；`ilink/client.rs` 预留泛化的 `send_items` 入口，媒体（CDN 上传 + AES-128-ECB）以后加不破坏 API
- 上游调用超时 15s；请求体上限 1MB
- **发送日志**：每次调用记录时间/收件人/文本前 200 字/结果/错误码，环形保留 500 条
- 不配 CORS（调用方是脚本，无浏览器同源需求；默认无 CORS 头顺带挡浏览器跨域调用）

## 7. 存储

位置：`%APPDATA%\com.liuli.weixin-clawbot-webhook\`（Tauri app_data_dir 使用 identifier，跨平台由 Tauri 解析）

| 文件 | 内容 |
|---|---|
| `credentials.json` | `bot_token`、`baseurl`、`ilink_bot_id`、`ilink_user_id` |
| `config.json` | 端口（默认 9720）、API key、默认收件人、收件人列表 |
| `logs.json` | 环形日志，最近 500 条 |

变更即全量重写（文件均为 KB 级）。**取舍**：token 明文 JSON，不用 Windows DPAPI——单用户本地场景收益低、跨平台会分叉；文件位置固定，将来可升级。

## 8. 安全

- API key 常数时间比较；鉴权失败记日志（含来源 IP）
- 日志不记录 key / token 任何片段
- 监听 0.0.0.0:9720，依赖 API key 而非网络隔离保证安全（用户明确要求局域网可调）

## 9. UI 页面

1. **登录页**：未登录时展示二维码 SVG + 状态（等待扫码 / 已扫 / 过期刷新 / 成功）；已登录显示账号信息与"退出登录"
2. **收件人**：列表 + 增删改（ID + 备注），设默认
3. **设置**：端口、API key 查看/重置；webhook 服务启停开关
4. **日志**：发送记录表，最近 500 条

## 10. 打包与发布

- `tauri-apps/tauri-action`，push `v*` tag 触发；矩阵 v1 仅 `windows-latest`
- 产物：NSIS 安装包 + 便携 exe，均传 GitHub Release；rust-cache 加速
- 版本号以 `tauri.conf.json` 为准，与 tag 不符时 action 报错
- 未签名：SmartScreen 首次会告警，个人工具可接受；后续可加证书解密步骤，架构不变

## 11. 测试策略

- `ilink/`：单元测试覆盖请求头构造（X-WECHAT-UIN 生成、版本编码）、消息体组装、错误码识别，上游用 mock HTTP（wiremock）
- `server/`：集成测试覆盖鉴权（对/错 key、两种头格式）、400/503/429/502 映射、health
- `store/`：读写往返、环形日志上限裁剪
- 手工验收：真机扫码登录 → curl 发消息到手机

## 12. v1 明确不做

- 接收消息（getupdates 长轮询）、媒体/文件发送、多账号、代码签名、macOS/Linux 构建、自动重试与消息队列、DPAPI 加密
