<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { api, type LoginView, type StatusInfo } from "$lib/api";

  let { status, onchange }: { status: StatusInfo | null; onchange: () => void } = $props();

  let view = $state<LoginView | null>(null);
  let busy = $state(false);
  let error = $state("");
  let timer: ReturnType<typeof setInterval> | null = null;

  const label: Record<string, string> = {
    wait: "请用微信「扫一扫」",
    scanned: "已扫码，请在手机上点确认",
    confirmed: "登录成功",
    expired: "二维码已过期，请刷新",
    error: "登录出错",
  };

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
      const s = v.status.state;
      if (s === "confirmed") {
        stopPolling();
        onchange();
      } else if (s === "expired" || s === "error") {
        stopPolling();
      }
    } catch (e) {
      error = String(e);
      stopPolling();
    }
  }

  async function start() {
    busy = true;
    error = "";
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

  async function logout() {
    stopPolling();
    view = null;
    await api.logout();
    onchange();
  }

  // 页面切换回来时恢复后端仍在进行中的登录会话，避免二维码"消失"
  onMount(async () => {
    try {
      const v = await api.loginStatus();
      if (!v) return;
      const s = v.status.state;
      if (s === "wait" || s === "scanned") {
        view = v;
        stopPolling();
        timer = setInterval(poll, 1500);
      } else if (s === "expired" || s === "error") {
        view = v;
      }
    } catch (e) {
      error = String(e);
    }
  });

  onDestroy(stopPolling);
</script>

<h2 class="text-lg font-semibold">登录</h2>
<p class="mt-1 text-sm text-slate-500">用微信扫码授权后，本机才能通过 ClawBot 发消息。</p>

{#if status?.logged_in}
  <div class="mt-6 max-w-md rounded-xl border border-emerald-200 bg-emerald-50 p-5">
    <div class="flex items-center gap-2 text-emerald-700">
      <span class="h-2.5 w-2.5 rounded-full bg-emerald-500"></span>
      <span class="font-medium">已登录</span>
    </div>
    <dl class="mt-3 space-y-1 text-sm">
      <div class="flex gap-3"><dt class="w-20 text-slate-500">Bot ID</dt><dd class="font-mono">{status.bot_id}</dd></div>
      <div class="flex gap-3"><dt class="w-20 text-slate-500">扫码账号</dt><dd class="font-mono">{status.user_id}</dd></div>
    </dl>
    <p class="mt-3 text-xs text-slate-500">未指定收件人时，消息默认发给扫码账号。</p>
    <button class="mt-4 rounded-md border border-slate-300 bg-white px-3 py-1.5 text-sm hover:bg-slate-100" onclick={logout}>
      退出登录
    </button>
  </div>
{:else}
  {#if status?.token_expired}
    <div class="mt-6 max-w-md rounded-xl border border-rose-300 bg-rose-50 px-4 py-3 text-sm font-medium text-rose-700">
      登录已失效（微信侧 token 过期），请重新扫码登录。
    </div>
  {/if}
  <div class="mt-6 max-w-md rounded-xl border border-slate-200 bg-white p-6">
    {#if view}
      <div class="flex flex-col items-center">
        <div class="rounded-lg border border-slate-200 p-2 [&>svg]:h-60 [&>svg]:w-60">
          {@html view.svg}
        </div>
        <p class="mt-4 text-sm {view.status.state === 'error' || view.status.state === 'expired' ? 'text-rose-600' : 'text-slate-700'}">
          {label[view.status.state]}
          {#if view.status.state === "error"}：{view.status.message}{/if}
        </p>
        {#if view.status.state === "expired" || view.status.state === "error"}
          <button class="mt-3 rounded-md bg-slate-900 px-4 py-1.5 text-sm text-white hover:bg-slate-700" onclick={start} disabled={busy}>
            刷新二维码
          </button>
        {/if}
      </div>
    {:else}
      <p class="text-sm text-slate-600">点击下方按钮获取二维码。二维码约 5 分钟内有效。</p>
      <button class="mt-4 rounded-md bg-slate-900 px-4 py-2 text-sm text-white hover:bg-slate-700 disabled:opacity-50" onclick={start} disabled={busy}>
        {busy ? "获取中…" : "扫码登录"}
      </button>
    {/if}
    {#if error}
      <p class="mt-3 text-sm text-rose-600">{error}</p>
    {/if}
  </div>
{/if}
