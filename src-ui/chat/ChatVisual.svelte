<script lang="ts">
  import ExternalLink from '@lucide/svelte/icons/external-link';
  import RefreshCw from '@lucide/svelte/icons/refresh-cw';
  import Button from '../lib/ui/Button.svelte';
  import IconButton from '../lib/ui/IconButton.svelte';
  import { controllerManager } from '../controller-manager.svelte';
  import { showComposition } from '../views/workspace';
  import { imageUrl } from '../model/images';

  let { show, focus = '' }: { show: string; focus?: string } = $props();
  const doc = controllerManager.docClient;
  let src = $state('');
  let error = $state('');
  let pending = $state(false);
  let capture: AbortController | undefined;

  async function refresh(source: string) {
    capture?.abort();
    const request = new AbortController();
    capture = request;
    const generation = doc.generation;
    pending = true;
    error = '';
    try {
      const result = await doc.capturePicture(source, request.signal);
      if (!request.signal.aborted && generation === doc.generation) src = imageUrl(result);
    } catch (reason) {
      if (!request.signal.aborted && generation === doc.generation) error = reason instanceof Error ? reason.message : String(reason);
    } finally { if (!request.signal.aborted) pending = false; }
  }

  function open() {
    showComposition(show);
    if (focus) void doc.command({ op: 'focus', node: focus }).catch(reason => { error = String(reason); });
  }

  $effect(() => {
    src = '';
    void refresh(show);
    return () => capture?.abort();
  });
</script>

<figure class="visual" data-chat-visual={show}>
  <figcaption><span>{show}</span><span class="hint">2D snapshot</span><IconButton icon={RefreshCw} label="Refresh visual snapshot" disabled={pending} onclick={() => refresh(show)} /></figcaption>
  {#if src}<img {src} alt={`Composition ${show}`} />{/if}
  {#if error}<p role="status">{error}</p>{:else if pending}<p role="status">Loading visual…</p>{/if}
  <div class="actions"><Button small variant="outline" onclick={open}><ExternalLink size={14} strokeWidth={1.75} />Open composition</Button></div>
</figure>

<style>
  .visual { margin: 10px 0; border: 1px solid var(--border); border-radius: var(--radius-lg); overflow: hidden; background: var(--bg-sunken); }
  figcaption { display: flex; align-items: center; gap: 8px; padding: 6px 10px; font-size: var(--text-md); font-weight: 500; border-bottom: 1px solid var(--border); }
  .hint { margin-left: auto; color: var(--fg-muted); font-size: var(--text-sm); font-weight: 400; }
  img { display: block; width: 100%; height: 190px; object-fit: contain; background: var(--bg); }
  p { font-size: var(--text-sm); color: var(--fg-muted); margin: 10px; overflow-wrap: anywhere; }
  .actions { padding: 8px 10px; }
</style>
