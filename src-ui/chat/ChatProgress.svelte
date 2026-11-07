<script lang="ts">
  // A turn's work log (plans/specs/agent-streaming.md §3): the step it is on
  // with elapsed time and Stop while it runs, each step with the reasoning the
  // runtime shared, collapsed to "Worked for …" once it ends. Every row is
  // backed by an event from the runtime; nothing here is decorative.
  import { Collapsible } from 'bits-ui';
  import ChevronRight from '@lucide/svelte/icons/chevron-right';
  import FileSearch from '@lucide/svelte/icons/file-search';
  import Brain from '@lucide/svelte/icons/brain';
  import PencilLine from '@lucide/svelte/icons/pencil-line';
  import Eye from '@lucide/svelte/icons/eye';
  import WandSparkles from '@lucide/svelte/icons/wand-sparkles';
  import Square from '@lucide/svelte/icons/square';
  import Button from '../lib/ui/Button.svelte';
  import { formatTokens, stepLabel, stepTokens, turnTokens, type TurnProgress, type WorkStep } from './conversation';

  let { progress, running, startedAt, completedAt, stop }: {
    progress: TurnProgress; running: boolean; startedAt: number; completedAt?: number; stop?: () => void;
  } = $props();

  let now = $state(Date.now());
  $effect(() => {
    if (!running) return;
    const timer = setInterval(() => { now = Date.now(); }, 1000);
    return () => clearInterval(timer);
  });
  // Open while the turn runs; folded away when it ends (the reader may reopen it).
  let open = $state(true);
  $effect(() => { open = running; });

  const current = $derived(progress.steps[progress.steps.length - 1]);
  const seconds = (from: number, to: number) => {
    const s = Math.max(0, (to - from) / 1000);
    return s < 10 ? `${s.toFixed(1)}s` : s < 60 ? `${Math.round(s)}s` : `${Math.floor(s / 60)}m ${Math.round(s % 60)}s`;
  };
  const headline = $derived(running
    ? (current ? stepLabel(current, false) : 'Starting')
    : `Worked for ${seconds(startedAt, completedAt ?? now)}`);
  // Tokens: "~" while estimated; exact once every model call has reported.
  const count = $derived(turnTokens(progress, now, running));
  const tokenText = $derived(count.tokens
    ? `${count.exact ? '' : '~'}${formatTokens(count.tokens)} tokens${!running && count.thinking ? ` (${formatTokens(count.thinking)} thinking)` : ''}${count.perSecond ? ` · ${Math.round(count.perSecond)} tok/s` : ''}`
    : '');
  const stepTokenText = (step: WorkStep) => {
    if (step.phase !== 'thinking') return '';
    const counted = stepTokens(step);
    return counted.tokens ? `${counted.exact ? '' : '~'}${formatTokens(counted.tokens)} tok · ` : '';
  };
  const iconFor = (step: WorkStep) => step.phase === 'reading' ? FileSearch : step.phase === 'looking' ? Eye : step.phase === 'applying' ? WandSparkles : step.wrote ? PencilLine : Brain;
</script>

<div class="work" class:running data-chat-progress data-phase={running ? current?.phase : 'done'}>
  <Collapsible.Root bind:open>
    <div class="head">
      <Collapsible.Trigger class="work-trigger" aria-label={open ? 'Hide work log' : 'Show work log'}>
        <span class="chevron" class:open><ChevronRight size={12} strokeWidth={1.75} /></span>
        {#if running}<span class="pulse" aria-hidden="true"></span>{/if}
        <span class="headline" role={running ? 'status' : undefined}>{headline}</span>
        {#if running}<span class="elapsed">{seconds(startedAt, now)}{tokenText ? ` · ${tokenText}` : ''}</span>
        {:else if tokenText}<span class="elapsed">{tokenText}</span>{/if}
      </Collapsible.Trigger>
      {#if running && stop}
        <Button small variant="outline" class="stop" onclick={stop} aria-label="Stop the agent"><Square size={10} strokeWidth={2} />Stop</Button>
      {/if}
    </div>
    <Collapsible.Content>
      <ol class="steps">
        {#each progress.steps as step, index (index)}
          {@const done = !running || index < progress.steps.length - 1}
          {@const Icon = iconFor(step)}
          <li class:current={!done}>
            <div class="row">
              <Icon size={12} strokeWidth={1.75} />
              <span class="label">{stepLabel(step, done)}</span>
              <span class="time">{stepTokenText(step)}{seconds(step.startedAt, step.endedAt ?? now)}</span>
            </div>
            {#if step.reasoning.trim()}
              <div class="reasoning"><p>{step.reasoning.trim()}</p></div>
            {/if}
          </li>
        {/each}
      </ol>
    </Collapsible.Content>
  </Collapsible.Root>
</div>

<style>
  .work { margin: 0 0 10px; }
  .head { display: flex; align-items: center; justify-content: space-between; gap: 8px; min-height: 24px; }
  :global(.work-trigger) { display: flex; align-items: center; gap: 6px; min-width: 0; background: transparent; border: none; border-radius: 4px; padding: 3px 0; font: inherit; font-size: 12px; color: var(--fg-muted); cursor: pointer; }
  :global(.work-trigger:hover) { color: var(--fg); }
  :global(.work-trigger:focus-visible) { outline: 2px solid var(--accent); outline-offset: 2px; }
  .chevron { display: inline-flex; transition: transform 120ms ease; }
  .chevron.open { transform: rotate(90deg); }
  .running .headline { color: var(--fg); }
  .headline { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .elapsed { color: var(--fg-faint); font-size: 11px; font-variant-numeric: tabular-nums; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-width: 0; }
  .pulse { width: 6px; height: 6px; flex: none; border-radius: 50%; background: var(--accent); animation: pulse 1.2s ease-in-out infinite; }
  @keyframes pulse { 50% { opacity: 0.35; } }
  @media (prefers-reduced-motion: reduce) { .pulse { animation: none; } .chevron { transition: none; } }
  :global(.work .stop) { gap: 5px; }
  .steps { list-style: none; margin: 4px 0 0 5px; padding: 0 0 0 12px; border-left: 1px solid var(--border); display: flex; flex-direction: column; gap: 6px; }
  .row { display: flex; align-items: center; gap: 7px; font-size: 11px; color: var(--fg-muted); }
  .current .row { color: var(--fg); }
  .label { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .time { color: var(--fg-faint); font-variant-numeric: tabular-nums; }
  /* Newest reasoning stays in view: the column runs bottom-up. */
  .reasoning { display: flex; flex-direction: column-reverse; max-height: 132px; overflow-y: auto; margin: 4px 0 0 19px; }
  .reasoning p { margin: 0; font-size: 11px; line-height: 1.5; color: var(--fg-muted); font-style: italic; white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
