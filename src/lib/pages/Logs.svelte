<script lang="ts">
  import { onMount } from "svelte";
  import { api, type AccountView, type LogEntry } from "$lib/api";

  let logs = $state<LogEntry[]>([]);
  let accounts = $state<AccountView[]>([]);
  let confirmClear = $state(false);

  const nameOf = (id: string) => accounts.find((a) => a.user_id === id)?.name ?? id;

  async function refresh() {
    try {
      logs = await api.listLogs();
    } catch (e) {
      console.error(e);
    }
  }

  async function clearAll() {
    if (!confirmClear) {
      confirmClear = true;
      setTimeout(() => (confirmClear = false), 3000);
      return;
    }
    confirmClear = false;
    try {
      await api.clearLogs();
      await refresh();
    } catch (e) {
      console.error(e);
    }
  }

  onMount(() => {
    api.listAccounts().then((a) => (accounts = a)).catch(console.error);
    refresh();
    const t = setInterval(refresh, 3000);
    return () => clearInterval(t);
  });
</script>

<div class="flex items-end justify-between gap-4">
  <div>
    <h2 class="page-title">日志</h2>
    <p class="page-desc">最近 500 条发送与鉴权记录，每 3 秒自动刷新。</p>
  </div>
  <div class="flex shrink-0 gap-2">
    <button class="btn-secondary" onclick={refresh}>刷新</button>
    <button
      class={confirmClear ? "btn-confirm" : "btn-secondary"}
      onclick={clearAll}
      disabled={logs.length === 0}
    >
      {confirmClear ? "确认清空？" : "清空"}
    </button>
  </div>
</div>

<div class="card mt-5 overflow-hidden">
  {#if logs.length === 0}
    <p class="p-6 text-sm text-ink-3">还没有记录。</p>
  {:else}
    <table class="w-full text-sm">
      <thead class="bg-panel text-left text-xs text-ink-3">
        <tr>
          <th class="px-4 py-2.5 font-medium">时间</th>
          <th class="px-4 py-2.5 font-medium">结果</th>
          <th class="px-4 py-2.5 font-medium">通道</th>
          <th class="px-4 py-2.5 font-medium">账号 · 应用</th>
          <th class="px-4 py-2.5 font-medium">内容</th>
        </tr>
      </thead>
      <tbody>
        {#each logs as l, i (i)}
          <tr class="border-t border-line align-top transition-colors duration-200 hover:bg-panel/50">
            <td class="whitespace-nowrap px-4 py-2 font-mono text-xs text-ink-3">{l.ts}</td>
            <td class="whitespace-nowrap px-4 py-2">
              {#if l.ok}
                <span class="badge bg-ok-soft text-ok">成功</span>
              {:else}
                <span class="badge bg-danger-soft text-danger">{l.code ?? "失败"}</span>
              {/if}
            </td>
            <td class="whitespace-nowrap px-4 py-2">
              {#if l.channel === "wecom"}
                <span class="badge bg-accent-soft text-accent">企微</span>
              {:else}
                <span class="badge bg-panel text-ink-2">微信</span>
              {/if}
            </td>
            <td class="px-4 py-2 text-xs text-ink-2" title={l.to}>
              {#if l.channel === "wecom"}
                {l.from || "-"} → {l.to || "-"}
              {:else}
                {l.to ? nameOf(l.to) : "-"}
              {/if}
            </td>
            <td class="px-4 py-2 break-all text-ink">{l.text}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>
