<script lang="ts">
  import { Popover, ToggleGroup } from "bits-ui";
  import X from "@lucide/svelte/icons/x";
  import Minus from "@lucide/svelte/icons/minus";
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
    <div class="paint-value">
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
        <header class="color-header">
          <span class="color-tab">{label}</span>
          <Popover.Close class="property-icon" aria-label="Close {label.toLowerCase()} settings"><X size={16} strokeWidth={1.5} /></Popover.Close>
        </header>
        {#if onMode}
          <ToggleGroup.Root type="single" class="paint-types" value={mode} onValueChange={(v) => { if (v && v !== mode) onMode(v); }} aria-label="Paint type">
            {#each types as t (t.value)}
              <ToggleGroup.Item class="property-icon large paint-type" value={t.value} aria-label={t.label} title={t.label}>
                <span class="paint-type-chip" style:background={t.chip}></span>
              </ToggleGroup.Item>
            {/each}
          </ToggleGroup.Root>
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
  {#if onRemove}<button class="remove property-icon" aria-label="Remove {label.toLowerCase()}" title="Remove {label.toLowerCase()}" onclick={onRemove}><Minus size={16} strokeWidth={1.5} /></button>{/if}
</div>

<style>
  .paint-row { display: flex; align-items: center; gap: 6px; min-width: 0; }
  .paint-value { display: grid; grid-template-columns: 30px minmax(0, 1fr) 64px; flex: 1; min-width: 0; align-items: center; background: var(--bg-sunken); border-radius: 6px; }
  .paint-value :global(.field .control) { background: transparent; }
  .paint-opacity { border-left: 1px solid var(--bg); min-width: 0; }
  .paint-name { text-align: left; border: 0; background: transparent; padding: 7px 3px; font-size: 12px; color: var(--fg); cursor: pointer; }
  .paint-value :global(.paint-swatch) { display: grid; place-items: center; width: 30px; height: 32px; padding: 0; border: 0; background: transparent; cursor: pointer; }
  .paint-chip { display: block; width: 19px; height: 19px; border-radius: 3px; box-shadow: inset 0 0 0 1px rgba(0,0,0,.05); }
  :global(.property-popover.color-popover) { width: 240px; }
  .color-header { padding: 8px 8px 8px 8px; }
  .color-tab { padding: 5px 8px; border-radius: 5px; background: var(--hover); font-weight: 600; }
  :global(.paint-types) { display: flex; gap: 4px; padding: 8px; border-bottom: 1px solid var(--border); }
  :global(.paint-type[data-state="on"]) { background: var(--hover); }
  .paint-type-chip { display: block; width: 14px; height: 14px; border-radius: 2px; box-shadow: inset 0 0 0 1.5px var(--fg-muted); }
  .stops { position: relative; margin: 16px 23px 0; height: 24px; }
  .stop-bar { position: absolute; inset: 6px 0; border-radius: 3px; background: var(--stops); box-shadow: inset 0 0 0 1px rgba(0,0,0,.1); }
  .stop { position: absolute; top: 0; width: 20px; height: 24px; padding: 3px; border: 0; border-radius: 4px; background: var(--bg); transform: translateX(-50%); cursor: pointer; box-shadow: 0 0 0 1px var(--border-strong), 0 1px 3px rgba(0,0,0,.2); }
  .stop span { display: block; width: 100%; height: 100%; border-radius: 2px; }
  .stop.active { box-shadow: 0 0 0 2px var(--accent), 0 1px 3px rgba(0,0,0,.2); }
  .stop:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
  .gradient-points { padding: 0 16px 16px; }
  .remove { flex: 0 0 24px; }
</style>
