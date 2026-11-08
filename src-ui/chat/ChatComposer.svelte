<script lang="ts">
  // The chat's composer, after T3 Code's: one rounded surface whose text
  // field grows with what is typed (to a cap, then scrolls), and a footer of
  // quiet controls: runtime, model and effort, settings, and a round Send that
  // becomes Stop while this conversation's turn runs.
  import ArrowUp from '@lucide/svelte/icons/arrow-up';
  import Square from '@lucide/svelte/icons/square';
  import TextInput from '../lib/ui/TextInput.svelte';
  import Select from '../lib/ui/Select.svelte';
  import AgentSettings from '../lib/AgentSettings.svelte';
  import ModelEffortPicker from './ModelEffortPicker.svelte';
  import { controllerManager } from '../controller-manager.svelte';
  import { shouldSubmit } from './conversation';
  import type { AgentProvider } from '../model/runtime-options';
  import type { Snippet } from 'svelte';

  let { value = $bindable(''), busy = false, compact = false, target, targetControl, submit, stop }: { value?: string; busy?: boolean; compact?: boolean; target: string; targetControl?: Snippet; submit: () => void; stop?: () => void } = $props();
  const doc = controllerManager.docClient;
  const providers: { value: AgentProvider; label: string; hint?: string }[] = [
    { value: 'codex', label: 'Codex', hint: 'ChatGPT login' },
    { value: 'claude', label: 'Claude', hint: 'Claude login' },
    { value: 'anthropic', label: 'Claude API', hint: 'API key' },
  ];
  const blocked = $derived(busy || doc.asking || !doc.canAsk || !value.trim());

  // Grow with the text: measure at `auto`, then fix the height; the CSS
  // max-height caps it and the field scrolls beyond that.
  let field = $state<HTMLTextAreaElement>();
  $effect(() => {
    void value;
    if (!field) return;
    field.style.height = 'auto';
    field.style.height = `${field.scrollHeight}px`;
  });
</script>

<form class="composer" class:compact aria-label="Chat composer" onsubmit={(event) => { event.preventDefault(); if (!blocked) submit(); }}>
  <div class="surface">
    <div class="target">{#if targetControl}{@render targetControl()}{:else}<span>{target}</span>{/if}</div>
    <TextInput multiline bind:element={field} aria-label="Agent instruction" rows={compact ? 1 : 2} placeholder="Ask, explain, or change something…" bind:value onkeydown={(event: KeyboardEvent) => { if (shouldSubmit(event)) { event.preventDefault(); if (!blocked) submit(); } }} />
    <div class="footer">
      <Select value={doc.agentProvider} items={providers} label="Agent runtime" disabled={busy || doc.asking} onChange={(provider) => doc.configureAgent({ provider })} />
      <ModelEffortPicker disabled={busy || doc.asking} />
      <AgentSettings />
      <span class="spacer"></span>
      {#if busy && stop}
        <button type="button" class="round stop" onclick={stop} aria-label="Stop the agent" title="Stop the agent"><Square size={11} strokeWidth={0} fill="currentColor" /></button>
      {:else}
        <button type="submit" class="round send" disabled={blocked} aria-label="Send message" title="Send · Cmd/Ctrl+Enter"><ArrowUp size={16} strokeWidth={2.25} /></button>
      {/if}
    </div>
  </div>
  {#if !doc.canAsk}<p class="hint">Open Agent settings to connect your runtime.</p>{/if}
</form>

<style>
  .composer { flex: none; padding: 6px 16px 14px; background: var(--bg); }
  .surface {
    max-width: 736px; margin: 0 auto; border: 1px solid var(--border-strong); border-radius: 16px;
    background: var(--bg-input); box-shadow: 0 1px 2px rgba(0, 0, 0, 0.05); overflow: hidden;
    transition: border-color 150ms ease, box-shadow 150ms ease;
  }
  .surface:focus-within { border-color: var(--fg-faint); box-shadow: 0 1px 3px rgba(0, 0, 0, 0.08); }
  .target { padding: 9px 16px 0; font-size: 11px; color: var(--fg-faint); }
  .target:empty { display: none; }
  .surface :global(textarea.ui-input) {
    display: block; width: 100%; box-sizing: border-box; border: 0; border-radius: 0; box-shadow: none; outline: none;
    background: transparent; resize: none; overflow-y: auto;
    min-height: 0; max-height: 208px; padding: 8px 16px 6px;
    font-size: 14px; line-height: 1.6; color: var(--fg);
  }
  .surface :global(textarea.ui-input::placeholder) { color: var(--fg-faint); }
  .footer { display: flex; gap: 2px; align-items: center; padding: 2px 8px 8px; min-width: 0; }
  .spacer { flex: 1; }
  /* Quiet "ghost" controls, as T3 Code's composer row. */
  .footer :global(.ui-select-trigger), .footer :global(.composer-control), .footer :global(.property-icon) {
    display: inline-flex; align-items: center; gap: 6px; height: 28px; min-width: 0; padding: 0 8px; box-sizing: border-box;
    border: 1px solid transparent; border-radius: 8px; background: transparent; color: var(--fg-muted);
    font: inherit; font-size: 12px; cursor: pointer; transition: background 150ms ease, color 150ms ease;
  }
  .footer :global(.composer-control) { max-width: 240px; }
  .footer :global(.property-icon) { width: 28px; padding: 0; justify-content: center; }
  .footer :global(.ui-select-trigger:hover), .footer :global(.composer-control:hover), .footer :global(.property-icon:hover),
  .footer :global([data-state='open']) { background: var(--hover); color: var(--fg); border-color: transparent; }
  .footer :global(.ui-select-trigger[data-disabled]), .footer :global(.composer-control:disabled) { opacity: 0.55; cursor: default; background: transparent; }
  .footer :global(.ui-select-trigger svg), .footer :global(.composer-control svg) { flex: none; color: var(--fg-faint); }
  .footer :global(.ui-select-trigger:focus-visible), .footer :global(.composer-control:focus-visible), .footer :global(.property-icon:focus-visible) { outline: 2px solid var(--accent); outline-offset: 1px; }
  .round {
    flex: none; display: inline-flex; align-items: center; justify-content: center; width: 30px; height: 30px; padding: 0;
    border: 0; border-radius: 50%; cursor: pointer; transition: background 150ms ease, transform 150ms ease, opacity 150ms ease;
  }
  .round:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .round:active:not(:disabled) { transform: scale(0.95); }
  .send { background: var(--accent); color: var(--accent-fg); box-shadow: 0 1px 2px rgba(0, 0, 0, 0.15); }
  .send:hover:not(:disabled) { filter: brightness(1.08); }
  .send:disabled { background: var(--active); color: var(--fg-faint); box-shadow: none; cursor: default; }
  .stop { background: var(--fg); color: var(--bg); }
  .stop:hover { opacity: 0.85; }
  .hint { text-align: center; margin: 6px 0 0; color: var(--fg-faint); font-size: 11px; }
  .compact { padding: 4px 10px 8px; }
  .compact .surface :global(textarea.ui-input) { max-height: 120px; font-size: 13px; }
  @media (prefers-reduced-motion: reduce) { .surface, .round, .footer :global(*) { transition: none; } }
</style>
