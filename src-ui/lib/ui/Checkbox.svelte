<script lang="ts">
  // The app's one checkbox (bits-ui Checkbox, `.ui-checkbox` in ui.css),
  // with its label beside it when `label` is given as text. Never use a
  // native checkbox.
  import { Checkbox } from "bits-ui";
  import Check from "@lucide/svelte/icons/check";
  import Minus from "@lucide/svelte/icons/minus";

  let {
    checked,
    onChange,
    label,
    showLabel = true,
    disabled = false,
    indeterminate = false,
    class: extra = "",
  }: { checked: boolean; onChange: (checked: boolean) => void; label: string; showLabel?: boolean; disabled?: boolean; indeterminate?: boolean; class?: string } = $props();
</script>

{#snippet box()}
  <Checkbox.Root class="ui-checkbox" {checked} {indeterminate} {disabled} aria-label={showLabel ? undefined : label} onCheckedChange={(v) => onChange(!!v)}>
    {#snippet children({ checked, indeterminate })}
      {#if indeterminate}<Minus size={10} strokeWidth={3} />{:else if checked}<Check size={10} strokeWidth={3} />{/if}
    {/snippet}
  </Checkbox.Root>
{/snippet}

{#if showLabel}
  <label class="ui-check-row {extra}">{@render box()}<span>{label}</span></label>
{:else}
  {@render box()}
{/if}
