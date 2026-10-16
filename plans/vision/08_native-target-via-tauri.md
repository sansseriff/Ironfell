# A native target via Tauri

**Status:** macOS experiment in progress, 2026-10-16. Standalone composition
verified in dev and packaged builds; real shared-engine workspace passes the
first automated native checks. Remaining feasibility gates are being tested. A full port and additional platforms
remain unscheduled. Work order lives in [`handoff-product.md`](../handoff-product.md)
§4.1.
**Created:** 2026-09-06. **Revised:** 2026-10-16 after source review and discussion
with the user.

**Related proposal (pending approval):**
[`../specs/model-interop.md`](../specs/model-interop.md) defines computational
Model jobs, live sessions and optional local acceleration. Its bulk-data path is
independent of the shell's UI protocol. No Model interop implementation is
authorized until the user approves that proposal.

## 1. Purpose and constraints

Add native Rust execution alongside the WebGPU and WebGL2 web builds: Bevy and
Vello run natively, while the existing Svelte shell runs in a Tauri webview.
Desktop integration and eventual local agent tools are additional motivations;
performance improvement is a hypothesis to measure, not an acceptance assumption.

The user's central requirement is maintainability: **one product implementation,
with a small, explicit platform boundary**. A new editing, rendering or inspector
feature should normally be implemented once and work on every target. Native
must not acquire a separate document model, tool system, UI or renderer fork.

The current [rendering architecture](../rendering/architecture.md) governs both
targets. Native does not replace caching, permit a Vello fork, or relax the web
targets' contracts. Caching is not a technical prerequisite for testing window
composition. Both web backends already have Cargo features; preserving them is
a regression requirement, not work awaiting a separate branch.

## 2. The deviation boundary

```text
shared Svelte views / DocumentClient / semantic input and panel layout
    -> runtime adapter (worker | main | native)
    -> shared Rust commands, queries and outbound events
    -> iron_document + solver + interaction + Bevy projection + Vello
    -> host-provided surface and frame driver
```

The diagram describes ownership, not new required crates or final API names.
Keep interfaces small and derived from existing operations. Extract only what
the experiment needs; do not build a general application framework first.

| Area | Shared implementation | Permitted platform-specific code |
|---|---|---|
| Document and session | `iron_document`, schemas, ids, constraints, physics, previews, history, serialisation | None; host supplies bytes and requests |
| Engine operations | Commands, queries, validation, version checks, errors/results and outbound event payloads | wasm-bindgen and Tauri wrappers decode/encode and enqueue |
| Interaction | Picking, manipulators, selection, tool state machines, shortcuts' semantic actions | Input acquisition, focus/capture and coordinate normalisation at the boundary |
| Rendering | Reconciliation, display lists, per-view textures, Vello, final Bevy composition, invalidation | Native/window or browser/canvas handle, surface configuration, device lifecycle |
| Shell | Svelte views, dock layout, inspector, document client and read projections | Runtime construction, transport, host-service adapter and surface attachment |
| Scheduling | Engine tick semantics, dirty/wake reasons, playhead and render-on-demand decisions | Browser RAF or native wake/presentation integration, suspend/resume and shutdown |
| Host services | Capability requests and results used by the shell/engine | Clipboard, file dialogs, bundled asset access, cursor and OS integration |

No `tauri`, `web_sys`, `js_sys` or platform-specific conditionals in document,
solver or tool semantics. Existing leaks must move behind the boundary as their
paths are enabled natively. Platform dependencies and `cfg` belong in host
modules/build configuration. Renderer capability differences belong in the
existing backend policy, not scattered `if native` branches.

Review each native change against this table. If it copies a command handler,
gesture algorithm, draw-list walk or Svelte view, extract the shared behavior
instead. Record every remaining platform exception with its file, reason and
test; a successful demo with duplicated semantics does not satisfy this plan.

## 3. Native host and protocol contract

### Window, GPU and event loop

The preferred experiment is **one native window with a native GPU surface under
a transparent webview**. Tauri supplies the native window handle; it does not
supply a pre-existing wgpu renderer. Bevy owns the engine's renderer/device and
Vello uses that device as it does today. The OS composites the webview above it;
this does not imply shared GPU textures with the webview.

Tauri owns the OS window/webview/event loop, particularly the macOS main-thread
lifecycle. Feed its native handle into Bevy's external-window bootstrap; do not
introduce a second winit window or competing Bevy window event loop. Keep the
handle owner alive until rendering has shut down. A raw-wgpu probe may establish
composition, but must be replaced by Bevy for the engine gate, rather than both
renderers presenting to the same surface.

Start with the engine in the Tauri host process. One owner serializes access to
the Bevy world/store and drains incoming commands at defined tick boundaries;
Tauri handlers must not independently mutate the world. Whether that owner ticks
on the main thread or a dedicated thread is a measured spike result, subject to
platform surface/thread constraints. ECS worker tasks are a separate choice.
Native does not automatically enable safe multithreading: audit Cargo features
and non-Send resources before enabling it.

Do not copy the examples' `run_iteration`/sleep loop or tick Bevy from JS RAF.
Use supported Tauri event-loop integration and a native frame/wake driver. Keep
render-on-demand; wake for input, commands, resize/expose, assets, animation and
surface recovery, and stop unnecessary work when idle/minimised. Document how
Bevy's existing manual `frame()` lifecycle and any pipelined rendering interact.

### Requests, events and lifecycle

- For shell UI traffic, JS -> Rust uses Tauri commands; Rust -> JS uses Tauri
  channels for updates.
  A channel is not a bidirectional replacement for `postMessage`. In-process
  Rust queues connect transport to the engine owner; no socket is required for
  this UI connection. External computational Models have a separate data path:
  their sockets terminate at the engine, rather than forwarding bulk results
  through the webview. The proposed tier/API contract is in
  [`model-interop.md`](../specs/model-interop.md), pending approval.
- All runtimes use the same semantic requests, responses and events. Use
  request correlation, session generations, document-version checks, errors and
  disposal semantics. Applying a transaction and presenting its pixels are
  distinct acknowledgments. No wasm pointers or native handles cross this API.
- Input and document commands have defined ordering. Coalesce replaceable pointer
  positions without dropping/reordering down/up, modifier or transaction edges;
  include the current position with a press. Bound pending work so motion cannot
  build an unbounded queue. A stale session cannot publish into its replacement.
- Native replies/events feed the same `DocumentClient` and Svelte projections;
  the shell is not a second document authority. Every authored mutation remains
  an op in an `iron_document` transaction; previews/session changes retain their
  existing store APIs. Clipboard and file access change hosts, not semantics.
- Implement the necessary subset of the existing
  [typed UI protocol plan](../specs/infinite-canvas-architecture-patterns.md#5-phase-2--typed-ui-protocol-subscriptions-and-lifecycle).
  Extend that contract to native; do not invent a parallel protocol or require
  all subscription optimisations before the first experiment.

### Input and layout

Prefer retaining DOM input acquisition over a transparent viewport hit region,
forwarding to the shared engine input path, as the web build already does.
Transparent pixels do not imply native click-through. Buttons, text fields,
popovers and viewport gestures must have explicit ownership; prevent duplicate
delivery if native OS input is also used. Keep a drag captured through release
or cancellation, including focus loss and leaving the viewport/window.

Retain the engine's physical-pixel coordinate convention. Normalize CSS pointer
positions and panel rectangles once at the host boundary, with an explicit
scale/layout generation; never apply both native DPI scaling and the existing
web conversion. Resize, dock layout and DPI changes must reach the engine in
order. Use one full-window native surface and the existing per-view textures,
panel rectangles and compositor; do not create one OS surface per dock panel.

### Window-adapter responsibilities and native concurrency

Adapting `canvas_view` means registering an externally owned surface with Bevy,
not implementing a replacement OS window manager. Tauri/Tao/Wry retain native
window creation and platform/webview lifecycle; DOM input still enters the shared
semantic path. Inventory the Iron glue for raw-handle ownership, Bevy window
messages, focus/capture cancellation, resize/DPI, outgoing host requests and the
native frame/wake driver. Ordinary Bevy apps use `bevy_winit` for this integration;
plugins that require its `WinitWindows` are not automatically compatible here.
If the glue grows into a general windowing layer, report that result and assess
winit plus a Wry child webview before expanding it. The Model proposal's §8
records the rationale and the suggested initial macOS thread arrangement.

Native multithreading is a requirement for high-count runtime simulation, even
when the authored document is small. Enable `bevy/multi_threaded` in the native
feature graph and preserve task pools and pipelined rendering. Verify the custom
`WorkerApp::frame()` lifecycle against `RenderExtractApp`, in-flight rendering,
idle/wake and shutdown; disabling the render thread to make the host work would
not meet that goal. Dense simulation state belongs in runtime ECS, not one
document node or transaction per particle. Browser Bevy multithreading is not a
prerequisite for the shared product/API.

## 4. What must be extracted from today's code

The seams exist but are not yet transport-independent. Audit at the selected
commit before editing; this list is a source review, not a successful native build.

| Current coupling | Required boundary change |
|---|---|
| `src/web_ffi.rs` owns command behavior as well as wasm conversion and raw app pointers | Extract callable Rust operations once; keep wasm and Tauri entry points as thin adapters |
| `bevy_app/interaction.rs` builds JS arrays; `select.rs`, `pen.rs` and `document_bridge/mod.rs` call `web_ffi` notifications | Emit Rust event payloads/outbox; host adapters deliver them to JS |
| `SessionAdapter` constructs `WasmLoader` and requires `attachCanvas`; `AdapterBridge` imports the wasm main-thread adapter | Separate runtime creation and surface attachment from shared request/lifecycle logic; native must boot without loading wasm |
| `InputManager` attaches to the canvas; panels are measured in the DOM | Preserve shared semantic input/layout code with a replaceable hit region and host coordinate conversion |
| `canvas_view` wraps browser handles and `init_app` assumes canvas bootstrap | Generalise external-surface ownership/bootstrap; keep Tauri types out of core rendering/interaction |
| `.cargo/config.toml` defaults to wasm; root is `cdylib`; `lib.rs` requires a web backend; web dependencies are unconditional | Explicit native target/build entry, linkable shared library as needed, target-scoped dependencies and appropriate backend guards |
| Browser asset loading and web runtime services | Verify native compilation of asset modules and packaged resource lookup; do not assume existing native stubs/dependencies work |

Align with the current Bevy 0.19 / wgpu 29 dependency graph; the 0.18 example is
reference code, not a compatible dependency. Avoid unrelated renderer upgrades.
Keep wasm artifact loading out of the native frontend boot path and native-only
imports out of browser entries. The ordinary web build must not depend on a Tauri
runtime; DOM/Svelte code remains shared with the native webview.

## 5. Prior art and remaining platform questions

Source/issue review on 2026-10-15–16; none of these demos has been run locally for
Iron. Pin exact revisions when reproducing them and record the environment.

| Reference | Evidence and limits |
|---|---|
| [sunxfancy/BevyTauriExample](https://github.com/sunxfancy/BevyTauriExample) | Native Bevy with Tauri-owned window; original Bevy 0.15.1. Has macOS webview visibility and input reports |
| [4t145/BevyTauriExample](https://github.com/4t145/BevyTauriExample) | Bevy 0.18 / wgpu 27 port; demonstrates external handles and manual render setup, but retains the `run_iteration` runner |
| [FabianLars/tauri-v2-wgpu](https://github.com/FabianLars/tauri-v2-wgpu) | Minimal native surface created from a Tauri window |
| [clearlysid/tauri-wgpu-cam](https://github.com/clearlysid/tauri-wgpu-cam) | Web UI above native camera rendering; author reports Windows/macOS success |
| [Wry wgpu example](https://github.com/tauri-apps/wry/blob/dev/examples/wgpu.rs) | Transparent child webview over native wgpu; uses Wry/winit directly, not the full Tauri runtime; explicitly excludes Wayland |
| [langyo/native-bevy-with-tauri-hud-demo](https://github.com/langyo/native-bevy-with-tauri-hud-demo) | Older stacked-window alternative, not the preferred architecture |

The former blanket blockers are superseded:

- [tauri#12450](https://github.com/tauri-apps/tauri/issues/12450) is closed with a
  Windows solution involving the parent Bevy window's `clip_children: false`.
  It does not establish macOS support or require this setting on other hosts.
- [tauri#3662](https://github.com/tauri-apps/tauri/issues/3662) is closed, not an
  open feature dependency. Closure alone is not evidence of support.
- [tauri#9220](https://github.com/tauri-apps/tauri/issues/9220) documents Linux GTK
  and wgpu competing for the window surface. Wry's child-window path differs;
  do not generalise macOS/Windows evidence to X11 or Wayland.
- [Bevy example macOS issue](https://github.com/sunxfancy/BevyTauriExample/issues/3)
  implicates its runner. Tauri's
  [`run_iteration`](https://docs.rs/tauri/latest/tauri/struct.App.html#method.run_iteration)
  is deprecated with a busy-loop warning. A supported runner remains to be proven.
- [bevy#17686](https://github.com/bevyengine/bevy/issues/17686) documents webview
  focus/input interception. Input routing is a first-class gate, not inferred
  from visual transparency.

The initial test scope is macOS on the development machine only. Report Windows,
Linux X11, Linux Wayland and mobile as untested unless actually tested. Keep any
OS-specific layer setup behind the host surface adapter. If macOS requires a
large native compositor, separate synchronized top-level windows, engine fork or
pixel transfer through JS, report that result before expanding the architecture.

## 6. Feasibility gates and evidence

The following are exit criteria, not claims of completion. The handoff owns work
order and the timebox. The composition gate can use a minimal standalone probe;
passing the shared-engine gate requires real Iron code.

| Gate | Evidence required |
|---|---|
| Native composition | One native window, wgpu then Bevy content beneath interactive HTML; correct transparency/alpha and menu layering; no per-frame image transfer through IPC. Verify both dev and packaged macOS builds |
| Shared engine | Real Iron 2D Vello content and a 3D view; dock resize/split/tab changes, pan/zoom and embed placement/clipping; same document and rendering pipeline as web |
| Native concurrency | Resolved native dependency graph enables `multi_threaded`; traces show independent ECS work on multiple threads and rendering overlapping simulation. A small authored declaration drives a much larger runtime ECS population; a stalled webview does not stop it. Record native GPU backend/features/limits and compare the same workload in a conventional Bevy/winit host. Surface work, idle/wake and shutdown remain correct with pipelined rendering |
| Input and coordinates | Inspector text focus vs shortcuts, pointer/scroll/modifiers, drag capture/release/cancel, focus loss; display/DPI changes without jumps or double scaling; no duplicate input |
| Document parity | Same fixture and command trace through web/native: apply/reject, selection/session, preview/commit, undo/redo and save/reload. Compare canonical authored state and semantic results; a real pointer/keyboard gesture in the native app must also drive a drag, beyond injected protocol requests |
| Lifecycle and scheduling | Init/ready, resize, minimise/restore/expose, close, runtime replacement and outstanding-request disposal; no stale events/handles; idle GPU work stops and wake/animation resumes correctly |
| Maintainability | File-level inventory of platform adapters and every exception to §2; behavior handlers shared; no Tauri types in document/solver/tools and no native wasm load. Report native-only code added and shared code extracted separately |
| Regression and cost | Both wasm backend builds; existing native store tests and run-iron smoke for behavior changes; packaged asset/clipboard round trip. Record comparable frame CPU, input latency, idle CPU/GPU activity and texture memory for a fixed fixture/size/DPR |

For each gate record pass/fail/untested, reproducible commands, git/dependency
revisions, OS/GPU/display scale, screenshots/logs and remaining failures here.
Declare visual/float tolerances where necessary; do not compare unstable GPU
handles or demand byte-identical screenshots across backends. Canonical document
state and transaction semantics must still match. Use existing smoke scenarios
rather than maintaining a second list of native-only product semantics. A shared
command trace complements, rather than replaces, native focus/input tests.

A minimal composition success is not a completed native port. A failure is useful
if it identifies the failing platform boundary without spreading changes through
the application. Preserve the evidence and functioning web builds.

## 7. Scope after feasibility

Local agent capabilities are a later integration: a replaceable harness process
can call the same shared document commands/views via a bridge or MCP. Agent edits
retain transaction/version/gesture rules. Filesystem, shell, browser and OS tools
belong to host capabilities, not document or rendering code. Do not bundle an agent
harness or redesign credentials/storage during the rendering experiment.

Running existing wasm inside Tauri can test desktop host services, but does not
pass the native-rendering gate. It retains browser rendering constraints while
still allowing Rust-side desktop services; it is an optional control, not a
substitute for the proposed native engine.

### Completion record

- **2026-10-16 — composition and shared-engine slice:** native host in `src-native/`
  uses Tauri 2.11.6 / Tao 0.35.3 / Wry 0.55.1, the existing Bevy 0.19.1 /
  wgpu 29.0.4 renderer, and an NSView `CADisplayLink` on the OS main thread.
  Tauri owns the single window/event loop; Bevy owns the Metal device/surface.
  No winit runner, engine fork, second compositor or per-frame IPC image transfer.
  The standalone probe passed 11 automated checks in both ordinary and officially
  packaged runs, including 90 native frames during a 1.5-second JS stall,
  multiple ECS worker IDs, physical resize, pause/wake and minimise/restore.
  Real keyboard input, HTML controls and clean Cmd+Q were also driven through
  macOS UI automation. Evidence: `.agents/native-run-2.log`,
  `.agents/native-packaged.log`, `.agents/native-packaged-composition.png`.
- **2026-10-16 — initial real-workspace checks:** `native-workspace-run-2.log`
  passes 14 checks: shared Svelte workspace, native canvas without browser GPU,
  shared edit parser / transactions, rejected edit atomicity, keyboard undo/redo,
  canonical save/reload, independent native ticks through a JS stall, zero extra
  render submissions while idle, resize, minimise/restore and no page errors.
  This is initial evidence, not completion of every gate below. Bevy's render-time
  receiver was incompatible with optional render frames: the shared engine now
  uses its own monotonic clock. Its diagnostic FPS text no longer wakes an idle
  renderer. Native rendering remains pipelined.
- **2026-10-16 — packaged workspace and concurrency:** the official unsigned
  dev-opt app passes 15 workspace checks (`.agents/native-packaged-workspace.log`)
  and 17 checks with the opt-in 100,000-runtime-entity fixture
  (`.agents/native-runtime-simulation.log`). One authored rectangle changes its
  simulation parameter; particles do not enter the authored document. The bounded
  trace records 428 intervals of parallel ECS work and 423 intervals where render
  work overlaps simulation on different threads. The webview's 1.5-second stall
  spans 102 native frames. This establishes actual native concurrency, not a
  throughput ceiling or GPU completion timing. The conventional winit comparison
  remains untested. GPU features/limits are recorded in both logs (Apple M5/Metal).
- **2026-10-16 — coordinates and direct motion:** the window root and WKWebView
  frame include title-bar space that the HTML viewport excludes. A sibling GPU
  NSView now intersects the webview frame with `contentLayoutRect`; backing size
  matches the DOM (1800×1136 at 900×568 CSS pixels, DPR 2 after resize). A local
  AppKit event monitor samples pointer motion directly for Bevy and returns every
  event unchanged. HTML retains button, wheel, keyboard and text ownership; DOM
  motion is suppressed only when the native monitor is active. Native presentation
  queue depth defaults to one. `IRON_NATIVE_JS_POINTER=1` and
  `IRON_NATIVE_FRAME_LATENCY=2` provide controls for comparison. Human pointer
  alignment/feel and display-crossing validation remain required; no event-to-photon
  latency measurement has been made. A short-drag ordering fix lets tools show the
  final held pose before a routed release when both arrive in one tick. Real
  native drag changed the square from (500,420) to (600,480) in one transaction;
  Cmd+Z restored it. HTML inspector typing changed x to 610, then undo restored
  500. Evidence: `.agents/native-input-trace.log` and macOS UI automation.
- **2026-10-16 — regressions:** both wasm backends compile; frontend checking has
  zero errors (eight existing Svelte warnings); mailbox lifecycle/bounds tests pass.
  The full run-iron smoke passes against the static WebGL2 preview (including the
  Figma SVG fixture). Native baseline: iron_document 122, spatial 5 and SVG 15 tests
  pass. Solver baseline has 8 passes and the existing
  `three_link_chain_reaches_a_target` failure; no solver changes were made.
- **2026-10-16 — human pacing report and follow-up:** user reported a translucent
  title bar and latency that rises/falls over several seconds. The title bar was
  backed by a clear NSWindow: only the WKWebView requires transparency. The host
  now sets an opaque NSWindow/background, alpha 1 and normal title-bar drawing;
  transparent webview/native content composition remains visible. Current user
  document v34 was saved through the actual Save button before restarting tests
  (`Downloads/iron-document (1).json`, backup `.agents/native-user-session-v34.json`).
  At the start of this investigation, scheduling was view CADisplayLink plus
  Metal FIFO/VSync, latency 1;
  the callback's timestamp/target were previously ignored. New opt-in tracing
  records callback, host, drawable acquisition and CPU render/present-return
  intervals. Native GPU completion/scanout are **not** measured.

  Four 25-second runs on the 60 Hz screen used the same 2560×1636 dev-opt workspace
  with Assembly visible and forced continuous rendering; discard the first five
  seconds. These are single comparison runs, not repeated production benchmarks:

  | Metal mode / latency setting | Acquire p95 (ms) | Host tick p95 (ms) | Callback → CPU present return p95 (ms) |
  |---|---:|---:|---:|
  | FIFO / 1 (original default) | 15.670 | 16.540 | 32.711 |
  | FIFO / 2 | 12.600 | 3.274 | 18.406 |
  | Immediate / 1 | 15.271 | 4.457 | 20.549 |
  | Immediate / 2 | 0.098 | 3.707 | 5.913 |

  FIFO/1 has bursts of long host ticks (max 30.5 ms). Immediate/1 alone still
  has multi-second acquisition stalls; Immediate/2 largely removes drawable
  backpressure in this sample. This supports a pacing/backpressure problem but
  does not prove a particular beat frequency or establish displayed input latency.
  Immediate/2 remains diagnostic: its acquisition waits improve, but callback
  jitter persists. The human retest uses FIFO/2, retaining VSync and the usual
  three-drawable capacity. This is now the native default; latency 1 remains an
  explicit comparison control. Extra capacity does not force an extra queued
  frame. CADisplayLink still rate-limits updates.
  `pacing.rs` and `test/analyze-pacing.py` retain reproducible diagnostics; raw
  runs are `.agents/pacing-{fifo,immediate}-{1,2}.json` and summary is
  `.agents/pacing-summary.jsonl`. Normal launch enables no trace or forced render.
  Adapter selection and final surface creation can make separate helper Metal
  sublayers. The host now supplies one explicit CAMetalLayer, shared by both
  surfaces, with explicit backing scale. Actual layer logs verify FIFO/1 uses
  displaySyncEnabled=true / two drawables, and Immediate/2 uses false / three,
  both at 2560×1636 and scale 2. Repeated 25-second checks with this layer measured
  acquire p95 14.609 → 0.100 ms and callback-to-CPU-present-return p95
  20.583 → 5.297 ms. Callback jitter remains in the latter run (p95 23.94 ms), so
  shorter acquisition waits alone do not establish smooth display delivery.
  Those traces are `pacing-owned-{fifo-1,immediate-2}.json`. The display driver now
  explicitly requests the current screen maximum frame rate, refreshing it on
  wake/move; that request remains subject to OS scheduling. The final FIFO/2
  check has callback interval p95 16.694 ms (with an isolated 41.521 ms outlier),
  host tick p95 6.041 ms / max 8.150 ms, acquisition p95 15.017 ms and
  callback-to-CPU-present-return p95 22.059 ms. The explicit-rate Immediate/2
  comparison has acquisition p95 0.091 ms but callback interval p95 22.644 ms.
  This supports keeping VSync for the human retest and leaves pacing feasibility
  open; no claim that the latency waves are fixed. Final traces:
  `pacing-explicit-rate-{fifo-2,immediate-2}.json`. A duplicate idle app instance
  was later found during UI handoff and removed; repeat controlled comparisons
  with exactly one app before treating these exploratory timings as definitive.
  The single fresh retest app has the saved v34 document restored via native Load.
  The opaque-backing build passes the 15 workspace and 17 simulation checks again
  (`.agents/native-pacing-final-workspace.log`, `native-pacing-final-simulation.log`),
  including the final FIFO/2 defaults and explicit Metal layer.

#### Feasibility gate status and adapter inventory (2026-10-16)

The user requested integration into the main checkout on 2026-10-16. Source,
packaged app and experiment evidence were transferred from the `native-tauri`
worktree to `document-spine`; the pre-existing inspector/SVG fixes are retained as
a separate baseline commit. Run the native commands from the repository root
(`src-native/README.md`), without entering a worktree. The latest authored native
document (v90) was saved before relocation; its backup and prior diagnostic logs
are preserved under `.agents/native-transfer/` (gitignored). Worktree guidance now
asks the user before isolating substantial work and allows small-scale work in the
main checkout without asking. This integration does not close the remaining
feasibility gates below.

Human follow-up: latency is now reported as "okay, decent"; this is subjective
acceptance, not a measured latency ceiling. Custom cursor states still failed.
The packaged cursor probe reproduced an `img-src` CSP rejection for generated
PNG data URLs while pen events and fixed image loading already passed. Native
`img-src 'self' data:` now permits generated cursors. The shared cursor adapter
keeps meaningful CSS keyword fallbacks and gives WKWebView 64px/2x cursor rasters
(32 CSS px), retaining 256px artwork for rotation. The browser path keeps its
existing 8x images. Six packaged cursor checks pass, including pen event → CSS,
64px image loading and generated images without CSP violations; evidence:
`.agents/native-cursors-{baseline,fixed}.log`. Human appearance verification is
still required. The 15 packaged workspace checks and complete rebuilt WebGL2
smoke also pass (`native-cursor-zoom-workspace.log`, `cursor-zoom-web-smoke.log`).
The current v68 document was saved and restored through native Save/Load before
human cursor verification. The separate wheel zoom change is recorded in
`select-tool.md`.

| Gate | Evidence and remaining work |
|---|---|
| Native composition | Pass on this macOS machine: packaged shared Svelte UI above native Metal 2D/3D; no wasm or image IPC. Other platforms untested. |
| Native concurrency | Parallel ECS and pipelined render overlap proven with 100,000 runtime entities. Same-workload conventional winit comparison remains open. |
| Coordinates/input/lifecycle | DOM/backing dimensions, resize, minimize/restore, JS stall and shutdown pass. A real native short drag commits and undoes; HTML inspector typing also commits and undoes (macOS UI automation). Alignment/latency feel and crossing displays need human validation. |
| Document parity | Shared operation bodies, transaction/rejection, undo/redo and canonical reload pass. A real native drag and keyboard undo pass. An identical full web/native command trace remains open. |
| Maintainability | Shared handlers extracted; host exceptions inventoried below. No engine fork or Tauri types in product logic. |
| Regression/cost | Both wasm builds, frontend check and smoke pass. Idle has zero additional render submissions. Clipboard/assets round trip, comparable CPU/texture memory and measured input latency remain open. |

Shared extraction: `engine_api.rs` (941 lines), `engine_dispatch.rs` (41),
`host_events.rs` (30), `host_services.rs` (24), `surface.rs` (15). Existing wasm
exports delegate to these same operation bodies. The browser `canvas_view` now
wraps the external-window bootstrap; it is not the native event-loop owner.
Native host adapters: `src-native/src/main.rs` (415 lines), `bridge.rs` (395),
`display_link.rs` (79), `native_surface.rs` (136), `native_input.rs` (about 70).
The diagnostic composition probe (371), opt-in pacing instrumentation (145)
and simulation instrumentation (174) are separate from ordinary application execution. Native JS adapters total
136 lines (`native_adapter`, `native_bridge`, `native_host`). Counts are a snapshot,
not an assertion that every adapter line is platform-independent.

Exceptions to the shared-boundary ideal: async document calls required the existing
LLM authoring API to accept promises; native boot selects transparent HTML and
actual DPR without browser GPU/wasm probing; small host-service functions select
clock, origin-token and SIMD detection. Model bulk transport remains unimplemented.

- **2026-10-16 — experiment started (no gate passed yet):** user authorized the
  macOS Tauri attempt and delegated builds/automated testing. Isolated checkout:
  `.agents/worktrees/native-tauri`, branch `codex/native-tauri`, base `ebdd66e` plus
  a preserved snapshot of the existing working-tree edits. Initial composition
  gate budget: 90 minutes including the first dependency build, starting
  20:06 UTC. Environment: macOS 27.2 (26B5091g), Apple M5/Metal 4, Bevy 0.19.1,
  wgpu 29.0.4, Rust nightly-2026-07-15. Begin with real native Bevy, a transparent
  HTML probe and a supported main-thread display driver; no `run_iteration` loop.
  Model protocol implementation remains outside this experiment. Record actual
  results below before requesting human focus/input/display checks.
- **2026-10-16 — Model/concurrency clarification, planning only:** separated shell
  UI transport from computational Model sockets; linked the pending Model interop
  proposal; recorded window-adapter responsibilities and an explicit native
  multithreading/runtime-simulation gate. No implementation or new test results.
- **2026-10-16 — planning only:** replaced the cache/upstream-issue prerequisites
  with a macOS feasibility scope, explicit deviation boundary, shared protocol and
  ownership rules, current prior art and evidence gates. No native tests or port
  implementation have been performed in this revision.
