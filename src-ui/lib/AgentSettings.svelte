<script lang="ts">
  import { Popover } from 'bits-ui';
  import Settings2 from '@lucide/svelte/icons/settings-2';
  import { controllerManager } from '../controller-manager.svelte';
  import TextInput from './ui/TextInput.svelte';
  import Button from './ui/Button.svelte';

  const doc = controllerManager.docClient;
</script>

<Popover.Root onOpenChange={(open) => { if (open) void doc.connectLocalAgent(); }}>
  <Popover.Trigger class="property-icon" aria-label="Agent settings" title="Agent settings"><Settings2 size={14} strokeWidth={1.75} /></Popover.Trigger>
  <Popover.Portal>
    <Popover.Content class="agent-settings ui-menu" side="top" align="end" sideOffset={8}>
      <div class="ui-menu-heading">Agent connection · {doc.agentProvider}</div>
      {#if doc.agentProvider === 'anthropic'}
        <label>Anthropic API key<TextInput type="password" aria-label="Anthropic API key" value={doc.apiKey} onchange={(event: Event) => doc.setApiKey((event.currentTarget as HTMLInputElement).value)} /></label>
        <label>API model<TextInput aria-label="API model" bind:value={doc.modelName} /></label>
      {:else}
        <label>Local endpoint<TextInput aria-label="Local agent endpoint" bind:value={doc.agentEndpoint} /></label>
        <label>Bridge session token<TextInput type="password" aria-label="Bridge session token" placeholder="Paste the local bridge token" bind:value={doc.agentToken} /></label>
        <label>Model<TextInput aria-label="Runtime model" placeholder="Runtime default" bind:value={doc.agentModel} /></label>
        <p role="status">{#if doc.agentPairing}Connecting to the local bridge…{:else if doc.agentAutoPaired}Connected automatically on this computer. Refreshing the page reconnects.{:else}Start <code>bun run agent:serve</code>, then connect automatically or enter its token manually.{/if}</p>
        <Button small variant="outline" disabled={doc.agentPairing || doc.asking} onclick={() => doc.connectLocalAgent()}>Connect automatically</Button>
        <p>The token stays in page memory, not the URL or browser storage. It is not an OpenAI API key.</p>
        {#if doc.agentProvider === 'claude'}<p>Claude requires an API key in the bridge environment.</p>{/if}
      {/if}
    </Popover.Content>
  </Popover.Portal>
</Popover.Root>

<style>
  :global(.agent-settings) { display: flex; flex-direction: column; gap: 9px; padding: 12px; width: min(300px, calc(100vw - 40px)); z-index: 1000; }
  :global(.agent-settings label) { display: flex; flex-direction: column; gap: 4px; font-size: 11px; color: var(--fg-muted); }
  :global(.agent-settings p) { margin: 0; font-size: 10px; line-height: 1.5; color: var(--fg-muted); }
</style>
