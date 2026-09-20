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

<h2 class="text-lg font-semibold">账号</h2>
<p class="mt-1 text-sm text-slate-500">
  每扫一次码就接入一个微信账号，消息只能发给已接入的账号。同一账号重复扫码即刷新登录。
</p>

{#if accounts.length > 0}
  <div class="mt-5 max-w-2xl space-y-3">
    {#each accounts as a (a.user_id)}
      <div class="rounded-xl border bg-white p-4 {a.token_expired ? 'border-rose-200' : 'border-slate-200'}">
        <div class="flex items-start justify-between gap-4">
          <div class="min-w-0 flex-1">
            <div class="flex items-center gap-2">
              <span class="h-2.5 w-2.5 shrink-0 rounded-full {a.token_expired ? 'bg-rose-500' : 'bg-emerald-500'}"></span>
              {#if editingId === a.user_id}
                <input
                  class="rounded-md border border-slate-300 px-2 py-1 text-sm"
                  bind:value={editName}
                  onkeydown={(e) => { if (e.key === "Enter") saveEdit(); if (e.key === "Escape") editingId = null; }}
                />
                <button class="text-xs text-emerald-700 hover:underline" onclick={saveEdit}>保存</button>
                <button class="text-xs text-slate-500 hover:underline" onclick={() => (editingId = null)}>取消</button>
              {:else}
                <span class="font-medium">{a.name}</span>
                <button class="text-xs text-slate-400 hover:text-slate-700 hover:underline" onclick={() => startEdit(a)}>编辑</button>
              {/if}
              {#if a.is_default}
                <span class="rounded bg-slate-900 px-1.5 py-0.5 text-[11px] text-white">默认</span>
              {/if}
            </div>
            <dl class="mt-2 space-y-0.5 text-xs">
              <div class="flex gap-2"><dt class="w-14 text-slate-400">账号 ID</dt><dd class="truncate font-mono text-slate-700">{a.user_id}</dd></div>
              <div class="flex gap-2"><dt class="w-14 text-slate-400">Bot ID</dt><dd class="truncate font-mono text-slate-500">{a.bot_id}</dd></div>
            </dl>
            {#if a.token_expired}
              <p class="mt-2 text-xs font-medium text-rose-600">登录已失效（微信侧 token 过期），请用该微信重新扫码。</p>
            {/if}
          </div>
          <div class="flex shrink-0 flex-col items-end gap-1.5 text-xs">
            {#if a.token_expired}
              <button class="rounded-md bg-rose-600 px-2.5 py-1 text-white hover:bg-rose-700" onclick={startScan} disabled={busy}>重新扫码</button>
            {/if}
            {#if !a.is_default}
              <button class="rounded-md border border-slate-300 px-2.5 py-1 hover:bg-slate-100" onclick={() => setDefault(a)}>设为默认</button>
            {/if}
            <button
              class="rounded-md border px-2.5 py-1 {confirmRemoveId === a.user_id ? 'border-rose-400 bg-rose-50 text-rose-700' : 'border-slate-300 hover:bg-slate-100'}"
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
    <button class="mt-4 rounded-md bg-slate-900 px-4 py-2 text-sm text-white hover:bg-slate-700 disabled:opacity-50" onclick={startScan} disabled={busy}>
      + 添加账号
    </button>
  {/if}
{/if}

{#if notice}
  <p class="mt-4 max-w-2xl rounded-md border border-emerald-200 bg-emerald-50 px-3 py-2 text-sm text-emerald-700">{notice}</p>
{/if}

{#if showScan}
  <div class="mt-5 max-w-md rounded-xl border border-slate-200 bg-white p-6">
    <div class="flex items-center justify-between">
      <h3 class="font-medium">{accounts.length === 0 ? "扫码登录" : "添加账号"}</h3>
      {#if accounts.length > 0}
        <button class="text-xs text-slate-500 hover:underline" onclick={closeScan}>收起</button>
      {/if}
    </div>
    {#if view}
      <div class="mt-4 flex flex-col items-center">
        <div class="rounded-lg border border-slate-200 p-2 [&>svg]:h-60 [&>svg]:w-60">
          {@html view.svg}
        </div>
        <p class="mt-4 text-sm {view.status.state === 'error' || view.status.state === 'expired' ? 'text-rose-600' : 'text-slate-700'}">
          {label[view.status.state]}
          {#if view.status.state === "error"}：{view.status.message}{/if}
        </p>
        {#if view.status.state === "expired" || view.status.state === "error"}
          <button class="mt-3 rounded-md bg-slate-900 px-4 py-1.5 text-sm text-white hover:bg-slate-700" onclick={startScan} disabled={busy}>
            刷新二维码
          </button>
        {:else if view.status.state === "confirmed"}
          <button class="mt-3 rounded-md border border-slate-300 px-4 py-1.5 text-sm hover:bg-slate-100" onclick={startScan} disabled={busy}>
            再扫一个
          </button>
        {/if}
      </div>
    {:else}
      <p class="mt-3 text-sm text-slate-600">点击下方按钮获取二维码，用要接入的微信扫码。二维码约 5 分钟内有效。</p>
      <button class="mt-4 rounded-md bg-slate-900 px-4 py-2 text-sm text-white hover:bg-slate-700 disabled:opacity-50" onclick={startScan} disabled={busy}>
        {busy ? "获取中…" : "获取二维码"}
      </button>
    {/if}
  </div>
{/if}

{#if error}
  <p class="mt-3 text-sm text-rose-600">{error}</p>
{/if}
