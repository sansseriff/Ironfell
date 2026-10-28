# macOS native feasibility target

The shared Iron engine and Vello renderer run in native Bevy on Metal. Tauri owns
one OS window and its transparent Svelte webview. An NSView CADisplayLink drives
Bevy on the OS main thread; ECS work and pipelined rendering use native threads.
No WASM loads in this build. This is a feasibility target, not a finished port.

From the repository root, on Apple Silicon macOS 14 or newer:

```sh
bun install
bun run native:build
```

The unsigned local app is
`target/aarch64-apple-darwin/dev-opt/bundle/macos/Iron Native.app`.
`bun run native:run` runs the latest static frontend without packaging (first
run `bun run native:ui`). The build uses the repository's dev-opt profile; do not
use its timings as an optimized production performance claim.

Automated workspace checks (run sequentially, with other builds/tests stopped):

```sh
IRON_NATIVE_SELF_TEST=1 'target/aarch64-apple-darwin/dev-opt/bundle/macos/Iron Native.app/Contents/MacOS/iron-native'
IRON_NATIVE_SELF_TEST=1 IRON_NATIVE_SIMULATION=100000 'target/aarch64-apple-darwin/dev-opt/bundle/macos/Iron Native.app/Contents/MacOS/iron-native'
```

Each prints `IRON_NATIVE_TEST` with pass/fail results and closes the app. The
simulation fixture creates runtime ECS particles, drives their parameter from
one authored rectangle, and records actual concurrent ECS/render intervals in
`.agents/native-concurrency.json`. Its instrumentation is absent unless
`IRON_NATIVE_SIMULATION` is set. Test host controls are disabled unless
`IRON_NATIVE_SELF_TEST` is set. Ordinary launches run neither self-test nor fixture.

Mailbox lifecycle/backpressure tests:

```sh
MACOSX_DEPLOYMENT_TARGET=14.0 cargo test -p iron-native --bin iron-native --target aarch64-apple-darwin --profile dev-opt
```

Packaged cursor/CSP checks (six checks, then closes):

```sh
IRON_NATIVE_SELF_TEST=1 IRON_NATIVE_CURSOR_TEST=1 'target/aarch64-apple-darwin/dev-opt/bundle/macos/Iron Native.app/Contents/MacOS/iron-native'
```

Native cursor images use 64px/2x rasters with keyword fallbacks. The CSP permits
PNG data URLs used by rotated/generated cursors; image permission is independent
of the script policy. The probe checks engine → CSS delivery and image loading;
actual cursor appearance still needs a person at the native window.

The independent composition probe is retained as `iron-native-probe`, with
`tauri.probe.conf.json`. It is diagnostic code, not a second product engine.
The real target shares `engine_api`, `engine_dispatch`, `HostEvents`, the document
store, tools, projection and renderer with web. Native-only code handles the
window, display callbacks, bounded session mailbox and test instrumentation.

Current control limits are 256 messages per batch, 1024 queued messages and 2 MiB
queued serialized payload. Pointer moves coalesce only when adjacent, preserving
button/key boundaries. Native channels carry engine events/replies; invocation
acknowledges enqueueing, not document completion. Request IDs carry completion.
A reload replaces the webview session while keeping the native authored document.
Web uses the same `engine_dispatch::call` table through its single wasm
`document_call` export. All hosts use `makeDocumentApi(call)` for the model
surface; argument construction and result decoding are shared. This consolidation
is followed by the typed protocol implementation: generated Rust-derived named
commands, one shared input/control dispatcher, session/disposal guards and scoped
UI reads. Hosts retain bootstrap, surfaces, scheduling and font/file services.
The protocol checkpoint passed 22 workspace checks, including an actual webview reload
that retains the native authored state and undo history. Run `bun run protocol:generate` after changing Rust command/request/event signatures and
`bun run check` to verify generated types and negative type fixtures.
Bulk computational Model transport remains a separate, unimplemented proposal.

See `plans/vision/08_native-target-via-tauri.md` for acceptance evidence, remaining
gates and platform exceptions. No agent harness or additional OS target is included.

Native mouse motion uses a local AppKit NSEvent monitor and a latest-sample slot;
Bevy samples it without a webview/IPC round trip. The monitor returns every event
unchanged, so HTML keeps its ordinary hit testing. Buttons, wheel, text and
shortcuts still use the shared UI protocol. `IRON_NATIVE_JS_POINTER=1` restores
the original JS motion path for A/B testing. This does not establish measured
event-to-photon latency; pointer feel and mixed HTML/native ownership need human
validation. The native surface is a child NSView fitted to AppKit's
`contentLayoutRect`, including Retina backing conversion, rather than the
window root's title-bar-inclusive bounds.

The monitor also observes button-edge coordinates. When final motion and a routed
release reach one tick together, the host gives the tools one held frame to show
that pose before release. This preserves short drags. `IRON_NATIVE_INPUT_TRACE=1`
prints local native event coordinates for diagnosis (ordinary launches do not).

The interactive presentation allowance defaults to two frames (Bevy's usual
setting; three Metal drawables). `IRON_NATIVE_FRAME_LATENCY=1` restores the earlier
one-frame/two-drawable setting for comparisons.
ECS multithreading remains enabled in both cases. Pipelined rendering is the
default; the independent render-mode control below can disable its render thread.

The native window now has an opaque background and standard title-bar drawing;
only the WKWebView overlay is transparent. The GPU NSView owns one explicit
CAMetalLayer, reused during adapter selection and final surface creation; its
backing scale follows the window. CADisplayLink requests the current screen
maximum rate explicitly and refreshes that request on wake/move. `IRON_NATIVE_PRESENTATION` logs the
actual window opacity, display maximum refresh rate and Metal layer settings.

Pacing comparisons (normal default is FIFO/VSync with latency 2):

```sh
IRON_NATIVE_PRESENT_MODE=immediate IRON_NATIVE_FRAME_LATENCY=2 bun run native:run
```

This diagnostic keeps CADisplayLink driving updates at the display cadence but
disables the Metal layer's additional VSync wait. It is a comparison mode, not a
confirmed production policy; check tearing and actual pointer feel. FIFO remains
available with `IRON_NATIVE_PRESENT_MODE=fifo`. Metal uses two drawables for a
latency setting of 1 and three for 2; extra capacity does not force that many
frames to be queued. A display callback and FIFO backpressure can interact.

Set `IRON_NATIVE_PACING_TRACE=.agents/pacing.json` to retain up to 24,000 CPU timing
rows, written on clean shutdown. `IRON_NATIVE_PACING_SECONDS=25` closes a diagnostic
run automatically; `IRON_NATIVE_ALWAYS_RENDER=1` forces continuous submissions.
Neither option is needed for normal input. Analyze uninterrupted always-render
runs with `python3 src-native/test/analyze-pacing.py .agents/pacing-*.json`.
Callback/host/acquisition/present-return times are CPU observations; they do not
establish GPU completion, displayed frame times or event-to-photon latency.

## Native latency controls

After `bun run native:ui` (or `bun run native:build`), start the opt-in
synchronous rendering + deadline pacing preset with:

```sh
bun run native:low-latency
```

`bun run native:late-fifo` (sync render, frame latency 1, `IRON_NATIVE_PACING=late-fifo`)
budgets tick start → Metal GPU completion to finish a feedback-adapted margin
(initial 4 ms, `IRON_NATIVE_PACING_MARGIN_MS`) before a CVDisplayLink vsync, and drains
FIFO backlogs from `presentedTime` feedback. Light 3D: 21.1 ms HUD present delay, as
smooth as baseline's 45.0 ms. `IRON_NATIVE_GPU_BUDGET=0` budgets CPU only;
`IRON_NATIVE_FRAME_CLOCK=wall` keeps wall-clock `Time`. Test scenes:
`IRON_NATIVE_3D_MOTION=1` strafes the 3D cameras; `IRON_NATIVE_3D_LOAD=N` adds N
high-poly spheres. `IRON_NATIVE_2D_MOTION=1` (or a speed in px/s) pans the 2D views.

`bun run native:adaptive` uses late-fifo only while dragging (left button held or a
document gesture) and stock-style display pacing otherwise, so panning, flying and
animation keep stock smoothness. See vision 08, “Adaptive pacing”.

Engine event log: per-action and per-frame lines (`applied …`, `redraw (…)`,
`solve:`, drag locks) are DEBUG on the `iron_debug` target and off by default.
Show them with `RUST_LOG=info,iron_debug=debug bun run native:adaptive` (keep
`info`: `RUST_LOG` replaces the default filter). The web equivalent is
`?bevy=debuglog`; see the run-iron skill's “Engine event log”. `IRON_NATIVE_PRESENT_TRACE=1` prints `IRON_NATIVE_PRESENT` feedback
every 120 frames (exact only with sync rendering). Results and rationale: vision 08,
“Vsync-timed presentation”. The standalone Bevy comparison stays one command away:
`native:standalone:late-fifo`, `native:standalone:motion` (sweeping bars) and
`native:standalone:stock`.

This selects `IRON_NATIVE_RENDER_MODE=sync` and `IRON_NATIVE_PACING=late`:
simulation, extraction and CPU render submission run in one host update, started
closer to the display target. Parallel ECS systems remain available. FIFO and the
three-drawable pool stay enabled. The ordinary `native:run` default remains
pipelined rendering with immediate display-callback updates.

The controls are independent for comparisons:

```sh
bun run native:baseline
IRON_NATIVE_RENDER_MODE=sync IRON_NATIVE_PACING=display bun run native:run
IRON_NATIVE_RENDER_MODE=pipelined IRON_NATIVE_PACING=display bun run native:run
```

`native:baseline` selects the original scheduling behavior in the same binary;
it is not an older source checkout. For a human A/B comparison, save the document,
quit one instance before starting the other, and use the same document, view,
window size and monitor. This keeps application-version changes out of the test.

The final 60 Hz continuous-workspace comparison measured update-start → CPU
present return at 18.32/18.87 ms p50/p95 for ordinary pipelining, 16.58/16.98 ms
for sync alone, and 4.19/7.55 ms for sync + late pacing. Sync alone had an earlier
2.63 ms median that did not repeat; removing the render thread alone does not
reliably prevent drawable waits. A two-drawable pool worsened timing tails, so the
preset keeps three. These are exploratory dev-opt CPU measurements, not displayed
mouse latency. The full results and limitations are in
`plans/vision/08_native-target-via-tauri.md` and its benchmark summary.

`late` reserves a rolling CPU-work budget plus a safety margin before the display
link's target timestamp. A one-shot timer returns control to AppKit while waiting,
so native mouse input continues arriving and is sampled when work starts. There
is no main-thread sleep or busy wait. The budget excludes drawable-acquisition
backpressure, compensates for observed timer overshoot, and starts immediately
during warm-up, after idle, at refresh-rate changes, or when work exceeds the
available time. Only one pending timer is retained; pause/shutdown cancels it.

`IRON_NATIVE_PACING=display` uses the ordinary immediate display callback.
`IRON_NATIVE_PACING_MARGIN_MS` adjusts the late scheduler's extra allowance
(default 2 ms, accepted range 0.25–8 ms). The estimate covers CPU submission;
GPU time and WindowServer presentation can require more time. Late scheduling
can miss a target under variable load (9/1201 CPU submissions in the final
sync/late capture). The preset is opt-in; GPU/presentation latency remains unmeasured.

Trace schema 2 carries actual host frame IDs through extraction, so idle frames
no longer invalidate the input/update → CPU-submit join. It records callback,
planned/actual work start, native input receipt, acquisition and CPU present
return. The analyzer reports timer overshoot, target headroom and unmatched
submissions, and warns about captures with too few callbacks. These do not measure
physical mouse event time, GPU completion or displayed pixels.

Keep the screen unlocked and the diagnostic window visible. For a bounded capture
that prevents display sleep, use an absolute trace path when launching outside
the repository:

```sh
mkdir -p .agents
IRON_NATIVE_RENDER_MODE=sync IRON_NATIVE_PACING=late \
IRON_NATIVE_ALWAYS_RENDER=1 IRON_NATIVE_PACING_TRACE=.agents/late-sync.json \
IRON_NATIVE_PACING_SECONDS=25 caffeinate -d -u \
  'target/aarch64-apple-darwin/dev-opt/bundle/macos/Iron Native.app/Contents/MacOS/iron-native'
python3 src-native/test/analyze-pacing.py .agents/late-sync.json
```

Self-test/trace launches expose and center their own window once; normal launches
keep ordinary window behavior. `caffeinate` does not unlock a locked session.
Run captures sequentially with builds stopped and compare the same document,
window size and refresh rate. Pure pacing/analysis checks:

```sh
MACOSX_DEPLOYMENT_TARGET=14.0 cargo test -p iron-native --bin iron-native --target aarch64-apple-darwin --profile dev-opt
python3 src-native/test/test_pacing_analysis.py
```

`CAMetalDisplayLink` was investigated, not substituted: its callback supplies a
drawable, whereas wgpu 30's Metal surface obtains one internally with
`CAMetalLayer::nextDrawable`. Supporting the supplied drawable needs a surface or
external-target integration, including presentation ownership and feedback.
Combining the two acquisition paths would consume different drawables. No Bevy
or wgpu fork, Metal method interception, or second presentation loop is used.

Final native validation passes: 23 workspace checks in each of all four
render/pacing combinations; 25 checks with 100,000 entities in each of the
pipelined/display and sync/late configurations. Parallel ECS is observed in both,
and render/simulation overlap matches the chosen mode. Actual native pointer
dragging and keyboard undo were also inspected.


## Filming visible pointer delay

Build once with `bun run native:build`, then launch one comparison at a time:

```sh
bun run native:film             # sync / late
bun run native:film:baseline    # pipelined / display
```

For the browser, append `latencyFilm=1` to the normal URL, for example
`http://127.0.0.1:5185/Ironfell/?gfx=webgpu&latencyFilm=1` when serving on 5185.
The mode replaces the workspace with one diagnostic composition. Its yellow square
is an ordinary document object: creation, reset and dragging use the shared document
transaction path. Native webview reload reuses the existing fixture. A full browser
reload starts its usual fresh document and creates one fixture again.

Grab the yellow square **at its centre** and drag horizontally along the dashed
line. Keep the OS cursor visible. Cyan is a DOM-event follower, offset −96 CSS px;
magenta is an engine pointer follower, offset +96 CSS px, bypassing document
manipulation but using the normal renderer. Subtract these fixed offsets when
comparing trajectories. Reset after resizing or changing display scale. Continuous
rendering defaults off; the checkbox selects the normal composition always/auto
contract. Keep that choice identical between runs.

Film at 240 fps or faster, retain the original video, and use the same monitor,
window size, display settings and movement pattern. Run one app at a time and stop
builds during recording. Export setup saves the active build/backend, mode label,
viewport, DPR/render scale, marker offsets and entered camera rate beside the video.
The camera rate describes sampling resolution; it is not a measured latency.

This scene supports visible delay relative to the OS cursor and between targets.
Absolute mouse-to-photon latency additionally needs a physical input reference in
the recording (or dedicated measurement hardware). No automatic video analysis or
software-derived photon-latency number is produced by this mode.


## Isolating rendering and presentation delay

The native layer control adds a **green diamond at −192 CSS px**, drawn by Bevy's
ordinary sprite renderer directly into the final window camera. It bypasses Vello
and the intermediate composition textures, while sampling the same engine pointer
as the magenta cross. These commands use the original pipelined/display preset:

```sh
bun run native:film:layers
bun run native:film:detached
```

Run one instance at a time. The detached control waits for the filming scene, then
removes WKWebView from its parent while retaining the native GPU view. Its drawable
rectangle is frozen before removal, so the resolution does not change. Move the
cursor without holding a button: the HTML controls and button ownership are absent.
Quit and restart to restore the webview. Keep the same monitor, window size and
scale; do not resize the detached control. `IRON_NATIVE_PROBE_WEBVIEW=hidden` hides
the retained webview instead of detaching it. To compare sync/late, use
`IRON_NATIVE_LAYER_PROBE=1 bun run native:film` and the same webview flag.

`python3 src-native/test/validate-layers.py` runs six packaged GPU-output controls:
overlay/hidden/detached × pipelined/display or sync/late. These intentionally force
continuous rendering and inject scripted pointer positions. They check 12 valid
readbacks per case, correcting the two marker offsets and comparing both markers
with the requested sample. Empty transparent readbacks are retained as unavailable
and retried; an unavailable capture is not evidence of stale imagery. PNG readback
perturbs timing, so this suite establishes rendering correctness only. Output is
under `.agents/latency/layers/`; no physical mouse latency is measured.

For timing, omit `IRON_NATIVE_PROBE_CAPTURE_DIR`. Metal's HUD can show the actual
post-submit presentation interval, independently of the app's CPU-present-return
trace:

```sh
MTL_HUD_ENABLED=1 MTL_HUD_LOG_ENABLED=1 MTL_HUD_ALIGNMENT=bottomright \
MTL_HUD_ELEMENTS=device,layersize,fps,gputime,presentdelay,frameinterval \
IRON_NATIVE_LAYER_PROBE=1 bun run native:film:baseline
```

The process-local HUD menu can generate a five-second report. `MTL_HUD_REPORT_URL`
selects its output path. Its Present Delay runs from `presentDrawable` to reported
display presentation; it does not include earlier input processing or panel pixel
response. Apple documents the metrics at
<https://developer.apple.com/documentation/xcode/understanding-metal-performance-hud-metrics>.
`IRON_NATIVE_GPU_DONE_TRACE=1` additionally records queue-completion callback receipt
when the CPU pacing trace is enabled. Polling can delay that callback; it is an
upper bound on queue completion, not a GPU timestamp or presentation feedback.
