<script lang="ts">
  import { controllerManager } from '../../controller-manager.svelte';
  import type { ViewSpec } from '../views';
  import ChatPanel from '../../chat/ChatPanel.svelte';
  import { chatSession } from '../../chat/sessions.svelte';

  let { spec }: { spec: ViewSpec } = $props();
  const doc = controllerManager.docClient;
  const session = $derived(chatSession(`${doc.generation}:${doc.scope.current?.id ?? 'demo'}:model:${spec.source}`));
</script>

<section class="html-view model-view">
  <ChatPanel {session} target="Document and compositions" send={(text, history) => doc.ask(text, { history, allowTextReply: true })} />
</section>

<style>
  .model-view { padding: 0 !important; gap: 0 !important; }
</style>
