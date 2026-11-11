<script lang="ts">
  // The chat's composer, after T3 Code's: one rounded surface whose text
  // field grows with what is typed (to a cap, then scrolls), and a footer of
  // quiet controls: runtime, model and effort, settings, and a round Send that
  // becomes Stop while this conversation's turn runs.
  import X from '@lucide/svelte/icons/x';
  import ImagePlus from '@lucide/svelte/icons/image-plus';
  import IconButton from '../lib/ui/IconButton.svelte';
  import { readChatImage, imageUrl, MAX_IMAGES, type ModelImage } from '../model/images';
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

  let { value = $bindable(''), images = $bindable<ModelImage[]>([]), busy = false, compact = false, target, targetControl, submit, stop }: { value?: string; images?: ModelImage[]; busy?: boolean; compact?: boolean; target: string; targetControl?: Snippet; submit: () => void; stop?: () => void } = $props();
  const doc = controllerManager.docClient;
  const providers: { value: AgentProvider; label: string; hint?: string }[] = [
    { value: 'codex', label: 'Codex', hint: 'ChatGPT login' },
    { value: 'claude', label: 'Claude', hint: 'Claude login' },
    { value: 'anthropic', label: 'Claude API', hint: 'API key' },
  ];
  let reading = $state(false);
  let imageError = $state('');
  let picker: HTMLInputElement;
  export async function attach(files: File[]) {
    if (reading || busy || !files.length) return;
    const original = images;
    imageError = '';
    if (images.length + files.length > MAX_IMAGES) { imageError = `Attach at most ${MAX_IMAGES} images.`; return; }
    reading = true;
    try {
      const added = await Promise.all(files.map(readChatImage));
      // The draft moved on while the files were read (another chat opened, or
      // an image removed): say so rather than add them to the wrong draft.
      if (images === original) images = [...images, ...added];
      else imageError = 'The draft changed while the images were read; attach them again.';
    } catch (error) { imageError = error instanceof Error ? error.message : String(error); }
    finally { reading = false; }
  }
  const blocked = $derived(busy || doc.asking || !doc.canAsk || (!value.trim() && !images.length) || reading);

  // Grow with the text: measure at `auto`, then fix the height; the CSS
  // min-height keeps a full line and max-height caps it. A panel mounted
  // hidden measures 0, so measure again whenever the field's width changes
  // (shown, or the panel resized), and leave CSS in charge until it has a
  // size. The scrollbar appears only once the text passes the cap.
  let field = $state<HTMLTextAreaElement>();
  let overflowing = $state(false);
  function fit() {
    if (!field) return;
    field.style.height = 'auto';
    const natural = field.scrollHeight;
    if (!natural) { field.style.height = ''; overflowing = false; return; }
    const cap = parseFloat(getComputedStyle(field).maxHeight) || Infinity;
    field.style.height = `${Math.min(natural, cap)}px`;
    overflowing = natural > cap + 1;
  }
  $effect(() => { void value; fit(); });
  $effect(() => {
    const el = field;
    if (!el) return;
    let width = el.clientWidth;
    const observer = new ResizeObserver(() => { if (el.clientWidth !== width) { width = el.clientWidth; fit(); } });
    observer.observe(el);
    return () => observer.disconnect();
  });

  // Dictation (Wispr Flow, Handy, macOS Dictation) may insert text through
  // the accessibility API, which can change the field without an `input`
  // event; `bind:value` would then miss it, leaving Send disabled and Enter
  // sending nothing. The caret still moves, so `selectionchange` fires: take
  // the field's text then, on blur, and before sending.
  function sync() { if (field && field.value !== value) value = field.value; }
  $effect(() => {
    const el = field;
    if (!el) return;
    const onSelection = () => { if (document.activeElement === el) sync(); };
    el.addEventListener('selectionchange', sync);
    document.addEventListener('selectionchange', onSelection);
    return () => { el.removeEventListener('selectionchange', sync); document.removeEventListener('selectionchange', onSelection); };
  });
</script>

<form class="composer" class:compact aria-label="Chat composer"
  onpaste={(event) => {
    // Text wins: an app that copies text with an image of it pastes the text.
    const files = Array.from(event.clipboardData?.files ?? []);
    if (files.length && !event.clipboardData?.getData('text/plain')) { event.preventDefault(); event.stopPropagation(); void attach(files); }
  }}
  ondragover={(event) => { if (event.dataTransfer?.types.includes('Files')) event.preventDefault(); }}
  ondrop={(event) => { if (event.dataTransfer?.files.length) { event.preventDefault(); event.stopPropagation(); void attach(Array.from(event.dataTransfer.files)); } }}
  onsubmit={(event) => { event.preventDefault(); sync(); if (!blocked) { imageError = ''; submit(); } }}>
  <div class="surface">
    <div class="target">{#if targetControl}{@render targetControl()}{:else}<span>{target}</span>{/if}</div>
    {#if images.length}<div class="attachments">{#each images as image, index}<div class="attachment"><img src={imageUrl(image)} alt={image.name} /><IconButton icon={X} label={`Remove ${image.name}`} size={12} class="attachment-remove" onclick={() => { images = images.filter((_, i) => i !== index); }} /></div>{/each}</div>{/if}
    {#if imageError}<p class="image-error" role="alert">{imageError}</p>{/if}
    {#if reading}<p class="hint" role="status">Reading images…</p>{/if}
    {#if !doc.canAsk}<p class="notice">Open Agent settings to connect your runtime.</p>{/if}
    <TextInput multiline class={overflowing ? 'overflowing' : ''} bind:element={field} aria-label="Agent instruction" rows={compact ? 1 : 2} placeholder="Ask, explain, or change something…" bind:value onblur={sync} onkeydown={(event: KeyboardEvent) => { if (shouldSubmit(event)) { event.preventDefault(); sync(); if (!blocked) { imageError = ''; submit(); } } }} />
    <input hidden type="file" bind:this={picker} accept="image/png,image/jpeg,image/webp,image/gif,image/heic,image/heif,image/svg+xml,.heic,.heif,.svg" multiple onchange={() => { void attach(Array.from(picker.files ?? [])); picker.value = ''; }} />
    <div class="footer">
      <IconButton icon={ImagePlus} label="Attach images" disabled={busy || reading} onclick={() => picker.click()} />
      <Select variant="quiet" value={doc.agentProvider} items={providers} label="Agent runtime" disabled={busy || doc.asking} onChange={(provider) => doc.configureAgent({ provider })} />
      <ModelEffortPicker disabled={busy || doc.asking} />
      <AgentSettings />
      <span class="spacer"></span>
      {#if busy && stop}
        <IconButton icon={Square} large class="round stop" type="button" onclick={stop} label="Stop the agent" />
      {:else}
        <IconButton icon={ArrowUp} size={16} large class="round send" type="submit" disabled={blocked} label="Send message" title="Send · Enter (Shift+Enter for a new line)" />
      {/if}
    </div>
  </div>
</form>

<style>
  .attachments { display: flex; gap: 10px; padding: 10px 16px 4px; flex-wrap: wrap; }
  .attachment { position: relative; width: 72px; height: 72px; flex: none; }
  .attachment img { display: block; width: 100%; height: 100%; object-fit: contain; border-radius: var(--radius-lg); }
  .attachment :global(.attachment-remove) {
    position: absolute; top: -6px; right: -6px; width: 22px; height: 22px;
    border: 2px solid var(--bg-input); border-radius: 50%;
    background: var(--fg); color: var(--bg); box-shadow: var(--menu-shadow);
  }
  .attachment :global(.attachment-remove:hover) { background: var(--danger); color: var(--accent-fg); }
  .image-error { color: var(--danger); margin: 8px 16px; font-size: var(--text-md); }
  .composer { flex: none; padding: 6px 16px 14px; background: var(--bg); }
  .surface {
    max-width: 736px; margin: 0 auto; border: 1px solid var(--border-strong); border-radius: var(--radius-xl);
    background: var(--bg-input); overflow: hidden;
    transition: border-color 150ms ease;
  }
  .surface:focus-within { border-color: var(--fg-faint); }
  .target { padding: 9px 16px 0; font-size: var(--text-sm); color: var(--fg-faint); }
  .target:empty { display: none; }
  /* Why Send is off, inside the card and on the text's left edge. */
  .notice { margin: 0; padding: 9px 16px 0; font-size: var(--text-sm); color: var(--fg-muted); }
  .surface :global(textarea.ui-input) {
    display: block; width: 100%; box-sizing: border-box; border: 0; border-radius: 0; box-shadow: none; outline: none;
    background: transparent; resize: none; overflow-y: hidden;
    /* One full line plus the padding, so the caret is never clipped. */
    min-height: calc(var(--text-lg) * 1.6 + 14px); max-height: 208px; padding: 8px 16px 6px;
    font-size: var(--text-lg); line-height: 1.6; color: var(--fg);
  }
  .surface :global(textarea.ui-input.overflowing) { overflow-y: auto; }
  .footer { display: flex; gap: 2px; align-items: center; padding: 2px 8px 8px; min-width: 0; }
  .footer :global(.ui-select-trigger.is-quiet) { flex: none; color: var(--fg-muted); }
  .spacer { flex: 1; }
  /* The primary action: a filled round Send (Stop while a turn runs). */
  .footer :global(.round) { border-radius: 50%; }
  .footer :global(.round.send) { background: var(--accent); color: var(--accent-fg); }
  .footer :global(.round.send:hover:not(:disabled)) { background: var(--accent); color: var(--accent-fg); filter: brightness(1.1); }
  .footer :global(.round.send:disabled) { background: var(--active); color: var(--fg-faint); opacity: 1; }
  .footer :global(.round.stop) { background: var(--fg); color: var(--bg); }
  .footer :global(.round.stop svg) { fill: currentColor; }
  .footer :global(.round.stop:hover) { background: var(--fg); color: var(--bg); opacity: 0.85; }
  .hint { margin: 6px 16px 0; color: var(--fg-faint); font-size: var(--text-sm); }
  .compact { padding: 4px 10px 8px; }
  .compact .surface :global(textarea.ui-input) { max-height: 120px; }
  @media (prefers-reduced-motion: reduce) { .surface { transition: none; } }
</style>
