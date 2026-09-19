# weixin-clawbot-webhook

把微信 ClawBot（腾讯官方 iLink 协议）包成一个本地 webhook：扫码登录一次，之后局域网内任何脚本 / curl / CI 一行请求就能给你的微信发消息。Tauri 2 + Rust，单 exe。

## 安装

到 [Releases](../../releases) 下载：

- `*-setup.exe`：安装版
- `*-portable.exe`：免安装单文件（Win10/11 自带 WebView2 即可运行）

未做代码签名，首次运行 SmartScreen 会提示「未知发布者」，点「更多信息 → 仍要运行」。首次启动 Windows 防火墙会询问是否允许监听，请勾选专用网络。

## 使用

1. **登录**：打开应用 → 登录页 → 扫码登录 → 手机微信确认。
2. **收件人**（可选）：默认发给扫码的账号。收件人 ID 是 **iLink 用户 ID**（形如 `o9cq8…@im.wechat`），**不是微信号也不是 wxid**，填微信号会被微信返回 `ret=-3 invalid arguments`。iLink 没有好友列表或按微信号查询的接口，别人的 ID 只能从他发给机器人的消息（`getupdates` 的 `from_user_id`）中获得——当前版本不接收消息，因此实际上只能发给扫码账号自己；给第三方发送需要后续版本加入消息接收。备注可在列表中直接编辑；收件人 ID 如需更改，请删除后重新添加。
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
| 429 | `rate_limited` | 微信侧拒绝（ret=-2）：首次给某人推送前，需对方先在微信里给机器人发过一条消息建立会话；否则是频率限制（约 7 条/5 分钟），稍后重试 |
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
- 首次给某个收件人推送前，对方必须先在微信里给机器人发过一条消息（建立会话），否则微信返回 `ret=-2`；会话可能过期，过期后需对方再发一条。
- bot_token 有效期数天到数周，失效后 `/send` 返回 503 `token_expired`，重新扫码即可。
- 协议为腾讯官方 iLink Bot API（`ilinkai.weixin.qq.com`），非逆向；协议演进可能导致失效。
