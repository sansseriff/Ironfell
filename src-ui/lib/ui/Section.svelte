<script lang="ts">
  // A panel's section (`.ui-section` in ui.css): a 12 px semibold title (or
  // a `heading` snippet in its place), an optional muted `meta` at the
  // right (`metaTitle` its tooltip) and `actions` (IconButtons), then the
  // content. Every inspector and settings section uses this; other
  // attributes (`data-section`, `aria-label`) go on the section.
  import type { Snippet } from "svelte";

  let { title, meta = undefined, metaTitle = undefined, heading = undefined, actions = undefined, children = undefined, class: extra = "", headerClass = "", ...rest }:
    { title: string; meta?: string; metaTitle?: string; heading?: Snippet; actions?: Snippet; children?: Snippet; class?: string; headerClass?: string; [key: string]: unknown } = $props();
</script>

<section class="ui-section {extra}" aria-label={title} {...rest}>
  <!-- One line: scripts read the header's text, so no whitespace in it. -->
  <header class="ui-section-header {headerClass}">{#if heading}{@render heading()}{:else}<h3>{title}</h3>{/if}{#if meta}<span class="ui-section-meta" title={metaTitle ?? meta}>{meta}</span>{/if}{#if actions}<span class="ui-section-actions">{@render actions()}</span>{/if}</header>
  {@render children?.()}
</section>

<style>
  h3 { min-width: 0; margin: 0; overflow: hidden; font: inherit; text-overflow: ellipsis; white-space: nowrap; }
</style>
