<script lang="ts">
  import { onMount, tick } from "svelte";
  import { api, type AccountView, type LogEntry, type StatusInfo } from "$lib/api";

  let { status }: { status: StatusInfo | null } = $props();

  const MANUAL = "__manual__";

  let accounts = $state<AccountView[]>([]);
  let fromId = $state("");
  let toSel = $state("");
  let manualId = $state("");
  let text = $state("");
  let sending = $state(false);
  let error = $state("");
  let logs = $state<LogEntry[]>([]);
  let listEl = $state<HTMLDivElement | null>(null);

  const codeLabel: Record<string, string> = {
    not_logged_in: "未登录",
    token_expired: "该账号登录已失效，请重新扫码",
    rate_limited: "微信侧拒绝：对方尚未给机器人发过消息，或触发频率限制（约 7 条/5 分钟）",
    upstream_error: "微信服务端或网络错误",
  };

  const targetId = $derived(toSel === MANUAL ? manualId.trim() : toSel);
  const targetValid = $derived(targetId.endsWith("@im.wechat") && targetId !== "@im.wechat");
  const fromAccount = $derived(accounts.find((a) => a.user_id === fromId) ?? null);
  const nameOf = (id: string) => accounts.find((a) => a.user_id === id)?.name ?? id;

  // 日志最新在前，聊天记录按时间正序展示；按收件人过滤，不限发送账号
  const messages = $derived(
    targetId ? logs.filter((l) => l.to === targetId).slice().reverse() : [],
  );

  const canSend = $derived(
    !!fromAccount && !fromAccount.token_expired && targetValid && text.trim().length > 0 && !sending,
  );

  async function loadAccounts() {
    try {
      accounts = await api.listAccounts();
      const def = accounts.find((a) => a.is_default) ?? accounts[0];
      if (def && !accounts.some((a) => a.user_id === fromId)) fromId = def.user_id;
      if (!toSel && def) toSel = def.user_id;
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
      await api.sendTest(fromId, targetId, body);
      text = "";
    } catch (e) {
      error = String(e);
    } finally {
      sending = false;
      await refreshLogs();
      await loadAccounts(); // -14 会把账号标为失效
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
    loadAccounts();
    refreshLogs();
    const t = setInterval(refreshLogs, 3000);
    return () => clearInterval(t);
  });
</script>

<div class="flex h-[calc(100vh-4rem)] flex-col">
  <div>
    <h2 class="text-lg font-semibold">发消息</h2>
    <p class="mt-1 text-sm text-slate-500">
      用选定账号的凭据直接调用微信接口，记录与 webhook 共用同一份日志。每次发送都会消耗微信侧配额。
    </p>
  </div>

  <div class="mt-4 flex flex-wrap items-center gap-3">
    <label for="chat-from" class="text-sm text-slate-600">发送账号</label>
    <select id="chat-from" class="rounded-md border border-slate-300 bg-white px-3 py-1.5 text-sm" bind:value={fromId}>
      {#each accounts as a (a.user_id)}
        <option value={a.user_id}>{a.name}{a.token_expired ? "（已失效）" : ""} · {a.user_id}</option>
      {/each}
    </select>

    <label for="chat-to" class="ml-2 text-sm text-slate-600">收件人</label>
    <select id="chat-to" class="rounded-md border border-slate-300 bg-white px-3 py-1.5 text-sm" bind:value={toSel}>
      {#each accounts as a (a.user_id)}
        <option value={a.user_id}>{a.name} · {a.user_id}</option>
      {/each}
      <option value={MANUAL}>手动输入 ID…</option>
    </select>
    {#if toSel === MANUAL}
      <input
        class="w-72 rounded-md border border-slate-300 px-3 py-1.5 font-mono text-sm"
        placeholder="o9cq8…@im.wechat（iLink 用户 ID，不是微信号）"
        bind:value={manualId}
      />
      {#if manualId.trim() && !targetValid}
        <span class="text-xs text-rose-600">不是 iLink 用户 ID：需以 @im.wechat 结尾</span>
      {/if}
    {/if}
  </div>

  {#if fromAccount && targetId && fromAccount.user_id !== targetId}
    <p class="mt-2 text-xs text-amber-700">
      交叉发送：用「{fromAccount.name}」的机器人发给「{nameOf(targetId)}」。若对方从未给这个机器人发过消息，微信会以 ret=-2 拒绝。
    </p>
  {/if}

  <div bind:this={listEl} class="mt-4 flex-1 overflow-y-auto rounded-xl border border-slate-200 bg-slate-100 p-4">
    {#if accounts.length === 0}
      <p class="text-center text-sm text-slate-500">尚未接入任何账号，请先到「账号」页扫码。</p>
    {:else if !targetId}
      <p class="text-center text-sm text-slate-500">请选择或输入收件人</p>
    {:else if messages.length === 0}
      <p class="text-center text-sm text-slate-500">还没有发给 {nameOf(targetId)} 的消息</p>
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
              {#if m.from && m.from !== m.to}
                <span>· 由 {nameOf(m.from)} 发出</span>
              {/if}
              {#if !m.ok}
                <span class="text-rose-600">· 失败：{codeLabel[m.code ?? ""] ?? m.code ?? "未知错误"}</span>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>

  {#if fromAccount?.token_expired}
    <p class="mt-3 text-sm text-rose-600">发送账号「{fromAccount.name}」登录已失效，请到「账号」页重新扫码。</p>
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
      disabled={!fromAccount || fromAccount.token_expired || sending}
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
