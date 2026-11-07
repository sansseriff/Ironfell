<script lang="ts">
  import ChatTranscript from './ChatTranscript.svelte';
  import ChatComposer from './ChatComposer.svelte';
  import { chatHistory, failureSummary, type ChatSession, type ChatTurn, type SendTurn } from './conversation';
  import { controllerManager } from '../controller-manager.svelte';
  import type { Snippet } from 'svelte';

  let { session, send, target = 'Document', compact = false, review, targetControl }: { session: ChatSession; send: SendTurn; target?: string; compact?: boolean; review?: Snippet<[ChatTurn]>; targetControl?: Snippet } = $props();
  const doc = controllerManager.docClient;
  const busy = $derived(session.turns.some(turn => turn.status === 'running'));
  let scrollRequest = $state(0);

  export async function submit(text = session.draft, execute = send): Promise<ChatTurn | null> {
    text = text.trim();
    if (!text || busy || doc.asking) return null;
    const current = session;
    const history = chatHistory(current.turns);
    const turn: ChatTurn = { id: crypto.randomUUID(), instruction: text, target, status: 'running', startedAt: Date.now() };
    current.turns = [...current.turns, turn];
    current.draft = '';
    scrollRequest++;
    try {
      const result = await execute(text, history, turn);
      turn.result = result;
      turn.status = result.ok ? 'completed' : 'failed';
      turn.error = result.error || (!result.ok ? (result.outcome?.failure ? failureSummary(result.outcome.failure, result.attempts) : 'The request could not be completed.') : '');
    } catch (error) { turn.status = 'failed'; turn.error = String(error); }
    turn.completedAt = Date.now();
    current.turns = current.turns.map(item => item.id === turn.id ? { ...turn } : item);
    if (turn.status === 'failed' && !current.draft) current.draft = text;
    return turn;
  }
</script>

<section class="chat-panel" class:compact data-chat-panel aria-label="Agent conversation">
  {#if !compact || session.turns.length}<ChatTranscript turns={session.turns} {compact} {scrollRequest} {review} retry={(turn) => { if (!session.draft) session.draft = turn.instruction; }} />{/if}
  <ChatComposer bind:value={session.draft} {busy} {compact} {target} {targetControl} submit={() => { void submit(); }} />
</section>

<style>
  .chat-panel { display: flex; flex-direction: column; flex: 1; min-width: 0; min-height: 0; height: 100%; background: var(--bg); }
  .compact { flex: none; height: auto; max-height: 280px; }
</style>
