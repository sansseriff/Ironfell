# Bevy Web App: Hybrid HTML/Bevy UI Layout Architecture

## Context

We're building a Bevy app targeting WebAssembly. The UI consists of three kinds of panels arranged on screen:

1. **HTML DOM panels** — buttons, forms, static layout chrome
2. **Bevy UI panels** — `bevy_ui` rendered elements
3. **Bevy viewport panels** — actual 3D/2D game rendering via a `Camera`

The question was how to compose these without excessive complexity or GPU overhead.

## Decision

**Use a single full-window `<canvas>` for all Bevy rendering (UI + viewport panels), and overlay real HTML DOM elements on top of it via CSS positioning.** Do not attempt multiple canvases or multiple Bevy `App`/wgpu instances.

## Reasoning

### Why not multiple canvases

Bevy's web/wgpu backend is built around one surface per `App`. Rendering into multiple separate `<canvas>` elements would require either multiple full Bevy `App` instances (duplicated GPU context, duplicated asset loading, painful cross-instance state sync) or working against Bevy's window abstraction in unsupported ways. There's no payoff here — viewports already solve the "multiple independent rendered regions" problem within a single canvas.

### Why one canvas + camera viewports for Bevy content

- Bevy supports multiple `Camera` entities, each with `Camera.viewport` set to a sub-rectangle of the render target. Each camera renders only its own rectangle.
- All Bevy panels (game viewport, Bevy UI panels) live in one canvas, one `App`, one render world. Normal ECS systems can coordinate state across panels directly — no cross-context bridging needed.
- **GPU cost scales with viewport pixel area, not window/canvas size.** A small 800x600 3D viewport inside a 1920x1080 canvas costs roughly what an 800x600 render costs — fragment shading, post-processing, MSAA, etc. all scale with the camera's actual rectangle. Window size only affects the (cheap) final composite/blit and swapchain memory.
- Caveat: any pass applied across the *whole* canvas (e.g. a full-screen post effect) does scale with full canvas resolution — keep such passes scoped to the viewport that needs them, not applied window-wide.

### Why HTML DOM panels should be real DOM, not faked through Bevy

- Don't try to render "through" Bevy or punch holes in pixel data for DOM content. Just position real HTML elements absolutely on top of/around the canvas with CSS (`position: absolute`, `z-index`, etc.).
- Use `pointer-events` deliberately: `auto` on HTML elements that should capture clicks, and let the canvas handle input everywhere else. Areas where HTML sits over otherwise-blank canvas backdrop need no special handling on the Bevy side — Bevy doesn't need to know DOM is occluding it.
- This is the standard pattern for native-feeling form controls (text inputs, dropdowns, etc.) in browser-based engines: HTML for HTML-shaped problems, canvas for rendering-shaped problems.

## Practical Implementation Notes

- **Canvas sizing**: size the canvas to fill the viewport (`100vw` / `100vh` or equivalent). Bevy `Camera.viewport` rects subdivide it for game/Bevy-UI panels.
- **Layout source of truth**: pick one place that owns panel layout (likely Rust-side state), and mirror it out to both camera viewport rects and HTML CSS positions. Avoid having layout logic duplicated independently in JS and Rust — that's the main place this architecture can drift out of sync.
- **Resize handling**: on window resize, in the same tick: resize the canvas → recompute camera viewport rects → resync HTML overlay CSS positions. Doing these out of order or across frames causes visible mismatch for a frame or two.
- **Styling seams**: visual treatments like rounded corners, shadows, or borders around the game viewport are easier to apply via the HTML container framing the canvas than to replicate inside Bevy's render pipeline.
- **Framerate decoupling (if needed later)**: Bevy runs one render pass per app tick for all cameras — there's no built-in per-camera framerate. If some panel needs to visually "update" slower than 60Hz, do it by skipping content updates to that panel's entities/state every N frames (camera still redraws every tick, but nothing changed), or by rendering that panel to an offscreen texture updated on its own cadence and compositing it each frame. True independent GPU presentation rates per panel aren't available since presentation is a property of the single swapchain for the whole canvas.

## Open Questions for Implementation

- How will panel layout be defined/configured (fixed regions vs. dynamic/resizable panels)? This determines whether the "single source of truth" for layout needs to be reactive/live or can be computed once.
- Do any HTML panels need to overlap or blend with Bevy-rendered content (e.g. semi-transparent overlays), or are all HTML/Bevy regions cleanly partitioned? Overlap raises additional z-ordering and event-routing considerations not covered above.
- Is `wasm-bindgen`/`web-sys` already in use for other JS interop, or would this be the first such usage? Affects how layout sync code should be structured.