<script lang="ts">
  import ArrowUp from '@lucide/svelte/icons/arrow-up';
  import LoaderCircle from '@lucide/svelte/icons/loader-circle';
  import Button from '../lib/ui/Button.svelte';
  import TextInput from '../lib/ui/TextInput.svelte';
  import Select from '../lib/ui/Select.svelte';
  import AgentSettings from '../lib/AgentSettings.svelte';
  import { controllerManager } from '../controller-manager.svelte';
  import { shouldSubmit } from './conversation';
  import type { AgentProvider } from '../model/runtime-options';
  import type { Snippet } from 'svelte';

  let { value = $bindable(''), busy = false, compact = false, target, targetControl, submit }: { value?: string; busy?: boolean; compact?: boolean; target: string; targetControl?: Snippet; submit: () => void } = $props();
  const doc = controllerManager.docClient;
  const providers: { value: AgentProvider; label: string; hint?: string }[] = [
    { value: 'codex', label: 'Codex', hint: 'ChatGPT login' },
    { value: 'claude', label: 'Claude', hint: 'Claude login' },
    { value: 'anthropic', label: 'Claude API', hint: 'API key' },
  ];
  const blocked = $derived(busy || doc.asking || !doc.canAsk || !value.trim());
  // The model picker: what the runtime offers (its default first, for the
  // local runtimes), plus a model typed in Agent settings that it does not list.
  const DEFAULT = 'default';
  const current = $derived(doc.agentProvider === 'anthropic' ? doc.modelName : doc.agentModel);
  const listed = $derived(doc.agentModelsFor === doc.agentProvider ? doc.agentModels : []);
  const modelItems = $derived.by(() => {
    const items: { value: string; label: string; hint?: string }[] = [];
    if (doc.agentProvider !== 'anthropic') {
      const own = listed.find((m) => m.default);
      items.push({ value: DEFAULT, label: 'Default', hint: own?.label });
    }
    for (const m of listed) items.push({ value: m.id, label: m.label, hint: m.default ? 'default' : undefined });
    if (current && !items.some((i) => i.value === current)) items.push({ value: current, label: current, hint: 'custom' });
    return items;
  });
  const modelValue = $derived(current || DEFAULT);
</script>

<form class="composer" class:compact aria-label="Chat composer" onsubmit={(event) => { event.preventDefault(); if (!blocked) submit(); }}>
  <div class="surface">
    <div class="target">{#if targetControl}{@render targetControl()}{:else}<span>{target}</span>{/if}</div>
    <TextInput multiline aria-label="Agent instruction" rows={compact ? 2 : 3} placeholder="Ask, explain, or change something…" bind:value onkeydown={(event: KeyboardEvent) => { if (shouldSubmit(event)) { event.preventDefault(); if (!blocked) submit(); } }} />
    <div class="footer">
      <Select value={doc.agentProvider} items={providers} label="Agent runtime" disabled={busy || doc.asking} onChange={(provider) => doc.configureAgent({ provider })} />
      {#if modelItems.length > 1 || doc.agentProvider === 'anthropic'}
        <Select value={modelValue} items={modelItems} label="Model" disabled={busy || doc.asking} onChange={(v) => doc.setAgentModel(v === DEFAULT ? '' : v)} />
      {:else}
        <span class="model" title="The runtime's default model">{current || 'Runtime default'}</span>
      {/if}
      <AgentSettings />
      <Button variant="primary" small type="submit" disabled={blocked} aria-label={busy ? 'Agent working' : 'Send message'} title="Send · Cmd/Ctrl+Enter">{#if busy}<LoaderCircle size={14} strokeWidth={1.75} />Working…{:else}<ArrowUp size={14} strokeWidth={1.75} />Send{/if}</Button>
    </div>
  </div>
  <p class="hint">{#if !doc.canAsk}Open Agent settings to connect your runtime.{:else}Cmd/Ctrl+Enter to send · Enter for a new line{/if}</p>
</form>

<style>
  .composer { flex: none; padding: 8px 12px 7px; background: var(--bg); }
  .surface { max-width: 720px; margin: 0 auto; border: 1px solid var(--border-strong); border-radius: 12px; background: var(--bg-input); overflow: hidden; }
  .surface:focus-within { border-color: var(--accent); }
  .target { min-height: 20px; padding: 7px 10px 0; font-size: 10px; color: var(--fg-muted); }
  .surface :global(textarea.ui-input) { border: 0; border-radius: 0; box-shadow: none; outline: none; background: transparent; resize: vertical; min-height: 48px; max-height: 220px; width: 100%; box-sizing: border-box; padding: 8px 12px; font-size: 13px; line-height: 1.5; }
  .footer { display: flex; gap: 7px; align-items: center; padding: 4px 8px 8px; }
  .footer :global(.ui-select-trigger) { width: auto; min-width: 78px; font-size: 11px; height: 25px; background: var(--bg-sunken); }
  .model { min-width: 0; flex: 1; color: var(--fg-muted); font-size: 10px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .hint { text-align: center; margin: 5px 0 0; color: var(--fg-faint); font-size: 9px; }
  .compact { padding-top: 5px; }
  .compact .hint { display: none; }
</style>
