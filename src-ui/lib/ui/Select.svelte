<script lang="ts" module>
  export type SelectItem<V extends string> = { value: V; label: string; hint?: string; disabled?: boolean };
</script>

<script lang="ts" generics="T extends string">
  // A single-choice dropdown (bits-ui Select) in the app's look (ui.css):
  // each item a label, an optional hint, and optionally disabled.
  import { Select } from "bits-ui";
  import Check from "@lucide/svelte/icons/check";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";

  let {
    value,
    items,
    onChange,
    disabled = false,
    label,
  }: { value: T; items: SelectItem<T>[]; onChange: (value: T) => void; disabled?: boolean; label?: string } = $props();

  const selected = $derived(items.find((i) => i.value === value));
</script>

<Select.Root type="single" {value} {disabled} items={items.map((i) => ({ value: i.value, label: i.label, disabled: i.disabled }))} onValueChange={(v) => onChange(v as T)}>
  <Select.Trigger class="ui-select-trigger" aria-label={label}>
    <span>{selected?.label ?? ""}</span>
    <ChevronDown size={14} strokeWidth={1.75} />
  </Select.Trigger>
  <Select.Portal>
    <Select.Content class="ui-menu" sideOffset={4} align="start">
      <Select.Viewport>
        {#each items as item (item.value)}
          <Select.Item class="ui-menu-item" value={item.value} label={item.label} disabled={item.disabled}>
            {#snippet children({ selected })}
              <span style="width: 14px; display: flex">{#if selected}<Check size={14} strokeWidth={2} />{/if}</span>
              <span>{item.label}</span>
              {#if item.hint}<span class="ui-hint">{item.hint}</span>{/if}
            {/snippet}
          </Select.Item>
        {/each}
      </Select.Viewport>
    </Select.Content>
  </Select.Portal>
</Select.Root>
