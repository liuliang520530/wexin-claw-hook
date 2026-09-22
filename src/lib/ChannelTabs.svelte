<script lang="ts">
  import { channel, type Channel } from "$lib/channel.svelte";

  const items: { id: Channel; label: string }[] = [
    { id: "weixin", label: "微信" },
    { id: "wecom", label: "企业微信" },
  ];
</script>

<div class="mt-4 inline-flex rounded-lg border border-line bg-panel p-0.5" role="tablist" aria-label="通道">
  {#each items as it (it.id)}
    {@const active = channel.value === it.id}
    <button
      role="tab"
      aria-selected={active}
      class="rounded-md px-3.5 py-1.5 text-sm transition-colors duration-200
        {active ? 'bg-surface font-medium text-ink shadow-card' : 'text-ink-2 hover:text-ink'}"
      onclick={() => (channel.value = it.id)}
    >
      {it.label}
    </button>
  {/each}
</div>
