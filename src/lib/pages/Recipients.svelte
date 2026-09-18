<script lang="ts">
  import { onMount } from "svelte";
  import { api, type Recipient, type StatusInfo } from "$lib/api";

  let { status }: { status: StatusInfo | null } = $props();

  let recipients = $state<Recipient[]>([]);
  let defaultRecipient = $state<string | null>(null);
  let newId = $state("");
  let newName = $state("");
  let message = $state("");
  let error = $state("");
  let editingId = $state<string | null>(null);
  let editName = $state("");

  async function load() {
    try {
      const cfg = await api.getConfig();
      recipients = cfg.recipients;
      defaultRecipient = cfg.default_recipient;
    } catch (e) {
      error = String(e);
    }
  }

  async function save() {
    error = "";
    try {
      const cfg = await api.saveRecipients(recipients, defaultRecipient);
      recipients = cfg.recipients;
      defaultRecipient = cfg.default_recipient;
      message = "已保存";
      setTimeout(() => (message = ""), 1500);
    } catch (e) {
      error = String(e);
    }
  }

  async function add() {
    const id = newId.trim();
    if (!id) {
      error = "请输入收件人 ID";
      return;
    }
    if (recipients.some((r) => r.id === id)) {
      error = "该 ID 已存在";
      return;
    }
    recipients = [...recipients, { id, name: newName.trim() || id }];
    newId = "";
    newName = "";
    await save();
  }

  async function remove(id: string) {
    if (editingId === id) cancelEdit();
    recipients = recipients.filter((r) => r.id !== id);
    if (defaultRecipient === id) defaultRecipient = null;
    await save();
  }

  async function setDefault(id: string | null) {
    defaultRecipient = id;
    await save();
  }

  // 备注行内编辑；ID 不可改（需改 ID 请删除后重新添加）
  function startEdit(r: Recipient) {
    editingId = r.id;
    editName = r.name;
  }

  function cancelEdit() {
    editingId = null;
    editName = "";
  }

  async function saveEdit() {
    const id = editingId;
    if (id === null) return;
    const name = editName.trim() || id;
    recipients = recipients.map((r) => (r.id === id ? { ...r, name } : r));
    cancelEdit();
    await save();
  }

  onMount(load);
</script>

<h2 class="text-lg font-semibold">收件人</h2>
<p class="mt-1 text-sm text-slate-500">
  iLink 没有好友列表接口，收件人 ID（形如 <code class="rounded bg-slate-100 px-1">xxx@im.wechat</code>）需要手动录入。
  webhook 请求不带 <code class="rounded bg-slate-100 px-1">to</code> 时发给默认收件人。
</p>

<div class="mt-6 max-w-2xl overflow-hidden rounded-xl border border-slate-200 bg-white">
  <table class="w-full text-sm">
    <thead class="bg-slate-50 text-left text-xs uppercase text-slate-500">
      <tr>
        <th class="px-4 py-2.5">默认</th>
        <th class="px-4 py-2.5">备注</th>
        <th class="px-4 py-2.5">ID</th>
        <th class="px-4 py-2.5"></th>
      </tr>
    </thead>
    <tbody>
      <tr class="border-t border-slate-100">
        <td class="px-4 py-2.5">
          <input type="radio" name="default" checked={defaultRecipient === null} onchange={() => setDefault(null)} />
        </td>
        <td class="px-4 py-2.5">扫码账号（自己）</td>
        <td class="px-4 py-2.5 font-mono text-slate-600">{status?.user_id ?? "未登录"}</td>
        <td></td>
      </tr>
      {#each recipients as r (r.id)}
        <tr class="border-t border-slate-100">
          <td class="px-4 py-2.5">
            <input type="radio" name="default" checked={defaultRecipient === r.id} onchange={() => setDefault(r.id)} />
          </td>
          <td class="px-4 py-2.5">
            {#if editingId === r.id}
              <form class="flex items-center gap-2" onsubmit={(e) => { e.preventDefault(); saveEdit(); }}>
                <input class="w-40 rounded-md border border-slate-300 px-2 py-1 text-sm" bind:value={editName} />
                <button type="submit" class="text-xs text-slate-900 hover:underline">保存</button>
                <button type="button" class="text-xs text-slate-500 hover:underline" onclick={cancelEdit}>取消</button>
              </form>
            {:else}
              {r.name}
            {/if}
          </td>
          <td class="px-4 py-2.5 font-mono text-slate-600">{r.id}</td>
          <td class="px-4 py-2.5 text-right">
            {#if editingId !== r.id}
              <button class="mr-3 text-xs text-slate-600 hover:underline" onclick={() => startEdit(r)}>编辑</button>
            {/if}
            <button class="text-xs text-rose-600 hover:underline" onclick={() => remove(r.id)}>删除</button>
          </td>
        </tr>
      {/each}
    </tbody>
  </table>
</div>

<form class="mt-4 flex max-w-2xl items-end gap-3" onsubmit={(e) => { e.preventDefault(); add(); }}>
  <label class="flex-1 text-xs text-slate-500">
    收件人 ID
    <input class="mt-1 w-full rounded-md border border-slate-300 px-3 py-1.5 font-mono text-sm" placeholder="xxx@im.wechat" bind:value={newId} />
  </label>
  <label class="w-40 text-xs text-slate-500">
    备注
    <input class="mt-1 w-full rounded-md border border-slate-300 px-3 py-1.5 text-sm" placeholder="可选" bind:value={newName} />
  </label>
  <button type="submit" class="rounded-md bg-slate-900 px-4 py-2 text-sm text-white hover:bg-slate-700">添加</button>
</form>

{#if message}<p class="mt-3 text-sm text-emerald-600">{message}</p>{/if}
{#if error}<p class="mt-3 text-sm text-rose-600">{error}</p>{/if}
