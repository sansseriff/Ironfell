<script lang="ts">
  import { Popover, ToggleGroup } from "bits-ui";
  import X from "@lucide/svelte/icons/x";
  import Minus from "@lucide/svelte/icons/minus";
  import IconButton from "./IconButton.svelte";
  import PropertyField, { type Property } from "./PropertyField.svelte";
  import ColorPicker from "./ColorPicker.svelte";
  import { controllerManager } from "../../controller-manager.svelte";
  let { node, label, color, opacity, gradient = [], onSet, onRefresh, onError, onRemove, onMode }: {
    node: string; label: string; color: Property; opacity?: Property; gradient?: Property[];
    onSet: (field: Property, text: string) => void; onRefresh: () => void; onError: (error: string) => void;
    onRemove?: () => void; onMode?: (mode: string) => void;
  } = $props();
  const doc = controllerManager.docClient;
  let open = $state(false);
  const mode = $derived(gradient.find(f => f.field === "kind")?.text ?? "solid");
  const stop = (name: string) => gradient.find(f => f.field === name);
  const paint = $derived(gradient.length ? `${mode === "radial" ? "radial-gradient(circle" : "linear-gradient(135deg"}, ${stop("start")?.text}, ${stop("end")?.text})` : color.text ?? "#808080");
  // Which gradient stop the picker edits (Figma's selected stop).
  let active = $state<"start" | "end">("start");
  const picked = $derived(gradient.length ? stop(active) ?? color : color);
  // The composition's colours, read once each time the popover opens so the
  // swatches hold still while a drag recolours the canvas.
  let swatches = $state<string[]>([]);
  $effect(() => {
    if (!open) return;
    let live = true;
    doc.command({ op: "colors", node }).then((r) => { if (live && Array.isArray(r)) swatches = r; }).catch(() => {});
    return () => { live = false; };
  });
  const types = [
    { value: "solid", label: "Solid", chip: "var(--fg-muted)" },
    { value: "linear", label: "Linear gradient", chip: "linear-gradient(to bottom, var(--fg-muted), transparent)" },
    { value: "radial", label: "Radial gradient", chip: "radial-gradient(circle, transparent 15%, var(--fg-muted) 75%)" },
  ];
</script>

<div class="paint-row" data-paint={label.toLowerCase()}>
  <Popover.Root bind:open>
    <div class="ui-field paint-value">
      <Popover.Trigger class="paint-swatch" aria-label="{label} paint settings" title="Edit {label.toLowerCase()}">
        <span class="paint-chip" style:background={paint}></span>
      </Popover.Trigger>
      {#if gradient.length}
        <button class="paint-name" onclick={() => open = true}>{mode === "radial" ? "Radial" : "Linear"}</button>
      {:else}
        <PropertyField field={color} {node} compact hideSwatch colorDigits={6} {onSet} {onRefresh} {onError} label="{label} colour" />
      {/if}
      <div class="paint-opacity">
        <PropertyField field={opacity ?? color} {node} compact channel={opacity ? undefined : "alpha"} label="{label} opacity" {onSet} {onRefresh} {onError} />
      </div>
    </div>
    <Popover.Portal>
      <Popover.Content class="property-popover color-popover" side="left" align="start" sideOffset={12} aria-label="{label} paint settings">
        <header>
          <strong>{label}</strong>
          <Popover.Close class="property-icon" aria-label="Close {label.toLowerCase()} settings"><X size={14} strokeWidth={1.75} /></Popover.Close>
        </header>
        {#if onMode}
          <div class="paint-types-row">
            <ToggleGroup.Root type="single" class="ui-segmented is-inline paint-types" value={mode} onValueChange={(v) => { if (v && v !== mode) onMode(v); }} aria-label="Paint type">
              {#each types as t (t.value)}
                <ToggleGroup.Item class="ui-segment" value={t.value} aria-label={t.label} title={t.label}>
                  <span class="paint-type-chip" style:background={t.chip}></span>
                </ToggleGroup.Item>
              {/each}
            </ToggleGroup.Root>
          </div>
        {/if}
        {#if gradient.length}
          <div class="stops" style:--stops="linear-gradient(to right, {stop('start')?.text}, {stop('end')?.text})">
            <div class="stop-bar"></div>
            {#each ["start", "end"] as const as name (name)}
              <button class="stop" class:active={active === name} style:left={name === "start" ? "0%" : "100%"} aria-label="{name === 'start' ? 'Start' : 'End'} colour stop" aria-pressed={active === name} onclick={() => active = name}>
                <span style:background={stop(name)?.text}></span>
              </button>
            {/each}
          </div>
        {/if}
        {#key picked.leaf}
          <ColorPicker field={picked} {node} {swatches} {onSet} {onRefresh} {onError} />
        {/key}
        {#if gradient.length}
          <div class="property-grid gradient-points">
            {#each gradient.filter(f => f.kind === "number") as f (f.leaf)}
              <PropertyField field={f} {node} label={{ x0: "Start X", y0: "Start Y", x1: "End X", y1: "End Y" }[f.field]} {onSet} {onRefresh} {onError} />
            {/each}
          </div>
        {/if}
      </Popover.Content>
    </Popover.Portal>
  </Popover.Root>
  {#if onRemove}<IconButton icon={Minus} label="Remove {label.toLowerCase()}" onclick={onRemove} />{/if}
</div>

<style>
  .paint-row { display: flex; align-items: center; gap: 4px; min-width: 0; }
  /* One field (`.ui-field`): the swatch, the colour and the opacity. The
     fields inside lose their own shells. */
  .paint-value { display: grid; grid-template-columns: 28px minmax(0, 1fr) 64px; flex: 1; align-items: center; overflow: hidden; }
  .paint-value :global(.control.ui-field), .paint-value :global(.control.ui-field:hover) { min-height: calc(var(--control-h) - 2px); border-color: transparent; background: transparent; }
  .paint-opacity { min-width: 0; border-left: 1px solid var(--bg); }
  .paint-name { padding: 0 2px; border: 0; background: transparent; color: var(--fg); font-size: var(--text-md); text-align: left; cursor: pointer; }
  .paint-value :global(.paint-swatch) { display: grid; place-items: center; width: 28px; height: calc(var(--control-h) - 2px); padding: 0; border: 0; background: transparent; cursor: pointer; }
  .paint-chip { display: block; width: 16px; height: 16px; border-radius: var(--radius-sm); box-shadow: inset 0 0 0 1px var(--border); }
  :global(.property-popover.color-popover) { width: 240px; }
  .paint-types-row { padding: 8px var(--panel-pad); border-bottom: 1px solid var(--border); }
  :global(.paint-types .ui-segment) { width: 32px; }
  .paint-type-chip { display: block; width: 14px; height: 14px; border-radius: 2px; box-shadow: inset 0 0 0 1.5px currentColor; }
  .stops { position: relative; margin: 16px 23px 0; height: 24px; }
  .stop-bar { position: absolute; inset: 6px 0; border-radius: var(--radius-sm); background: var(--stops); box-shadow: inset 0 0 0 1px var(--border); }
  .stop { position: absolute; top: 0; width: 20px; height: 24px; padding: 3px; border: 0; border-radius: var(--radius); background: var(--raised); transform: translateX(-50%); cursor: pointer; box-shadow: 0 0 0 1px var(--border-strong); }
  .stop span { display: block; width: 100%; height: 100%; border-radius: 2px; }
  .stop.active { box-shadow: 0 0 0 2px var(--accent); }
  .gradient-points { padding: 0 var(--panel-pad) var(--panel-pad); }
</style>
