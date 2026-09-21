<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { api, type AccountView, type LoginView, type StatusInfo } from "$lib/api";

  let { status, onchange }: { status: StatusInfo | null; onchange: () => void } = $props();

  let accounts = $state<AccountView[]>([]);
  let view = $state<LoginView | null>(null);
  let scanning = $state(false); // 扫码区是否展开
  let busy = $state(false);
  let error = $state("");
  let notice = $state("");
  let editingId = $state<string | null>(null);
  let editName = $state("");
  let confirmRemoveId = $state<string | null>(null);
  let copiedId = $state<string | null>(null); // 刚复制过 ID 的账号，短暂显示对勾
  let timer: ReturnType<typeof setInterval> | null = null;

  const label: Record<string, string> = {
    wait: "请用要接入的微信「扫一扫」",
    scanned: "已扫码，请在手机上点确认",
    confirmed: "登录成功",
    expired: "二维码已过期，请刷新",
    error: "登录出错",
  };

  async function loadAccounts() {
    try {
      accounts = await api.listAccounts();
    } catch (e) {
      error = String(e);
    }
  }

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
      const s = v.status;
      if (s.state === "confirmed") {
        stopPolling();
        notice = s.refreshed
          ? `已刷新 ${s.user_id} 的登录。`
          : `已添加账号 ${s.user_id}。请用该微信给机器人发一条任意消息，之后才能收到推送。`;
        await loadAccounts();
        onchange();
      } else if (s.state === "expired" || s.state === "error") {
        stopPolling();
      }
    } catch (e) {
      error = String(e);
      stopPolling();
    }
  }

  async function startScan() {
    scanning = true;
    busy = true;
    error = "";
    notice = "";
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

  function closeScan() {
    stopPolling();
    view = null;
    scanning = false;
  }

  async function setDefault(a: AccountView) {
    error = "";
    try {
      accounts = await api.setDefaultAccount(a.user_id);
      onchange();
    } catch (e) {
      error = String(e);
    }
  }

  async function remove(a: AccountView) {
    if (confirmRemoveId !== a.user_id) {
      confirmRemoveId = a.user_id;
      return;
    }
    confirmRemoveId = null;
    error = "";
    try {
      accounts = await api.removeAccount(a.user_id);
      onchange();
    } catch (e) {
      error = String(e);
    }
  }

  function startEdit(a: AccountView) {
    editingId = a.user_id;
    editName = a.name;
  }

  async function saveEdit() {
    if (!editingId) return;
    error = "";
    try {
      accounts = await api.renameAccount(editingId, editName);
      editingId = null;
    } catch (e) {
      error = String(e);
    }
  }

  async function copyId(id: string) {
    try {
      await navigator.clipboard.writeText(id);
      copiedId = id;
      setTimeout(() => { if (copiedId === id) copiedId = null; }, 1500);
    } catch (e) {
      error = String(e);
    }
  }

  // 页面切换回来时恢复后端仍在进行中的登录会话
  onMount(async () => {
    await loadAccounts();
    try {
      const v = await api.loginStatus();
      if (!v) return;
      const s = v.status.state;
      if (s === "wait" || s === "scanned") {
        view = v;
        scanning = true;
        stopPolling();
        timer = setInterval(poll, 1500);
      } else if (s === "expired" || s === "error") {
        view = v;
        scanning = true;
      }
    } catch (e) {
      error = String(e);
    }
  });

  onDestroy(stopPolling);

  const showScan = $derived(scanning || accounts.length === 0);
</script>

<h2 class="page-title">账号</h2>
<p class="page-desc">
  每扫一次码就接入一个微信账号，消息只能发给已接入的账号。同一账号重复扫码即刷新登录。
</p>

{#if accounts.length > 0}
  <div class="mt-6 max-w-2xl space-y-3">
    {#each accounts as a (a.user_id)}
      <div class="card card-hover p-4 {a.token_expired ? 'border-danger/40' : ''}">
        <div class="flex items-start justify-between gap-4">
          <div class="min-w-0 flex-1">
            <div class="flex items-center gap-2">
              <span class="h-2.5 w-2.5 shrink-0 rounded-full {a.token_expired ? 'bg-danger' : 'bg-ok'}"></span>
              {#if editingId === a.user_id}
                <input
                  class="input py-1"
                  aria-label="账号名称"
                  bind:value={editName}
                  onkeydown={(e) => { if (e.key === "Enter") saveEdit(); if (e.key === "Escape") editingId = null; }}
                />
                <button class="btn-link text-ok hover:text-ok" onclick={saveEdit}>保存</button>
                <button class="btn-link" onclick={() => (editingId = null)}>取消</button>
              {:else}
                <span class="font-serif text-base font-medium">{a.name}</span>
                <button class="btn-link" onclick={() => startEdit(a)}>编辑</button>
              {/if}
              {#if a.is_default}
                <span class="badge bg-ink text-white">默认</span>
              {/if}
            </div>
            <dl class="mt-2.5 space-y-0.5 text-xs">
              <div class="flex gap-2">
                <dt class="w-14 text-ink-3">账号 ID</dt>
                <dd class="flex min-w-0 items-center gap-1.5 font-mono text-ink-2">
                  <span class="truncate">{a.user_id}</span>
                  <button
                    class="btn-link inline-flex shrink-0 {copiedId === a.user_id ? 'text-ok hover:text-ok' : ''}"
                    title={copiedId === a.user_id ? "已复制" : "复制账号 ID"}
                    aria-label="复制账号 ID"
                    onclick={() => copyId(a.user_id)}
                  >
                    {#if copiedId === a.user_id}
                      <svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M20 6 9 17l-5-5" /></svg>
                    {:else}
                      <svg class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="9" y="9" width="13" height="13" rx="2" /><path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" /></svg>
                    {/if}
                  </button>
                </dd>
              </div>
              <div class="flex gap-2"><dt class="w-14 text-ink-3">Bot ID</dt><dd class="truncate font-mono text-ink-3">{a.bot_id}</dd></div>
            </dl>
            {#if a.token_expired}
              <p class="mt-2 text-xs font-medium text-danger">登录已失效（微信侧 token 过期），请用该微信重新扫码。</p>
            {/if}
          </div>
          <div class="flex shrink-0 flex-col items-end gap-1.5">
            {#if a.token_expired}
              <button class="btn-danger btn-sm" onclick={startScan} disabled={busy}>重新扫码</button>
            {/if}
            {#if !a.is_default}
              <button class="btn-secondary btn-sm" onclick={() => setDefault(a)}>设为默认</button>
            {/if}
            <button
              class="btn-sm {confirmRemoveId === a.user_id ? 'btn-confirm' : 'btn-secondary'}"
              onclick={() => remove(a)}
            >
              {confirmRemoveId === a.user_id ? "确认退出？" : "退出"}
            </button>
          </div>
        </div>
      </div>
    {/each}
  </div>

  {#if !showScan}
    <button class="btn-primary mt-4" onclick={startScan} disabled={busy}>
      <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
      添加账号
    </button>
  {/if}
{/if}

{#if notice}
  <p class="alert-ok mt-4 max-w-2xl">{notice}</p>
{/if}

{#if showScan}
  <div class="card mt-6 max-w-md p-6">
    <div class="flex items-center justify-between">
      <h3 class="section-title">{accounts.length === 0 ? "扫码登录" : "添加账号"}</h3>
      {#if accounts.length > 0}
        <button class="btn-link" onclick={closeScan}>收起</button>
      {/if}
    </div>
    {#if view}
      <div class="mt-4 flex flex-col items-center">
        <div class="rounded-lg border border-line bg-white p-2 [&>svg]:h-60 [&>svg]:w-60">
          {@html view.svg}
        </div>
        <p class="mt-4 text-sm {view.status.state === 'error' || view.status.state === 'expired' ? 'text-danger' : 'text-ink-2'}">
          {label[view.status.state]}
          {#if view.status.state === "error"}：{view.status.message}{/if}
        </p>
        {#if view.status.state === "expired" || view.status.state === "error"}
          <button class="btn-primary mt-3" onclick={startScan} disabled={busy}>刷新二维码</button>
        {:else if view.status.state === "confirmed"}
          <button class="btn-secondary mt-3" onclick={startScan} disabled={busy}>再扫一个</button>
        {/if}
      </div>
    {:else}
      <p class="mt-3 text-sm leading-relaxed text-ink-2">点击下方按钮获取二维码，用要接入的微信扫码。二维码约 5 分钟内有效。</p>
      <button class="btn-primary mt-4" onclick={startScan} disabled={busy}>
        {busy ? "获取中…" : "获取二维码"}
      </button>
    {/if}
  </div>
{/if}

{#if error}
  <p class="alert-danger mt-3 max-w-2xl">{error}</p>
{/if}
