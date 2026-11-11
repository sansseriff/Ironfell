<script lang="ts">
  // The app's one "nothing here" message (`.ui-empty` in ui.css): an
  // optional 20 px Lucide icon, a title, a line of help and optional
  // actions. `inline` for a short note inside a section.
  import type { Component, Snippet } from "svelte";

  let { icon: Icon = undefined, title = "", children = undefined, actions = undefined, inline = false, class: extra = "" }:
    { icon?: Component<any>; title?: string; children?: Snippet; actions?: Snippet; inline?: boolean; class?: string } = $props();
</script>

<div class="ui-empty {inline ? 'is-inline' : ''} {extra}">
  {#if Icon && !inline}<Icon size={20} strokeWidth={1.5} />{/if}
  {#if title}<strong>{title}</strong>{/if}
  {#if children}<span>{@render children()}</span>{/if}
  {#if actions}<div class="ui-empty-actions">{@render actions()}</div>{/if}
</div>
