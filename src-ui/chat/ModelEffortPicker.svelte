<script lang="ts">
  // The composer's model and reasoning-effort control, one menu as in T3
  // Code's composer: models on the left, the chosen model's effort levels on
  // the right, and Fast below them where the model offers it (Codex's
  // `priority` tier). The trigger reads "Opus High", with a bolt when Fast.
  // Picking a model or Fast keeps the menu open; picking an effort closes it.
  import { DropdownMenu } from 'bits-ui';
  import Check from '@lucide/svelte/icons/check';
  import ChevronDown from '@lucide/svelte/icons/chevron-down';
  import Zap from '@lucide/svelte/icons/zap';
  import { controllerManager } from '../controller-manager.svelte';

  let { disabled = false }: { disabled?: boolean } = $props();
  const doc = controllerManager.docClient;

  const DEFAULT = 'default';
  const current = $derived(doc.agentProvider === 'anthropic' ? doc.modelName : doc.agentModel);
  const listed = $derived(doc.agentModelsFor === doc.agentProvider ? doc.agentModels : []);
  // What the runtime offers (its default first, for the local runtimes),
  // plus a model typed in Agent settings that it does not list.
  const models = $derived.by(() => {
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
  const modelLabel = $derived.by(() => {
    if (current) return listed.find((m) => m.id === current)?.label ?? current;
    return listed.find((m) => m.default)?.label ?? 'Default model';
  });
  const efforts = $derived(doc.agentEfforts);
  const effort = $derived(doc.effectiveEffort);
  // Fast: the tier named so (Codex lists `priority` as "Fast").
  const fastTier = $derived(doc.agentTiers.find((t) => t.name.toLowerCase() === 'fast') ?? doc.agentTiers.find((t) => t.id === 'priority'));
  const fast = $derived(!!fastTier && doc.effectiveTier === fastTier.id);
  const title = (level: string) => level ? level[0].toUpperCase() + level.slice(1) : 'Default';
</script>

<DropdownMenu.Root>
  <DropdownMenu.Trigger class="composer-control model-effort" {disabled} aria-label="Model and effort: {modelLabel}, {effort ? `${effort} effort` : 'default effort'}{fast ? ', fast' : ''}" title="Model, reasoning effort and speed">
    {#if fast}<Zap size={12} strokeWidth={0} fill="currentColor" class="fast-mark" />{/if}
    <span class="model-name">{modelLabel}</span>
    {#if effort}<span class="effort-name">{title(effort)}</span>{/if}
    <ChevronDown size={14} strokeWidth={1.75} />
  </DropdownMenu.Trigger>
  <DropdownMenu.Portal>
    <DropdownMenu.Content class="ui-menu model-effort-menu" side="top" align="start" sideOffset={6}>
      <div class="column models" class:wide={!efforts.length}>
        <div class="ui-menu-heading">Model</div>
        <DropdownMenu.RadioGroup value={modelValue} onValueChange={(v) => doc.setAgentModel(v === DEFAULT ? '' : v)}>
          {#each models as item (item.value)}
            <DropdownMenu.RadioItem class="ui-menu-item" value={item.value} closeOnSelect={false}>
              {#snippet children({ checked })}
                <span class="check">{#if checked}<Check size={14} strokeWidth={2} />{/if}</span>
                <span class="label">{item.label}</span>
                {#if item.hint}<span class="ui-hint">{item.hint}</span>{/if}
              {/snippet}
            </DropdownMenu.RadioItem>
          {:else}
            <p class="note">Connect the runtime to list its models.</p>
          {/each}
        </DropdownMenu.RadioGroup>
      </div>
      {#if efforts.length || fastTier}
        <div class="column efforts">
          {#if efforts.length}
          <div class="ui-menu-heading">Effort</div>
          <DropdownMenu.RadioGroup value={effort || DEFAULT} onValueChange={(v) => doc.setAgentEffort(v === DEFAULT ? '' : v)}>
            {#each [DEFAULT, ...efforts] as level (level)}
              <DropdownMenu.RadioItem class="ui-menu-item" value={level}>
                {#snippet children({ checked })}
                  <span class="check">{#if checked}<Check size={14} strokeWidth={2} />{/if}</span>
                  <span class="label">{title(level === DEFAULT ? '' : level)}</span>
                {/snippet}
              </DropdownMenu.RadioItem>
            {/each}
          </DropdownMenu.RadioGroup>
          <p class="note">Default: the runtime's own setting.</p>
          {/if}
          {#if fastTier}
            <div class="ui-menu-heading speed">Speed</div>
            <DropdownMenu.CheckboxItem class="ui-menu-item" checked={fast} closeOnSelect={false} onCheckedChange={(on) => doc.setAgentTier(on ? fastTier.id : '')}>
              {#snippet children({ checked })}
                <span class="check">{#if checked}<Check size={14} strokeWidth={2} />{/if}</span>
                <span class="label">Fast</span>
                <Zap size={12} strokeWidth={0} fill="currentColor" />
              {/snippet}
            </DropdownMenu.CheckboxItem>
            {#if fastTier.description}<p class="note">{fastTier.description}</p>{/if}
          {/if}
        </div>
      {/if}
    </DropdownMenu.Content>
  </DropdownMenu.Portal>
</DropdownMenu.Root>

<style>
  .model-name { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .effort-name { flex: none; color: var(--fg-faint); }
  :global(.composer-control.model-effort svg.fast-mark) { color: var(--warning); }
  .speed { margin-top: 4px; border-top: 1px solid var(--border); padding-top: 8px; }
  :global(.model-effort:hover) .effort-name, :global(.model-effort[data-state='open']) .effort-name { color: var(--fg-muted); }
  :global(.model-effort-menu) { display: flex; padding: 0; min-width: 0; max-width: calc(100vw - 32px); overflow: hidden; }
  .column { display: flex; flex-direction: column; padding: 4px; min-width: 0; max-height: min(340px, 60vh); overflow-y: auto; }
  .models { width: 220px; }
  .models.wide { width: 260px; }
  .efforts { width: 132px; border-left: 1px solid var(--border); background: var(--bg-sunken); }
  .check { width: 14px; flex: none; display: flex; }
  .label { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .note { margin: 4px 8px 4px; font-size: 10px; line-height: 1.4; color: var(--fg-faint); }
</style>
