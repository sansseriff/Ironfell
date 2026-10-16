<script lang="ts">
  import Scan from "@lucide/svelte/icons/scan";
  let { kind }: { kind: "opacity" | "radius" | "tl" | "tr" | "bl" | "br" } = $props();
  const rotation = $derived(({ tl: 0, tr: 90, br: 180, bl: 270 } as Record<string, number>)[kind] ?? 0);
</script>

{#if kind === "radius"}
  <Scan size={16} strokeWidth={1.5} />
{:else}
  <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
    {#if kind === "opacity"}
      <rect x="3" y="3" width="18" height="18" rx="3" />
      <g fill="currentColor" stroke="none">
        {#each [[7, 17], [11, 17], [15, 17], [11, 13], [15, 13], [15, 9]] as [cx, cy]}<circle {cx} {cy} r="0.85" />{/each}
      </g>
    {:else}
      <path d="M5 19V11a6 6 0 0 1 6-6h8" transform="rotate({rotation} 12 12)" />
    {/if}
  </svg>
{/if}
