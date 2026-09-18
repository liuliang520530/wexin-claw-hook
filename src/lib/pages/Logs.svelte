<script lang="ts">
  import { onMount } from "svelte";
  import { api, type LogEntry } from "$lib/api";

  let logs = $state<LogEntry[]>([]);

  async function refresh() {
    try {
      logs = await api.listLogs();
    } catch (e) {
      console.error(e);
    }
  }

  onMount(() => {
    refresh();
    const t = setInterval(refresh, 3000);
    return () => clearInterval(t);
  });
</script>

<div class="flex items-center justify-between">
  <div>
    <h2 class="text-lg font-semibold">日志</h2>
    <p class="mt-1 text-sm text-slate-500">最近 500 条 webhook 调用与鉴权记录，每 3 秒自动刷新。</p>
  </div>
  <button class="rounded-md border border-slate-300 px-3 py-1.5 text-sm hover:bg-slate-100" onclick={refresh}>刷新</button>
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
          <th class="px-4 py-2.5">收件人</th>
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
            <td class="px-4 py-2 font-mono text-xs text-slate-600">{l.to || "-"}</td>
            <td class="px-4 py-2 break-all text-slate-700">{l.text}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
</div>
