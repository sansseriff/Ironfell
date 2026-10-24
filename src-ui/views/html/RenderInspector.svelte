<script lang="ts">
  // The render inspector's legend (plans/specs/render-inspector.md §2): what
  // the watched panel's chunks are, how its view culls them and which of
  // them it draws from raster tiles, and what the inspector draws. The canvas
  // part under it draws the chunks in their colours, the watched viewport,
  // the dimmed culled area and the raster squares
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
    instance: {
      name: string;
      definition: string;
      inputs: Record<string, string>;
      parts: Part[];
      runs: { class: string; parts: number }[];
    } | null;
    raster: {
      on: boolean;
      scale: number | null;
      tile: number;
      stretched: number;
      live: number;
      split: number;
      tiles: number;
      mb: number;
      budget_mb: number;
      baked: number;
      bake_ms: number;
      live_squares: number;
    };
  };
  let info = $state<Info | null>(null);
  let options = $state({ tint: true, dim: true, outlines: true, tiles: true });

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

  type Part = { name: string; class: "static" | "keyed" | "live"; movable: boolean; reasons: string[] };
  // Consecutive parts of one class with nothing to explain (a dial's sixty
  // tick marks) read as one line.
  function partLines(parts: Part[]): Part[] {
    const out: (Part & { n: number })[] = [];
    for (const p of parts) {
      const last = out[out.length - 1];
      if (last && !p.reasons.length && !last.reasons.length && last.class === p.class && !p.movable && !last.movable) {
        last.n += 1;
        last.name = `${last.name.split(" and ")[0]} and ${last.n - 1} more`;
      } else {
        out.push({ ...p, n: 1 });
      }
    }
    return out;
  }

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
    <div class="title">Raster {#if !info.raster.on}<span class="hint">off: drawn live</span>{/if}</div>
    {#if info.raster.on}
      <dl>
        <dt>Squares</dt>
        <dd>
          <span class="key tile"></span>{info.raster.tile}
          <span class="key stretched"></span>{info.raster.stretched}
          <span class="key live"></span>{info.raster.live}
          <span class="key split"></span>{info.raster.split}
        </dd>
        {#if info.raster.scale != null}<dt>Tile scale</dt><dd>×{info.raster.scale.toFixed(2)}</dd>{/if}
      </dl>
    {/if}
    <dl>
      <dt>Atlas</dt><dd>{info.raster.tiles} tiles <span class="hint">{info.raster.mb} of {info.raster.budget_mb} MB</span></dd>
      <dt>Baked</dt><dd>{info.raster.baked} <span class="hint">{info.raster.bake_ms} ms, last second</span></dd>
      <dt>Live squares</dt><dd>{info.raster.live_squares} <span class="hint">last second</span></dd>
    </dl>
    {#if info.instance}
      <div class="title">{info.instance.name} <span class="hint">{info.instance.definition}, {info.instance.runs.length} runs</span></div>
      <dl>
        {#each Object.entries(info.instance.inputs) as [k, v]}<dt>{k}</dt><dd>{v}</dd>{/each}
      </dl>
      <ul class="parts">
        {#each partLines(info.instance.parts) as p}
          <li title={p.reasons.join("\n")}>
            <span class="class {p.class}">{p.class}{p.movable ? ", moved" : ""}</span>
            {p.name}
            {#if p.reasons.length}<span class="hint">{p.reasons[0]}</span>{/if}
          </li>
        {/each}
      </ul>
    {/if}
  {/if}
  <div class="toggles">
    <label><input type="checkbox" checked={options.tint} onchange={() => toggle("tint")} /> Chunk colours</label>
    <label><input type="checkbox" checked={options.outlines} onchange={() => toggle("outlines")} /> Chunk bounds</label>
    <label><input type="checkbox" checked={options.dim} onchange={() => toggle("dim")} /> Dim culled</label>
    <label><input type="checkbox" checked={options.tiles} onchange={() => toggle("tiles")} /> Raster squares</label>
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
  .parts {
    margin: 0 0 6px;
    padding: 0;
    list-style: none;
    max-width: 320px;
  }
  .parts li {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .class {
    display: inline-block;
    min-width: 44px;
    font-weight: 600;
  }
  .class.static {
    color: rgb(26, 140, 51);
  }
  .class.keyed {
    color: rgb(30, 100, 200);
  }
  .class.live {
    color: rgb(210, 26, 26);
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
  /* The raster squares' outline colours (inspector.rs). */
  .key {
    display: inline-block;
    width: 8px;
    height: 8px;
    margin: 0 3px 0 6px;
    border: 1.5px solid;
    vertical-align: -1px;
  }
  .key:first-child {
    margin-left: 0;
  }
  .key.tile {
    border-color: rgb(26, 166, 51);
  }
  .key.stretched {
    border-color: rgb(255, 153, 0);
  }
  .key.live {
    border-color: rgb(230, 26, 26);
  }
  .key.split {
    border-color: rgb(217, 26, 217);
  }
</style>
