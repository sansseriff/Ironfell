<script lang="ts">
  import { controllerManager } from "../controller-manager.svelte";
  import Github from "./Github.svelte";

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

  // The model prompt.
  let instruction = $state('');
  let askError = $state('');
  async function send() {
    askError = '';
    if (!instruction.trim()) return;
    try {
      await doc.ask(instruction);
    } catch (e) {
      askError = String(e);
    }
  }

  // Runtime mode toggle
  let pendingSwitch = $state(false);
  async function toggleMode(event: Event) {
    const input = event.currentTarget as HTMLInputElement | null;
    if (!input) return;
    const targetMode = input.checked ? "worker" : "main";
    pendingSwitch = true;
    try {
      await controllerManager.switchMode(targetMode);
    } finally {
      pendingSwitch = false;
    }
  }
</script>

<section class="controls-section">
  <h3>Document</h3>

  {#if controllerManager.showBackendWarning}
    <div class="backend-warning">
      <p class="backend-warning-text">
        <strong>⚠️ WebGL2 Not Available</strong><br />
        This build requires a WebGL2 context. Please check the main panel for
        instructions.
      </p>
    </div>
  {:else if !controllerManager.isInitialized}
    <p>
      {controllerManager.loadingInProgress ? "Loading..." : "Starting up..."}
    </p>
  {:else}
    <div class="row">
      <button onclick={() => doc.download()}>Save</button>
      <button onclick={() => fileInput?.click()}>Load</button>
      <input type="file" accept=".json,application/json" bind:this={fileInput} hidden onchange={onFile} />
      <label class="fidelity">
        view
        <select bind:value={doc.fidelity} onchange={() => doc.refresh()}>
          <option value="skeleton">skeleton</option>
          <option value="summary">summary</option>
          <option value="full">full</option>
        </select>
      </label>
      <span class="version">v{doc.version}</span>
    </div>
    {#if doc.error}
      <p class="error">{doc.error}</p>
    {/if}
    <pre class="view">{doc.view}</pre>
    <p class="hint">Drag the square or the torus. Cmd/Ctrl+Z undoes, with Shift redoes.</p>

    <div class="ask">
      <textarea
        class="prompt"
        rows="3"
        placeholder="Ask the model to change the scene…"
        bind:value={instruction}
        onkeydown={(e) => {
          if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) send();
        }}
      ></textarea>
      <div class="row">
        <button onclick={send} disabled={doc.asking || !doc.apiKey}>{doc.asking ? 'Asking…' : 'Ask'}</button>
        <input
          class="key"
          type="password"
          placeholder="Anthropic API key"
          value={doc.apiKey}
          onchange={(e) => doc.setApiKey((e.currentTarget as HTMLInputElement).value)}
        />
      </div>
      {#if askError}<p class="error">{askError}</p>{/if}
      {#if doc.lastAsk}
        <details class="result" open={!doc.lastAsk.ok}>
          <summary>{doc.lastAsk.ok ? 'applied' : 'not applied'} · {doc.lastAsk.attempts} attempt{doc.lastAsk.attempts === 1 ? '' : 's'} · {doc.lastAsk.transport}</summary>
          {#if doc.lastAsk.note}<p class="note">{doc.lastAsk.note}</p>{/if}
          {#if doc.lastAsk.error}<p class="error">{doc.lastAsk.error}</p>{/if}
          <pre class="edit">{doc.lastAsk.edit ?? ''}{doc.lastAsk.outcome?.failure ? '\n' + doc.lastAsk.outcome.failure : ''}</pre>
        </details>
      {/if}
    </div>

    <div class="mode-toggle-container">
      <label class="mode-toggle-label">
        <input
          type="checkbox"
          onchange={toggleMode}
          disabled={controllerManager.loadingInProgress || pendingSwitch}
          checked={controllerManager.runtimeMode === "worker"}
        />
        <span class="mode-text"
          >{controllerManager.runtimeMode === "worker"
            ? "Worker Mode"
            : "Main Thread Mode"}</span
        >
      </label>
      {#if pendingSwitch}<span class="switching-text">switching...</span>{/if}
    </div>
    <Github></Github>
  {/if}
</section>

<style>
  * {
    color: black;
  }

  h3 {
    margin: 0 0 10px;
    font-size: 16px;
    color: #333;
  }

  .controls-section {
    background-color: var(--body-color);
    padding: 20px;
    border-radius: 12px;
    height: 100%;
    display: flex;
    flex-direction: column;
    gap: 8px;
    box-sizing: border-box;
  }

  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .fidelity {
    font-size: 12px;
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .version {
    font-size: 11px;
    color: #666;
    margin-left: auto;
  }

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

  .hint,
  .error,
  .note {
    margin: 0;
    font-size: 12px;
  }

  .ask {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .prompt {
    width: 100%;
    box-sizing: border-box;
    font: inherit;
    font-size: 12px;
    resize: vertical;
  }

  .key {
    flex: 1;
    min-width: 0;
    font-size: 11px;
  }

  .result summary {
    font-size: 12px;
    cursor: pointer;
  }

  .edit {
    margin: 4px 0 0;
    padding: 6px;
    font-size: 10px;
    background: #f3f4f7;
    border-radius: 4px;
    overflow: auto;
    max-height: 160px;
    white-space: pre-wrap;
  }

  .error {
    color: #b3261e;
  }

  .backend-warning {
    background-color: var(--warning-bg-color);
    border: 1px solid var(--warning-border-color);
    border-radius: 8px;
    padding: 15px;
    margin: 10px 0;
  }

  .backend-warning-text {
    margin: 0;
    color: #856404;
  }

  .mode-toggle-container {
    margin-top: 8px;
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .mode-toggle-label {
    font-size: 12px;
    display: flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
  }

  .mode-text {
    color: black;
  }

  .switching-text {
    font-size: 11px;
    color: #666;
  }
</style>
