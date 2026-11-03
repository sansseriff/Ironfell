<script lang="ts">
  // A text field in the Inspector's look (`.ui-input`: PropertyField's
  // sunken control, AGENTS.md). Bits UI has no text input primitive, so this
  // is the one styled native input the views share; `multiline` makes it a
  // textarea.
  let {
    value = $bindable(""),
    multiline = false,
    element = $bindable(),
    class: extra = "",
    ...rest
  }: { value?: string; multiline?: boolean; element?: HTMLInputElement | HTMLTextAreaElement; class?: string; [key: string]: unknown } = $props();
</script>

{#if multiline}
  <textarea class="ui-input {extra}" bind:value bind:this={element} {...rest}></textarea>
{:else}
  <input class="ui-input {extra}" bind:value bind:this={element} {...rest} />
{/if}
