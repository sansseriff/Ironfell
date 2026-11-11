<script lang="ts">
  import { imageUrl } from '../model/images';
  import { Collapsible } from 'bits-ui';
  import Copy from '@lucide/svelte/icons/copy';
  import Check from '@lucide/svelte/icons/check';
  import ChevronDown from '@lucide/svelte/icons/chevron-down';
  import IconButton from '../lib/ui/IconButton.svelte';
  import Button from '../lib/ui/Button.svelte';
  import ChatMarkdown from './ChatMarkdown.svelte';
  import ChatVisual from './ChatVisual.svelte';
  import ChatProgress from './ChatProgress.svelte';
  import PencilLine from '@lucide/svelte/icons/pencil-line';
  import ClipboardList from '@lucide/svelte/icons/clipboard-list';
  import { appliedEdits, contentParts, streamView, turnLog, type ChatTurn } from './conversation';
  import type { Snippet } from 'svelte';

  let { turn, retry, review }: { turn: ChatTurn; retry: () => void; review?: Snippet<[ChatTurn]> } = $props();
  const parts = $derived(contentParts(turn.result?.note ?? ''));
  // While it runs: the reply as it streams, split into prose and the edit forming.
  const live = $derived(turn.status === 'running' ? streamView(turn.progress?.text ?? '') : null);
  const liveParts = $derived(live?.prose ? contentParts(live.prose) : []);
  /** Verbs listed while an edit forms; earlier ones are counted. */
  const SHOWN_VERBS = 6;
  let copied = $state(false);
  let copyError = $state('');
  // "Copy turn log": the turn as JSON, for a bug report (conversation.ts `turnLog`).
  let logCopied = $state(false);
  async function copyLog() {
    copyError = '';
    try {
      await navigator.clipboard.writeText(JSON.stringify(turnLog($state.snapshot(turn) as ChatTurn), null, 2));
      logCopied = true;
      setTimeout(() => { logCopied = false; }, 1500);
    } catch { copyError = 'Could not copy the turn log.'; }
  }
  async function copy() {
    copyError = '';
    try { await navigator.clipboard.writeText(turn.result?.note || turn.result?.edit || ''); copied = true; }
    catch { copyError = 'Could not copy; select the message text instead.'; }
  }
</script>

<article class="chat-turn" data-turn-id={turn.id} data-status={turn.status}>
  <div class="user-row">
    <div class="user-message"><p>{turn.instruction}</p>{#each turn.images ?? [] as image}<img class="attached-image" class:preview={image.preview} src={imageUrl(image)} alt={image.name} title={image.preview ? `${image.name} (a saved preview; the full image is no longer kept)` : image.name} />{/each}</div>
    {#if turn.target}<span class="target">{turn.target}</span>{/if}
  </div>
  <div class="assistant-message">
    {#if turn.progress && (turn.status === 'running' || turn.progress.steps.length)}
      <ChatProgress progress={turn.progress} running={turn.status === 'running'} startedAt={turn.startedAt} completedAt={turn.completedAt} />
    {/if}
    {#if turn.status === 'running'}
      {#if !turn.progress}<p class="working" role="status"><span class="pulse"></span>Working…</p>{/if}
      {#each liveParts as part, index (index)}
        {#if part.kind === 'markdown'}<ChatMarkdown text={part.text} />{:else}<ChatVisual show={part.show} focus={part.focus} />{/if}
      {/each}
      {#if live?.edit}
        <div class="forming" data-edit-forming>
          <div class="forming-head"><PencilLine size={12} strokeWidth={1.75} />{live.edit?.startsWith('<revise') ? 'Revising the edit' : 'Writing the edit'} · {live.verbs.length} command{live.verbs.length === 1 ? '' : 's'}</div>
          {#if live.verbs.length}
            <ul class="verbs">
              {#if live.verbs.length > SHOWN_VERBS}<li class="earlier">{live.verbs.length - SHOWN_VERBS} earlier</li>{/if}
              {#each live.verbs.slice(-SHOWN_VERBS) as verb, index (live.verbs.length - SHOWN_VERBS + index)}<li>{verb}</li>{/each}
            </ul>
          {/if}
          <Collapsible.Root>
            <Collapsible.Trigger class="edit-trigger"><ChevronDown size={12} strokeWidth={1.75} />Markup so far</Collapsible.Trigger>
            <Collapsible.Content><pre class="edit">{live.edit}</pre></Collapsible.Content>
          </Collapsible.Root>
        </div>
      {/if}
    {:else if turn.status === 'stopped'}
      <p class="summary">{turn.result?.trace?.some(step => step.kind === 'edit' && step.ok) ? 'Stopped. Earlier edits in this turn were applied.' : 'Stopped. Nothing was changed.'}</p>
      <div class="meta">{#if !appliedEdits(turn).length}<Button small variant="outline" onclick={retry}>Restore prompt</Button>{/if}<IconButton icon={logCopied ? Check : ClipboardList} label={logCopied ? 'Turn log copied' : 'Copy turn log (for a bug report)'} onclick={copyLog} /></div>
    {:else}
      {#each parts as part, index (index)}
        {#if part.kind === 'markdown'}<ChatMarkdown text={part.text} />{:else}<ChatVisual show={part.show} focus={part.focus} />{/if}
      {/each}
      {#if !parts.length && turn.result?.ok}<p class="summary">{turn.result.outcome ? 'Document updated.' : 'Done.'}</p>{/if}
      {#if turn.error}<p class="error" role="alert">{turn.error}</p>{#if !appliedEdits(turn).length}<Button small variant="outline" onclick={retry}>Restore prompt</Button>{/if}{/if}
      {@render review?.(turn)}
      {#if turn.result?.edit}
        <Collapsible.Root>
          <Collapsible.Trigger class="edit-trigger"><ChevronDown size={12} strokeWidth={1.75} />{turn.result.outcome?.ok ? 'Applied document edit' : 'Document edit details'} · {turn.result.attempts} attempt{turn.result.attempts === 1 ? '' : 's'}</Collapsible.Trigger>
          <Collapsible.Content><pre class="edit">{turn.result.edit}{turn.result.outcome?.failure ? `\n${turn.result.outcome.failure}` : ''}</pre></Collapsible.Content>
        </Collapsible.Root>
      {/if}
      <div class="meta"><span>{turn.result?.transport ?? 'Runtime error'}{turn.completedAt ? ` · ${Math.max(0.1, (turn.completedAt - turn.startedAt) / 1000).toFixed(1)}s` : ''}{turn.result?.merged ? ` · merged with ${turn.result.merged} change${turn.result.merged === 1 ? '' : 's'} made while it worked` : ''}</span><span class="actions"><IconButton icon={logCopied ? Check : ClipboardList} label={logCopied ? 'Turn log copied' : 'Copy turn log (for a bug report)'} onclick={copyLog} /><IconButton icon={copied ? Check : Copy} label={copied ? 'Message copied' : 'Copy assistant message'} onclick={copy} /></span></div>
      {#if copyError}<p class="error" role="status">{copyError}</p>{/if}
    {/if}
  </div>
</article>

<style>
  .attached-image { max-width: 100%; max-height: 240px; object-fit: contain; border-radius: var(--radius); margin-top: 8px; }
  .attached-image.preview { max-height: 96px; }
  .chat-turn { width: min(100%, 736px); margin: 0 auto; min-width: 0; }
  /* The user's turn: a bubble on the right, as T3 Code's; the reply below it
     is plain text across the column. */
  .user-row { display: flex; flex-direction: column; align-items: flex-end; gap: 4px; margin: 0 0 16px; }
  .user-message { max-width: min(80%, 560px); padding: 9px 14px; background: var(--bg-sunken); border-radius: var(--radius-xl); }
  .user-message p { white-space: pre-wrap; overflow-wrap: anywhere; font-size: var(--text-lg); line-height: 1.55; margin: 0; }
  .target { padding: 0 6px; color: var(--fg-faint); font-size: var(--text-sm); }
  .assistant-message { min-width: 0; padding: 0 4px; }
  .working, .summary { font-size: var(--text-lg); margin: 4px 0 10px; color: var(--fg-muted); }
  .working { display: flex; align-items: center; gap: 8px; }
  .pulse { width: 6px; height: 6px; border-radius: 50%; background: var(--accent); }
  .error { color: var(--danger); font-size: var(--text-md); overflow-wrap: anywhere; white-space: pre-wrap; }
  :global(.edit-trigger) { display: flex; align-items: center; gap: 5px; background: transparent; border: none; border-radius: var(--radius); font: inherit; font-size: var(--text-xs); color: var(--fg-muted); cursor: pointer; padding: 5px 0; margin-top: 8px; }
  :global(.edit-trigger:hover) { color: var(--fg); }
  .forming { margin: 6px 0 4px; padding: 8px 10px; border: 1px solid var(--border); border-radius: var(--radius); background: var(--bg-sunken); }
  .forming-head { display: flex; align-items: center; gap: 6px; font-size: var(--text-sm); color: var(--fg); }
  .verbs { list-style: none; margin: 6px 0 0; padding: 0 0 0 18px; display: flex; flex-direction: column; gap: 2px; font-size: var(--text-sm); color: var(--fg-muted); }
  .verbs li { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .verbs .earlier { color: var(--fg-faint); }
  .edit { font-size: var(--text-xs); background: var(--bg-sunken); padding: 10px; border-radius: var(--radius); max-height: 220px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; }
  .actions { display: flex; align-items: center; gap: 2px; }
  .meta { display: flex; align-items: center; justify-content: space-between; gap: 6px; margin-top: 6px; color: var(--fg-faint); font-size: var(--text-sm); }
  /* Actions show on hover or focus where there is a pointer to hover with. */
  @media (hover: hover) {
    .meta { opacity: 0.55; transition: opacity 150ms ease; }
    .chat-turn:hover .meta, .meta:focus-within { opacity: 1; }
  }
</style>
