# A native target via Tauri

**Status:** Boundary and feasibility-test plan agreed 2026-10-16; no native
implementation or local feasibility results yet. A bounded macOS experiment may
proceed independently of raster caching. A full port and additional platforms
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

- **2026-10-16 — Model/concurrency clarification, planning only:** separated shell
  UI transport from computational Model sockets; linked the pending Model interop
  proposal; recorded window-adapter responsibilities and an explicit native
  multithreading/runtime-simulation gate. No implementation or new test results.
- **2026-10-16 — planning only:** replaced the cache/upstream-issue prerequisites
  with a macOS feasibility scope, explicit deviation boundary, shared protocol and
  ownership rules, current prior art and evidence gates. No native tests or port
  implementation have been performed in this revision.
