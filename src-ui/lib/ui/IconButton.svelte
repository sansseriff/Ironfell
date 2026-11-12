<script lang="ts">
  // An icon-only button (AGENTS.md): Bits UI's Button, or its Toggle when
  // `pressed` is given, in the Inspector's `.ui-icon-button` look (`large`
  // for toolbars). `label` is the accessible name and the tooltip; the icon
  // is a Lucide component.
  import { Button, Toggle } from "bits-ui";
  import type { Component } from "svelte";

  let {
    icon: Icon,
    label,
    title = undefined,
    size = 14,
    large = false,
    pressed = undefined,
    onPressedChange = undefined,
    class: extra = "",
    ...rest
  }: {
    icon: Component<any>;
    label: string;
    title?: string;
    size?: number;
    large?: boolean;
    pressed?: boolean;
    onPressedChange?: (on: boolean) => void;
    class?: string;
    [key: string]: unknown;
  } = $props();
</script>

{#if pressed === undefined}
  <Button.Root class="ui-icon-button {large ? 'large' : ''} {extra}" aria-label={label} title={title ?? label} {...rest}>
    <Icon size={size} strokeWidth={1.75} />
  </Button.Root>
{:else}
  <Toggle.Root class="ui-icon-button {large ? 'large' : ''} {extra}" aria-label={label} title={title ?? label} {pressed} {onPressedChange} {...rest}>
    <Icon size={size} strokeWidth={1.75} />
  </Toggle.Root>
{/if}
