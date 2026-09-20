<script lang="ts">
  import { onMount } from "svelte";
  import { api, type StatusInfo } from "$lib/api";
  import Login from "$lib/pages/Login.svelte";
  import Chat from "$lib/pages/Chat.svelte";
  import Settings from "$lib/pages/Settings.svelte";
  import Logs from "$lib/pages/Logs.svelte";

  type Page = "login" | "chat" | "settings" | "logs";
  let page = $state<Page>("login");
  let status = $state<StatusInfo | null>(null);

  async function refresh() {
    try {
      status = await api.getStatus();
    } catch (e) {
      console.error(e);
    }
  }

  onMount(() => {
    refresh();
    const t = setInterval(refresh, 3000);
    return () => clearInterval(t);
  });

  const nav: { id: Page; label: string }[] = [
    { id: "login", label: "账号" },
    { id: "chat", label: "发消息" },
    { id: "settings", label: "设置" },
    { id: "logs", label: "日志" },
  ];
</script>

<div class="flex h-screen bg-slate-50 text-slate-800">
  <aside class="flex w-52 shrink-0 flex-col bg-slate-900 text-slate-200">
    <div class="border-b border-slate-800 px-5 py-5">
      <div class="text-base font-semibold text-white">ClawBot Webhook</div>
      <div class="mt-1 text-xs text-slate-400">v{status?.version ?? "-"}</div>
    </div>
    <nav class="flex-1 py-3">
      {#each nav as item (item.id)}
        <button
          class="w-full px-5 py-2.5 text-left text-sm transition hover:bg-slate-800 {page === item.id
            ? 'bg-slate-800 font-medium text-white'
            : ''}"
          onclick={() => (page = item.id)}
        >
          {item.label}
        </button>
      {/each}
    </nav>
    <div class="space-y-1.5 border-t border-slate-800 px-5 py-4 text-xs">
      <div class="flex items-center gap-2">
        <span class="h-2 w-2 rounded-full {status?.logged_in ? 'bg-emerald-400' : 'bg-rose-400'}"></span>
        {#if status?.account_count}
          {status.account_count} 个账号{status.expired_count ? `（${status.expired_count} 个已失效）` : ""}
        {:else}
          未登录
        {/if}
      </div>
      <div class="flex items-center gap-2">
        <span class="h-2 w-2 rounded-full {status?.server_running ? 'bg-emerald-400' : 'bg-slate-500'}"></span>
        {status?.server_running ? `服务运行中 :${status.port}` : "服务已停止"}
      </div>
    </div>
  </aside>

  <main class="flex-1 overflow-auto p-8">
    {#if page === "login"}
      <Login {status} onchange={refresh} />
    {:else if page === "chat"}
      <Chat {status} />
    {:else if page === "settings"}
      <Settings {status} onchange={refresh} />
    {:else}
      <Logs />
    {/if}
  </main>
</div>
