<script lang="ts">
  import { onMount, tick } from "svelte";
  import { api, type AccountView, type LogEntry, type StatusInfo } from "$lib/api";

  let { status }: { status: StatusInfo | null } = $props();

  let accounts = $state<AccountView[]>([]);
  let selected = $state("");
  let text = $state("");
  let sending = $state(false);
  let error = $state("");
  let logs = $state<LogEntry[]>([]);
  let listEl = $state<HTMLDivElement | null>(null);

  const codeLabel: Record<string, string> = {
    not_logged_in: "未登录",
    unknown_recipient: "收件人未接入",
    token_expired: "该账号登录已失效，请重新扫码",
    rate_limited: "微信侧拒绝：接入后需先用该微信给机器人发一条消息激活，或触发频率限制（约 7 条/5 分钟）",
    upstream_error: "微信服务端或网络错误",
  };

  const account = $derived(accounts.find((a) => a.user_id === selected) ?? null);

  // 日志最新在前，聊天记录按时间正序展示
  const messages = $derived(
    selected ? logs.filter((l) => l.to === selected).slice().reverse() : [],
  );

  const canSend = $derived(!!account && !account.token_expired && text.trim().length > 0 && !sending);

  async function loadAccounts() {
    try {
      accounts = await api.listAccounts();
      if (!accounts.some((a) => a.user_id === selected)) {
        const def = accounts.find((a) => a.is_default) ?? accounts[0];
        selected = def?.user_id ?? "";
      }
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
      await api.sendTest(selected, body);
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

<div class="flex h-[calc(100vh-5.25rem)] flex-col">
  <div>
    <h2 class="page-title">发消息</h2>
    <p class="page-desc">
      用账号自己的机器人给自己发，记录与 webhook 共用同一份日志。每次发送都会消耗微信侧配额。
    </p>
  </div>

  <div class="mt-4 flex items-center gap-3">
    <label for="chat-account" class="text-sm text-ink-2">账号</label>
    <select id="chat-account" class="input py-1.5" bind:value={selected}>
      {#each accounts as a (a.user_id)}
        <option value={a.user_id}>{a.name}{a.token_expired ? "（已失效）" : ""} · {a.user_id}</option>
      {/each}
    </select>
  </div>

  <div bind:this={listEl} class="card mt-4 flex-1 overflow-y-auto bg-panel/60 p-4">
    {#if accounts.length === 0}
      <p class="text-center text-sm text-ink-3">尚未接入任何账号，请先到「账号」页扫码。</p>
    {:else if messages.length === 0}
      <p class="text-center text-sm text-ink-3">还没有发给 {account?.name ?? selected} 的消息</p>
    {:else}
      <div class="flex flex-col gap-3">
        {#each messages as m, i (m.ts + i)}
          <div class="flex flex-col items-end animate-fade-in">
            <div
              class="max-w-[70%] whitespace-pre-wrap break-words rounded-2xl rounded-tr-sm px-4 py-2 text-sm {m.ok
                ? 'bg-accent text-white'
                : 'border border-danger/40 bg-danger-soft text-danger'}"
            >
              {m.text}
            </div>
            <div class="mt-1 text-[11px] text-ink-3">
              {m.ts}
              {#if !m.ok}
                <span class="text-danger">· 失败：{codeLabel[m.code ?? ""] ?? m.code ?? "未知错误"}</span>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>

  {#if account?.token_expired}
    <p class="alert-danger mt-3">「{account.name}」登录已失效，请到「账号」页重新扫码。</p>
  {/if}
  {#if error}
    <p class="alert-danger mt-3">{error}</p>
  {/if}

  <div class="mt-3 flex items-end gap-3">
    <textarea
      class="input flex-1 resize-none"
      rows="2"
      aria-label="消息内容"
      placeholder="输入消息，Enter 发送，Shift+Enter 换行"
      bind:value={text}
      onkeydown={onKeydown}
      disabled={!account || account.token_expired || sending}
    ></textarea>
    <button class="btn-primary px-5" onclick={send} disabled={!canSend}>
      {sending ? "发送中…" : "发送"}
    </button>
  </div>
</div>
