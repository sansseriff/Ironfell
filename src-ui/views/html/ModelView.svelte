<script lang="ts">
  import { controllerManager } from '../../controller-manager.svelte';
  import type { ViewSpec } from '../views';
  import ChatPanel from '../../chat/ChatPanel.svelte';
  import { panelHistory } from '../../chat/sessions.svelte';

  let { spec }: { spec: ViewSpec } = $props();
  const doc = controllerManager.docClient;
  // This panel's chats, saved with the scope (chat/sessions.svelte.ts).
  const history = $derived(panelHistory(`model:${spec.source}`));
  const session = $derived(history.current);
</script>

<section class="html-view model-view">
  <ChatPanel {session} {history} target="Document and compositions" send={(text, history, turn, live) => doc.ask(text, { history, allowTextReply: true, turnId: turn.id, images: turn.images }, live)} />
</section>

<style>
  .model-view { padding: 0 !important; gap: 0 !important; }
</style>
