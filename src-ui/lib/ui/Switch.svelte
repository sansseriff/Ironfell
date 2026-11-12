<script lang="ts">
  // The app's one on/off switch (bits-ui Switch, `.ui-switch` in ui.css), for
  // a setting that applies at once; with its label beside it when `label` is
  // shown. Use Checkbox for a choice inside a form.
  import { Switch } from "bits-ui";

  let { checked, onChange, label, showLabel = false, disabled = false, class: extra = "", ...rest }:
    { checked: boolean; onChange: (checked: boolean) => void; label: string; showLabel?: boolean; disabled?: boolean; class?: string; [key: string]: unknown } = $props();
</script>

{#snippet control()}
  <Switch.Root class="ui-switch {showLabel ? '' : extra}" {checked} {disabled} aria-label={showLabel ? undefined : label} onCheckedChange={(v) => onChange(v)} {...rest}>
    <Switch.Thumb class="ui-switch-thumb" />
  </Switch.Root>
{/snippet}

{#if showLabel}
  <label class="ui-check-row {extra}">{@render control()}<span>{label}</span></label>
{:else}
  {@render control()}
{/if}
