<script lang="ts">
  import type { StatusInfo } from "$lib/api";
  let { status }: { status: StatusInfo | null } = $props();
</script>

<h2 class="text-lg font-semibold">收件人</h2>
