<script lang="ts">
  // One view in a dock tab: its canvas part (a BevyPanel placeholder whose
  // rectangle Rust draws into) and its HTML part, either or both. The canvas
  // part exists only while the tab is visible, so a hidden tab costs Rust
  // nothing; the HTML part stays mounted and keeps its state.
  //
  // A composition view can show its render inspector beside it, past a
  // draggable divider (plans/specs/render-inspector.md): a second canvas
  // panel, `<id>.inspect`, that watches this one. Both halves are in one
  // tab, so the watched view is always on screen with it. The panel's
  // `inspect` parameter is the view's share of the width, absent when closed.
  // The composition settings button, which toggles it, sits in the tab's
  // top right corner, over whichever half is there.
  import type { DockviewPanelApi } from "dockview-core";
  import { onDestroy, setContext } from "svelte";
  import BevyPanel from "../lib/BevyPanel.svelte";
  import CompositionSettings from "./html/CompositionSettings.svelte";
  import RenderInspector from "./html/RenderInspector.svelte";
  import MissingComposition from "./html/MissingComposition.svelte";
  import FpsCounter from "./html/FpsCounter.svelte";
  import { controllerManager } from "../controller-manager.svelte";
  import { NOTATIONS, type ViewSpec } from "./views";
  import { setInspecting, setShowFps } from "./workspace";
  import ViewStrip from "./ViewStrip.svelte";
  import GroupGrab from "./GroupGrab.svelte";
  import { dockState } from "./dock_state.svelte";
  import { viewStrip } from "./view_strip.svelte";

  let { id, spec, api }: { id: string; spec: ViewSpec; api: DockviewPanelApi } = $props();

  const def = $derived(NOTATIONS[spec.notation]);
  // A composition view onto a composition this document lacks (another
  // scope's tab, a deleted one): said so, with no canvas to draw into.
  const doc = controllerManager.docClient;
  const missing = $derived(
    spec.notation === "composition" && doc.compositionsKnown && !!spec.source && !spec.source.startsWith("inspect:")
      && !doc.compositions.some((c) => c.id === spec.source || c.name === spec.source),
  );
  let visible = $state(api.isVisible);
  const sub = api.onDidVisibilityChange((e) => (visible = e.isVisible));
  const shareOf = (p: unknown) => (p as { inspect?: number } | undefined)?.inspect;
  const fpsOf = (p: unknown) => !!(p as { fps?: boolean } | undefined)?.fps;
  let share = $state(shareOf(api.getParameters()));
  let showFps = $state(fpsOf(api.getParameters()));
  const params = api.onDidParametersChange((p) => {
    share = shareOf(p);
    showFps = fpsOf(p);
  });
  onDestroy(() => {
    sub.dispose();
    params.dispose();
  });

  setContext('iron-view-active', () => visible);

  // A view the workspace's strip offers wears the strip at its left edge
  // (panel-primitives.md §5); its content sits right of it.
  const stopStrip = viewStrip.use();
  onDestroy(stopStrip);
  const stripped = $derived(!!viewStrip.itemOf(spec));
  // Its group's tab bar hidden: a grab handle instead (GroupGrab).
  const bare = $derived(dockState.hidden.includes(api.group.id));

  const inspecting = $derived(spec.notation === "composition" && share !== undefined);
  const inspectId = $derived(`${id}.inspect`);

  let row: HTMLDivElement;
  function drag(e: PointerEvent) {
    const bar = e.currentTarget as HTMLElement;
    bar.setPointerCapture(e.pointerId);
    const move = (m: PointerEvent) => {
      const r = row.getBoundingClientRect();
      share = Math.min(0.85, Math.max(0.15, (m.clientX - r.left) / r.width));
    };
    const up = () => {
      bar.removeEventListener("pointermove", move);
      bar.removeEventListener("pointerup", up);
      setInspecting(id, share);
    };
    bar.addEventListener("pointermove", move);
    bar.addEventListener("pointerup", up);
  }
</script>

<div class="row" class:stripped bind:this={row}>
  {#if stripped}<ViewStrip {api} {spec} />{/if}
  {#if bare && visible}<GroupGrab {api} />{/if}
  {#if missing}
    <MissingComposition {spec} {api} />
  {:else}
  <div class="view" class:hybrid={def.canvas && def.html} style:flex-basis={inspecting ? `${share! * 100}%` : "100%"}>
    {#if def.canvas && visible}
      <div class="canvas-part">
        <BevyPanel {id} kind={def.canvas} composition={spec.source} />
      </div>
    {/if}
    {#if def.html}
      {@const Html = def.html}
      <div class="html-part">
        <Html {spec} panel={id} />
      </div>
    {/if}
    {#if !def.canvas && !def.html}
      <p class="unknown">No notation “{spec.notation}”.</p>
    {/if}
  </div>
  {#if inspecting}
    <div class="divider canvas-ui" role="separator" aria-orientation="vertical" onpointerdown={drag}></div>
    <div class="view hybrid inspector">
      {#if visible}
        <div class="canvas-part">
          <BevyPanel id={inspectId} kind="composition" composition={`inspect:${id}`} />
        </div>
      {/if}
      <div class="html-part">
        <RenderInspector {spec} panel={inspectId} />
      </div>
    </div>
  {/if}
  <!-- Composition settings belong to a canvas the user edits: not to the
       session's Library, which lays out the library's definitions. -->
  {#if spec.notation === "composition" && spec.source !== "library"}
    <CompositionSettings {spec} {inspecting} onInspect={() => setInspecting(id, inspecting ? undefined : 0.5)} {showFps} onShowFps={() => setShowFps(id, !showFps)} />
    {#if showFps && visible}<FpsCounter panel={id} />{/if}
  {/if}
  {/if}
</div>

<style>
  .row {
    position: relative;
    display: flex;
    width: 100%;
    height: 100%;
    box-sizing: border-box;
  }
  .row.stripped {
    padding-left: 40px;
  }
  .view {
    position: relative;
    flex: 0 0 100%;
    min-width: 0;
    height: 100%;
  }
  .inspector {
    flex: 1 1 0;
  }
  .canvas-part,
  .html-part {
    position: absolute;
    inset: 0;
  }
  /* Over a canvas part, HTML takes the pointer only where it draws. */
  .hybrid .html-part {
    pointer-events: none;
  }
  .divider {
    flex: 0 0 1px;
    position: relative;
    background: var(--border-strong);
    cursor: col-resize;
  }
  /* A wider grab area than the line it draws. */
  .divider::before {
    content: "";
    position: absolute;
    inset: 0 -3px;
  }
  .unknown {
    padding: 8px;
    pointer-events: auto;
    background: var(--bg);
  }
</style>
