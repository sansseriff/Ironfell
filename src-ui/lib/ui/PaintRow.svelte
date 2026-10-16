<script lang="ts">
  import { Popover } from "bits-ui";
  import X from "@lucide/svelte/icons/x";
  import Minus from "@lucide/svelte/icons/minus";
  import PropertyField, { type Property } from "./PropertyField.svelte";
  import Select from "./Select.svelte";
  let { node, label, color, opacity, gradient = [], onSet, onRefresh, onError, onRemove, onMode }: {
    node: string; label: string; color: Property; opacity?: Property; gradient?: Property[];
    onSet: (field: Property, text: string) => void; onRefresh: () => void; onError: (error: string) => void;
    onRemove?: () => void; onMode?: (mode: string) => void;
  } = $props();
  let open = $state(false);
  const mode = $derived(gradient.find(f => f.field === "kind")?.text ?? "solid");
  const paint = $derived(gradient.length ? `${mode === "radial" ? "radial-gradient(circle" : "linear-gradient(135deg"}, ${gradient.find(f => f.field === "start")?.text}, ${gradient.find(f => f.field === "end")?.text})` : color.text ?? "#808080");
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
      <Popover.Content class="property-popover" side="left" align="start" sideOffset={12} aria-label="{label} paint settings">
        <header><strong>{label}</strong><Popover.Close class="property-icon" aria-label="Close {label.toLowerCase()} settings"><X size={15} /></Popover.Close></header>
        <div class="property-popover-body">
          {#if onMode}<Select label="Fill type" value={mode} items={[{ value: "solid", label: "Solid" }, { value: "linear", label: "Linear gradient" }, { value: "radial", label: "Radial gradient" }]} onChange={onMode} />{/if}
          <div class="paint-preview" style:background={paint}></div>
          <div class="property-grid">
            {#if gradient.length}
              {#each gradient.filter(f => f.kind === "color") as f (f.leaf)}
                <PropertyField field={f} {node} label={f.field === "start" ? "Start colour" : "End colour"} {onSet} {onRefresh} {onError} />
                <PropertyField field={f} {node} channel="alpha" label={f.field === "start" ? "Start alpha" : "End alpha"} {onSet} {onRefresh} {onError} />
              {/each}
              {#each gradient.filter(f => f.kind === "number") as f (f.leaf)}
                <PropertyField field={f} {node} label={{ x0: "Start X", y0: "Start Y", x1: "End X", y1: "End Y" }[f.field]} {onSet} {onRefresh} {onError} />
              {/each}
            {:else}
              <PropertyField field={color} {node} label="Colour" {onSet} {onRefresh} {onError} />
              <PropertyField field={color} {node} channel="alpha" label="Colour alpha" {onSet} {onRefresh} {onError} />
            {/if}
          </div>
        </div>
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
  .paint-preview { height: 112px; border-radius: 6px; border: 1px solid var(--border); }
  .remove { flex: 0 0 24px; }
</style>
