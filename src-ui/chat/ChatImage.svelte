<script lang="ts">
  import Button from '../lib/ui/Button.svelte';
  import { safeChatUrl } from './conversation';

  let { src, alt }: { src: string; alt: string } = $props();
  let loaded = $state(false);
  let failed = $state(false);
  const safe = $derived(safeChatUrl(src, true));
</script>

{#if safe && loaded && !failed}
  <img src={safe} {alt} referrerpolicy="no-referrer" loading="lazy" onerror={() => failed = true} />
{:else if safe && !failed}
  <Button variant="outline" small title={safe} onclick={() => loaded = true}>Load external image: {alt || 'image'}</Button>
{:else}<span class="unavailable">{alt || 'Image'} ({failed ? 'image unavailable' : 'unsupported image address'})</span>{/if}

<style>
  img { display: block; max-width: 100%; max-height: 360px; object-fit: contain; border-radius: var(--radius); margin: 8px 0; }
  .unavailable { color: var(--fg-muted); }
</style>
