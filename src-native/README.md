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
ECS multithreading and pipelined rendering remain enabled in both cases.

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
