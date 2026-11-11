<script lang="ts" module>
  export type SelectItem<V extends string> = { value: V; label: string; hint?: string; disabled?: boolean };
</script>

<script lang="ts" generics="T extends string">
  // The app's one single-choice dropdown (bits-ui Select, ui.css): each item
  // a label, an optional hint, and optionally disabled. `variant` "field" is
  // a filled field the width of its container (the inspector, forms);
  // "quiet" a label and chevron with no box (headers, toolbars). Never use a
  // native <select>.
  import { Select } from "bits-ui";
  import Check from "@lucide/svelte/icons/check";
  import ChevronDown from "@lucide/svelte/icons/chevron-down";

  let {
    value,
    items,
    onChange,
    disabled = false,
    label,
    variant = "field",
    class: extra = "",
    placeholder = "",
  }: { value: T; items: SelectItem<T>[]; onChange: (value: T) => void; disabled?: boolean; label?: string; variant?: "field" | "quiet"; class?: string; placeholder?: string } = $props();

  const selected = $derived(items.find((i) => i.value === value));
</script>

<Select.Root type="single" {value} {disabled} items={items.map((i) => ({ value: i.value, label: i.label, disabled: i.disabled }))} onValueChange={(v) => onChange(v as T)}>
  <Select.Trigger class="ui-select-trigger {variant === 'quiet' ? 'is-quiet' : ''} {extra}" aria-label={label} title={variant === "quiet" ? label : undefined}>
    <span class="ui-select-label">{selected?.label ?? placeholder}</span>
    <ChevronDown size={12} strokeWidth={1.75} />
  </Select.Trigger>
  <Select.Portal>
    <Select.Content class="ui-menu" sideOffset={4} align="start">
      <Select.Viewport>
        {#each items as item (item.value)}
          <Select.Item class="ui-menu-item" value={item.value} label={item.label} disabled={item.disabled}>
            {#snippet children({ selected })}
              <span class="ui-menu-check">{#if selected}<Check size={14} strokeWidth={1.75} />{/if}</span>
              <span>{item.label}</span>
              {#if item.hint}<span class="ui-hint">{item.hint}</span>{/if}
            {/snippet}
          </Select.Item>
        {/each}
      </Select.Viewport>
    </Select.Content>
  </Select.Portal>
</Select.Root>
