<script lang="ts">
  // A conversation with the model about the document (source: document,
  // notation: conversation): one instruction, one edit applied.
  import { controllerManager } from "../../controller-manager.svelte";
  import type { ViewSpec } from "../views";

  let { spec }: { spec: ViewSpec } = $props();
  void spec;

  const doc = controllerManager.docClient;
  let instruction = $state("");
  let askError = $state("");
  async function send() {
    askError = "";
    if (!instruction.trim()) return;
    try {
      await doc.ask(instruction);
    } catch (e) {
      askError = String(e);
    }
  }
</script>

<section class="html-view">
  <textarea
    class="prompt"
    rows="4"
    placeholder="Ask the model to change the scene…"
    bind:value={instruction}
    onkeydown={(e) => {
      if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) send();
    }}
  ></textarea>
  <div class="row">
    <button onclick={send} disabled={doc.asking || !doc.apiKey}>{doc.asking ? "Asking…" : "Ask"}</button>
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
      <summary>{doc.lastAsk.ok ? "applied" : "not applied"} · {doc.lastAsk.attempts} attempt{doc.lastAsk.attempts === 1 ? "" : "s"} · {doc.lastAsk.transport}</summary>
      {#if doc.lastAsk.note}<p class="small">{doc.lastAsk.note}</p>{/if}
      {#if doc.lastAsk.error}<p class="error">{doc.lastAsk.error}</p>{/if}
      <pre class="edit">{doc.lastAsk.edit ?? ""}{doc.lastAsk.outcome?.failure ? "\n" + doc.lastAsk.outcome.failure : ""}</pre>
    </details>
  {/if}
</section>

<style>
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
    background: var(--bg-sunken);
    border-radius: 4px;
    overflow: auto;
    max-height: 200px;
    white-space: pre-wrap;
  }
</style>
