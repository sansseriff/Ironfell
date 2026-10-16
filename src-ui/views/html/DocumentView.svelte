<script lang="ts">
  // The document as markup (source: document, notation: markup): the view
  // the model reads, with save and load. Runtime switches are in Settings.
  import { controllerManager } from "../../controller-manager.svelte";
  import Github from "../../lib/Github.svelte";
  import Select from "../../lib/ui/Select.svelte";
  import type { ViewSpec } from "../views";
  import { beginCopy, copied } from "../../runtime/clipboard";

  let { spec }: { spec: ViewSpec } = $props();
  void spec;

  const doc = controllerManager.docClient;
  let fileInput: HTMLInputElement | undefined = $state();

  async function onFile(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    const file = input.files?.[0];
    if (file) {
      try {
        await doc.openFile(file);
      } catch {
        // doc.error carries the message
      }
    }
    input.value = "";
  }

  // SVG export (plans/specs/svg-import-export.md): the selection, or a 2D
  // composition, as a download or as clipboard text.
  let svgScope = $state("selection");
  let compositions: { id: string; name: string | null; dims: string }[] = $state([]);
  $effect(() => {
    void doc.version;
    if (controllerManager.isInitialized) void doc.call("compositions", []).then((list) => (compositions = list)).catch(() => {});
  });
  const scopeItems = $derived([
    { value: "selection", label: "selection" },
    ...compositions.filter((c) => c.dims === "2d").map((c) => ({ value: c.id, label: c.name ?? c.id })),
  ]);
  const scopeName = () => scopeItems.find((i) => i.value === svgScope)?.label ?? "iron";
  function copySvg() {
    // The clipboard write starts inside the click, as Cmd+C's does.
    beginCopy(() => void doc.exportSvg(svgScope).then((r) => copied(r.ok ? (r.svg ?? "") : ""), () => copied("")));
  }
</script>

<section class="html-view">
  {#if controllerManager.showBackendWarning}
    <p class="warning"><strong>WebGL2 not available.</strong> This build needs a WebGL2 context.</p>
  {:else if !controllerManager.isInitialized}
    <p>{controllerManager.loadingInProgress ? "Loading..." : "Starting up..."}</p>
  {:else}
    <div class="row">
      <button onclick={() => doc.download()}>Save</button>
      <button onclick={() => fileInput?.click()}>Load</button>
      <input type="file" accept=".json,application/json,.svg,image/svg+xml" bind:this={fileInput} hidden onchange={onFile} />
      <label class="small">
        view
        <Select
          label="Document view detail"
          value={doc.fidelity}
          items={[{ value: "skeleton", label: "skeleton" }, { value: "summary", label: "summary" }, { value: "full", label: "full" }]}
          onChange={(v) => {
            doc.fidelity = v;
            doc.refresh();
          }}
        />
      </label>
      <span class="version">v{doc.version}</span>
    </div>
    <div class="row">
      <label class="small">
        SVG of
        <Select label="What to export as SVG" value={svgScope} items={scopeItems} onChange={(v) => (svgScope = v)} />
      </label>
      <button onclick={() => doc.downloadSvg(svgScope, scopeName())}>Export SVG</button>
      <button title="Copy as SVG (Alt+Shift+C copies the selection)" onclick={copySvg}>Copy SVG</button>
    </div>
    {#if doc.error}<p class="error">{doc.error}</p>{/if}
    <pre class="view">{doc.view}</pre>
    <p class="hint">Cmd/Ctrl+Z undoes, with Shift redoes. Shift-click a second picture to Link. Paste or drop SVG on the canvas to import it; Load opens an .svg too.</p>
    <Github></Github>
  {/if}
</section>

<style>
  .view {
    flex: 1 1 auto;
    min-height: 120px;
    margin: 0;
    padding: 8px;
    overflow: auto;
    font-size: 11px;
    line-height: 1.35;
    background: var(--bg-sunken);
    border: 1px solid var(--border);
    border-radius: 6px;
    white-space: pre;
  }
  .version {
    font-size: 11px;
    color: var(--fg-faint);
    margin-left: auto;
  }
  .warning {
    color: var(--warning);
  }
</style>
