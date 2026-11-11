<script lang="ts" module>
  import type { Component } from "svelte";
  export type Segment<V extends string> = { value: V; label: string; icon?: Component<any>; title?: string };
</script>

<script lang="ts" generics="T extends string">
  // The app's one segmented control (bits-ui ToggleGroup, `.ui-segmented` in
  // ui.css): a filled track with the chosen segment raised. A segment shows
  // its label, or only its icon when `iconOnly` (the label is then its name
  // and tooltip). `inline` sizes it to its segments instead of its row.
  import { ToggleGroup } from "bits-ui";

  let {
    value,
    items,
    onChange,
    label,
    disabled = false,
    inline = false,
    iconOnly = false,
    class: extra = "",
  }: { value: T; items: Segment<T>[]; onChange: (value: T) => void; label: string; disabled?: boolean; inline?: boolean; iconOnly?: boolean; class?: string } = $props();
</script>

<ToggleGroup.Root type="single" class="ui-segmented {inline ? 'is-inline' : ''} {extra}" {value} {disabled} aria-label={label}
  onValueChange={(v) => { if (v && v !== value) onChange(v as T); }}>
  {#each items as item (item.value)}
    <ToggleGroup.Item class="ui-segment" value={item.value} aria-label={iconOnly ? item.label : undefined} title={item.title ?? (iconOnly ? item.label : undefined)}>
      {#if item.icon}<item.icon size={14} strokeWidth={1.75} />{/if}
      {#if !iconOnly}<span>{item.label}</span>{/if}
    </ToggleGroup.Item>
  {/each}
</ToggleGroup.Root>
