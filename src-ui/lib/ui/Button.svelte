<script lang="ts">
  // A text button (AGENTS.md): Bits UI's Button in the Inspector's look
  // (`.ui-button` in ui.css: quiet, like its `.add` buttons). `variant`
  // picks outline, primary or danger; `small` the compact size.
  import { Button } from "bits-ui";
  import type { Snippet } from "svelte";

  let {
    variant = "default",
    small = false,
    class: extra = "",
    children,
    ...rest
  }: { variant?: "default" | "outline" | "primary" | "danger"; small?: boolean; class?: string; children?: Snippet; [key: string]: unknown } = $props();
</script>

<Button.Root class="ui-button {variant === 'default' ? '' : `is-${variant}`} {small ? 'is-small' : ''} {extra}" {...rest}>
  {@render children?.()}
</Button.Root>
