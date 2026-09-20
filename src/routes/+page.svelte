<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
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

  const win = getCurrentWindow();

  const nav: { id: Page; label: string; icon: string }[] = [
    { id: "login", label: "账号", icon: "M20 21v-2a4 4 0 0 0-4-4H8a4 4 0 0 0-4 4v2M16 7a4 4 0 1 1-8 0 4 4 0 0 1 8 0z" },
    { id: "chat", label: "发消息", icon: "M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" },
    { id: "settings", label: "设置", icon: "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 1 1-4 0v-.09a1.65 1.65 0 0 0-1-1.51 1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06A1.65 1.65 0 0 0 4.6 15a1.65 1.65 0 0 0-1.51-1H3a2 2 0 1 1 0-4h.09a1.65 1.65 0 0 0 1.51-1 1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06A1.65 1.65 0 0 0 9 4.6a1.65 1.65 0 0 0 1-1.51V3a2 2 0 1 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06A1.65 1.65 0 0 0 19.4 9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 1 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z" },
    { id: "logs", label: "日志", icon: "M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8zM14 2v6h6M16 13H8M16 17H8M10 9H8" },
  ];
</script>

<div class="flex h-screen select-none bg-canvas text-ink">
  <aside class="flex w-56 shrink-0 flex-col border-r border-line bg-panel">
    <!-- 顶部：可拖拽的品牌区 -->
    <div data-tauri-drag-region class="px-5 pt-5 pb-4">
      <div data-tauri-drag-region class="font-serif text-lg font-medium tracking-tight">ClawBot Webhook</div>
      <div data-tauri-drag-region class="mt-0.5 text-xs text-ink-3">v{status?.version ?? "-"}</div>
    </div>

    <nav class="flex-1 space-y-0.5 px-2">
      {#each nav as item (item.id)}
        {@const active = page === item.id}
        <button
          class="flex w-full items-center gap-2.5 rounded-md border-l-2 px-3 py-2 text-left text-sm transition-colors duration-200
            {active
              ? 'border-ink bg-panel-active font-medium text-ink'
              : 'border-transparent text-ink-2 hover:bg-panel-active/60 hover:text-ink'}"
          aria-current={active ? "page" : undefined}
          onclick={() => (page = item.id)}
        >
          <svg class="h-4 w-4 shrink-0" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
            <path d={item.icon} />
          </svg>
          {item.label}
        </button>
      {/each}
    </nav>

    <div class="space-y-1.5 border-t border-line px-5 py-4 text-xs text-ink-2">
      <div class="flex items-center gap-2">
        <span class="h-2 w-2 rounded-full {status?.logged_in ? 'bg-ok' : 'bg-danger'}"></span>
        {#if status?.account_count}
          {status.account_count} 个账号{status.expired_count ? `（${status.expired_count} 个已失效）` : ""}
        {:else}
          未登录
        {/if}
      </div>
      <div class="flex items-center gap-2">
        <span class="h-2 w-2 rounded-full {status?.server_running ? 'bg-ok' : 'bg-line-strong'}"></span>
        {status?.server_running ? `服务运行中 :${status.port}` : "服务已停止"}
      </div>
    </div>
  </aside>

  <div class="flex min-w-0 flex-1 flex-col">
    <!-- 无标题栏：顶部保留一条细拖拽区 + 窗口按钮 -->
    <div data-tauri-drag-region class="flex h-9 shrink-0 items-center justify-end pr-2">
      <button class="flex h-7 w-9 items-center justify-center rounded-md text-ink-3 transition-colors duration-200 hover:bg-panel hover:text-ink" aria-label="最小化" onclick={() => win.minimize()}>
        <svg class="h-3 w-3" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2"><path d="M1 6h10" /></svg>
      </button>
      <button class="flex h-7 w-9 items-center justify-center rounded-md text-ink-3 transition-colors duration-200 hover:bg-panel hover:text-ink" aria-label="最大化" onclick={() => win.toggleMaximize()}>
        <svg class="h-3 w-3" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2"><rect x="1.5" y="1.5" width="9" height="9" rx="1" /></svg>
      </button>
      <button class="flex h-7 w-9 items-center justify-center rounded-md text-ink-3 transition-colors duration-200 hover:bg-danger hover:text-white" aria-label="关闭" onclick={() => win.close()}>
        <svg class="h-3 w-3" viewBox="0 0 12 12" fill="none" stroke="currentColor" stroke-width="1.2"><path d="M2 2l8 8M10 2l-8 8" /></svg>
      </button>
    </div>

    <main class="flex-1 select-text overflow-auto px-10 pb-10 pt-2">
      {#key page}
        <div class="animate-fade-in">
          {#if page === "login"}
            <Login {status} onchange={refresh} />
          {:else if page === "chat"}
            <Chat {status} />
          {:else if page === "settings"}
            <Settings {status} onchange={refresh} />
          {:else}
            <Logs />
          {/if}
        </div>
      {/key}
    </main>
  </div>
</div>
