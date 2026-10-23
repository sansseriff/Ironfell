<script lang="ts">
  // The render inspector's legend (plans/specs/render-inspector.md §2): what
  // the watched panel's chunks are and how its view culls them, and what the
  // inspector draws. The canvas part under it draws the chunks in their
  // colours, the watched viewport and the dimmed culled area
  // (src/document_bridge/inspector.rs).
  import { onDestroy } from "svelte";
  import { controllerManager } from "../../controller-manager.svelte";
  import type { ViewSpec } from "../views";

  let { spec, panel }: { spec: ViewSpec; panel?: string } = $props();

  const doc = controllerManager.docClient;

  type Info = {
    ok: boolean;
    watching: string | null;
    zoom: number | null;
    chunks: number;
    visible: number;
    nodes: number;
    largest: number;
    recent: number;
    commands: number | null;
    culled: number | null;
  };
  let info = $state<Info | null>(null);
  let options = $state({ tint: true, dim: true, outlines: true });

  async function refresh() {
    if (!panel) return;
    try {
      const r: Info = await doc.call("render_inspector", [panel]);
      info = r?.ok ? r : null;
    } catch {
      info = null;
    }
  }
  refresh();
  const poll = setInterval(refresh, 500);
  onDestroy(() => clearInterval(poll));

  async function toggle(key: keyof typeof options) {
    if (!panel) return;
    options[key] = !options[key];
    await doc.call("render_inspector_options", [panel, JSON.stringify({ [key]: options[key] })]);
  }
</script>

<div class="legend">
  <div class="title">Render inspector <span class="hint">watching {info?.watching ?? spec.source}</span></div>
  {#if info}
    <dl>
      <dt>Chunks</dt><dd>{info.chunks.toLocaleString()} <span class="hint">{info.visible} in view</span></dd>
      <dt>Nodes</dt><dd>{info.nodes.toLocaleString()}</dd>
      <dt>Largest chunk</dt><dd>{info.largest.toLocaleString()} <span class="hint">cost</span></dd>
      <dt>Re-recorded</dt><dd>{info.recent} <span class="hint">last second</span></dd>
      {#if info.commands != null}<dt>Encoded</dt><dd>{info.commands.toLocaleString()} <span class="hint">commands, {info.culled ?? 0} layers culled</span></dd>{/if}
      {#if info.zoom != null}<dt>Zoom</dt><dd>{(info.zoom * 100).toFixed(0)}%</dd>{/if}
    </dl>
  {/if}
  <div class="toggles">
    <label><input type="checkbox" checked={options.tint} onchange={() => toggle("tint")} /> Chunk colours</label>
    <label><input type="checkbox" checked={options.outlines} onchange={() => toggle("outlines")} /> Chunk bounds</label>
    <label><input type="checkbox" checked={options.dim} onchange={() => toggle("dim")} /> Dim culled</label>
  </div>
</div>

<style>
  .legend {
    position: absolute;
    top: 8px;
    left: 8px;
    min-width: 200px;
    padding: 8px 10px;
    border-radius: 6px;
    background: rgba(255, 255, 255, 0.94);
    box-shadow: 0 1px 4px rgba(0, 0, 0, 0.18);
    font: 11px/1.4 var(--ui-font, system-ui, sans-serif);
    color: #222;
    pointer-events: auto;
  }
  .title {
    font-weight: 600;
    margin-bottom: 4px;
  }
  .hint {
    color: #888;
    font-weight: normal;
  }
  dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 1px 10px;
    margin: 0 0 6px;
  }
  dt {
    color: #666;
  }
  dd {
    margin: 0;
    font-variant-numeric: tabular-nums;
  }
  .toggles {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  label {
    display: flex;
    align-items: center;
    gap: 5px;
  }
</style>
