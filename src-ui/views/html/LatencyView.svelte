<script lang="ts" module>
  /** Each tracker: its colour (sRGB, kept apart in hue from the accent blue
   *  of selection chrome so recordings can tell them apart) and its radius
   *  in CSS px. Every tracker is centred on the pointer, so with the pointer
   *  still they are concentric; a lagging one slides off centre while it
   *  moves. scripts/latency/ reads the same table. */
  export const TRACKERS = [
    { key: "square", label: "Document drag (Vello, select tool)", color: "#ffd600", radius: 20, square: true },
    { key: "engine", label: "Engine pointer (Vello overlay)", color: "#ff00aa", radius: 34 },
    { key: "canvas", label: "Canvas2D, drawn in the event", color: "#00e000", radius: 44 },
    { key: "raf", label: "Canvas2D, drawn on rAF", color: "#ff7a00", radius: 54 },
    { key: "svg", label: "SVG, moved in the event", color: "#9a4dff", radius: 64 },
    { key: "konva", label: "Konva, batchDraw", color: "#ff2a2a", radius: 74 },
    // Controls outside Iron: the browser's GPU canvases drawn on rAF, to
    // tell what a GPU canvas costs from what Iron's engine adds.
    { key: "webgpu", label: "WebGPU canvas, rAF (control)", color: "#00e5ff", radius: 84 },
    { key: "webgl", label: "WebGL2 canvas, rAF (control)", color: "#a0ff00", radius: 94 },
  ] as const;
</script>

<script lang="ts">
  // The Latency view (plans/rendering/latency-view.md): a developer view for
  // judging input-to-display latency inside the app, beside whatever other
  // views are open. Its canvas part is the session `latency` composition: a
  // yellow square dragged with the ordinary select tool (input → engine →
  // gesture → document → Vello), and a magenta ring the engine draws at its
  // pointer (src/bevy_app/latency_view.rs). Over them, on the main thread, a
  // Canvas2D ring drawn in the pointer event, another drawn on the next
  // animation frame, and an SVG ring moved in the event. Grab the square at
  // its centre and every shape shares one centre while the pointer is
  // still; while it moves, each lags by its own latency.
  import { getContext, onMount } from "svelte";
  import { Switch } from "bits-ui";
  import Crosshair from "@lucide/svelte/icons/crosshair";
  import { controllerManager } from "../../controller-manager.svelte";
  import type { ViewSpec } from "../views";
  import { konvaTracker, webglTracker, webgpuTracker, type Tracker } from "./latency_trackers";

  let { spec, panel }: { spec: ViewSpec; panel?: string } = $props();

  const doc = controllerManager.docClient;
  const active = getContext<(() => boolean) | undefined>("iron-view-active");
  const STROKE = 3;

  let stage: HTMLDivElement;
  let eventCanvas: HTMLCanvasElement;
  let rafCanvas: HTMLCanvasElement;
  let svgRing: SVGGElement;
  let konvaBox: HTMLDivElement;
  let webgpuCanvas: HTMLCanvasElement;
  let webglCanvas: HTMLCanvasElement;
  let ready = $state(false);
  let error = $state("");
  let continuous = $state(false);
  let stats = $state({ rate: 0, age: 0, raf: 0 });

  /** The panel's id in the engine's protocol; ViewPanel passes it. */
  const id = $derived(panel ?? "");

  async function latency(fields: Record<string, unknown> = {}) {
    return doc.command<{ ok: boolean; ready?: boolean; error?: string; centre?: [number, number] }>({ op: "latency", panel: id, fields: JSON.stringify(fields) });
  }

  /** Put the square in the middle of the view; wait for the composition and
   *  the panel to exist first (the engine builds it on the first call). */
  async function recentre() {
    error = "";
    for (let i = 0; i < 100; i++) {
      if (!stage) return;
      const r = stage.getBoundingClientRect();
      try {
        const answer = await latency({ place: [Math.round(r.width / 2), Math.round(r.height / 2)] });
        if (!answer.ok) throw new Error(answer.error ?? "latency failed");
        if (answer.ready) {
          ready = true;
          doc.setTool("select");
          return;
        }
      } catch (e) {
        error = String(e);
        return;
      }
      await new Promise((r) => setTimeout(r, 100));
    }
    error = "The latency composition did not appear.";
  }

  async function setContinuous(next: boolean) {
    try {
      const r = await doc.command<{ ok: boolean; error?: string }>({ op: "set_run_mode", source: spec.source, mode: next ? "always" : "auto" });
      if (!r.ok) throw new Error(r.error ?? "Could not change run mode");
      continuous = next;
    } catch (e) {
      error = String(e);
    }
  }

  $effect(() => {
    // Again whenever another document replaces this one (a scope switch):
    // the session composition goes with the old store.
    const generation = doc.generation;
    if (controllerManager.isInitialized && id && generation >= 0) void recentre();
  });

  function ring(ctx: CanvasRenderingContext2D, x: number, y: number, r: number, color: string) {
    ctx.clearRect(0, 0, ctx.canvas.width, ctx.canvas.height);
    if (Number.isNaN(x)) return;
    const k = ctx.canvas.width / Math.max(1, ctx.canvas.clientWidth);
    ctx.beginPath();
    ctx.arc(x * k, y * k, r * k, 0, Math.PI * 2);
    ctx.lineWidth = STROKE * k;
    ctx.strokeStyle = color;
    ctx.stroke();
  }

  onMount(() => {
    const [, , canvasT, rafT, , konvaT, webgpuT, webglT] = TRACKERS;
    // Konva and the GPU controls; a missing API leaves its ring out.
    const extra: Tracker[] = [konvaTracker(konvaBox, konvaT.color, konvaT.radius, STROKE)];
    const gl = webglTracker(webglCanvas, webglT.color, webglT.radius, STROKE);
    if (gl) extra.push(gl);
    let alive = true;
    void webgpuTracker(webgpuCanvas, webgpuT.color, webgpuT.radius, STROKE).then((t) => {
      if (!t) return;
      if (!alive) return t.destroy();
      t.resize(rect.width, rect.height, window.devicePixelRatio || 1);
      extra.push(t);
    }).catch(() => {});
    const ectx = eventCanvas.getContext("2d")!;
    const rctx = rafCanvas.getContext("2d")!;
    let rect = stage.getBoundingClientRect();
    // Canvas backing stores in device px so the rings are as sharp as the
    // engine's; the rings themselves are sized in CSS px.
    const size = () => {
      rect = stage.getBoundingClientRect();
      const dpr = window.devicePixelRatio || 1;
      for (const c of [eventCanvas, rafCanvas]) {
        c.width = Math.max(1, Math.round(rect.width * dpr));
        c.height = Math.max(1, Math.round(rect.height * dpr));
      }
      for (const t of extra) t.resize(rect.width, rect.height, dpr);
    };
    size();
    const observer = new ResizeObserver(size);
    observer.observe(stage);

    let x = NaN;
    let y = NaN;
    let drawn = true;
    let events = 0;
    let ages: number[] = [];
    let frames = 0;
    const move = (event: PointerEvent) => {
      const list = event.getCoalescedEvents?.();
      const e = list?.length ? list[list.length - 1] : event;
      // Window-relative layout can move under us (dock splits), so measure
      // on every event; it is one rect read.
      rect = stage.getBoundingClientRect();
      const lx = e.clientX - rect.left;
      const ly = e.clientY - rect.top;
      const inside = lx >= 0 && ly >= 0 && lx <= rect.width && ly <= rect.height;
      x = inside ? lx : NaN;
      y = inside ? ly : NaN;
      events++;
      ages.push(performance.now() - event.timeStamp);
      // In the handler, no Svelte update and no frame wait.
      ring(ectx, x, y, canvasT.radius, canvasT.color);
      svgRing.style.visibility = inside ? "visible" : "hidden";
      svgRing.setAttribute("transform", `translate(${lx} ${ly})`);
      for (const t of extra) t.move(x, y);
      drawn = false;
    };
    let raf = 0;
    // Experiment (plans/rendering/latency-view.md §7): `?latencyBusy=N` keeps the
    // main thread busy N ms in each frame, delaying the page's own frame.
    const busy = Number(new URLSearchParams(location.search).get("latencyBusy") ?? 0);
    const frame = () => {
      frames++;
      if (busy > 0) {
        const until = performance.now() + busy;
        while (performance.now() < until) { /* spin */ }
      }
      if (!drawn) {
        ring(rctx, x, y, rafT.radius, rafT.color);
        drawn = true;
      }
      raf = requestAnimationFrame(frame);
    };
    raf = requestAnimationFrame(frame);
    let last = performance.now();
    const tick = setInterval(() => {
      const now = performance.now();
      const dt = (now - last) / 1000;
      last = now;
      ages.sort((a, b) => a - b);
      stats = { rate: Math.round(events / dt), age: ages.length ? ages[ages.length >> 1] : 0, raf: Math.round(frames / dt) };
      events = 0;
      frames = 0;
      ages = [];
    }, 500);
    window.addEventListener("pointermove", move, { capture: true, passive: true });
    return () => {
      alive = false;
      for (const t of extra) t.destroy();
      observer.disconnect();
      cancelAnimationFrame(raf);
      clearInterval(tick);
      window.removeEventListener("pointermove", move, true);
    };
  });
</script>

<div class="latency" bind:this={stage} data-latency-view data-ready={ready} data-visible={active?.() ?? true}>
  <canvas class="layer" bind:this={webglCanvas} aria-hidden="true"></canvas>
  <canvas class="layer" bind:this={webgpuCanvas} aria-hidden="true"></canvas>
  <div class="layer" bind:this={konvaBox} aria-hidden="true"></div>
  <canvas class="layer" bind:this={rafCanvas} aria-hidden="true"></canvas>
  <canvas class="layer" bind:this={eventCanvas} aria-hidden="true"></canvas>
  <svg class="layer" aria-hidden="true">
    <g bind:this={svgRing} style:visibility="hidden">
      <circle r={TRACKERS[4].radius} fill="none" stroke={TRACKERS[4].color} stroke-width={STROKE} />
    </g>
  </svg>
  <div class="card canvas-ui">
    <div class="head">
      <strong>Latency</strong>
      <button class="ui-button is-small is-outline" disabled={!controllerManager.isInitialized} onclick={recentre} title="Put the square back in the middle">
        <Crosshair size={13} strokeWidth={1.75} /> Recentre
      </button>
    </div>
    <ul class="legend">
      {#each TRACKERS as t (t.key)}
        <li><span class="swatch" class:square={"square" in t} style:border-color={t.color}></span>{t.label}</li>
      {/each}
    </ul>
    <div class="setting">
      <span>Continuous rendering</span>
      <Switch.Root class="ui-switch" checked={continuous} disabled={!ready} onCheckedChange={setContinuous} aria-label="Continuous rendering">
        <Switch.Thumb class="ui-switch-thumb" />
      </Switch.Root>
    </div>
    <p class="readout" data-latency-stats>
      {stats.rate} events/s · event age p50 {stats.age.toFixed(1)} ms · rAF {stats.raf} Hz
    </p>
    <p class="note" class:error={!!error} role={error ? "alert" : "status"}>
      {error || (ready ? "Grab the square at its centre and drag. Lag shows as rings sliding off centre." : "Preparing…")}
    </p>
  </div>
</div>

<style>
  .latency {
    position: absolute;
    inset: 0;
    overflow: hidden;
    pointer-events: none;
  }
  .layer {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
  }
  svg.layer {
    overflow: visible;
  }
  .card {
    position: absolute;
    top: 8px;
    left: 8px;
    width: 250px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--fg);
    font-size: 12px;
    box-shadow: var(--menu-shadow);
    pointer-events: auto;
  }
  .head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .head .ui-button {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .legend {
    margin: 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 3px;
    color: var(--fg-muted);
  }
  .legend li {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .swatch {
    flex: none;
    width: 10px;
    height: 10px;
    border: 2px solid;
    border-radius: 50%;
  }
  .swatch.square {
    border-radius: 2px;
    background: #ffd600;
  }
  .setting {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }
  .readout {
    margin: 0;
    font-variant-numeric: tabular-nums;
    color: var(--fg-muted);
  }
  .note {
    margin: 0;
    font-size: 11px;
    color: var(--fg-faint);
  }
  .note.error {
    color: var(--danger);
  }
</style>
