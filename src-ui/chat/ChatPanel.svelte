<script lang="ts">
  import ChatTranscript from './ChatTranscript.svelte';
  import ChatComposer from './ChatComposer.svelte';
  import ChatThreads from './ChatThreads.svelte';
  import type { ChatHistory } from './sessions.svelte';
  import type { ChatThread } from './history';
  import { applyAgentEvents, appliedEdits, chatHistory, failureSummary, finishProgress, setDraft, type ChatSession, type ChatTurn, type SendTurn } from './conversation';
  import { STOPPED, type AgentEvent } from '../model/progress';
  import { controllerManager } from '../controller-manager.svelte';
  import type { Snippet } from 'svelte';

  let { session, history, send, target = 'Document', compact = false, review, targetControl }: { session: ChatSession; history?: ChatHistory; send: SendTurn; target?: string; compact?: boolean; review?: Snippet<[ChatTurn]>; targetControl?: Snippet } = $props();
  const doc = controllerManager.docClient;
  const busy = $derived(session.turns.some(turn => turn.status === 'running'));
  let scrollRequest = $state(0);
  let composer: { attach(files: File[]): Promise<void> } | undefined = $state();

  // Each running turn's Stop (plans/specs/agent-streaming.md §2).
  const controllers = new Map<string, AbortController>();

  export async function submit(text = session.draft, execute = send): Promise<ChatTurn | null> {
    text = text.trim();
    if ((!text && !session.draftImages?.length) || busy || doc.asking) return null;
    const current = session;
    const owner = history;
    const priorMessages = chatHistory(current.turns);
    const turn: ChatTurn = { id: crypto.randomUUID(), instruction: text || 'Describe this image.', images: current.draftImages, target, status: 'running', startedAt: Date.now(), progress: { steps: [], text: '' } };
    current.turns = [...current.turns, turn];
    // The state proxy of the turn just added: what the transcript renders, so
    // progress written to it shows as it arrives.
    const entry = current.turns[current.turns.length - 1];
    current.draft = '';
    current.draftImages = [];
    scrollRequest++;
    const controller = new AbortController();
    controllers.set(entry.id, controller);
    const live = { signal: controller.signal, onEvent: (event: AgentEvent) => { if (entry.progress) applyAgentEvents(entry.progress, [event], Date.now()); } };
    try {
      const result = await execute(entry.instruction, priorMessages, entry, live);
      entry.result = result;
      // Stop pressed once the edit was applying: it applied, so the turn completed.
      if (result.error === STOPPED) entry.status = 'stopped';
      else {
        entry.status = result.ok ? 'completed' : 'failed';
        const applied = appliedEdits({ result }).length > 0;
        entry.error = result.error
          ? (applied && !result.ok ? `Earlier edits in this turn remain applied. ${result.error}` : result.error)
          : (!result.ok ? (result.outcome?.failure ? failureSummary(result.outcome.failure, result.attempts, applied) : 'The request could not be completed.') : '');
      }
    } catch (error) {
      if (controller.signal.aborted) entry.status = 'stopped';
      else { entry.status = 'failed'; entry.error = String(error); }
    } finally {
      controllers.delete(entry.id);
    }
    entry.completedAt = Date.now();
    finishProgress(entry.progress, entry.completedAt);
    // Put an unfinished request back to send again, unless it already changed
    // the document: sending it again would repeat those edits.
    if (entry.status !== 'completed' && !appliedEdits(entry).length && !current.draft && !current.draftImages?.length) { current.draft = text; current.draftImages = restorable(entry.images); }
    // Saved with the scope, and to the top of the panel's chats.
    if (owner && 'updated' in current) owner.touch(current as ChatThread);
    return $state.snapshot(entry) as ChatTurn;
  }

  /** Stop this conversation's running turn (the composer's Stop). */
  /** A turn's images back in the composer: a saved preview is not the image, so it stays out. */
  function restorable(images: ChatTurn['images']) { return (images ?? []).filter(image => !image.preview); }

  function stop() { for (const turn of session.turns) if (turn.status === 'running') controllers.get(turn.id)?.abort(); }
</script>

<section class="chat-panel" class:compact data-chat-panel aria-label="Agent conversation"
  ondragover={(event) => { if (event.dataTransfer?.types.includes('Files')) event.preventDefault(); }}
  ondrop={(event) => { if (event.dataTransfer?.files.length) { event.preventDefault(); event.stopPropagation(); void composer?.attach(Array.from(event.dataTransfer.files)); } }}>
  {#if history}<ChatThreads {history} {busy} />{/if}
  {#if !compact || session.turns.length}<ChatTranscript turns={session.turns} {compact} {scrollRequest} {review} retry={(turn) => { if (!session.draft && !session.draftImages?.length) { session.draft = turn.instruction; session.draftImages = restorable(turn.images); } }} />{/if}
  <!-- The draft belongs to the conversation (chat/sessions.svelte.ts), not
       to this panel: bound through its getter and setter. -->
  <ChatComposer bind:this={composer} bind:images={() => session.draftImages ?? [], (images) => { session.draftImages = images; }} bind:value={() => session.draft, (draft) => setDraft(session, draft)} {busy} {compact} {target} {targetControl} submit={() => { void submit(); }} {stop} />
</section>

<style>
  .chat-panel { display: flex; flex-direction: column; flex: 1; min-width: 0; min-height: 0; height: 100%; background: var(--bg); }
  .compact { flex: none; height: auto; max-height: 280px; }
</style>
