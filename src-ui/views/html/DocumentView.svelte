<script lang="ts">
  // The document as markup (source: document, notation: markup): the view
  // the model reads, with save and load. Runtime switches are in Settings.
  import { getContext } from "svelte";
  import { controllerManager } from "../../controller-manager.svelte";
  import Select from "../../lib/ui/Select.svelte";
  import Button from "../../lib/ui/Button.svelte";
  import IconButton from "../../lib/ui/IconButton.svelte";
  import Undo2 from "@lucide/svelte/icons/undo-2";
  import Redo2 from "@lucide/svelte/icons/redo-2";
  import Download from "@lucide/svelte/icons/download";
  import FolderOpen from "@lucide/svelte/icons/folder-open";
  import Copy from "@lucide/svelte/icons/copy";
  import type { ViewSpec } from "../views";
  import { beginCopy, copied } from "../../runtime/clipboard";

  let { spec }: { spec: ViewSpec } = $props();
  void spec;

  const doc = controllerManager.docClient;
  const active = getContext<() => boolean>("iron-view-active") ?? (() => true);
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
    void doc.generation;
    if (!active()) return;
    return doc.subscribe<{ id: string; name: string | null; dims: string }[]>({ op: 'compositions' }, list => { compositions = list; });
  });
  $effect(() => {
    void doc.generation;
    const query_json = JSON.stringify({ scope: null, depth: doc.depth, fidelity: doc.fidelity });
    if (!active()) return;
    return doc.subscribe<string>({ op: 'view', query_json }, view => { doc.view = view; }, error => { doc.error = String(error); });
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
      <Button variant="outline" onclick={() => doc.download()} title="Save the document as a file"><Download size={14} strokeWidth={1.75} />Save</Button>
      <Button variant="outline" onclick={() => fileInput?.click()} title="Open a saved document or an .svg"><FolderOpen size={14} strokeWidth={1.75} />Load</Button>
      <IconButton icon={Undo2} label="Undo" title="Undo (⌘Z)" onclick={() => doc.undo()} disabled={!doc.canUndo} />
      <IconButton icon={Redo2} label="Redo" title="Redo (⇧⌘Z)" onclick={() => doc.redo()} disabled={!doc.canRedo} />
      <input type="file" accept=".json,application/json,.svg,image/svg+xml" bind:this={fileInput} hidden onchange={onFile} />
      <label class="small">
        view
        <Select
          label="Document view detail"
          value={doc.fidelity}
          items={[{ value: "skeleton", label: "skeleton" }, { value: "summary", label: "summary" }, { value: "full", label: "full" }]}
          onChange={(v) => {
            doc.fidelity = v;

          }}
        />
      </label>
      <span class="version" title="Document version">v{doc.version}</span>
    </div>
    <div class="row">
      <label class="small">
        SVG of
        <Select label="What to export as SVG" value={svgScope} items={scopeItems} onChange={(v) => (svgScope = v)} />
      </label>
      <Button variant="outline" onclick={() => doc.downloadSvg(svgScope, scopeName())} title="Download as SVG"><Download size={14} strokeWidth={1.75} />Export SVG</Button>
      <Button variant="outline" title="Copy as SVG (Alt+Shift+C copies the selection)" onclick={copySvg}><Copy size={14} strokeWidth={1.75} />Copy SVG</Button>
    </div>
    {#if doc.error}<p class="error">{doc.error}</p>{/if}
    <pre class="view">{doc.view}</pre>
    <p class="doc-footer">Paste or drop SVG on the canvas to import it.</p>
  {/if}
</section>

<style>
  .view {
    flex: 1 1 auto;
    min-height: 120px;
    margin: 0;
    padding: 8px;
    overflow: auto;
    font-family: var(--font-mono);
    font-size: var(--text-sm);
    line-height: 1.35;
    background: var(--bg-sunken);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    white-space: pre;
  }
  .doc-footer {
    font-size: var(--text-sm);
    color: var(--fg-faint);
  }
  .version {
    font-size: var(--text-sm);
    color: var(--fg-faint);
    margin-left: auto;
  }
  .warning {
    color: var(--warning);
  }
</style>
