<script lang="ts">
  import ChatTranscript from './ChatTranscript.svelte';
  import ChatComposer from './ChatComposer.svelte';
  import { applyAgentEvents, chatHistory, failureSummary, finishProgress, type ChatSession, type ChatTurn, type SendTurn } from './conversation';
  import { STOPPED, type AgentEvent } from '../model/progress';
  import { controllerManager } from '../controller-manager.svelte';
  import type { Snippet } from 'svelte';

  let { session, send, target = 'Document', compact = false, review, targetControl }: { session: ChatSession; send: SendTurn; target?: string; compact?: boolean; review?: Snippet<[ChatTurn]>; targetControl?: Snippet } = $props();
  const doc = controllerManager.docClient;
  const busy = $derived(session.turns.some(turn => turn.status === 'running'));
  let scrollRequest = $state(0);

  // Each running turn's Stop (plans/specs/agent-streaming.md §2).
  const controllers = new Map<string, AbortController>();

  export async function submit(text = session.draft, execute = send): Promise<ChatTurn | null> {
    text = text.trim();
    if (!text || busy || doc.asking) return null;
    const current = session;
    const history = chatHistory(current.turns);
    const turn: ChatTurn = { id: crypto.randomUUID(), instruction: text, target, status: 'running', startedAt: Date.now(), progress: { steps: [], text: '' } };
    current.turns = [...current.turns, turn];
    // The state proxy of the turn just added: what the transcript renders, so
    // progress written to it shows as it arrives.
    const entry = current.turns[current.turns.length - 1];
    current.draft = '';
    scrollRequest++;
    const controller = new AbortController();
    controllers.set(entry.id, controller);
    const live = { signal: controller.signal, onEvent: (event: AgentEvent) => { if (entry.progress) applyAgentEvents(entry.progress, [event], Date.now()); } };
    try {
      const result = await execute(text, history, entry, live);
      entry.result = result;
      // Stop pressed once the edit was applying: it applied, so the turn completed.
      if (result.error === STOPPED) entry.status = 'stopped';
      else {
        entry.status = result.ok ? 'completed' : 'failed';
        entry.error = result.error || (!result.ok ? (result.outcome?.failure ? failureSummary(result.outcome.failure, result.attempts) : 'The request could not be completed.') : '');
      }
    } catch (error) {
      if (controller.signal.aborted) entry.status = 'stopped';
      else { entry.status = 'failed'; entry.error = String(error); }
    } finally {
      controllers.delete(entry.id);
    }
    entry.completedAt = Date.now();
    finishProgress(entry.progress, entry.completedAt);
    if (entry.status !== 'completed' && !current.draft) current.draft = text;
    return $state.snapshot(entry) as ChatTurn;
  }

  function stop(turn: ChatTurn) { controllers.get(turn.id)?.abort(); }
</script>

<section class="chat-panel" class:compact data-chat-panel aria-label="Agent conversation">
  {#if !compact || session.turns.length}<ChatTranscript turns={session.turns} {compact} {scrollRequest} {review} {stop} retry={(turn) => { if (!session.draft) session.draft = turn.instruction; }} />{/if}
  <ChatComposer bind:value={session.draft} {busy} {compact} {target} {targetControl} submit={() => { void submit(); }} />
</section>

<style>
  .chat-panel { display: flex; flex-direction: column; flex: 1; min-width: 0; min-height: 0; height: 100%; background: var(--bg); }
  .compact { flex: none; height: auto; max-height: 280px; }
</style>
