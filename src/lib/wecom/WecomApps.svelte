<script lang="ts">
  import { onMount } from "svelte";
  import { api, type WecomAppView } from "$lib/api";

  let { onchange }: { onchange: () => void } = $props();

  let apps = $state<WecomAppView[]>([]);
  let error = $state("");
  let notice = $state("");
  let busy = $state(false);

  // 行内改名
  let editingName = $state<string | null>(null);
  let editName = $state("");
  let confirmRemove = $state<string | null>(null);
  let copied = $state<string | null>(null);

  // 添加 / 编辑凭据表单；editingCreds 为 null 表示添加
  let formOpen = $state(false);
  let editingCreds = $state<string | null>(null);
  let fName = $state("");
  let fCorpid = $state("");
  let fAgentid = $state("");
  let fSecret = $state("");
  let showSecret = $state(false);

  async function load() {
    try {
      apps = await api.wecomListApps();
    } catch (e) {
      error = String(e);
    }
  }

  function openAdd() {
    editingCreds = null;
    fName = "";
    fCorpid = "";
    fAgentid = "";
    fSecret = "";
    showSecret = false;
    formOpen = true;
    error = "";
    notice = "";
  }

  function openEdit(a: WecomAppView) {
    editingCreds = a.name;
    fName = a.name;
    fCorpid = a.corpid;
    fAgentid = String(a.agentid);
    fSecret = "";
    showSecret = false;
    formOpen = true;
    error = "";
    notice = "";
  }

  function closeForm() {
    formOpen = false;
    editingCreds = null;
  }

  async function submitForm() {
    const agentid = Number(fAgentid);
    if (!fCorpid.trim()) return void (error = "corpid 不能为空");
    if (!Number.isInteger(agentid) || agentid <= 0) return void (error = "agentid 必须是正整数");
    if (editingCreds === null && !fSecret.trim()) return void (error = "secret 不能为空");
    busy = true;
    error = "";
    try {
      const r =
        editingCreds === null
          ? await api.wecomAddApp({ name: fName.trim() || undefined, corpid: fCorpid.trim(), agentid, secret: fSecret.trim() })
          : await api.wecomUpdateApp({
              name: editingCreds,
              newName: fName.trim() || undefined,
              corpid: fCorpid.trim(),
              agentid,
              secret: fSecret.trim() || undefined,
            });
      apps = r.apps;
      const verb = editingCreds === null ? "已添加" : "已更新";
      const shown = fName.trim() || r.apps.find((a) => a.corpid === fCorpid.trim() && a.agentid === agentid)?.name || "";
      notice = `${verb}「${shown}」${r.notice ? `。${r.notice}` : ""}`;
      closeForm();
      onchange();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function startRename(a: WecomAppView) {
    editingName = a.name;
    editName = a.name;
  }

  async function saveRename() {
    if (!editingName) return;
    error = "";
    try {
      const r = await api.wecomUpdateApp({ name: editingName, newName: editName });
      apps = r.apps;
      editingName = null;
      onchange();
    } catch (e) {
      error = String(e);
    }
  }

  async function setDefault(a: WecomAppView) {
    error = "";
    try {
      apps = await api.wecomSetDefault(a.name);
      onchange();
    } catch (e) {
      error = String(e);
    }
  }

  async function remove(a: WecomAppView) {
    if (confirmRemove !== a.name) {
      confirmRemove = a.name;
      return;
    }
    confirmRemove = null;
    error = "";
    try {
      apps = await api.wecomRemoveApp(a.name);
      onchange();
    } catch (e) {
      error = String(e);
    }
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      copied = text;
      setTimeout(() => { if (copied === text) copied = null; }, 1500);
    } catch (e) {
      error = String(e);
    }
  }

  onMount(load);

  const showForm = $derived(formOpen || apps.length === 0);
</script>

<p class="page-desc">
  corpid 在企业微信管理后台「我的企业」页底部；agentid 与 secret 在「应用管理」里打开自建应用查看。收件人必须在该应用的可见范围内。
</p>

{#if apps.length > 0}
  <div class="mt-6 max-w-2xl space-y-3">
    {#each apps as a (a.name)}
      <div class="card card-hover p-4 {a.invalid ? 'border-danger/40' : ''}">
        <div class="flex items-start justify-between gap-4">
          <div class="min-w-0 flex-1">
            <div class="flex items-center gap-2">
              <span class="h-2.5 w-2.5 shrink-0 rounded-full {a.invalid ? 'bg-danger' : 'bg-ok'}"></span>
              {#if editingName === a.name}
                <input
                  class="input py-1"
                  aria-label="应用备注"
                  bind:value={editName}
                  onkeydown={(e) => { if (e.key === "Enter") saveRename(); if (e.key === "Escape") editingName = null; }}
                />
                <button class="btn-link text-ok hover:text-ok" onclick={saveRename}>保存</button>
                <button class="btn-link" onclick={() => (editingName = null)}>取消</button>
              {:else}
                <span class="font-serif text-base font-medium">{a.name}</span>
                <button class="btn-link" onclick={() => startRename(a)}>编辑</button>
              {/if}
              {#if a.is_default}
                <span class="badge bg-ink text-white">默认</span>
              {/if}
            </div>
            <dl class="mt-2.5 space-y-0.5 text-xs">
              <div class="flex gap-2">
                <dt class="w-14 text-ink-3">corpid</dt>
                <dd class="flex min-w-0 items-center gap-1.5 font-mono text-ink-2">
                  <span class="truncate">{a.corpid}</span>
                  <button class="btn-link shrink-0 {copied === a.corpid ? 'text-ok hover:text-ok' : ''}" onclick={() => copy(a.corpid)}>
                    {copied === a.corpid ? "已复制" : "复制"}
                  </button>
                </dd>
              </div>
              <div class="flex gap-2"><dt class="w-14 text-ink-3">agentid</dt><dd class="font-mono text-ink-3">{a.agentid}</dd></div>
            </dl>
            {#if a.invalid}
              <p class="mt-2 text-xs font-medium text-danger">凭据无效：{a.invalid}</p>
            {/if}
          </div>
          <div class="flex shrink-0 flex-col items-end gap-1.5">
            {#if !a.is_default}
              <button class="btn-secondary btn-sm" onclick={() => setDefault(a)}>设为默认</button>
            {/if}
            <button class="btn-secondary btn-sm" onclick={() => openEdit(a)}>编辑凭据</button>
            <button class="btn-sm {confirmRemove === a.name ? 'btn-confirm' : 'btn-secondary'}" onclick={() => remove(a)}>
              {confirmRemove === a.name ? "确认删除？" : "删除"}
            </button>
          </div>
        </div>
      </div>
    {/each}
  </div>

  {#if !showForm}
    <button class="btn-primary mt-4" onclick={openAdd}>
      <svg class="h-4 w-4" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M12 5v14M5 12h14" /></svg>
      添加应用
    </button>
  {/if}
{/if}

{#if notice}
  <p class="alert-ok mt-4 max-w-2xl">{notice}</p>
{/if}

{#if showForm}
  <div class="card mt-6 max-w-md p-6">
    <div class="flex items-center justify-between">
      <h3 class="section-title">{editingCreds === null ? "添加应用" : `编辑「${editingCreds}」`}</h3>
      {#if apps.length > 0}
        <button class="btn-link" onclick={closeForm}>收起</button>
      {/if}
    </div>
    <div class="mt-4 space-y-3">
      <label class="block text-xs text-ink-3">
        备注
        <input class="input mt-1 block w-full" bind:value={fName} placeholder="留空则使用应用名称" />
      </label>
      <label class="block text-xs text-ink-3">
        corpid
        <input class="input mt-1 block w-full font-mono" bind:value={fCorpid} placeholder="ww…" />
      </label>
      <label class="block text-xs text-ink-3">
        agentid
        <input class="input mt-1 block w-full font-mono" type="number" min="1" bind:value={fAgentid} placeholder="1000002" />
      </label>
      <label class="block text-xs text-ink-3">
        secret
        <div class="mt-1 flex gap-2">
          <input
            class="input block w-full font-mono"
            type={showSecret ? "text" : "password"}
            bind:value={fSecret}
            placeholder={editingCreds === null ? "" : "留空表示不修改"}
          />
          <button class="btn-secondary shrink-0" type="button" onclick={() => (showSecret = !showSecret)}>{showSecret ? "隐藏" : "显示"}</button>
        </div>
      </label>
    </div>
    <button class="btn-primary mt-4" onclick={submitForm} disabled={busy}>
      {busy ? "验证中…" : "验证并保存"}
    </button>
  </div>
{/if}

{#if error}
  <p class="alert-danger mt-3 max-w-2xl">{error}</p>
{/if}
