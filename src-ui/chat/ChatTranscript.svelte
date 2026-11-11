<script lang="ts">
  import { tick, onMount } from 'svelte';
  import ArrowDown from '@lucide/svelte/icons/arrow-down';
  import MessageSquare from '@lucide/svelte/icons/message-square';
  import ChatMessage from './ChatMessage.svelte';
  import Button from '../lib/ui/Button.svelte';
  import EmptyState from '../lib/ui/EmptyState.svelte';
  import { nearBottom, type ChatTurn } from './conversation';
  import type { Snippet } from 'svelte';

  let { turns, compact = false, scrollRequest = 0, retry, review }: { turns: ChatTurn[]; compact?: boolean; scrollRequest?: number; retry: (turn: ChatTurn) => void; review?: Snippet<[ChatTurn]> } = $props();
  let viewport: HTMLDivElement;
  let content: HTMLDivElement;
  let following = $state(true);
  const visible = $derived(compact ? turns.slice(-1) : turns);

  // Follow the end until the reader scrolls up. Content growing under a
  // scroll event (an image loading) moves the end away without moving the
  // reader, so only an upward scroll stops following.
  let lastTop = 0;
  function scrolled() {
    const top = viewport.scrollTop;
    if (nearBottom(top, viewport.scrollHeight, viewport.clientHeight)) following = true;
    else if (top < lastTop) following = false;
    lastTop = top;
  }

  async function latest() {
    following = true;
    await tick();
    if (viewport) viewport.scrollTop = viewport.scrollHeight;
  }

  $effect(() => {
    void turns.map(turn => `${turn.id}:${turn.status}`).join('|');
    if (following) void tick().then(() => { if (viewport) viewport.scrollTop = viewport.scrollHeight; });
  });
  $effect(() => { if (scrollRequest) void latest(); });
  onMount(() => {
    const observer = new ResizeObserver(() => { if (following && viewport) viewport.scrollTop = viewport.scrollHeight; });
    observer.observe(content);
    observer.observe(viewport);
    return () => observer.disconnect();
  });
</script>

<div class="transcript-shell" class:compact>
  <div class="transcript" role="log" aria-label="Conversation" aria-live="polite" aria-relevant="additions" bind:this={viewport} onscroll={scrolled}>
    <div class="messages" bind:this={content}>
      {#if !visible.length}<EmptyState icon={MessageSquare} title="Work with your model" class="empty">Ask a question, discuss a design, or change the document. Prose and visuals appear here; edits stay undoable.</EmptyState>{/if}
      {#each visible as turn (turn.id)}<ChatMessage {turn} {review} retry={() => retry(turn)} />{/each}
    </div>
  </div>
  {#if !following && turns.length}<div class="latest"><Button variant="outline" small onclick={latest}><ArrowDown size={14} strokeWidth={1.75} />Latest</Button></div>{/if}
</div>

<style>
  .transcript-shell { position: relative; flex: 1; min-height: 0; }
  .transcript { height: 100%; overflow: auto; overscroll-behavior: contain; scrollbar-gutter: stable; }
  .messages { display: flex; flex-direction: column; gap: 28px; padding: 20px 16px 24px; min-height: 100%; box-sizing: border-box; }
  .compact .messages { gap: 10px; padding: 10px 14px; }
  .messages :global(.empty) { margin: auto; max-width: 310px; }
  /* "Latest": just above the composer, as T3 Code's scroll-to-end. */
  .latest { position: absolute; bottom: 10px; left: 50%; transform: translateX(-50%); }
  .latest :global(.ui-button) { background: var(--raised); box-shadow: var(--menu-shadow); }
  .latest :global(.ui-button:hover) { background: var(--raised); color: var(--fg); }
</style>
