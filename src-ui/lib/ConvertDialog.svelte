<script lang="ts">
  // Asked before a rect or an ellipse becomes a path when that would cut
  // something: a binding on its size or radius, another node reading them,
  // a clip animating them (iron_document::shape). With nothing to cut the
  // engine converts without asking.
  import { AlertDialog } from "bits-ui";
  import { controllerManager } from "../controller-manager.svelte";

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
    <AlertDialog.Overlay class="convert-overlay" />
    <AlertDialog.Content class="convert-dialog">
      <AlertDialog.Title class="convert-title">Edit this {noun} as a path?</AlertDialog.Title>
      <AlertDialog.Description class="convert-body">
        Editing its points turns it into a path, which has no {q?.type === "rect" ? "width or height" : "radii"} to bind. This cuts:
      </AlertDialog.Description>
      <ul class="convert-cuts">
        {#each q?.cuts ?? [] as cut}
          <li>{cut}</li>
        {/each}
      </ul>
      <p class="convert-body">Undo turns it back into the {noun}, with everything restored.</p>
      <div class="convert-actions">
        <AlertDialog.Cancel class="convert-button">Cancel</AlertDialog.Cancel>
        <AlertDialog.Action class="convert-button primary" onclick={convert}>Convert to path</AlertDialog.Action>
      </div>
    </AlertDialog.Content>
  </AlertDialog.Portal>
</AlertDialog.Root>

<style>
  :global(.convert-overlay) {
    position: fixed;
    inset: 0;
    z-index: 1000;
    background: var(--overlay);
  }
  :global(.convert-dialog) {
    position: fixed;
    left: 50%;
    top: 50%;
    transform: translate(-50%, -50%);
    z-index: 1001;
    width: min(440px, calc(100vw - 32px));
    padding: 18px 20px 16px;
    border: 1px solid var(--border-strong);
    border-radius: 10px;
    background: var(--bg);
    color: var(--fg);
    box-shadow: var(--menu-shadow);
    font-size: 13px;
  }
  :global(.convert-title) {
    margin: 0 0 8px;
    font-size: 14px;
    font-weight: 600;
  }
  :global(.convert-body) {
    margin: 0 0 8px;
    color: var(--fg-muted);
    line-height: 1.45;
  }
  .convert-cuts {
    margin: 0 0 10px;
    padding-left: 18px;
    line-height: 1.45;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12px;
  }
  .convert-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 14px;
  }
  :global(.convert-button) {
    padding: 5px 12px;
    border: 1px solid var(--border-strong);
    border-radius: 6px;
    background: var(--bg-input);
    color: var(--fg);
    font: inherit;
    cursor: pointer;
  }
  :global(.convert-button:hover) {
    background: var(--hover);
  }
  :global(.convert-button.primary) {
    border-color: var(--accent);
    background: var(--accent);
    color: var(--accent-fg);
  }
</style>
