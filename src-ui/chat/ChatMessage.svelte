<script lang="ts">
  import { Collapsible } from 'bits-ui';
  import Copy from '@lucide/svelte/icons/copy';
  import Check from '@lucide/svelte/icons/check';
  import ChevronDown from '@lucide/svelte/icons/chevron-down';
  import IconButton from '../lib/ui/IconButton.svelte';
  import Button from '../lib/ui/Button.svelte';
  import ChatMarkdown from './ChatMarkdown.svelte';
  import ChatVisual from './ChatVisual.svelte';
  import { contentParts, type ChatTurn } from './conversation';
  import type { Snippet } from 'svelte';

  let { turn, retry, review }: { turn: ChatTurn; retry: () => void; review?: Snippet<[ChatTurn]> } = $props();
  const parts = $derived(contentParts(turn.result?.note ?? ''));
  let copied = $state(false);
  let copyError = $state('');
  async function copy() {
    copyError = '';
    try { await navigator.clipboard.writeText(turn.result?.note || turn.result?.edit || ''); copied = true; }
    catch { copyError = 'Could not copy; select the message text instead.'; }
  }
</script>

<article class="chat-turn" data-turn-id={turn.id} data-status={turn.status}>
  <div class="user-message"><span class="role">You</span><p>{turn.instruction}</p></div>
  <div class="assistant-message">
    <div class="heading"><span class="role">{turn.result?.transport.split(':')[0] || 'Assistant'}</span><span class="target">{turn.target}</span></div>
    {#if turn.status === 'running'}
      <p class="working" role="status"><span class="pulse"></span>Working…</p>
    {:else}
      {#each parts as part, index (index)}
        {#if part.kind === 'markdown'}<ChatMarkdown text={part.text} />{:else}<ChatVisual show={part.show} focus={part.focus} />{/if}
      {/each}
      {#if !parts.length && turn.result?.ok}<p class="summary">{turn.result.outcome ? 'Document updated.' : 'Done.'}</p>{/if}
      {#if turn.error}<p class="error" role="alert">{turn.error}</p><Button small variant="outline" onclick={retry}>Restore prompt</Button>{/if}
      {@render review?.(turn)}
      {#if turn.result?.edit}
        <Collapsible.Root>
          <Collapsible.Trigger class="edit-trigger"><ChevronDown size={12} strokeWidth={1.75} />{turn.result.outcome?.ok ? 'Applied document edit' : 'Document edit details'} · {turn.result.attempts} attempt{turn.result.attempts === 1 ? '' : 's'}</Collapsible.Trigger>
          <Collapsible.Content><pre class="edit">{turn.result.edit}{turn.result.outcome?.failure ? `\n${turn.result.outcome.failure}` : ''}</pre></Collapsible.Content>
        </Collapsible.Root>
      {/if}
      <div class="meta"><span>{turn.result?.transport ?? 'Runtime error'}{turn.completedAt ? ` · ${Math.max(0.1, (turn.completedAt - turn.startedAt) / 1000).toFixed(1)}s` : ''}</span><IconButton icon={copied ? Check : Copy} label={copied ? 'Message copied' : 'Copy assistant message'} onclick={copy} /></div>
      {#if copyError}<p class="error" role="status">{copyError}</p>{/if}
    {/if}
  </div>
</article>

<style>
  .chat-turn { width: min(100%, 720px); margin: 0 auto; min-width: 0; }
  .user-message { margin: 0 0 18px 24px; padding: 10px 14px; background: var(--bg-sunken); border: 1px solid var(--border); border-radius: 12px 12px 3px 12px; }
  .user-message p { white-space: pre-wrap; overflow-wrap: anywhere; font-size: 13px; line-height: 1.55; margin: 4px 0 0; }
  .role { font-size: 11px; font-weight: 600; text-transform: capitalize; }
  .heading { display: flex; flex-wrap: wrap; align-items: baseline; gap: 6px 10px; margin-bottom: 8px; }
  .target { color: var(--fg-faint); font-size: 10px; }
  .assistant-message { min-width: 0; }
  .working, .summary { font-size: 12px; margin: 4px 0 10px; color: var(--fg-muted); }
  .working { display: flex; align-items: center; gap: 8px; }
  .pulse { width: 6px; height: 6px; border-radius: 50%; background: var(--accent); }
  .error { color: var(--danger); font-size: 12px; overflow-wrap: anywhere; white-space: pre-wrap; }
  :global(.edit-trigger) { display: flex; align-items: center; gap: 5px; background: transparent; border: none; border-radius: 4px; font: inherit; font-size: 10px; color: var(--fg-muted); cursor: pointer; padding: 5px 0; margin-top: 8px; }
  :global(.edit-trigger:hover) { color: var(--fg); }
  :global(.edit-trigger:focus-visible) { outline: 2px solid var(--accent); outline-offset: 2px; }
  .edit { font-size: 10px; background: var(--bg-sunken); padding: 10px; border-radius: 6px; max-height: 220px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; }
  .meta { display: flex; align-items: center; justify-content: space-between; gap: 6px; margin-top: 8px; color: var(--fg-faint); font-size: 10px; }
</style>
