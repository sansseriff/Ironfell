<script lang="ts">
  import { tick, onMount } from 'svelte';
  import ArrowDown from '@lucide/svelte/icons/arrow-down';
  import MessageSquare from '@lucide/svelte/icons/message-square';
  import Button from '../lib/ui/Button.svelte';
  import ChatMessage from './ChatMessage.svelte';
  import { nearBottom, type ChatTurn } from './conversation';
  import type { Snippet } from 'svelte';

  let { turns, compact = false, scrollRequest = 0, retry, review }: { turns: ChatTurn[]; compact?: boolean; scrollRequest?: number; retry: (turn: ChatTurn) => void; review?: Snippet<[ChatTurn]> } = $props();
  let viewport: HTMLDivElement;
  let content: HTMLDivElement;
  let following = $state(true);
  const visible = $derived(compact ? turns.slice(-1) : turns);

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
  <div class="transcript" role="log" aria-label="Conversation" aria-live="polite" aria-relevant="additions" bind:this={viewport} onscroll={() => following = nearBottom(viewport.scrollTop, viewport.scrollHeight, viewport.clientHeight)}>
    <div class="messages" bind:this={content}>
      {#if !visible.length}<div class="empty"><MessageSquare size={22} strokeWidth={1.5} /><strong>Work with your model</strong><p>Ask a question, discuss a design, or change the document. Prose and visuals appear here; edits stay undoable.</p></div>{/if}
      {#each visible as turn (turn.id)}<ChatMessage {turn} {review} retry={() => retry(turn)} />{/each}
    </div>
  </div>
  {#if !following && turns.length}<div class="latest"><Button small variant="outline" onclick={latest}><ArrowDown size={13} strokeWidth={1.75} />Latest</Button></div>{/if}
</div>

<style>
  .transcript-shell { position: relative; flex: 1; min-height: 0; }
  .transcript { height: 100%; overflow: auto; overscroll-behavior: contain; scrollbar-gutter: stable; }
  .transcript:focus-visible { outline: 2px solid var(--accent); outline-offset: -2px; }
  .messages { display: flex; flex-direction: column; gap: 28px; padding: 18px 18px 22px; min-height: 100%; box-sizing: border-box; }
  .compact .messages { gap: 10px; padding: 10px 14px; }
  .empty { margin: auto; max-width: 310px; display: flex; flex-direction: column; align-items: center; gap: 10px; text-align: center; color: var(--fg-muted); padding: 24px 8px; font-size: 12px; }
  .empty strong { color: var(--fg); font-size: 13px; font-weight: 500; }
  .empty p { margin: 0; line-height: 1.6; }
  .latest { position: absolute; bottom: 10px; left: 50%; transform: translateX(-50%); background: var(--bg); border-radius: 6px; box-shadow: var(--menu-shadow); }
</style>
