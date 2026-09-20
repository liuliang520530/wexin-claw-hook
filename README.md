# weixin-clawbot-webhook

把微信 ClawBot（腾讯官方 iLink 协议）包成一个本地 webhook：扫码登录一次，之后局域网内任何脚本 / curl / CI 一行请求就能给你的微信发消息。Tauri 2 + Rust，单 exe。

## 安装

到 [Releases](../../releases) 下载：

- `*-setup.exe`：安装版
- `*-portable.exe`：免安装单文件（Win10/11 自带 WebView2 即可运行）

未做代码签名，首次运行 SmartScreen 会提示「未知发布者」，点「更多信息 → 仍要运行」。首次启动 Windows 防火墙会询问是否允许监听，请勾选专用网络。

## 使用

1. **账号**：打开应用 → 账号页 → 获取二维码 → 用要接入的微信扫码并确认。每扫一次码接入一个账号，可接入多个；同一账号重复扫码即刷新登录。接入后请用该微信给机器人发一条任意消息（建立会话），否则微信会以 `ret=-2` 拒绝推送。
2. **收件人就是已接入的账号**：消息发给某个账号时，用的是该账号自己扫码得到的 bot 凭据。收件人 ID 是 iLink 用户 ID（形如 `o9cq8…@im.wechat`），不是微信号/wxid，填微信号会得到 `ret=-3`。iLink 没有好友列表接口，未接入者的 ID 只能从他发给机器人的消息中获得，而当前版本不接收消息。「发消息」页可分别选择“发送账号”与“收件人”做交叉测试（用 A 的机器人发给 B）。
3. **设置**：查看/复制 API key，按需改端口（默认 9720）。
4. **调用**：

```bash
curl -X POST http://<本机IP>:9720/send \
  -H "X-API-Key: <你的key>" \
  -H "Content-Type: application/json" \
  -d '{"text":"部署完成 ✅","to":"可选，已接入账号的 ID"}'
```

也可以用 `Authorization: Bearer <key>`。

## 接口

| 方法 | 路径 | 说明 |
|---|---|---|
| `POST /send` | `{"text": "...", "to": "可选"}` | 发文本消息，需鉴权。`to` 缺省 = 默认账号发给自己；`to` 是已接入账号则用它自己的凭据发给它；否则用默认账号的凭据发给 `to` |
| `GET /health` | — | `{"ok":true,"logged_in":bool,"version":"..."}`，不鉴权 |

| HTTP | code | 含义 |
|---|---|---|
| 200 | — | 已发送 |
| 400 | `bad_request` | 缺 `text` 或 JSON 非法 |
| 401 | `unauthorized` | API key 错误 |
| 429 | `rate_limited` | 微信侧拒绝（ret=-2）：首次给某人推送前，需对方先在微信里给机器人发过一条消息建立会话；否则是频率限制（约 7 条/5 分钟），稍后重试 |
| 503 | `not_logged_in` / `token_expired` | 未登录或登录失效，回应用重新扫码 |
| 502 | `upstream_error` | 微信服务端或网络错误，body 里有原始信息 |

失败响应统一为 `{"ok":false,"code":"...","error":"..."}`。

## 数据目录

`%APPDATA%\com.liuli.weixin-clawbot-webhook\`：`accounts.json`（各账号的登录凭据，明文，请勿外传；v1 的 `credentials.json` 首次启动时自动迁移）、`config.json`（端口/API key/收件人）、`logs.json`（最近 500 条）。

## 开发

```bash
pnpm install
pnpm tauri dev          # 开发运行
cd src-tauri && cargo test   # 先在根目录 pnpm build 一次
```

发版：把 `src-tauri/tauri.conf.json` 里的 `version` 改好，打 tag `vX.Y.Z` 推上去，GitHub Actions 自动构建并发布 Release。

## 说明与限制

- 只做「主动发送」，不接收消息；只支持文本。
- 首次给某个收件人推送前，对方必须先在微信里给机器人发过一条消息（建立会话），否则微信返回 `ret=-2`；会话可能过期，过期后需对方再发一条。
- bot_token 有效期数天到数周，失效后 `/send` 返回 503 `token_expired`，重新扫码即可。
- 协议为腾讯官方 iLink Bot API（`ilinkai.weixin.qq.com`），非逆向；协议演进可能导致失效。
