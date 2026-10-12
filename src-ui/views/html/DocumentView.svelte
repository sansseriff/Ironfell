<script lang="ts">
  // The document as markup (source: document, notation: markup): the view
  // the model reads, with save, load and the runtime switches.
  import { controllerManager } from "../../controller-manager.svelte";
  import Github from "../../lib/Github.svelte";
  import type { ViewSpec } from "../views";
  import { resetWorkspace } from "../workspace";

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

  let pendingSwitch = $state(false);
  async function toggleMode(event: Event) {
    const input = event.currentTarget as HTMLInputElement | null;
    if (!input) return;
    pendingSwitch = true;
    try {
      await controllerManager.switchMode(input.checked ? "worker" : "main");
    } finally {
      pendingSwitch = false;
    }
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
      <input type="file" accept=".json,application/json" bind:this={fileInput} hidden onchange={onFile} />
      <label class="small">
        view
        <select bind:value={doc.fidelity} onchange={() => doc.refresh()}>
          <option value="skeleton">skeleton</option>
          <option value="summary">summary</option>
          <option value="full">full</option>
        </select>
      </label>
      <span class="version">v{doc.version}</span>
    </div>
    {#if doc.error}<p class="error">{doc.error}</p>{/if}
    <pre class="view">{doc.view}</pre>
    <p class="hint">Cmd/Ctrl+Z undoes, with Shift redoes. Shift-click a second picture to Link.</p>
    <div class="row">
      <label class="small">
        <input
          type="checkbox"
          onchange={toggleMode}
          disabled={controllerManager.loadingInProgress || pendingSwitch}
          checked={controllerManager.runtimeMode === "worker"}
        />
        {controllerManager.runtimeMode === "worker" ? "Worker mode" : "Main thread mode"}
      </label>
      {#if pendingSwitch}<span class="small">switching…</span>{/if}
      <button class="reset" onclick={resetWorkspace}>Reset layout</button>
    </div>
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
    background: #f3f4f7;
    border: 1px solid #d9dce3;
    border-radius: 6px;
    white-space: pre;
  }
  .version {
    font-size: 11px;
    color: #666;
    margin-left: auto;
  }
  .reset {
    margin-left: auto;
  }
  .warning {
    color: #856404;
  }
</style>
