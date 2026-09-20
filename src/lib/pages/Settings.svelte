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
    try {
      const cfg = await api.getConfig();
      apiKey = cfg.api_key;
      port = cfg.port;
    } catch (e) {
      error = String(e);
    }
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
  function curlWith(key: string) {
    return `curl -X POST http://<本机IP>:${status?.port ?? port}/send -H "X-API-Key: ${key}" -H "Content-Type: application/json" -d '{"text":"你好"}'`;
  }
  // 复制用真实 key；页面展示跟随"显示/隐藏"遮罩
  const curl = $derived(curlWith(apiKey));
  const curlDisplay = $derived(curlWith(masked));

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
  <pre class="mt-3 overflow-x-auto rounded-md bg-slate-900 p-3 text-xs text-slate-100">{curlDisplay}</pre>
  <button class="mt-2 rounded-md border border-slate-300 px-3 py-1.5 text-sm hover:bg-slate-100" onclick={() => copy(curl)}>复制命令</button>
  <table class="mt-4 w-full text-xs">
    <tbody class="text-slate-600">
      <tr><td class="py-1 pr-3 font-mono">200</td><td>已发送</td></tr>
      <tr><td class="py-1 pr-3 font-mono">400</td><td>缺 text / JSON 非法 / 收件人 ID 格式错误或未接入（unknown_recipient）</td></tr>
      <tr><td class="py-1 pr-3 font-mono">401</td><td>API key 错误</td></tr>
      <tr><td class="py-1 pr-3 font-mono">429</td><td>微信侧拒绝：对方尚未与机器人对话（请先让对方在微信里给机器人发一条消息），或触发频率限制（约 7 条/5 分钟）</td></tr>
      <tr><td class="py-1 pr-3 font-mono">503</td><td>未登录 / 登录已失效（重新扫码）</td></tr>
      <tr><td class="py-1 pr-3 font-mono">502</td><td>微信服务端或网络错误</td></tr>
    </tbody>
  </table>
</section>

{#if message}<p class="mt-3 text-sm text-emerald-600">{message}</p>{/if}
{#if error}<p class="mt-3 text-sm text-rose-600">{error}</p>{/if}
