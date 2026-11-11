<script lang="ts">
  // Asked before a rect or an ellipse becomes a path when that would cut
  // something: a binding on its size or radius, another node reading them,
  // a clip animating them (iron_document::shape). With nothing to cut the
  // engine converts without asking.
  import { AlertDialog } from "bits-ui";
  import { controllerManager } from "../controller-manager.svelte";
  import Button from "./ui/Button.svelte";

  const doc = controllerManager.docClient;
  const q = $derived(doc.convertQuestion);
  const noun = $derived(q?.type === "rect" ? "rectangle" : (q?.type ?? "shape"));

  async function convert() {
    const node = q?.node;
    doc.convertQuestion = null;
    if (node) await doc.toPath(node, true);
  }
</script>

<AlertDialog.Root open={!!q} onOpenChange={(open) => { if (!open) doc.convertQuestion = null; }}>
  <AlertDialog.Portal>
    <AlertDialog.Overlay class="ui-dialog-overlay" />
    <AlertDialog.Content class="ui-dialog">
      <AlertDialog.Title class="ui-dialog-title">Edit this {noun} as a path?</AlertDialog.Title>
      <div class="ui-dialog-body">
        <AlertDialog.Description class="ui-dialog-description">
          Editing its points turns it into a path, which has no {q?.type === "rect" ? "width or height" : "radii"} to bind. This cuts:
        </AlertDialog.Description>
        <ul class="cuts">
          {#each q?.cuts ?? [] as cut}
            <li>{cut}</li>
          {/each}
        </ul>
        <p class="ui-dialog-description">Undo turns it back into the {noun}, with everything restored.</p>
      </div>
      <div class="ui-dialog-actions">
        <AlertDialog.Cancel>{#snippet child({ props })}<Button variant="outline" {...props}>Cancel</Button>{/snippet}</AlertDialog.Cancel>
        <AlertDialog.Action onclick={convert}>{#snippet child({ props })}<Button variant="primary" {...props}>Convert to path</Button>{/snippet}</AlertDialog.Action>
      </div>
    </AlertDialog.Content>
  </AlertDialog.Portal>
</AlertDialog.Root>

<style>
  .cuts { margin: 0; padding-left: 18px; list-style: disc; line-height: 1.45; font-family: var(--font-mono); font-size: var(--text-md); }
  p { margin: 0; }
</style>
