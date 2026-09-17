<script lang="ts">
  import type { StatusInfo } from "$lib/api";
  let { status, onchange }: { status: StatusInfo | null; onchange: () => void } = $props();
</script>

<h2 class="text-lg font-semibold">设置</h2>
