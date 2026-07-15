# Single-Canvas Migration (Plan A)

Supersedes the multi-canvas implementation. One full-window `<canvas>`, one Bevy `Window`,
camera viewports for Bevy panels, HTML overlaid via CSS. DOM owns layout; Bevy consumes
rectangles.

## Architecture

### Rust
- **One window.** `canvas_view` collapses to a single-window bootstrap (no `CanvasViews`
  map, no `WindowId`, no `CanvasName`). The window is `PrimaryWindow`, scale factor forced
  to 1.0, resolution = physical canvas pixels. All coordinates Rust-side are physical px.
- **`bevy_vello` unforked** — crates.io `0.10.3` (single `SSRenderTarget`, full-window
  texture, one `VelloView` camera). The git submodule fork is removed.
- **Panels resource** (`src/panels.rs`): `Panels: HashMap<String, Panel>` where
  `Panel { kind, rect }` and rect is physical px, top-left origin. Upserted from JS.
  - `viewer` panel → `MainCamera3D.viewport` (camera inactive until the panel exists).
  - `timeline` panel → timeline vello systems draw into that rect (screen space + clip).
  - Overlay content is drawn in "panel world" coords (y-up, origin = viewer panel center)
    and mapped to screen space with a single affine; clipped to the panel rect.
- **Cameras** (all target the one window):
  - `order -10`: background `Camera2d`, `RenderLayers::none()`, clears full window to bg.
  - `order 0`: `MainCamera3D` with `Camera.viewport` = viewer rect (viewport-scoped clear).
  - `order 10`: vello `Camera2d`, full-window, `ClearColorConfig::None`, `VelloView`,
    `RenderLayers::layer(1)`, `IsDefaultUiCamera` (hosts the FPS overlay UI).
- **FFI protocol** (replaces per-canvas window creation):
  - `create_window_by_offscreen_canvas(ptr, canvas, scale_factor)` — once, the full-window
    canvas.
  - `resize(ptr, w, h)` — full-window canvas size (physical px).
  - `set_panel_viewport(ptr, id, kind, x, y, w, h)` — upsert a panel rect (physical px).
  - `despawn_panel(ptr, id)` — remove a panel.
  - Input FFI unchanged; all picking maps window coords → per-panel/viewport coords in Rust.

### TypeScript / Svelte
- **Full-window canvas** at z-index 0, never resized during pane drags (only on window
  resize). SplitPane layout floats above it with `pointer-events: none` at the container
  level; HTML panels opt back in with `pointer-events: auto`.
- **`BevyPanel.svelte`**: placeholder div occupying a pane; on mount registers
  `id`/`kind` with the panel manager; a `ResizeObserver` posts rect changes (device px)
  as `setPanelViewport`; on destroy posts `despawnPanel`.
- **`panel_manager.ts`** (replaces `canvas_manager.ts`/`canvas_control.ts`): owns the
  session, the canvas element, panel registry, and mode switching.
- **Session protocol** (worker.ts and main-thread-adapter.ts implement identically):
  `wasmData`, `init` (canvas), `resize`, `setPanelViewport`, `despawnPanel`,
  `startRunning`/`stopRunning`, input messages, inspector messages, `releaseApp`.

### Mode switch (worker ↔ main), clean lifecycle
Fixes for the observed bugs:
1. **Stale input binding** (mouse dead after switch): `InputManager` now removes its
   listeners on dispose and is re-bound to the recreated canvas after every switch.
2. **Resolution wrong after switch**: after `enginePrepared`, the manager
   deterministically re-sends (a) the full-window canvas size and (b) every registered
   panel rect. No reliance on queued/debounced resizes.
3. **Leaked app in main mode**: `release_app(ptr)` is called on dispose of the
   main-thread adapter (worker mode: `terminate()` reclaims the context).
4. **Main-mode input latency**: main mode calls wasm FFI synchronously in the event
   handler (no postMessage hop, no buffering) — event → `mouse_move` → same-frame RAF
   render. Worker mode keeps the buffered `enter_frame_with_mouse` batching.

## Deleted
- `src/bevy_vello/` submodule (fork) — replaced by crates.io dep.
- `src/canvas_view/canvas_views.rs`, `CanvasName`, window-matching/init-order logic.
- `create_window_by_offscreen_canvas_with_id`, per-canvas `resize(canvasId, …)`.
- `src-ui/canvas_manager.ts`, `src-ui/canvas_control.ts`, `src-ui/control.svelte.ts`,
  `src-ui/runtime/resize_manager.ts`, `src-ui/lib/Scene.svelte`.

## Later (enabled, not built now)
- Dynamic panel factory: `set_panel_viewport` is already id+kind-keyed; a Rust factory
  can spawn cameras/scenes per kind on first sight of a new id.
- Widget tier: in-scene vello UI entities with interaction components (no per-widget
  windows/targets).
