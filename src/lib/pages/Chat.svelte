<script lang="ts">
  import { onMount, tick } from "svelte";
  import { api, type LogEntry, type Recipient, type StatusInfo } from "$lib/api";

  let { status }: { status: StatusInfo | null } = $props();

  const SELF = "__self__";
  const MANUAL = "__manual__";

  let recipients = $state<Recipient[]>([]);
  let selected = $state<string>(SELF);
  let manualId = $state("");
  let text = $state("");
  let sending = $state(false);
  let error = $state("");
  let logs = $state<LogEntry[]>([]);
  let listEl = $state<HTMLDivElement | null>(null);

  const codeLabel: Record<string, string> = {
    not_logged_in: "未登录",
    token_expired: "登录已失效，请重新扫码",
    rate_limited: "微信侧拒绝：对方尚未给机器人发过消息，或触发频率限制（约 7 条/5 分钟）",
    upstream_error: "微信服务端或网络错误",
  };

  // 当前选中的收件人 ID
  const targetId = $derived(
    selected === SELF ? (status?.user_id ?? "") : selected === MANUAL ? manualId.trim() : selected,
  );

  // 日志最新在前，聊天记录按时间正序展示
  const messages = $derived(
    targetId ? logs.filter((l) => l.to === targetId).slice().reverse() : [],
  );

  const targetValid = $derived(targetId.endsWith("@im.wechat") && targetId !== "@im.wechat");

  const canSend = $derived(
    !!status?.logged_in && !!targetId && targetValid && text.trim().length > 0 && !sending,
  );

  async function loadRecipients() {
    try {
      const cfg = await api.getConfig();
      recipients = cfg.recipients;
      if (cfg.default_recipient) selected = cfg.default_recipient;
    } catch (e) {
      error = String(e);
    }
  }

  async function refreshLogs() {
    try {
      logs = await api.listLogs();
    } catch (e) {
      console.error(e);
    }
  }

  async function scrollToBottom() {
    await tick();
    if (listEl) listEl.scrollTop = listEl.scrollHeight;
  }

  async function send() {
    if (!canSend) return;
    const body = text.trim();
    sending = true;
    error = "";
    try {
      await api.sendTest(targetId, body);
      text = "";
    } catch (e) {
      error = String(e);
    } finally {
      sending = false;
      await refreshLogs();
      await scrollToBottom();
    }
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send();
    }
  }

  let lastCount = 0;
  $effect(() => {
    if (messages.length !== lastCount) {
      lastCount = messages.length;
      scrollToBottom();
    }
  });

  onMount(() => {
    loadRecipients();
    refreshLogs();
    const t = setInterval(refreshLogs, 3000);
    return () => clearInterval(t);
  });
</script>

<div class="flex h-[calc(100vh-4rem)] flex-col">
  <div class="flex items-center justify-between">
    <div>
      <h2 class="text-lg font-semibold">发消息</h2>
      <p class="mt-1 text-sm text-slate-500">直接调用微信接口发送，记录与 webhook 共用同一份日志。每次发送都会消耗微信侧配额。</p>
    </div>
  </div>

  <div class="mt-4 flex items-center gap-3">
    <label for="chat-to" class="text-sm text-slate-600">收件人</label>
    <select id="chat-to" class="rounded-md border border-slate-300 bg-white px-3 py-1.5 text-sm" bind:value={selected}>
      <option value={SELF}>扫码账号（自己）{status?.user_id ? ` · ${status.user_id}` : ""}</option>
      {#each recipients as r (r.id)}
        <option value={r.id}>{r.name} · {r.id}</option>
      {/each}
      <option value={MANUAL}>手动输入 ID…</option>
    </select>
    {#if selected === MANUAL}
      <input
        class="w-72 rounded-md border border-slate-300 px-3 py-1.5 font-mono text-sm"
        placeholder="o9cq8…@im.wechat（iLink 用户 ID，不是微信号）"
        bind:value={manualId}
      />
      {#if manualId.trim() && !targetValid}
        <span class="text-xs text-rose-600">不是 iLink 用户 ID：需以 @im.wechat 结尾，微信号/wxid 不可用</span>
      {/if}
    {/if}
  </div>

  <div
    bind:this={listEl}
    class="mt-4 flex-1 overflow-y-auto rounded-xl border border-slate-200 bg-slate-100 p-4"
  >
    {#if !targetId}
      <p class="text-center text-sm text-slate-500">请先选择或输入收件人</p>
    {:else if messages.length === 0}
      <p class="text-center text-sm text-slate-500">还没有发给 {targetId} 的消息</p>
    {:else}
      <div class="flex flex-col gap-3">
        {#each messages as m, i (m.ts + i)}
          <div class="flex flex-col items-end">
            <div
              class="max-w-[70%] whitespace-pre-wrap break-words rounded-2xl rounded-tr-sm px-4 py-2 text-sm shadow-sm {m.ok
                ? 'bg-emerald-500 text-white'
                : 'border border-rose-300 bg-rose-50 text-rose-800'}"
            >
              {m.text}
            </div>
            <div class="mt-1 text-[11px] text-slate-400">
              {m.ts}
              {#if !m.ok}
                <span class="text-rose-600">· 失败：{codeLabel[m.code ?? ""] ?? m.code ?? "未知错误"}</span>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>

  {#if !status?.logged_in}
    <p class="mt-3 text-sm text-rose-600">尚未登录，请先到「登录」页扫码。</p>
  {/if}
  {#if error}
    <p class="mt-3 text-sm text-rose-600">{error}</p>
  {/if}

  <div class="mt-3 flex items-end gap-3">
    <textarea
      class="flex-1 resize-none rounded-md border border-slate-300 px-3 py-2 text-sm disabled:bg-slate-100"
      rows="2"
      placeholder="输入消息，Enter 发送，Shift+Enter 换行"
      bind:value={text}
      onkeydown={onKeydown}
      disabled={!status?.logged_in || sending}
    ></textarea>
    <button
      class="rounded-md bg-emerald-600 px-5 py-2 text-sm text-white hover:bg-emerald-700 disabled:opacity-50"
      onclick={send}
      disabled={!canSend}
    >
      {sending ? "发送中…" : "发送"}
    </button>
  </div>
</div>
