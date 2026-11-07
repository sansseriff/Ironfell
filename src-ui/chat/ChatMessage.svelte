<script lang="ts">
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
  import { contentParts, streamView, turnLog, type ChatTurn } from './conversation';
  import type { Snippet } from 'svelte';

  let { turn, retry, stop, review }: { turn: ChatTurn; retry: () => void; stop?: () => void; review?: Snippet<[ChatTurn]> } = $props();
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
  <div class="user-message"><span class="role">You</span><p>{turn.instruction}</p></div>
  <div class="assistant-message">
    <div class="heading"><span class="role">{turn.result?.transport.split(':')[0] || 'Assistant'}</span><span class="target">{turn.target}</span></div>
    {#if turn.progress && (turn.status === 'running' || turn.progress.steps.length)}
      <ChatProgress progress={turn.progress} running={turn.status === 'running'} startedAt={turn.startedAt} completedAt={turn.completedAt} {stop} />
    {/if}
    {#if turn.status === 'running'}
      {#if !turn.progress}<p class="working" role="status"><span class="pulse"></span>Working…</p>{/if}
      {#each liveParts as part, index (index)}
        {#if part.kind === 'markdown'}<ChatMarkdown text={part.text} />{:else}<ChatVisual show={part.show} focus={part.focus} />{/if}
      {/each}
      {#if live?.edit}
        <div class="forming" data-edit-forming>
          <div class="forming-head"><PencilLine size={12} strokeWidth={1.75} />Writing the edit · {live.verbs.length} command{live.verbs.length === 1 ? '' : 's'}</div>
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
      <p class="summary">Stopped. Nothing was changed.</p>
      <div class="meta"><Button small variant="outline" onclick={retry}>Restore prompt</Button><IconButton icon={logCopied ? Check : ClipboardList} label={logCopied ? 'Turn log copied' : 'Copy turn log (for a bug report)'} onclick={copyLog} /></div>
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
      <div class="meta"><span>{turn.result?.transport ?? 'Runtime error'}{turn.completedAt ? ` · ${Math.max(0.1, (turn.completedAt - turn.startedAt) / 1000).toFixed(1)}s` : ''}</span><span class="actions"><IconButton icon={logCopied ? Check : ClipboardList} label={logCopied ? 'Turn log copied' : 'Copy turn log (for a bug report)'} onclick={copyLog} /><IconButton icon={copied ? Check : Copy} label={copied ? 'Message copied' : 'Copy assistant message'} onclick={copy} /></span></div>
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
  .forming { margin: 6px 0 4px; padding: 8px 10px; border: 1px solid var(--border); border-radius: 6px; background: var(--bg-sunken); }
  .forming-head { display: flex; align-items: center; gap: 6px; font-size: 11px; color: var(--fg); }
  .verbs { list-style: none; margin: 6px 0 0; padding: 0 0 0 18px; display: flex; flex-direction: column; gap: 2px; font-size: 11px; color: var(--fg-muted); }
  .verbs li { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .verbs .earlier { color: var(--fg-faint); }
  .edit { font-size: 10px; background: var(--bg-sunken); padding: 10px; border-radius: 6px; max-height: 220px; overflow: auto; white-space: pre-wrap; overflow-wrap: anywhere; }
  .actions { display: flex; align-items: center; gap: 2px; }
  .meta { display: flex; align-items: center; justify-content: space-between; gap: 6px; margin-top: 8px; color: var(--fg-faint); font-size: 10px; }
</style>
