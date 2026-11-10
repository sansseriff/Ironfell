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
    <TextInput multiline class={overflowing ? 'overflowing' : ''} bind:element={field} aria-label="Agent instruction" rows={compact ? 1 : 2} placeholder="Ask, explain, or change something…" bind:value onblur={sync} onkeydown={(event: KeyboardEvent) => { if (shouldSubmit(event)) { event.preventDefault(); sync(); if (!blocked) { imageError = ''; submit(); } } }} />
    <input hidden type="file" bind:this={picker} accept="image/png,image/jpeg,image/webp,image/gif,image/heic,image/heif,image/svg+xml,.heic,.heif,.svg" multiple onchange={() => { void attach(Array.from(picker.files ?? [])); picker.value = ''; }} />
    <div class="footer">
      <IconButton icon={ImagePlus} label="Attach images" disabled={busy || reading} onclick={() => picker.click()} />
      <Select value={doc.agentProvider} items={providers} label="Agent runtime" disabled={busy || doc.asking} onChange={(provider) => doc.configureAgent({ provider })} />
      <ModelEffortPicker disabled={busy || doc.asking} />
      <AgentSettings />
      <span class="spacer"></span>
      {#if busy && stop}
        <button type="button" class="round stop" onclick={stop} aria-label="Stop the agent" title="Stop the agent"><Square size={11} strokeWidth={0} fill="currentColor" /></button>
      {:else}
        <button type="submit" class="round send" disabled={blocked} aria-label="Send message" title="Send · Enter (Shift+Enter for a new line)"><ArrowUp size={16} strokeWidth={2.25} /></button>
      {/if}
    </div>
  </div>
  {#if !doc.canAsk}<p class="hint">Open Agent settings to connect your runtime.</p>{/if}
</form>

<style>
  .attachments { display: flex; gap: 10px; padding: 10px 16px 4px; flex-wrap: wrap; }
  .attachment { position: relative; width: 72px; height: 72px; flex: none; }
  .attachment img { display: block; width: 100%; height: 100%; object-fit: contain; border-radius: 10px; }
  .attachment :global(.attachment-remove) {
    position: absolute; top: -6px; right: -6px; width: 22px; height: 22px;
    border: 2px solid var(--bg-input); border-radius: 50%;
    background: var(--fg); color: var(--bg); box-shadow: 0 1px 3px rgba(0, 0, 0, 0.24);
  }
  .attachment :global(.attachment-remove:hover) { background: var(--danger); color: white; }
  .attachment :global(.attachment-remove:focus-visible) { outline: 2px solid var(--accent); outline-offset: 2px; }
  .image-error { color: var(--danger); margin: 8px 16px; font-size: 12px; }
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
    background: transparent; resize: none; overflow-y: hidden;
    /* One full line plus the padding, so the caret is never clipped. */
    min-height: calc(14px * 1.6 + 14px); max-height: 208px; padding: 8px 16px 6px;
    font-size: 14px; line-height: 1.6; color: var(--fg);
    scrollbar-width: thin; scrollbar-color: var(--border-strong) transparent;
  }
  .surface :global(textarea.ui-input.overflowing) { overflow-y: auto; }
  .surface :global(textarea.ui-input::-webkit-scrollbar) { width: 8px; }
  .surface :global(textarea.ui-input::-webkit-scrollbar-track) { background: transparent; }
  .surface :global(textarea.ui-input::-webkit-scrollbar-thumb) { background: var(--border-strong); border-radius: 4px; border: 2px solid transparent; background-clip: padding-box; }
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
  .compact .surface :global(textarea.ui-input) { max-height: 120px; min-height: calc(13px * 1.6 + 14px); font-size: 13px; }
  @media (prefers-reduced-motion: reduce) { .surface, .round, .footer :global(*) { transition: none; } }
</style>
