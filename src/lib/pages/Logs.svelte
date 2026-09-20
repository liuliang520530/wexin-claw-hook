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

<div class="flex items-center justify-between">
  <div>
    <h2 class="text-lg font-semibold">日志</h2>
    <p class="mt-1 text-sm text-slate-500">最近 500 条发送与鉴权记录，每 3 秒自动刷新。</p>
  </div>
  <div class="flex gap-2">
    <button class="rounded-md border border-slate-300 px-3 py-1.5 text-sm hover:bg-slate-100" onclick={refresh}>刷新</button>
    <button
      class="rounded-md border px-3 py-1.5 text-sm disabled:opacity-50 {confirmClear ? 'border-rose-400 bg-rose-50 text-rose-700' : 'border-slate-300 hover:bg-slate-100'}"
      onclick={clearAll}
      disabled={logs.length === 0}
    >
      {confirmClear ? "确认清空？" : "清空"}
    </button>
  </div>
</div>

<div class="mt-4 overflow-hidden rounded-xl border border-slate-200 bg-white">
  {#if logs.length === 0}
    <p class="p-6 text-sm text-slate-500">还没有记录。</p>
  {:else}
    <table class="w-full text-sm">
      <thead class="bg-slate-50 text-left text-xs uppercase text-slate-500">
        <tr>
          <th class="px-4 py-2.5">时间</th>
          <th class="px-4 py-2.5">结果</th>
          <th class="px-4 py-2.5">账号</th>
          <th class="px-4 py-2.5">内容</th>
        </tr>
      </thead>
      <tbody>
        {#each logs as l, i (i)}
          <tr class="border-t border-slate-100 align-top">
            <td class="whitespace-nowrap px-4 py-2 font-mono text-xs text-slate-500">{l.ts}</td>
            <td class="whitespace-nowrap px-4 py-2">
              {#if l.ok}
                <span class="rounded bg-emerald-100 px-2 py-0.5 text-xs text-emerald-700">成功</span>
              {:else}
                <span class="rounded bg-rose-100 px-2 py-0.5 text-xs text-rose-700">{l.code ?? "失败"}</span>
              {/if}
            </td>
            <td class="px-4 py-2 text-xs text-slate-600" title={l.to}>{l.to ? nameOf(l.to) : "-"}</td>
            <td class="px-4 py-2 break-all text-slate-700">{l.text}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>
