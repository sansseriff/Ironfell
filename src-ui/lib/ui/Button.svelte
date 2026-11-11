<script lang="ts">
  // The app's one text button (AGENTS.md): Bits UI's Button, `.ui-button` in
  // ui.css. 28 px, or 24 px when `small`; `variant` picks outline, primary or
  // danger; `block` fills the row. Put a 14 px Lucide icon before the label.
  import { Button } from "bits-ui";
  import type { Snippet } from "svelte";

  let {
    variant = "default",
    small = false,
    block = false,
    class: extra = "",
    children,
    ...rest
  }: { variant?: "default" | "outline" | "primary" | "danger"; small?: boolean; block?: boolean; class?: string; children?: Snippet; [key: string]: unknown } = $props();
</script>

<Button.Root class="ui-button {variant === 'default' ? '' : `is-${variant}`} {small ? 'is-small' : ''} {block ? 'is-block' : ''} {extra}" {...rest}>
  {@render children?.()}
</Button.Root>
