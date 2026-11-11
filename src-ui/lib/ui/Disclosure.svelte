<script lang="ts">
  // The app's one show/hide disclosure (bits-ui Collapsible): a chevron that
  // turns and a label, then the content when open. The layers' twisty and
  // the chat's details use the same chevron.
  import { Collapsible } from "bits-ui";
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import type { Snippet } from "svelte";

  let { label, open = $bindable(false), children, class: extra = "" }: { label: string; open?: boolean; children: Snippet; class?: string } = $props();
</script>

<Collapsible.Root bind:open class={extra}>
  <Collapsible.Trigger class="ui-disclosure-trigger"><ChevronRight size={12} strokeWidth={1.75} /><span>{label}</span></Collapsible.Trigger>
  <Collapsible.Content>{@render children()}</Collapsible.Content>
</Collapsible.Root>
