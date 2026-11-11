<script lang="ts">
  // A panel's section (`.ui-section` in ui.css): a 12 px semibold title, an
  // optional muted `meta` at the right or `actions` (IconButtons), then the
  // content. Every inspector and settings section uses this.
  import type { Snippet } from "svelte";

  let { title, meta = undefined, actions = undefined, children = undefined, class: extra = "", ...rest }:
    { title: string; meta?: string; actions?: Snippet; children?: Snippet; class?: string; [key: string]: unknown } = $props();
</script>

<section class="ui-section {extra}" aria-label={title} {...rest}>
  <header class="ui-section-header">
    <span>{title}</span>
    {#if meta}<span class="ui-section-meta" title={meta}>{meta}</span>{/if}
    {#if actions}<span class="ui-section-actions">{@render actions()}</span>{/if}
  </header>
  {@render children?.()}
</section>
