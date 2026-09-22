<script lang="ts">
  import { onMount, tick } from "svelte";
  import { api, type LogEntry, type StatusInfo, type WecomAppView } from "$lib/api";

  let { status }: { status: StatusInfo | null } = $props();

  let apps = $state<WecomAppView[]>([]);
  let selected = $state("");
  let to = $state("@all");
  let text = $state("");
  let sending = $state(false);
  let error = $state("");
  let undelivered = $state<string[]>([]);
  let logs = $state<LogEntry[]>([]);
  let listEl = $state<HTMLDivElement | null>(null);

  const codeLabel: Record<string, string> = {
    not_configured: "未添加应用",
    unknown_app: "应用不存在",
    invalid_credentials: "凭据无效",
    unknown_recipient: "收件人全部无效或不在可见范围",
    rate_limited: "企业微信频率限制",
    upstream_error: "企业微信服务端或网络错误",
  };

  const app = $derived(apps.find((a) => a.name === selected) ?? null);
  const messages = $derived(
    selected ? logs.filter((l) => l.channel === "wecom" && l.from === selected).slice().reverse() : [],
  );
  const canSend = $derived(!!app && !app.invalid && text.trim().length > 0 && !sending);

  async function loadApps() {
    try {
      apps = await api.wecomListApps();
      if (!apps.some((a) => a.name === selected)) {
        selected = (apps.find((a) => a.is_default) ?? apps[0])?.name ?? "";
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
    sending = true;
    error = "";
    undelivered = [];
    try {
      const r = await api.wecomSendTest(selected, to, text.trim());
      undelivered = r.invalid_users;
      text = "";
    } catch (e) {
      error = String(e);
    } finally {
      sending = false;
      await refreshLogs();
      await loadApps(); // 40056/301002 会把应用标为失效
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
    loadApps();
    refreshLogs();
    const t = setInterval(refreshLogs, 3000);
    return () => clearInterval(t);
  });
</script>

<p class="page-desc">
  用企业微信应用给成员发文本，记录与 webhook 共用同一份日志。收件人为成员 UserID（用 | 分隔）或 @all。
</p>

<div class="mt-4 flex flex-wrap items-center gap-3">
  <label for="wecom-app" class="text-sm text-ink-2">应用</label>
  <select id="wecom-app" class="input py-1.5" bind:value={selected}>
    {#each apps as a (a.name)}
      <option value={a.name}>{a.name}{a.invalid ? "（凭据无效）" : ""} · {a.agentid}</option>
    {/each}
  </select>
  <label for="wecom-to" class="text-sm text-ink-2">收件人</label>
  <input id="wecom-to" class="input w-64 py-1.5 font-mono" bind:value={to} placeholder="UserID 用 | 分隔，或 @all" />
</div>

<div bind:this={listEl} class="card mt-4 flex-1 overflow-y-auto bg-panel/60 p-4">
  {#if apps.length === 0}
    <p class="text-center text-sm text-ink-3">尚未添加任何企业微信应用，请先到「账号 → 企业微信」添加。</p>
  {:else if messages.length === 0}
    <p class="text-center text-sm text-ink-3">还没有用「{app?.name ?? selected}」发过消息</p>
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
            {m.ts} · → {m.to}
            {#if !m.ok}
              <span class="text-danger">· 失败：{codeLabel[m.code ?? ""] ?? m.code ?? "未知错误"}</span>
            {/if}
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

{#if app?.invalid}
  <p class="alert-danger mt-3">「{app.name}」凭据无效：{app.invalid}。请到「账号 → 企业微信」修改。</p>
{/if}
{#if undelivered.length > 0}
  <p class="alert-danger mt-3">以下收件人未送达（不在可见范围或无许可）：{undelivered.join(", ")}</p>
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
    disabled={!app || !!app.invalid || sending}
  ></textarea>
  <button class="btn-primary px-5" onclick={send} disabled={!canSend}>
    {sending ? "发送中…" : "发送"}
  </button>
</div>
