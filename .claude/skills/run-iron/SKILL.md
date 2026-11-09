---
name: run-iron
description: Build, run, drive, screenshot, and test the Iron app (Bevy + Vello in wasm, Svelte shell). Use when asked to start Iron, build the wasm, take a screenshot of the running app, drag something in the viewer, check the console for applied transactions, or run the document store tests.
---

Iron is a Bevy app compiled to wasm, running in a web worker behind a Svelte
shell served by Vite. Drive it with `.claude/skills/run-iron/drive.ts`: a
stdin-scripted Playwright driver that launches the installed Google Chrome
headless (SwiftShader, WebGL2), waits for the engine, and takes screenshots,
drags, and console dumps. All paths below are relative to the repo root.

Verified on macOS (Apple Silicon) on 2026-09-18 and on a Linux cloud container
(4 cores, SwiftShader only) on 2026-10-17. Platform-specific pieces:

- The Chrome path (`CHROME` env var overrides it; Playwright's bundled build
  works: `CHROME=/opt/pw-browsers/chromium-*/chrome-linux/chrome`).
- The scripts are `#!/bin/zsh`; where there is no zsh, `bash build.sh` and
  `bash serve.sh start` run them unchanged.
- Native tests take the host triple: `--target x86_64-unknown-linux-gnu` on
  Linux instead of `aarch64-apple-darwin`. Two store tests compare solver
  output to the last digit (`the_pendulum_view_matches_the_golden_file`,
  `converting_an_ellipse_reports_and_bakes_what_reads_or_drives_its_radii`)
  and differ at 1e-12 between platforms' libm (constraint-system.md I7); they
  are not regressions.
- Without a GPU the app runs at ~8 fps. The 200 ms settle waits in
  `spatial.txt` are then too short (reference and indexed hits are read from
  different frames and differ spuriously); run it with the waits raised
  (`sed 's/^sleep 200$/sleep 1500/; s/^sleep 500$/sleep 2000/'`). Its
  through-plane step at viewer (960, 180) depends on the panel geometry;
  on the container the card's dot is at (934, 150). Driver-paced drags may
  also stop a few pixels short of their target, so values like the
  slider's 0.8 can read 0.76 and a resize's 120 can read 116 in the smoke
  output (a baseline build on the same container showed 0.81 and the same
  solver-count and version drift); a wrong *target* (another node) is the
  failure to look for.

## Typical durations

Wall-clock on the Apple Silicon dev machine, from timed runs on 2026-10-22 and 2026-10-26. Per
AGENTS.md, check in with the user before running a step marked ⏱ (over a minute).

| step | typical time |
|---|---|
| `BACKENDS=webgl2 build.sh`, warm, after a source change | ~45 s (cargo ~25 s) |
| ⏱ `build.sh` (both backends), warm | ~1–1.5 min (1m 04s, 2026-11-04; ~55 s after a `web_ffi.rs` edit, 2026-11-06) |
| ⏱ `build.sh` after a dependency or feature change | ~3.5 min |
| ⏱ `build.sh` after an `iron_document` change | ~3.5–4.5 min (3m 40s, 3m 23s, 4m 18s, 2026-11-08): the document crate rebuilds everything above it |
| ⏱ `build.sh` cold | ~5 min (5m 08s in a fresh T3 worktree: webgpu 2m 56s, webgl2 2m 12s, 2026-11-06) |
| ⏱ `BACKENDS=webgl2 build.sh`, cold isolated worktree | ~4 min (cargo 3m 25s, plus binding generation; 2026-11-03) |
| `smoke.txt` (`GPU=1`), fast default | ~5 s warm (4.72 s, 2026-11-04); startup-dependent |
| `regression.txt` (`GPU=1`): bindings, model edits, the pendulum, linking, save/load | ~1 min (1m 09s, 2026-11-09) |
| ⏱ `gestures.txt` (`GPU=1`): select tool, pen, shapes, clipboard, Alt-drag, gizmo, SVG, components | ~2.5 min (2m 25s, 2026-11-09) |
| `snapping.txt` | ~30 s |
| `layout.txt` | ~35 s |
| `contexts.txt` | ~35 s |
| `keys.txt` | ~30 s |
| `sequence.txt` | ~30 s |
| `schematic.txt` | ~40 s |
| `scopes.txt` | ~15 s |
| `workspace.txt` | ~25 s |
| `layers.txt` (`GPU=1`) | ~22 s (21.5 s, 2026-11-06) |
| `scope-menu.txt` | ~20 s |
| `narrative.txt` | ~25 s |
| `timeline.txt` | ~30 s |
| `cargo test -p iron_document --target aarch64-apple-darwin`, warm after an `iron_document` change | ~1 min |
| `cargo test -p iron_document --target aarch64-apple-darwin`, cold isolated document-test cache | ~15 s (14.42 s, 2026-11-03) |
| `bun run attachments.ts` | ~10 s (9.52 s, 2026-11-04) |
| `bun test tests/` (protocol, document API, scopes) | <1 s |
| `agent-runtime.ts`, fixture runtime, worker or main | ~2 s (1.90 s, 2026-11-03) |
| `agent-components.ts`, fixture runtime, worker or main | ~3 s (2.71 s, 2026-11-06) |
| `agent-runtime.ts`, live Codex, worker or main | ~10–30 s; capped at 55 s |
| `agent-pairing.ts`, local fixture | ~3–5 s; capped at 55 s |
| `bun test tests/dev-agent.test.ts`, warm launcher tests | <1 s (2026-11-04) |
| `bun run dev:agent:demo`, warm startup to ready | ~1 s; stays running until Ctrl+C |
| `chat-ui.ts`, fixture, worker or main | ~4–7 s; capped at 55 s |
| `chat-history.ts`, fixture, saved scope | ~10 s; capped at 55 s |
| `chat-ui.ts`, real Codex prose/visual + shape, worker | ~10–25 s; capped at 55 s |
| `tables.txt` (`GPU=1`), warm | ~25 s |
| `document-calls.txt`, each browser runtime (`GPU=1`) | ~3–5 s |
| `protocol-lifecycle.txt` (`GPU=1`) | ~10 s |
| `text-styles.txt` (`GPU=1`), warm | ~18 s |
| shared-engine Rust unit suite (4 tests, native `dev-opt`), warm | ~8 s |
| ⏱ dispatcher Rust unit test, cold native Bevy 0.20 graph (`dev-opt`) | ~3 min |
| ⏱ `bun run native:build`, cold native Bevy/Tauri graph (`dev-opt`) | ~6 min (6m 04s in a fresh worktree, 2026-10-27) |
| `bun run native:build`, warm, after workspace test changes | ~21 s |
| `bun run native:build`, warm, after native Rust / plugin-selection changes | ~20–45 s |
| ⏱ `bun run native:build`, after shared engine + native IPC changes | ~2 min (1m 59s, 2026-11-04) |
| native Rust bin tests, warm (2 mailbox + 5 frame-budget checks) | ~3 s |
| ⏱ native Rust bin tests, cold test/build feature graph | ~2 min (2m 00s for native connection test, 2026-11-04) |
| ⏱ first standalone Bevy/winit dependency build | ~3 min |
| standalone Bevy control rebuild, warm | ~2 s |
| ⏱ `cargo build -p iron-native`, warm, after adding a native dependency (objc2-core-video) | ~2 min (2m 02s, 2026-10-27) |
| five-second Metal HUD report of the host via System Events (`.agents/latency/standalone-hud/capture-host.sh`) | ~25 s |
| native CPU pacing capture, with unlocked screen | ~25 s plus startup |
| packaged native workspace self-test (23 checks, including reload/mode) | ~19–23 s |
| packaged native filming self-test (14 checks, warm) | ~4 s |
| `validate-layers.py`, six native GPU readback controls | ~25 s when drawables are available |
| five-second Metal HUD report, including startup/menu/countdown | ~15–30 s |
| `latency-film.ts`, three runtime/DPR configurations per backend | ~10–15 s |
| `latency-view.ts`, two runtimes × two layouts, DPR 2, plus the menu | ~15 s (15.4 s, 2026-11-05) |
| `latency-measure.ts`, one run (OS pointer, hands off) | ~15 s (Chrome launch, 6 s sweep, analysis) |
| ⏱ `latency-measure.ts`, Chrome `REPEAT=2` (8 runs) | ~3 min |
| ⏱ `latency-measure.ts`, Chrome `REPEAT=2`, worker+main × solo+panels+full (12 runs) | ~4 min |
| `latency-trace.ts`, one 3 s traced measurement | ~30 s |
| ⏱ `cargo build -p iron-native`, warm, after a `main.rs` edit (`dev-opt`) | ~1 min (58.7 s, 2026-11-05) |
| `scripts/latency/build.sh` (Swift probe) | ~5 s |
| ⏱ `cargo build -p iron-native` cold in a fresh worktree (`dev-opt`) | ~6 min (5m 43s, 2026-11-05) |
| `check-latency-film.py`, six screenshots | ~2 s |
| `appearance.ts` (DPR 1, warm) | ~30 s |
| ⏱ `raster-pixels.txt` (with the Prussia import, then the meters) | ~1.5 min |
| ⏱ `bench-instances.txt` | ~1.5 min |
| `instance-pixels.txt` | ~1 min |
| ⏱ `instance_cost` (native, release) | ~1.5 min |
| `bench-large-svg.txt` | ~1 min |
| a short ad-hoc driver script (a few commands) | 20–40 s |
| ⏱ `cargo test -p iron_document`, after a dependency change | a few minutes (not yet timed) |

## Local agent runtime bootstrap

Native with the second Claude profile: `bun run native:agent` (warm native
build/run, then stays open). Expected banner includes `Native agent bridge ready`,
`Claude profile: …/.claude_2nd`, automatic connection guidance and the Ctrl+C
instruction. Claude is selected automatically; no bridge token is printed or
pasted. Closing the window or Ctrl+C stops the app/process group and bridge.
`IRON_CLAUDE_CONFIG_DIR` overrides the default profile explicitly. No inference
at startup; `bun run native:agent --help` lists profile/executable overrides.

Since 2026-11-06 the native default frame mode is sync rendering with adaptive
pacing and two frames in flight, so `bun run native:agent` and `bun run native:run`
use it with no settings; `IRON_NATIVE_RENDER_MODE`, `IRON_NATIVE_PACING` and
`IRON_NATIVE_FRAME_LATENCY` override it (`src-native/README.md`, “Frame modes”).
The launcher fixture tests verify these reach the child process.
Checked 2026-11-06 after `bun run native:build` (1m 45s): with no settings the
packaged app reports `renderMode` `sync` and `pacing` `adaptive`. The packaged
self-tests had been failing their first check, `shared workspace ready`, since
`1a0c992` (2026-11-05: the native app routes like the web, so a URL with no scope
is the scopes page, which loads no engine). The host now opens the workspace and
cursor self-tests, and `IRON_NATIVE_LATENCY` runs, on `?demo` (the agent test
already did). After that fix: workspace 23/23 (default and `pipelined`/`display`),
100k simulation 25/25, cursor 6/6, each ~20 s, no errors.

`bun test tests/native-agent.test.ts` exercises second-profile isolation, IPC
validation, authenticated requests, child exit/stop and occupied-port safety
with fixtures, not paid inference. `IRON_NATIVE_SELF_TEST=1` and
`IRON_NATIVE_AGENT_TEST=1` select the native pairing/reload test (`agent.js`):
the report contains only boolean checks, never tokens or account details.

Native agent verification (2026-11-04): fresh packaged app and the exact command
below pass **12 checks** without errors. Normal launches request the last saved
scope when native persistence is enabled; this test uses an ephemeral demo and closes itself, leaving saved scopes
alone. No paid inference is performed.

```sh
IRON_NATIVE_SELF_TEST=1 IRON_NATIVE_AGENT_TEST=1 bun run native:agent
```

The final `IRON_NATIVE_TEST` report must contain 12 true checks and an empty
`errors` array; do not treat exit code alone or the initial phase report as a
pass. Coverage: workspace and Model readiness, private connection, document
reads with Model active, client automatic pairing/Claude selection, authenticated
model listing, no token in URL/storage, and connection/client/document reads
after webview reload. The test uses credential-free `__iron.agentState()` for
pairing readiness, not popup timing. Logs: `.agents/native-agent-command.log`
and `.agents/native-agent-selftest.log`. Unit suite: 51 JS tests; native connection
Rust test passes; frontend check: 0 errors / 10 existing warnings. Initial engine
readiness is now independent of display callbacks, so loading/occlusion cannot
prevent the control handshake.

Commit validation 2026-11-05: the isolated agent/native-launcher snapshot passes
52 JS tests, frontend checking (0 errors / 9 existing warnings), and fast smoke
with six `assertion passed` lines and `(no page errors)`. Expected output is
unchanged. Cached wasm was reused; unrelated scope, FPS, timeline and UI edits
were excluded. Logs: main checkout `.agents/native-agent-commit-{tests,check,smoke}.log`.

The owning plan is [`plans/specs/agent-runtimes.md`](../../../plans/specs/agent-runtimes.md).
For manual testing, use **one terminal** in the repository root:

```sh
bun run dev:agent:demo --open  # ephemeral demo
# bun run dev:agent --open    # last saved scope instead
```

Expected startup banner: `Iron + Codex + Claude ready`, followed by the app URL on
127.0.0.1:5175, bridge URL on 127.0.0.1:4318, detected Codex executable, the
Claude Code executable with `(your Claude subscription)` (or `not found`; with
`IRON_CLAUDE_AUTH=api-key`, `(ANTHROPIC_API_KEY)`),
automatic-pairing/login guidance and `Ctrl+C stops both services.` No token is
printed by this launcher. Claude runs on the Claude Code login unless
`IRON_CLAUDE_AUTH=api-key`; the composer's model picker lists each runtime's
models (`POST /models`). It checks executability, not login validity;
an authenticated model turn remains the login test. It reuses built wasm,
does not perform inference at startup, and never kills an occupied-port owner.
`--open` is optional; Ctrl+C stops both services. `bun run dev` remains UI-only.
Since 2026-11-06 `dev:agent` serves the app cross-origin isolated (the banner's
`Frame hold: on`), so Chromium runs the worker frame hold. Since the same day
plain `bun run dev` is isolated too (`vite.config.ts`); `IRON_COI=0` turns it off
for either. Checked: `crossOriginIsolated` true,
`__frameHold.stats()` running, automatic pairing still works, no page errors.

From the worktree root, with Vite on port 5175:

```sh
bun test tests/agent-runtime.test.ts
bun test tests/dev-agent.test.ts
bun .claude/skills/run-iron/agent-pairing.ts
bun .claude/skills/run-iron/agent-runtime.ts
RUNTIME=main bun .claude/skills/run-iron/agent-runtime.ts
LIVE_AGENT=1 IRON_CODEX_BIN=/path/to/codex bun .claude/skills/run-iron/agent-runtime.ts
```

The fixture test uses the real document engine and a scripted Codex protocol
process; `LIVE_AGENT=1` uses the signed-in CLI and consumes model usage. Each
browser run is capped at 55 seconds. `URL` overrides the worktree app address;
`RUNTIME=main` selects main-thread mode (default worker). A separate temporary
bridge is started and stopped by the test; no manually running bridge is needed.

Local Vite development pages automatically discover the CLI bridge token; no
paste is needed on refresh. Start the bridge with `IRON_AGENT_ORIGINS` matching
the exact app origins (for example `http://127.0.0.1:5175,http://localhost:5175`).
The settings gear provides a retry and manual fallback. Native/static builds
still need manual entry. Pairing trusts those local app origins and is not an
OS multi-user security boundary. Tokens stay in memory, not URLs or storage.

Pairing fixture expected output (no model usage):

```text
PASS LOCAL PAIRING: autofill, reload, token rotation, authenticated send, no credential storage; no page errors
```

Expected output (one line per browser run):

```text
PASS FIXTURE CODEX worker: create, edit, preserve dimensions, undo; no page errors
PASS FIXTURE CODEX main: create, edit, preserve dimensions, undo; no page errors
PASS LIVE CODEX worker: create, edit, preserve dimensions, undo; no page errors
PASS LIVE CODEX main: create, edit, preserve dimensions, undo; no page errors
```

All four pass on 2026-11-03. The live worker repeat after disabling runtime
hooks/plugins takes 10.86 s. Token-free readback evidence is saved under
`.agents/agent-runtime/`. These checks use copied cached wasm; they do not
replace a fresh build or the full smoke. The existing full smoke expected
output is unchanged. Unit suite: 31 pass including 11 new runtime tests.

### Agent component verbs

Owning plan: `plans/specs/component-model.md` §14, phase 7. With Vite serving this
checkout (`URL` must include `?demo`; 5175 is the default, `serve.sh start` gives 5173):

```sh
bun .claude/skills/run-iron/agent-components.ts
RUNTIME=main bun .claude/skills/run-iron/agent-components.ts
```

The scripted runtime (`tests/fixtures/fake-agent-runtime.ts`) answers one request with
`<make-component>` of a rect and an ellipse, `<place>` of it in one composition,
`<place entity>` in a second and a `std:laser` with `power="5 mW"`, then a second with
`<look expand>` at one picture, and renames the entity only if the look showed its
derived picture. The test checks the model view lists both figures, each `use` sits
where it was placed, the look took no repair attempt, and undo removes the placement
and returns the shapes. Expected output:

```text
PASS FIXTURE COMPONENTS worker: make, place in two compositions, std:laser with an argument, look expand, one undo each; no page errors
PASS FIXTURE COMPONENTS main: make, place in two compositions, std:laser with an argument, look expand, one undo each; no page errors
```

Both pass 2026-11-06 on a fresh webgl2 build. Evidence: `.agents/agent-runtime/components-fixture-*.json`.
On the same day `chat-ui.ts` times out waiting for the Lesson tab, with or without this
change (it predates it; not yet investigated).

## Shared Model/Lesson chat

Owning plan: `plans/specs/narrative-panel.md` §9 (T3 source investigation,
component boundaries, explicit deferrals and validation). From the worktree root
with Vite on 5175:

```sh
bun .claude/skills/run-iron/chat-ui.ts
RUNTIME=main bun .claude/skills/run-iron/chat-ui.ts
LIVE_AGENT=1 IRON_CODEX_BIN=/path/to/codex bun .claude/skills/run-iron/chat-ui.ts
```

Expected output:

```text
PASS CHAT worker: shared Model/Lesson, Markdown/visuals, safe content, history, scroll follow, IME, dictated text, drafts/modes/cursor, prose-only, review/revert/conflict, failure recovery, live progress and Stop, 7 turns in the agent log; no page errors
PASS CHAT main: shared Model/Lesson, Markdown/visuals, safe content, history, scroll follow, IME, dictated text, drafts/modes/cursor, prose-only, review/revert/conflict, failure recovery, live progress and Stop, 7 turns in the agent log; no page errors
PASS LIVE CHAT worker: real Codex prose/visual without mutation, then validated shape creation, 2 turns in the agent log; no page errors
```

### Chat history

Each chat panel (Model, each narrative's Lesson chat) keeps its own list of
conversations, saved with the scope as `chats.json` (`src-ui/chat/sessions.svelte.ts`).
`chat-ui.ts` runs on `?demo`, which saves nothing, so this script opens a saved
scope instead (`&new=demo`, in a fresh browser profile):

```sh
URL='http://127.0.0.1:5175/Ironfell/?gfx=webgl2&new=demo' bun .claude/skills/run-iron/chat-history.ts
```

Expected output:

```text
PASS CHAT HISTORY: new chat, past chats menu, open, draft per chat, saved with the scope across reload, delete with confirm, separate Lesson history; no page errors
```

Screenshot: `.agents/chat/history-menu.png`. With `&new=demo` the address is then
replaced with `scope=…`; wait for that scope to be open before acting (the script
polls for it), since acting while it opens can reload the page.

All pass 2026-11-03 with cached wasm; again 2026-11-06 with the streaming checks
(`plans/specs/agent-streaming.md`): a slow streamed fixture turn shows its work
log, prose and forming edit (`.agents/chat/streaming-*.png`), then Stop restores
the prompt and leaves the document unchanged. Each run gives its bridge an agent log
in a temporary folder and checks every finished turn has its calls and outcome there
(`KEEP_AGENT_LOG=1` keeps the folder and prints it). The scripts need `playwright-core`;
if `node_modules` lacks it, install it in a scratch folder and pass
`NODE_PATH=<that>/node_modules`. Each fixture run uses the real engine and
a scripted local runtime, covering the shared components in both panels and all
three narrative modes. It also checks unsafe content, failure input restoration,
history, scroll-follow release/Latest and review conflicts. Live mode consumes
signed-in Codex usage. `URL` overrides the address but must include `?demo`.
Waits require the UI, loader completion and a successful engine schema read,
not merely the early diagnostic `__iron` object. Screenshots are in gitignored
`.agents/chat/`, inspected for each layout. Total timeout is 55 seconds.
Unit suite now has 40 passing tests. Full smoke expected output is unchanged;
these focused tests do not replace full smoke/fresh-build/native validation.

Integration validation 2026-11-04 (main `fcba711` incorporated): **45 JS tests**,
frontend check (0 errors / 9 existing warnings), strict launcher/bridge types,
fresh WebGPU + WebGL2 wasm (85.53 s total), web/native-mode frontend bundles,
fast and broad smoke, `narrative.txt`, pairing and worker/main agent/chat fixtures
pass. Smoke expected output is unchanged; browser checks report no page errors.
No new paid inference. Native packaging/runtime and live Claude remain untested.
Logs and fixture evidence: main checkout `.agents/agent-runtimes-validation/`.
The existing `narrative.txt` smoke also passes on worktree port 5175 after this
refactor, including typing/undo, references, modes, review and durability. Its
expected output is unchanged. Vite caches are root-local `.agents/vite`, not
under a possibly shared `node_modules/.vite`.

## Prerequisites

Already present on the machine this was verified on; nothing was installed:

- Rust nightly from `rust-toolchain.toml` (rustup picks it up), with the
  `wasm32-unknown-unknown` target.
- `wasm-bindgen` CLI matching `Cargo.lock` — `build.sh` installs the right
  version if it differs.
- `bun` (runs Vite and the driver), `curl`, `lsof`.
- Google Chrome at `/Applications/Google Chrome.app`.

## Setup

Once per clone:

```bash
bun install
cd .claude/skills/run-iron && bun install && cd -
```

## Build

`bun run wasm` and `bun run wasm:fast` (repo root) run this script for dev-opt
and wasm-bench; `bun run wasm:smallfast` runs `build-wasm.sh` (the shipped
build) for both backends. Settings (and the tooltip on a view's FPS readout) names
the running build (`Build dev-opt (opt 1, no SIMD) - SLOW` or `Build
wasm-bench (opt 3, SIMD)`).

```bash
.claude/skills/run-iron/build.sh
```

Builds the `dev-opt` profile twice, with the `webgpu` and the `webgl2`
feature, and binds each into `src-ui/wasm/<backend>/`; the loader picks one per
browser (`src-ui/runtime/backend_policy.ts`), so desktop Chromium on macOS gets
the WebGPU build. About 2.5 minutes cold, 15 seconds warm, per backend.
`BACKENDS=webgl2` builds only what the headless smoke runs; a backend that has
never been built is then stubbed with a copy (marked `.stub`) so Vite's import
of both artifact URLs resolves. The wasm is ~1.4 GB before binding and well over
100 MB after because the profile keeps debug info; that is expected, and says
nothing about the shipped size (`build-wasm.sh`, ~20 MB, ~4.6 MB brotli).

**Performance comparisons require `PROFILE=wasm-bench`.** The default dev-opt
artifact is level 1 and the helper does not enable SIMD for it; it can make the
1k selected-group drag ~19 FPS even when optimized SIMD is near 60. A localhost
link alone does not identify the build. Do not publish development timings as
renderer ceilings. The 2026-10-13 sanity investigation intentionally leaves the
optimized artifacts served for the owner; a normal build replaces them.

## Engine event log (DEBUG)

Since 2026-10-27 the engine's per-action and per-frame log lines are DEBUG on
the `iron_debug` tracing target, off by default, so normal runs stay quiet:
`applied …` transactions and their `… re-solved` cause lines, `loaded document`,
`nothing to Undo/Redo`, `redraw (…)` 2D-layer rebuilds, per-frame `solve:`
lines, and `locked for this drag` / `dragged through cluster`. Turn them on:

- **Web:** add `debuglog` to the `bevy=` URL parameter (`?bevy=debuglog`, or
  `?bevy=views,debuglog` alongside other variants). `drive.ts`'s default URL
  includes it, so `smoke.txt` and the other scripts see these lines; keep it
  when overriding `URL=`. Without it, `logs applied|…` checks print nothing.
- **Native:** `RUST_LOG=info,iron_debug=debug bun run native:run` (any
  native script). `RUST_LOG` replaces the default filter, so keep `info`.

Warnings (`rejected`, `differs from Bevy`, `drawn a frame late`) and one-off
INFO lines (startup, copy/paste/import results) are unaffected.

## Run (agent path)

```bash
.claude/skills/run-iron/serve.sh start
cd .claude/skills/run-iron && SIZE=1680x1000@1 bun run drive.ts < smoke.txt; cd -
.claude/skills/run-iron/serve.sh stop
```

The shell is a dock of views (`src-ui/views/`, plans/specs/panels-and-session.md):
tabs and splits, each a view onto a composition or an HTML view. The
document-space commands below address the **Main** tab, whose view id is
`viewer`; it shows the `main` composition, which embeds the scene, the
pendulum, the controls and the 3D assembly where they sat before the split.
The other tabs (Scene, Pendulum, Controls, Assembly, Card, …) are the
document's: a layout with none saved gets one tab per composition, named
after it, so their view ids are the compositions' names (`pendulum`). A
hidden tab has no panel rectangle (`panel <id>` prints zeros). Each driver
run starts with a fresh browser profile, so the derived layout; in a normal
browser each scope saves its own (`workspace.json`; an unsaved page in
browser storage), and `?layout=default` or the Settings view's **Reset
layout** lays it out again (see "Layouts per scope" below).

### Fast default smoke

`smoke.txt` is the default gate: engine startup, a real pointer drag, propagation
to the bound shadow, undo/redo through the keyboard, atomic rejection of an invalid
edit, canonical save/reload, one screenshot and no page errors. Its six `assert`
commands must print `assertion passed`; a false assertion exits nonzero. Observable
results are polled with a five-second timeout instead of fixed post-command sleeps.

Run the relevant focused fixture alongside it when changing a feature (for example,
`attachments.ts` for shared point controls, `timeline.txt` for timeline behavior).
Use the broad pair for cross-cutting changes or before a release:

```sh
GPU=1 SIZE=1680x1000@1 bun run drive.ts < regression.txt
GPU=1 SIZE=1680x1000@1 bun run drive.ts < gestures.txt
```

**The default layout (2026-11-09, the layout council).** The sides run the window's full
height and the timeline sits under the canvas only: at `SIZE=1680x1000@1` the left side is
340 px (its tab bar hidden; its view strip switches Layers, Model and Library; the grab pill
at its top shows the bar again), the Inspector 300 px, the timeline 250 px, and the viewer
reads `viewer panel {"x":340,"y":36,"w":1040,"h":714}`. The scope menu sits at the left of
the centre's tab bar. `workspace.ts` `baseLayout` scales the sizes with the window.
**Known broken, before this:** `schematic.txt` parses markup the demo no longer prints (R2
is now a resistor component, not a slider), 3 step errors on the old layout and the new.

**Which scripts are checks (2026-11-09).** A script earns its place by driving an
interaction a unit test cannot reach: a real pointer or keyboard gesture through the
canvas and the DOM. Checks: `smoke.txt` (always), `regression.txt`, `gestures.txt`,
`timeline.txt`, `layers.txt`, `snapping.txt`, `sequence.txt`, `text.txt`,
`text-styles.txt`, `keys.txt`, `tables.txt`, `schematic.txt` (broken, see above), `contexts.txt`,
`narrative.txt`, `scopes.txt`, `scope-menu.txt`, `workspace.txt`, `document-calls.txt`,
`protocol-lifecycle.txt`. Measurement tools, not checks: `bench-*.txt`,
`raster-pixels.txt`, `instance-pixels.txt`, `spatial.txt`, `picking.txt`,
`selection-cache.txt`. Retired: `smoke-full.txt` (split into `regression.txt` and
`gestures.txt`), and the exit demos `chrome`, `controls`, `showcase`, `piston`, `focus`.
**Positions are relative to views** (`pmove`, `pdrag`, `pclick`, `pdouble`, `dmove`,
`ddrag`, `dclick`): no script uses raw window coordinates, so a layout change moves
nothing; `tab <title>` activates a view through the dock, so a hidden tab is reached.

### Regression and gestures (the broad pair)

`smoke-full.txt` was split in two on 2026-11-09 at its stage E, which began with a
fresh `nav`: `regression.txt` (to the save/load round trip) and `gestures.txt` (stage E
on). Splitting ended the old drift where the second `nav` booted back into the durable
scope; its few raw coordinates became view-relative. Both pass with no errors.
The walkthrough and expected output below describe the pair, in order.
It opens the app, waits for the engine, screenshots, drags the
square (step by step, with a mid-drag screenshot and view showing the
shadow rect bound to it following) and the torus, undoes both with Cmd+Z,
redoes one with Cmd+Shift+Z, scrubs the slider and checks the bar bound to
it, drags the bar so its bound y inverts onto the slider, scrubs the
timeline and checks the clip driving the torus, applies one good and one
bad edit in the model's markup, runs the pendulum through every
constraint-system phase 3 exit with Cmd held, since the pendulum is a
group and a plain click selects all of it (drag the ball, drag beyond reach, undo,
a locked arm, an out-of-reach model edit adjusted then refused with
exact, the length slider, the fix moved to the ball and the platform
dragged, `.by` markers), runs the stage C1 exit in the main layout
(select the cartoon box, shift-click the torus in the embedded 3D view, Link; the model and
composition views; hover either to light both; Show in each way; undo the
link in one step; the model surface's `<link-figures>`, its merge question
and `keep`), round-trips the document through save and load,
and prints the transactions the store applied. Then, on a fresh app, it
runs the select tool (stage E, plans/specs/select-tool.md): click, a corner
resize, a Shift-snapped rotation, undo of both, a group's click /
double-click / Esc / Esc, a marquee, arrow nudges, and delete of the
pendulum undone in one step; then the pen (a path drawn, closed, the close
undone; a click selects the path, a double-click edits its points and one
is dragged; the pen on a segment of the edited path adds a point there, undone; Cmd-drag bends a
segment, Delete and the pen's click on a point each remove one, all undone), the inspector (a width set, a bound x set
through its binding), and stage F's optics chain (laser → fiber → meter in
the Optics and Schematic tabs: the fiber lengthened by its slider dims both
gauges, the gauge dragged sets the laser's power back through the chain,
an rf_voltage output refused by the meter's optical input; the Thermal tab's
heater, link and sink solved as an across/through network, the heater's power
doubled by its slider), and shapes and paths (a rectangle and a circle drawn with
the bottom toolbar's R and O tools, the circle stretched into an ellipse, a
double-click turning it into a path with the same id, undo, and the bound bar's
conversion asking first, then cancelled; then a corner dot rounding the
rectangle, a shadow added in the inspector, and the ellipse's sweep handle making
a quarter pie), and copy and paste (`iron_document::copy`, in the Optics tab:
Cmd+D on the laser's body and beam makes a second laser entity both copies
picture, the connection to the fiber left behind; Cmd+C reaches the system
clipboard, which the driver may read because it grants clipboard permissions,
and a paste event carrying it makes a third laser; Cmd+X then a paste relinks
the figures to the third laser instead of making a fourth; four undos leave one
laser. Each `[lasers, connections, [figures per laser]]` line counts them), and
gesture ownership (in the same tab: a model edit to the laser body's x mid-drag
cancels the drag, so the edit stands and the release commits nothing; an edit
to the dial mid-drag leaves the drag running to its own "move rect"; three undos
put the body back), Alt-drag (Alt held from the press on the laser body drags
a copy, with a second laser entity, and leaves the original; one undo takes back
the duplicate and its move, which history holds as one entry, though the log
shows both), and the 3D gizmo (the torus selected in the Assembly tab, its
green Y arrow dragged up: y only, one "move mesh", undone), and SVG
(plans/specs/svg-import-export.md: the Controls composition exported, with its
cuts; Figma's "Copy as SVG" fixture `src/iron_svg/tests/fixtures/figma.svg`
imported into the Controls tab by `__iron.importSvg` as one "import SVG": two rects
(one rounded) and an ellipse recognised, the line a packed path of two anchors; Alt+Shift+C putting the
selection's SVG on the clipboard; undo; the same SVG imported by a paste event and
by the model's `<import-svg>`, each undone; each count is the Controls
composition's nodes). Expected output, minus screenshot lines,
`nav`/`ready` lines, and the per-frame `solve:` lines (2026-11-09; `regression.txt`, then
`gestures.txt`; a long number's last digits and a hunt's found point may vary):

```
{"enabled":false,"guides":[]}
["<rect id=\"#M\" name=\"square\" x=\"620\" y=\"340\"","<rect id=\"#U\" name=\"shadow\" x=\"720\" x.bind=\"#M.transform2d.x + 100\" y=\"340\" y.bind=\"#M.transform2d.y\""]
DEBUG src/document_bridge/render.rs:408 redraw (document): 2 of 36 nodes changed [#M #U]; damage 560,340–800,460
"<rect id=\"#12\" name=\"bar\" x=\"780\" y=\"370\" y.bind=\"640 - #11.port.signal * 300\" rot=\"0\" sx=\"1\" sy=\"1\" w=\"60\" h=\"270\" h.bind=\"#11.port.signal * 300\" color=\"#3380e6ff\" opacity=\"1\">"
"<rect id=\"#12\" name=\"bar\" x=\"730\" y=\"400\""
"value=\"0.8\"/>"
"<mesh id=\"#Q\" name=\"torus\" pos=\"0,1.5,0\" rot=\"0,4.886921905584122,0\" scale=\"1,1,1\" asset=\"torus\"> <driven-by ref=\"#S\" rel=\"animates\" prop=\"transform3d.rot\"/>"
{"ok":true,"version":9,"aliases":{"$n":"#3x"}}
"<edit-failed applied=\"none\" version=\"9\">\n  <error op=\"0\" verb=\"set #zz\" kind=\"unknown_node\">no node #zz</error>\n  <error op=\"1\" verb=\"set #N\" kind=\"cycle\">binding #N.radius.rx would depend on itself</error>\n</edit-failed>"
"<rect id=\"#3x\" name=\"from-model\" x=\"300\" y=\"120\" rot=\"0\" sx=\"1\" sy=\"1\" w=\"80\" w.bind=\"#O.slider.value * 100\" h=\"40\" color=\"#22aa55ff\" opacity=\"1\"/>"
["<ellipse id=\"#a\" name=\"ball\" x=\"440\" x.by=\"#V\" y=\"300\" y.by=\"#V\""," rot=\"-1.1254151928860334\""," h=\"240\""]
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 25 iterations, 5546 evaluations, 1.15 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 3 iterations, 1159 evaluations, 0.31 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.09 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.08 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.08 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.09 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.11 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.11 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.11 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.11 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.10 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.11 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.10 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.11 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.12 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.13 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.12 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.14 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.13 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 26 iterations, 5318 evaluations, 0.95 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 3 iterations, 394 evaluations, 0.27 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.13 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.08 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.10 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.10 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.10 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.09 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.12 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.10 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.11 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.11 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.10 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.11 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.12 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.12 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.12 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.12 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.12 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.12 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.14 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.20 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.15 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.24 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.23 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.14 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.12 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.09 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.07 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.07 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.06 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.07 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.09 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.08 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.08 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.09 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.09 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.07 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.09 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.14 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.10 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.10 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.12 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.12 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.12 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.11 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.14 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.14 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.18 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.25 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.26 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.23 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.14 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.21 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.20 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.27 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.22 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.14 ms
DEBUG src/bevy_app/manipulate.rs:426 solve: cluster #V, 12 vars, 1 iterations, 196 evaluations, 0.24 ms
DEBUG src/document_bridge/mod.rs:497 "move ellipse" by Human: cluster #V re-solved; moved 2 node(s): #Y #Z
"<ellipse id=\"#a\" name=\"ball\" x=\"323.714903353\" x.by=\"#V\" y=\"359.976998273\" y.by=\"#V\""
["<ellipse id=\"#a\" name=\"ball\" x=\"320\" x.by=\"#V\" y=\"360\" y.by=\"#V\"","<ellipse id=\"#Y\" name=\"j1\" x=\"320\" x.by=\"#V\" y=\"160\" y.by=\"#V\""]
DEBUG src/bevy_app/select.rs:3010 #b locked for this drag: #b.transform2d.x is bound to 4 free leaves (#X.transform2d.x, #X.transform2d.y, #Y.transform2d.x, #Y.transform2d.y)
DEBUG src/bevy_app/select.rs:3010 #b locked for this drag: #b.transform2d.y is bound to 4 free leaves (#X.transform2d.x, #X.transform2d.y, #Y.transform2d.x, #Y.transform2d.y)
"<applied version=\"14\">\n  <solver clusters=\"#V\" moved=\"#Y #Z #a\"/>\n  <unmet leaf=\"#a.transform2d.x\" requested=\"900\" solved=\"619.999999990091\">constrained by cluster #V: the constraints allow 619.999999990091 at most</unmet>\n</applied>"
"<edit-failed applied=\"none\" version=\"14\">\n  <error op=\"0\" verb=\"set #a\" kind=\"unsatisfiable\">exact transaction not honoured: #a.transform2d.x requested 900 but constraints allow 619.999999999234</error>\n</edit-failed>"
"<constraint id=\"#i\" name=\"link1\" kind=\"distance\" value=\"120.00000000000001\" value.bind=\"#f.slider.value * 100\" enabled=\"true\">"
true
"<ellipse id=\"#a\" name=\"ball\" x=\"320\" y=\"420\""
["<ellipse id=\"#a\" name=\"ball\" x=\"320\" y=\"420\"","<rect id=\"#W\" name=\"platform\" x=\"281.006443362\" x.by=\"#V\" y=\"38.199796347\" y.by=\"#V\""]
"x.by=\"#V\""
"#s"
found at 940 165
true
["#l","#Q"]
clicked Link
DEBUG src/document_bridge/mod.rs:492 applied "link" (4 ops, Human) -> v21
"<entity id=\"#3z\" name=\"box\" def=\"#3y\" mode=\"linked\" definition=\"thing\">\n    <figure id=\"#l\" type=\"rect\" name=\"box\" in=\"#1\" composition=\"scene\"/>\n    <figure id=\"#Q\" type=\"mesh\" name=\"torus\" in=\"#P\" composition=\"assembly\"/>\n  </entity>"
"<mesh id=\"#Q\" name=\"torus\" … asset=\"torus\" of=\"#3z\">"
clicked Show in assembly
clicked Show in scene
INFO src/bevy_app/scene3d.rs:258 focus #Q: panel assembly camera frames it
INFO src/bevy_app/scene3d.rs:258 focus #Q: panel assembly camera frames it
INFO src/bevy_app/scene3d.rs:232 focus #l: panel scene pans to centre it
INFO src/bevy_app/scene3d.rs:232 focus #l: panel scene pans to centre it
false
true
"<edit-failed applied=\"none\" version=\"23\">\n  <question kind=\"merge\" a=\"#12\" b=\"#Q\">\n    <entity id=\"#10\" name=\"bar\" definition=\"bar\" level=\"{= #O.slider.value}\" figures=\"#12\"/>\n    <entity id=\"#41\" name=\"box\" definition=\"thing\" figures=\"#l #Q\"/>\n    Both already picture an entity. Resend <link-figures> with keep=\"#10\" or keep=\"#41\"; the other entity is deleted and its figures move to the one kept.\n  </question>\n</edit-failed>"
[true,"<entity id=\"#10\" name=\"bar\" def=\"#w\" mode=\"detached\" level=\"0.8\" level.bind=\"#O.slider.value\" definition=\"bar\">\n    <port id=\"#11\" name=\"level\" kind=\"value\" dir=\"in\" signal=\"0.8\"/>\n    <figure id=\"#12\" type=\"rect\" name=\"bar\" in=\"#1\" composition=\"scene\"/>\n    <figure id=\"#l\" type=\"rect\" name=\"box\" in=\"#1\" composition=\"scene\"/>\n    <figure id=\"#Q\" type=\"mesh\" name=\"torus\" in=\"#P\" composition=\"assembly\"/>\n  </entity>"]
{"roundtrip_equal":true,"bytes":61933}
"<document version=\"25\"> |   <composition id=\"#1\" name=\"scene\" notation=\"free\" dims=\"2d\" run=\"auto\" count=\"26\"> |     <rect id=\"#2\" x=\"111\" y=\"231\" rot=\"0\" sx=\"1\" sy=\"1\" w=\"30\" h=\"30\" color=\"#986ecaff\" opacity=\"1\"/> |     <rect id=\"#3\" x=\"406\" y=\"241\" rot=\"0\" sx=\"1\" sy=\"1\" w=\"30\" h=\"30\" color=\"#9d399aff\" opacity=\"1\"/>"
Access to fetch at 'http://127.0.0.1:4318/pair' from origin 'http://localhost:5173' has been blocked by CORS policy: Response to preflight request doesn't pass access control check: No 'Access-Control-Allow-Origin' header is present on the requested resource.
DEBUG src/document_bridge/mod.rs:492 applied "open demo scene" (302 ops, System) -> v1
DEBUG src/document_bridge/mod.rs:492 applied "move rect" (2 ops, Human) -> v2
DEBUG src/document_bridge/mod.rs:492 applied "move mesh" (1 ops, Human) -> v3
DEBUG src/document_bridge/mod.rs:492 applied "undo: move mesh" (1 ops, System) -> v4
DEBUG src/document_bridge/mod.rs:492 applied "undo: move rect" (2 ops, System) -> v5
DEBUG src/document_bridge/mod.rs:492 applied "redo: move rect" (2 ops, System) -> v6
DEBUG src/document_bridge/mod.rs:492 applied "set slider" (1 ops, Human) -> v7
DEBUG src/document_bridge/mod.rs:492 applied "move rect" (2 ops, Human) -> v8
DEBUG src/document_bridge/mod.rs:492 applied "model edit" (2 ops, Model) -> v9
DEBUG src/document_bridge/mod.rs:492 applied "move ellipse" (2 ops, Human) -> v10
DEBUG src/document_bridge/mod.rs:497 "move ellipse" by Human: cluster #V re-solved; moved 2 node(s): #Y #Z
DEBUG src/document_bridge/mod.rs:492 applied "move ellipse" (3 ops, Human) -> v11
DEBUG src/document_bridge/mod.rs:497 "move ellipse" by Human: cluster #V re-solved; moved 2 node(s): #Y #Z
DEBUG src/document_bridge/mod.rs:492 applied "undo: move ellipse" (7 ops, System) -> v12
DEBUG src/document_bridge/mod.rs:492 applied "undo: move ellipse" (6 ops, System) -> v13
DEBUG src/bevy_app/select.rs:3010 #b locked for this drag: #b.transform2d.x is bound to 4 free leaves (#X.transform2d.x, #X.transform2d.y, #Y.transform2d.x, #Y.transform2d.y)
DEBUG src/bevy_app/select.rs:3010 #b locked for this drag: #b.transform2d.y is bound to 4 free leaves (#X.transform2d.x, #X.transform2d.y, #Y.transform2d.x, #Y.transform2d.y)
DEBUG src/document_bridge/mod.rs:492 applied "model edit" (1 ops, Model) -> v14
DEBUG src/document_bridge/mod.rs:497 "model edit" by Model: cluster #V re-solved; moved 3 node(s): #Y #Z #a; unmet: #a.transform2d.x wanted 900 got 619.999999990091
DEBUG src/document_bridge/mod.rs:492 applied "undo: model edit" (7 ops, System) -> v15
DEBUG src/document_bridge/mod.rs:492 applied "set slider" (1 ops, Human) -> v16
DEBUG src/document_bridge/mod.rs:497 "set slider" by Human: cluster #V re-solved; moved 3 node(s): #Y #Z #a
DEBUG src/document_bridge/mod.rs:492 applied "model edit" (2 ops, Model) -> v17
DEBUG src/document_bridge/mod.rs:492 applied "move rect" (3 ops, Human) -> v18
DEBUG src/document_bridge/mod.rs:497 "move rect" by Human: cluster #V re-solved; moved 3 node(s): #X #Y #Z
DEBUG src/document_bridge/mod.rs:492 applied "move ellipse" (2 ops, Human) -> v19
DEBUG src/document_bridge/mod.rs:492 applied "undo: move ellipse" (2 ops, System) -> v20
DEBUG src/document_bridge/mod.rs:492 applied "link" (4 ops, Human) -> v21
DEBUG src/document_bridge/mod.rs:492 applied "undo: link" (4 ops, System) -> v22
DEBUG src/document_bridge/mod.rs:492 applied "link" (4 ops, Model) -> v23
DEBUG src/document_bridge/mod.rs:492 applied "link" (5 ops, Model) -> v24
DEBUG src/document_bridge/mod.rs:492 applied "undo: link" (5 ops, System) -> v25
DEBUG src/document_bridge/mod.rs:433 loaded document v25 (294 live nodes)
(no page errors)
```

```
{"enabled":false,"guides":[]}
undefined
["#M"]
["#M viewer [#o #M]"]
{"x":500,"y":420,"w":120,"h":100,"deg":0}
{"x":553,"y":392,"w":120,"h":100,"deg":45}
{"x":500,"y":420,"w":80,"h":80,"deg":0}
["#V"]
["#a"]
["#V"]
["#15"]
[]
["#o","#15","#16"]
{"x":501,"y":430,"w":80,"h":80,"deg":0}
true
true
tab Controls
"Pen (P)"
undefined
["closed=\"true\"",3,"smooth point"]
"closed=\"false\""
["#3x"]
"<point id=\"#41\" x=\"130\" y=\"220\""
"pen-add"
[4,"<point id=\"#42\" x=\"86.9\" y=\"-22.4\" ix=\"-47.2\" iy=\"7.5\" ox=\"47.6\" oy=\"-7.5\"/>"]
3
"bend"
" ox=\"0.0\" oy=\"-63.4\""
2
"pen-remove"
2
tab Main
undefined
true
true
"<rect id=\"#M\" name=\"square\" x=\"620\" y=\"430\" rot=\"0\" sx=\"1\" sy=\"1\" w=\"160\" h=\"80\""
tab Optics
undefined
undefined
[190.6,71.5]
["power=\"10 mW\"","signal=\"7.94328234724 mW\"",["<connection a=\"laser.out\" b=\"fiber.in\" kind=\"optical_power\"/>","<connection a=\"fiber.out\" b=\"meter.in\" kind=\"optical_power\"/>","<connection a=\"heater.out\" b=\"thermal link.a\" kind=\"heat_flow\"/>","<connection a=\"thermal link.b\" b=\"ambient.p\" kind=\"heat_flow\"/>","<connection a=\"source.out\" b=\"fiber.in\" kind=\"optical_power\"/>","<connection a=\"fiber.out\" b=\"meter.in\" kind=\"optical_power\"/>"]]
[75.9,28.5]
["power=\"15.7894737167 mW\"","signal=\"4.99307 mW\"",["<connection a=\"laser.out\" b=\"fiber.in\" kind=\"optical_power\"/>","<connection a=\"fiber.out\" b=\"meter.in\" kind=\"optical_power\"/>","<connection a=\"heater.out\" b=\"thermal link.a\" kind=\"heat_flow\"/>","<connection a=\"thermal link.b\" b=\"ambient.p\" kind=\"heat_flow\"/>","<connection a=\"source.out\" b=\"fiber.in\" kind=\"optical_power\"/>","<connection a=\"fiber.out\" b=\"meter.in\" kind=\"optical_power\"/>"]]
"<edit-failed applied=\"none\" version=\"27\">\n  <error op=\"2\" verb=\"link $c connects $p\" kind=\"bad_connection\">$c: cannot connect rf_voltage ($p) to optical_power (#…); a connection joins ports of one kind</error>\n</edit-failed>"
tab Thermal
undefined
["out 303.15 K -5 W","a 303.15 K 5 W","b 293.15 K -5 W","p 293.15 K 5 W"]
["out 313.15 K -10 W","a 313.15 K 10 W","b 293.15 K -10 W","p 293.15 K 10 W"]
tab Controls
undefined
clicked .toolbar button[aria-label="Rectangle"]
"Rectangle (R)"
["<rect id=\"#43\" name=\"rectangle\" x=\"500\" y=\"100\" rot=\"0\" sx=\"1\" sy=\"1\" w=\"140\" h=\"80\"","Move (V)"]
"<ellipse id=\"#44\" name=\"ellipse\" x=\"740\" y=\"140\" rot=\"0\" sx=\"1\" sy=\"1\" rx=\"40\" ry=\"40\""
"<ellipse id=\"#44\" name=\"ellipse\" x=\"770\" y=\"140\" rot=\"0\" sx=\"1\" sy=\"1\" rx=\"70\" ry=\"40\""
4
"<ellipse id=\"#44\" name=\"ellipse\" x=\"770\" y=\"140\" rot=\"0\" sx=\"1\" sy=\"1\" rx=\"70\" ry=\"40\""
tab Main
{"cuts":["its h is bound to #11.port.signal * 300; the path keeps the current value"],"ok":false}
["its h is bound to #11.port.signal * 300; the path keeps the current value"]
clicked Cancel
null
tab Controls
"<rect id=\"#43\" name=\"rectangle\" x=\"500\" y=\"100\" rot=\"0\" sx=\"1\" sy=\"1\" w=\"140\" h=\"80\" tl=\"30\" tr=\"30\" br=\"30\" bl=\"30\""
clicked Add effect
clicked [data-effect-type="shadow"]
" shadow.color=\"#00000040\" dx=\"0\" dy=\"4\" blur=\"4\""
"<ellipse id=\"#44\" name=\"ellipse\" x=\"770\" y=\"140\" rot=\"0\" sx=\"1\" sy=\"1\" rx=\"70\" ry=\"40\" start=\"0\" sweep=\"1.5707963267948966\" inner=\"0\""
tab Optics
undefined
undefined
[2,2,6,[2,2]]
true
undefined
[3,6,[2,2,2]]
[3,6,[2,2]]
undefined
[3,6,[2,2,2]]
[1,6,[2]]
tab Optics
undefined
true
" x=\"80\" y=\"140\""
true
" x=\"180\" y=\"190\""
" x=\"60\" y=\"140\""
undefined
[["60,140","160,290"],2,1]
[["60,140"],1]
tab Assembly
undefined
[true,true,true]
[0,1.5,0]
tab Pendulum
[true,false]
tab Main
[false,true]
tab Controls
undefined
"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"741\" height=\"527\" viewBox=\"99 98 741 527\" data-iron=\"1\">\n  <defs>\n    <filter id=\"shadow-1\" filterUnits=\"userSpaceOnUse\" x=\"494\" y=\"98\" width=\"152\" height=\"92\" color-interpolation-filters=\"sRGB\">\n      <feDropShadow dx=\"0\" dy=\"4\" stdDeviation=\"2\" flood-color=\"#000000\" flood-opacity=\"0.25\"/>\n    </filter>\n  </defs>\n  <g id=\"level\" data-iron-type=\"slider\" data-iron-name=\"level\" transform=\"translate(100 600)\">\n    <rect y=\"9\" width=\"300\" height=\"6\" rx=\"3\" fill=\"#bfc2cc\"/>\n    <rect y=\"9\" width=\"120\" height=\"6\" rx=\"3\" fill=\"#3380e6\"/>\n    <circle cx=\"120\" cy=\"12\" r=\"12\" fill=\"#ffffff\" stroke=\"#4d4d59\" stroke-width=\"1.5\"/>\n  </g>\n  <path id=\"path\" data-iron-type=\"path\" data-iron-name=\"path\" transform=\"translate(200 200)\" d=\"M0 0 C0 0 160 -60 220 0 C280 60 130 220 130 220\" fill=\"none\" stroke=\"#000000\" stroke-width=\"2\" stroke-linejoin=\"round\" stroke-linecap=\"round\"/>\n  <g filter=\"url(#shadow-1)\">\n    <rect id=\"rectangle\" data-iron-type=\"rect\" data-iron-name=\"rectangle\" transform=\"translate(500 100)\" width=\"140\" height=\"80\" rx=\"30\" fill=\"#d9d9d9\" data-iron-shadow=\"#000000 0.25 0 4 4\"/>\n  </g>\n  <path id=\"ellipse\" data-iron-type=\"ellipse\" data-iron-name=\"ellipse\" transform=\"translate(770 140)\" d=\"M0 0 L70 0 C70 22.0914 38.6599 40 0 40 Z\" data-iron-rx=\"70\" data-iron-ry=\"40\" data-iron-arc=\"0 1.5707963267948966 0\" fill=\"#d9d9d9\"/>\n</svg>\n"
["slider level is exported as a picture of itself; it imports as shapes"]
4
{"anchors":2,"cuts":["filters are not imported"],"definitions":0,"nodes":4,"ok":true,"references":0}
[8,4]
["<rect id=\"#4Q\" x=\"10\" y=\"10\" rot=\"0\" sx=\"1\" sy=\"1\" w=\"100\" h=\"60\" tl=\"8\" tr=\"8\" br=\"8\" bl=\"8\"","<ellipse id=\"#4R\" x=\"150\" y=\"40\" rot=\"0\" sx=\"1\" sy=\"1\" rx=\"30\" ry=\"30\"","<path id=\"#4S\" x=\"0\" y=\"0\" rot=\"0\" sx=\"1\" sy=\"1\" fill_rule=\"nonzero\" d=\"1 contour, 2 anchors\"","<rect id=\"#4T\" x=\"140\" y=\"80\" rot=\"0\" sx=\"1\" sy=\"1\" w=\"40\" h=\"40\""]
" d=\"1 contour, 2 anchors\""
[true,4]
4
undefined
8
4
{"ok":true,"version":60,"summary":"<applied version=\"60\">\n  <imported roots=\"#4Y #4Z #4a #4b\"/>\n  <cut>filters are not imported</cut>\n</applied>"}
8
4
tab Components
undefined
["fiber detached","meter detached","source linked","fiber linked","meter linked","spare laser linked","short fiber linked","long fiber linked"]
undefined
["#…/3V","7.94328234724 mW"]
{"derivedNodesSaved":0,"connectionsToPorts":4}
[0,5]
["#…/3V","15.8865646945 mW"]
["source lasersource",["power"]]
clicked button[aria-label="Detach instance"]
["fiber detached","meter detached","source linked","fiber linked","meter linked","spare laser detached","short fiber linked","long fiber linked"]
["fiber detached","meter detached","source linked","fiber linked","meter linked","spare laser linked","short fiber linked","long fiber linked"]
undefined
[true,"-0.64","20 mW"]
[true,["<override leaf=\"transform2d.x\" value=\"620\"/>","<override leaf=\"fill.color\" value=\"#3366ccff\"/>"]]
[" x=\"620\""," fill.color=\"#3366ccff\""]
[" x=\"600\"",0]
undefined
[[545,175],[545,175]]
[true,[[665,435],[665,435]]]
[true,[[645,435],[645,435]]]
[[545,175],[545,175]]
clicked button[title^="Make a component"]
[["power value in","out optical_power out"],"#….slider.value * 0.001 W",["fiber detached","meter detached","meter linked","spare laser linked","short fiber linked","long fiber linked"]]
["#…/3V","15.8865646945 mW"]
undefined
[2,[[240,175],[545,175]]]
[2,[[240,175],[665,435]]]
["fiber detached","meter detached","source linked","fiber linked","meter linked","spare laser linked","short fiber linked","long fiber linked"]
"kind=\"out_of_scope\""
undefined
undefined
[[545,175],[240,475],["meter 10 mW","short fiber 0 W"]]
[[340,475],[340,475]]
[[545,175],[545,175],["meter 15.8865646945 mW","short fiber 0 W"]]
[[240,475],["meter 15.8865646945 mW","short fiber 10 mW"]]
["meter 15.8865646945 mW","short fiber 0 W"]
clicked button[title^="Make a component"]
"component use #52"
clicked button[title="Shape tools"]
clicked [role="menuitem"]:has-text("Port")
"in"
clicked button[title^="Wrap the selection"]
"\"Link in\""
clicked .link-popover button.primary
1
1
[[545,175],[683,320],["meter 0 W","short fiber 0 W"]]
[[465,300],[545,175]]
[[600,300],[600,300],["meter 15.8865646945 mW","short fiber 0 W"]]
tab Library
["slider level 18","laser power 18 mW"]
tab Controls
"controls"
["Render inspector watching controls","Chunks","1 1 in view","Nodes","8"]
DEBUG src/document_bridge/mod.rs:492 applied "open demo scene" (302 ops, System) -> v1
DEBUG src/document_bridge/mod.rs:492 applied "resize rect" (2 ops, Human) -> v2
DEBUG src/document_bridge/mod.rs:492 applied "rotate rect" (3 ops, Human) -> v3
DEBUG src/document_bridge/mod.rs:492 applied "undo: rotate rect" (3 ops, System) -> v4
DEBUG src/document_bridge/mod.rs:492 applied "undo: resize rect" (2 ops, System) -> v5
DEBUG src/document_bridge/mod.rs:492 applied "nudge rect" (1 ops, Human) -> v6
DEBUG src/document_bridge/mod.rs:492 applied "nudge rect" (1 ops, Human) -> v7
DEBUG src/document_bridge/mod.rs:492 applied "delete group" (1 ops, Human) -> v8
DEBUG src/document_bridge/mod.rs:492 applied "undo: delete group" (16 ops, System) -> v9
DEBUG src/document_bridge/mod.rs:492 applied "draw path" (3 ops, Human) -> v10
DEBUG src/document_bridge/mod.rs:492 applied "add point" (1 ops, Human) -> v11
DEBUG src/document_bridge/mod.rs:492 applied "add point" (1 ops, Human) -> v12
DEBUG src/document_bridge/mod.rs:492 applied "close path" (1 ops, Human) -> v13
DEBUG src/document_bridge/mod.rs:492 applied "undo: close path" (1 ops, System) -> v14
DEBUG src/document_bridge/mod.rs:492 applied "move point" (2 ops, Human) -> v15
DEBUG src/document_bridge/mod.rs:492 applied "add point" (5 ops, Human) -> v16
DEBUG src/document_bridge/mod.rs:492 applied "undo: add point" (5 ops, System) -> v17
DEBUG src/document_bridge/mod.rs:492 applied "bend segment" (4 ops, Human) -> v18
DEBUG src/document_bridge/mod.rs:492 applied "undo: bend segment" (4 ops, System) -> v19
DEBUG src/document_bridge/mod.rs:492 applied "delete point" (1 ops, Human) -> v20
DEBUG src/document_bridge/mod.rs:492 applied "undo: delete point" (1 ops, System) -> v21
DEBUG src/document_bridge/mod.rs:492 applied "delete point" (1 ops, Human) -> v22
DEBUG src/document_bridge/mod.rs:492 applied "undo: delete point" (1 ops, System) -> v23
DEBUG src/document_bridge/mod.rs:492 applied "set size.w" (1 ops, Human) -> v24
DEBUG src/document_bridge/mod.rs:492 applied "set transform2d.x" (1 ops, Human) -> v25
DEBUG src/document_bridge/mod.rs:492 applied "set slider" (1 ops, Human) -> v26
DEBUG src/document_bridge/mod.rs:492 applied "resize rect" (1 ops, Human) -> v27
DEBUG src/document_bridge/mod.rs:492 applied "set slider" (1 ops, Human) -> v28
DEBUG src/document_bridge/mod.rs:492 applied "draw rectangle" (1 ops, Human) -> v29
DEBUG src/document_bridge/mod.rs:492 applied "draw ellipse" (1 ops, Human) -> v30
DEBUG src/document_bridge/mod.rs:492 applied "resize ellipse" (2 ops, Human) -> v31
DEBUG src/document_bridge/mod.rs:492 applied "convert ellipse to path" (7 ops, Human) -> v32
DEBUG src/document_bridge/mod.rs:492 applied "undo: convert ellipse to path" (7 ops, System) -> v33
DEBUG src/document_bridge/mod.rs:492 applied "round corners" (1 ops, Human) -> v34
DEBUG src/document_bridge/mod.rs:492 applied "add shadow" (1 ops, Human) -> v35
DEBUG src/document_bridge/mod.rs:492 applied "edit arc" (1 ops, Human) -> v36
DEBUG src/document_bridge/mod.rs:492 applied "duplicate 2 nodes" (7 ops, Human) -> v37
DEBUG src/document_bridge/mod.rs:492 applied "paste 2 nodes" (7 ops, Human) -> v38
DEBUG src/document_bridge/mod.rs:492 applied "cut 2 nodes" (2 ops, Human) -> v39
DEBUG src/document_bridge/mod.rs:492 applied "paste 2 nodes" (4 ops, Human) -> v40
DEBUG src/document_bridge/mod.rs:492 applied "undo: paste 2 nodes" (4 ops, System) -> v41
DEBUG src/document_bridge/mod.rs:492 applied "undo: cut 2 nodes" (2 ops, System) -> v42
DEBUG src/document_bridge/mod.rs:492 applied "undo: paste 2 nodes" (7 ops, System) -> v43
DEBUG src/document_bridge/mod.rs:492 applied "undo: duplicate 2 nodes" (7 ops, System) -> v44
DEBUG src/document_bridge/mod.rs:492 applied "model edit" (1 ops, Model) -> v45
DEBUG src/document_bridge/mod.rs:492 applied "model edit" (1 ops, Model) -> v46
DEBUG src/document_bridge/mod.rs:492 applied "move rect" (2 ops, Human) -> v47
DEBUG src/document_bridge/mod.rs:492 applied "undo: move rect" (2 ops, System) -> v48
DEBUG src/document_bridge/mod.rs:492 applied "undo: model edit" (1 ops, System) -> v49
DEBUG src/document_bridge/mod.rs:492 applied "undo: model edit" (1 ops, System) -> v50
DEBUG src/document_bridge/mod.rs:492 applied "duplicate rect" (5 ops, Human) -> v51
DEBUG src/document_bridge/mod.rs:492 applied "duplicate rect" (2 ops, Human) -> v52
DEBUG src/document_bridge/mod.rs:492 applied "undo: duplicate rect" (7 ops, System) -> v53
DEBUG src/document_bridge/mod.rs:492 applied "move mesh" (1 ops, Human) -> v54
DEBUG src/document_bridge/mod.rs:492 applied "undo: move mesh" (1 ops, System) -> v55
DEBUG src/document_bridge/mod.rs:492 applied "import SVG" (4 ops, Human) -> v56
DEBUG src/document_bridge/mod.rs:492 applied "undo: import SVG" (4 ops, System) -> v57
DEBUG src/document_bridge/mod.rs:492 applied "import SVG" (4 ops, Human) -> v58
DEBUG src/document_bridge/mod.rs:492 applied "undo: import SVG" (4 ops, System) -> v59
DEBUG src/document_bridge/mod.rs:492 applied "import SVG" (4 ops, Model) -> v60
DEBUG src/document_bridge/mod.rs:492 applied "undo: import SVG" (4 ops, System) -> v61
DEBUG src/document_bridge/mod.rs:492 applied "set slider" (1 ops, Human) -> v62
DEBUG src/document_bridge/mod.rs:492 applied "detach instance" (6 ops, Human) -> v63
DEBUG src/document_bridge/mod.rs:492 applied "undo: detach instance" (6 ops, System) -> v64
DEBUG src/document_bridge/mod.rs:492 applied "model edit" (1 ops, Model) -> v65
DEBUG src/document_bridge/mod.rs:492 applied "undo: model edit" (1 ops, System) -> v66
DEBUG src/document_bridge/mod.rs:492 applied "model edit" (2 ops, Model) -> v67
DEBUG src/document_bridge/mod.rs:492 applied "undo: model edit" (2 ops, System) -> v68
DEBUG src/document_bridge/mod.rs:492 applied "model edit" (2 ops, Model) -> v69
DEBUG src/document_bridge/mod.rs:492 applied "model edit" (1 ops, Model) -> v70
DEBUG src/document_bridge/mod.rs:492 applied "undo: model edit" (1 ops, System) -> v71
DEBUG src/document_bridge/mod.rs:492 applied "undo: model edit" (2 ops, System) -> v72
DEBUG src/document_bridge/mod.rs:492 applied "make component component" (24 ops, Human) -> v73
DEBUG src/document_bridge/mod.rs:492 applied "model edit" (2 ops, Model) -> v74
DEBUG src/document_bridge/mod.rs:492 applied "undo: model edit" (2 ops, System) -> v75
DEBUG src/document_bridge/mod.rs:492 applied "undo: make component component" (24 ops, System) -> v76
DEBUG src/document_bridge/mod.rs:492 applied "disconnect in" (3 ops, Human) -> v77
DEBUG src/document_bridge/mod.rs:492 applied "disconnect in" (2 ops, Human) -> v78
DEBUG src/document_bridge/mod.rs:492 applied "connect in to out" (2 ops, Human) -> v79
DEBUG src/document_bridge/mod.rs:492 applied "connect in to out" (3 ops, Human) -> v80
DEBUG src/document_bridge/mod.rs:492 applied "move use" (2 ops, Human) -> v81
DEBUG src/document_bridge/mod.rs:492 applied "undo: move use" (4 ops, System) -> v82
DEBUG src/document_bridge/mod.rs:492 applied "undo: connect in to out" (10 ops, System) -> v83
DEBUG src/document_bridge/mod.rs:492 applied "connect in to out" (3 ops, Human) -> v84
DEBUG src/document_bridge/mod.rs:492 applied "undo: connect in to out" (3 ops, System) -> v85
DEBUG src/document_bridge/mod.rs:492 applied "draw ellipse" (1 ops, Human) -> v86
DEBUG src/document_bridge/mod.rs:492 applied "draw rectangle" (1 ops, Human) -> v87
DEBUG src/document_bridge/mod.rs:492 applied "make component component" (9 ops, Human) -> v88
DEBUG src/document_bridge/mod.rs:492 applied "add port" (2 ops, Human) -> v89
DEBUG src/document_bridge/mod.rs:492 applied "wrap in empty" (4 ops, Human) -> v90
DEBUG src/document_bridge/mod.rs:492 applied "bind transform2d.rot" (1 ops, Human) -> v91
DEBUG src/document_bridge/mod.rs:492 applied "duplicate use" (3 ops, Human) -> v92
DEBUG src/document_bridge/mod.rs:492 applied "disconnect in" (3 ops, Human) -> v93
DEBUG src/document_bridge/mod.rs:492 applied "disconnect in" (2 ops, Human) -> v94
DEBUG src/document_bridge/mod.rs:492 applied "disconnect in" (2 ops, Human) -> v95
DEBUG src/document_bridge/mod.rs:492 applied "undo: disconnect in" (7 ops, System) -> v96
DEBUG src/document_bridge/mod.rs:492 applied "disconnect out" (3 ops, Human) -> v97
DEBUG src/document_bridge/mod.rs:492 applied "disconnect out" (2 ops, Human) -> v98
DEBUG src/document_bridge/mod.rs:492 applied "disconnect out" (2 ops, Human) -> v99
DEBUG src/document_bridge/mod.rs:492 applied "undo: disconnect out" (7 ops, System) -> v100
DEBUG src/document_bridge/mod.rs:492 applied "move use" (2 ops, Human) -> v101
DEBUG src/document_bridge/mod.rs:492 applied "undo: move use" (2 ops, System) -> v102
DEBUG src/document_bridge/mod.rs:492 applied "add slider" (8 ops, Human) -> v103
DEBUG src/document_bridge/mod.rs:492 applied "add slider" (3 ops, Human) -> v104
DEBUG src/document_bridge/mod.rs:492 applied "add laser" (7 ops, Human) -> v105
DEBUG src/document_bridge/mod.rs:492 applied "add laser" (3 ops, Human) -> v106
DEBUG src/document_bridge/mod.rs:492 applied "connect value to power" (3 ops, Human) -> v107
DEBUG src/document_bridge/mod.rs:492 applied "set slider" (1 ops, Human) -> v108
(no page errors)
```

Both halves of the smoke start with `__iron.snapping(false)` (the
`{"enabled":false,…}` lines): their drags aim at exact points, and object snapping
would pull them a few px onto nearby edges. `snapping.txt` tests snapping itself.

The Components section (component-model.md phase 2) lists every entity with
its `mode`: the optics tab's are `detached` (static instances, copies), the
Components tab's `linked`. The meter's `in` port has a derived id
(`#<instance>/<local>`, shown as `#…/1M`); the saved file holds no derived
node but four connection ends that name derived ports; the tree view of the
composition shows no `derived="true"` node until the meter's `use` is
expanded (3). Dragging the slider to its end doubles the source's power, so
the meter reads 15.88… mW; clicking the laser's body selects its `use` and the
inspector's Component section reads `laser` / `source` with one input,
`power`; Detach makes the spare laser `detached` and undo links it again.
Phase 3: turning the dial meter's needle pivot to full scale (rot 0) is
translated to the slider, which stops at its bound, so the needle settles at −0.64 and the
source reads 20 mW; setting the dial's x and fill colour makes two `override`
children of its `use` (undo removes them); a text bound to the needle's
rotation from outside is refused `out_of_scope`. Anchors: the fiber's `out`
end and the meter's `in` port both sit at (545, 175); moving the meter to
(120, 260) carries both to (665, 435); setting the fiber end's x to 645
inverts onto the meter, so both read (645, 435); two undos put them back. Make
component (phase 4): shift-picking the source laser and the fiber and pressing
Make component gives one `component` entity with ports `power value in` (its
binding the slider's, `#….slider.value * 0.001 W`) and `out optical_power out`;
the source and fiber entities leave the model view; the meter still reads
15.88… mW; the component's picture holds 2 inner pictures, whose `out` anchors
sit at (240, 175) (the laser's) and (545, 175) (the fiber's end), and moving the
meter carries the nested fiber end to (665, 435); two undos restore the
source and fiber. Plugging (component-model.md §4.3): the meter dragged by its
`in` port onto the spare laser's `out` is unplugged from the fiber for the
drag ("disconnect in", then the fiber end pinned where it was, both merged),
snaps on, and connects ("connect in to out", the move and the connection
merged into the same entry). It reads the spare's 10 mW, and the fiber's `out`
end stays at (545, 175). Dragging the spare laser by its body moves the
mated meter with it (both anchors at (340, 475), one "move use" with the
solver's writes). Two undos restore the chain (15.88… mW). The short fiber's
loose `in` end dropped on the spare laser plugs in (10 mW), and one undo
unplugs it. The meter is grabbed by its half of the joint handle (its port
is joined to the fiber's end). Component authoring (component-authoring.md):
an ellipse and a needle drawn top right and marquee-picked become a
`component` whose main is selected; the port tool on its left edge adds
`in`; entering the main (two clicks on the needle) and Wrap in empty makes
the needle's empty; a line from the main's `in` pin onto it opens the link
popover ("Link in"), whose Link binds the empty's rotation ("bind
transform2d.rot"); the main's Ports list has 1 row; Cmd+D on the main makes
1 `component` entity. The joint handle on the meter: its half pulls the
meter off (0 W, the fiber's end stays), the fiber's half pulls the end off
(the meter stays), the centre moves the meter to (600, 300) with the end on
it (15.88… mW); each undone. The Library tab: a slider and a laser dragged
out (each "add …" twice: the definition copied in, then the instance, one
history entry), the slider's `value` pin lined to the laser's `power`
("connect value to power"), its knob scrubbed to 18: the laser reads 18 mW. The first `redraw (document)`
line, listing the dial meter's readout (`#3b`, its template's text, and
`#3i/3b`, the instance's), is the default font arriving: whether it lands
before the square's drag, and so appears, depends on the font fetch.

Solved values carry solver noise in their last digits (shown as `…`
above); everything else is exact. The square's drag logs one or two `redraw
(document)` lines, depending on how many frames land before the `peek`. After the platform drag the pendulum is
not always fully settled, so a later transaction (the links) may log a
`"link" by …: cluster #V re-solved; moved 1 node(s): #W` line and its undo
one op more; that is the under-converged solve noted in
`constraint-system.md` §14, not the link. Each drag frame through a cluster logs
`solve: cluster #U, 12 vars, N iterations, M evaluations, T ms`; T is
0.0–0.7 ms here, with a single ~2 ms frame where the chain straightens.

Any `locked for this drag` line names an axis the drag held still and why;
any `differs from Bevy` line means the store's world transform disagrees
with the entity's and is a bug. Every rebuild of the 2D layer logs one
`redraw (<reason>): N of M nodes changed [ids]; damage x0,y0–x1,y1` line (see
"Engine event log" for turning it on):
the reason is `document` (the reconciler's changed-node set, with
`, structure` when the tree's shape changed), `panel layout`, or
`selection`, `embedded view`, `new view` or `spatial projection`; damage
is the diagnostic union of independently prepared old/new painted bounds.
Those bounds belong to canonical composition spaces, so that union is not yet
a view-space partial-rasterization command. `M` now counts submitted visible
leaves across views, so it can be smaller than historical expected-output totals.

`__iron.edit(xml)` applies `<edit>` markup exactly as a model reply would be
applied (see `src-ui/model/edit.ts` for the verbs); `__iron.ask(text)` runs a
real model turn and needs an Anthropic API key entered in the Document
panel, so the smoke script does not call it.

`window.__iron` is the document API for `eval`: `await __iron.save()` returns
the canonical JSON, `await __iron.load(text)` replaces the document, and
`await __iron.view({fidelity:'summary', depth: 2})` renders a view.
`await __iron.bench(20, 200)` runs the constraint solver's benchmark
(`iron_solve::bench`, a 12-variable chain) in this build and returns its
per-solve time in microseconds, for comparison with the native
`cargo run -p iron_solve --release --example bench --target aarch64-apple-darwin`.
Under SwiftShader the number is still meaningful: the solver is CPU work in
the worker, not rendering. The
Document panel on the left shows the full tree view and refreshes on every
applied transaction.

Screenshots land in `.claude/skills/run-iron/screenshots/<name>.png`. Look at
them; a frame showing only "Loading..." means the engine never came up.

The viewer shows the 2D `main` composition (ECM §7.4), which embeds the
`scene` composition at the origin (so document coordinates read the same
in the viewer) and the 3D `assembly` composition as a camera view at
document (815,10), 200×350, through the derived default view that frames
the torus and the card plane. Loose 2D roots such as the pendulum draw in
the viewer too. The
right panel is a placeholder. `?panels=2` makes it an `assembly` panel
with its own camera; `?panels=4` adds a second view of each, for
measuring per-panel cost. A 3D view inside 2D is its camera's texture,
drawn in painter order under the embed's whole transform: it rotates and
scales with the embed, covers what is under it (the square after the
timeline scrub slides beneath it), and the 2D picker treats it as opaque.

Driver environment: `GPU=1` runs Chrome on the machine's GPU (ANGLE/Metal)
instead of SwiftShader, which caps the app near 10 fps; `UNTHROTTLED=1`
lifts vsync and the frame-rate limit so frames per second shows
throughput; `SIZE=2560x1440@2` sets the window and device scale (that one
is 5K); `URL=…` replaces the default page. For frame times:
`__probeReset()`, act, then `__probe()` logs and stores `__lastProbeStats`
(`effectiveFps`, `tickMs`, `rafDeltaMs`).
`__loseDevice()` destroys the GPU device: with `?gfx=webgpu` the console
shows `WebGPU device lost`, `[gfx] restarting the renderer after DeviceLost`,
and the views render again with the document kept (src/render_loss.rs).

Every view renders into its own texture and the window only composites
(`plans/specs/view-textures.md`), and a frame that changes nothing visible is not
rendered at all (render on demand), so an idle app costs ≈0.1 ms per tick
at any size. A frame that keeps rendering while nothing moves means
something writes state every frame; `crate::decide_redraw` lists what
counts as a change.

The driver reads one command per line, so a heredoc is a script:

```bash
cd .claude/skills/run-iron && bun run drive.ts <<'EOS'
nav
ready
ddrag 540 460 690 360
shot moved
logs applied
quit
EOS
cd -
```

| command | what it does |
|---|---|
| `nav [url]` | open the app; default `http://localhost:5173/Ironfell/?gfx=webgl2&showcase=0&demo&bevy=debuglog` (`&demo`: an unsaved demo, not the last scope; plans/specs/scopes.md D11) |
| `ready` | wait until the `.loading` overlay leaves the DOM and the frame loop is cycling (cadence probe frame count advancing between polls), then read the viewer panel rect. About 6 s under SwiftShader |
| `shot <name>` | screenshot to `screenshots/<name>.png` |
| `move x y`, `down [button]`, `up [button]`, `drag x0 y0 x1 y1 [steps]` | pointer in window pixels; `down right` … `up right` around `move`s orbits a 3D panel's camera |
| `dmove x y`, `ddrag x0 y0 x1 y1 [steps]` | the same in document 2D space: viewer-panel top-left origin, y-down, the space `transform2d.x/y` are in |
| `pdrag <panel> x0 y0 x1 y1 [steps]` | drag relative to a named DOM panel (`viewer`, `assembly`, `timeline`); in the timeline, a press or drag anywhere on the ruler (y 0–56, time t at x = 160 + t·100) scrubs, a drag of a bar retimes its clip and a drag of a key diamond moves the key |
| `key <Key>` | press a key (Playwright key names); `Meta+z` undoes, `Meta+Shift+z` redoes |
| `keydown <Key>`, `keyup <Key>` | hold and release a key, e.g. `Shift` around a `pclick` to add to the selection |
| `pclick <panel> x y` | click inside a named panel, panel-relative |
| `pdouble <panel> x y` | double-click inside a named panel (two clicks 90 ms apart; two `pclick`s, 0.6 s each, miss the select tool's 0.4 s window) |
| `wheel dx dy` | scroll at the pointer: pans a 2D view; hold `Control` (`keydown Control`) to zoom about the pointer, as a pinch does |
| `tab <title>` | activate the dock tab with that title (`Pendulum`, `Assembly`, …) |
| `dragtab <title> x y` | drag a dock tab to a window point: a group's edge splits, its centre or tab bar stacks |
| `dclick x y`, `ddblclick x y` | click or double-click at a document point in the viewer (the select tool: a double-click enters a group or embed) |
| `click <text>` | click the shell button with that accessible name (`Link`, `Show in assembly`); the name matches as a substring, so `Pen` can hit `Close Pendulum` |
| `clickel <selector>` | click the element a CSS selector names; fails if another element (the canvas) would receive the click |
| `hunt x0 y0 x1 y1 step <js>` | sweep the pointer over a document-space grid until `<js>` is truthy (e.g. `__iron.hovered().includes(id)`); prints where. For targets whose screen position is hard to predict, such as a shape on a plane in 3D |
| `dragfound dx dy` | drag from the last `hunt` hit by (dx, dy) document px |
| `eval <js>` | evaluate in the page and print the JSON result; an async IIFE is awaited |
| `logs [regex]` | print console output since the last `logs`, optionally filtered; Rust `info!`/`warn!` lines from the worker appear here |
| `peek [regex]` | like `logs`, without clearing the buffer |
| `errors` | print uncaught page errors, then fail the run on any of them or on any `late:` line (the engine's `drawn a frame late` warning, kept since the start of the run) |
| `panel [name]` | print a panel's rect (default the viewer) |
| `sleep ms` | wait |
| `viewport w h [steps]` | resize the window to w×h over `steps` frames, as an animated browser sidebar does; the canvas follows at most every 100 ms plus once at the end (`PanelManager.handleWindowResize`) |
| `quit` | close the browser |

Where things are in the demo scene, in document space: the draggable square
covers (500,420)–(580,500); the orange ellipse (a circle, rx = ry) is centred at (160,330); the slider
track runs (100,600)–(400,624) and scrubbing along it previews the value
through the store; the bar at x=780 has its height and top bound to the
slider through a definition: the library (a root) holds a `bar` definition
with one input (an `in` port), `level`, and a template rect; the `model` root
holds an entity instantiated from it (its `args.level` bound to the slider, its
`level` port reading that argument) and the scene holds the rect that figures
it, whose y and h read the entity's port. The
template itself is never drawn. `scene` (#1) is a 2D composition; the torus
lives in a 3D composition, `assembly`, seen through `main`'s embed, near
document (875,200), beside the `card` composition (a 2D card with a dot and
a bar) shown in the assembly as a plane in perspective; the purple cartoon `box` covers (520,590)–(630,660);
the pendulum group is a loose root, drawn in the viewer. The "shadow" rect
at (600,420) has x and y bound to the square's. The timeline holds a "slide" clip (keys at 0.5 s and 2.5 s, square x 500→700)
and a "spin" clip (1–4 s, torus rotation). Any `<attr>.bind` in a view is a
binding and any `<driven-by>` names a clip.

The pendulum (constraint-system.md phase 3) is a group with a fixed
platform (240,40)–(400,60), joints every 100 px below its hinge at (320,60),
a ball (rx = ry = 14) at (320,360), arms drawn between the joints by bindings, a
"length" slider (100,660)–(400,684) whose value times 100 is each link's
`distance`, and a height bar at x=700 bound to the ball's y. Its ids: group
#U, platform #V, joints #W #X #Y, ball #Z, arms #a #b #c, fix constraint
#f (relation r3 to the platform), links #h #i #j. Dragging the ball or the
platform goes through the solver (the joints follow); the arms are bound
and locked; `.by="#U"` in a view marks a solver variable.

Every position drag goes through the store's gesture overlay, so anything
bound to the dragged node follows in the same frame and a view taken
mid-drag shows the previewed values. A leaf bound linearly to one constant
leaf (the bar's y to the slider, the shadow's x to the square) inverts: the
drag sets that source, and the commit writes it. Any other bound axis, or
one clip-driven at the current time, is locked for the whole drag and the
console says why; a node with both axes locked does not drag.

## Render benchmark

Measures what a 2D composition of N shapes costs per frame, phase by phase,
before any culling or raster cache (roadmap stage H). Build at opt-level 3
first, since CPU timings at `dev-opt`'s opt-level 1 overstate the encode:

```bash
PROFILE=wasm-bench .claude/skills/run-iron/build.sh   # ~2.5 min cold
.claude/skills/run-iron/serve.sh start
.claude/skills/run-iron/bench-render.sh 1000 5000 10000 20000 | tee bench.txt \
  | python3 .claude/skills/run-iron/bench-render-table.py
.claude/skills/run-iron/build.sh                      # back to dev-opt for the smoke
```

The **+** menu in any tab bar has a *Stress test* section (1k–20k shapes; another
size replaces the scene). `?stress=N` (with `layout=default` if a saved layout lacks the tab) adds a
`stress` composition of N shapes in cells of 100 and shows it in a Stress
tab (`src/document_bridge/stress.rs`); `await __iron.stress(N)` does the same
from the console. `await __iron.timings(reset)` reads per-phase CPU times
(`src/perf.rs`): `sync`, `gesture_reconcile`, `display_list`, `extract`,
`encode` (content views), `overlay_encode` (editor-feedback views), `render`, `pick`,
`overlay_geometry`, `bounds`, `propagate` (Bevy's transform propagation, timed from
outside its set; every frame), `produce` (a Bevy-side producer's step and publish,
only while one runs), and whole frames as `frame` and `changed_frame`
(a frame in which some view, content or overlay, re-encoded). Each phase records only frames on
which it did work.

The script runs headless Chrome on the GPU, unthrottled, through these
scenarios: `idle` (pointer on empty canvas); `hover-group` (pointer resting
over the field group); `hover-sweep` (the pointer swept on and off the field,
so hover comes and goes: content `encode.n` must stay 0, only
`overlay_encode` runs); `select-group` (the field group selected); `spin`
(scrub the slider bound to the field's rotation, everything moves);
`drag-one` (Cmd-drag one rect inside its cell); `pan` (wheel 200 px and
back); and `idle`, `spin` and `pan` again at zoom ≈0.37. Selection/hover
geometry is now cached; `overlay_geometry.n` should be zero during resting
hover/selection intervals. The script still keeps the pointer off content and
selection empty except in their own scenarios so highlight commands do not
contaminate changed-frame comparisons. Assertions check selected node identity
and deselection; a failed assertion, driver command, page error, or Rust panic
fails the run. `bench-render.sh` propagates failures through its output pipeline.
Frames per second counts idle frames between pointer events too, so read
`changed_frame` for the cost of a change. Driver commands it adds:
`pmove`, `psweep`, `pwheel`, `measure-reset`, `measure <label>`, `assert <js>`.

`PANEL_PIXELS=physical` converts panel-relative pointer offsets from physical
pixels to Playwright CSS pixels using DPR. The benchmark sets this because the
stress fixture maps document pixels to physical pixels at zoom 1. Normal driver
commands keep CSS offsets by default. Wheel deltas retain their original units.
The 1k/10k SIMD before/after tables and raw data are in `plans/rendering/render-benchmark.md`
§6–7; older scalar rows are historical. `bounds_updated` and `pick_candidates`
report maximum work counts per update/walk in the interval; `drawn` is the last
visible draw walk. Idle `pick.n` and `bounds.n` should be zero.

Selection cache regression checks (run after the build, with Vite up):

```bash
cd .claude/skills/run-iron
GPU=1 PANEL_PIXELS=physical SIZE=1600x1000@1 bun run drive.ts < selection-cache.txt
GPU=1 PANEL_PIXELS=physical SIZE=2560x1440@2 bun run drive.ts < selection-cache.txt
```

Every assertion must pass, with no page errors or panics. Checks cover unchanged
selection reuse, descendant edits, undo, same-version reload, pan/zoom reuse, and
correct slider/sample addressing at DPR 1/2. The slider must commit approximately
0.5. Inspect the `selection-cache-view-mapping` and `selection-cache-dpr-fixture`
screenshots. Do not build/rebind wasm during a driver run: a later navigation can
mix cached glue with a new wasm artifact and make the run invalid.

## Raster tiles (render-cache phase 2)

Cold chunks of a 2D view draw from raster tiles (`src/vector/raster.rs`,
plans/specs/render-cache.md §5 and §10) once its cold visible content passes
2,000 commands, so the demo document draws live and the Prussia file rasters.
`await __iron.raster(false)` draws every view live (the reference raster output
is compared against); `__iron.raster(true, mb)` turns tiles back on with an
atlas budget of `mb` MB (0: the default 256), which is how eviction is tested.
`__iron.timings()` carries the cache's state under `raster` (pages, tiles,
groups, and since the last reset `baked`, `bake_ms`, `live_squares`,
`stretched_squares`, `evicted`; `views` has each view's squares by kind), and
the driver's `measure` lines carry `bake` (the phase) and `raster` (those counts
without `views`). Encode excludes bake time.

Two checks, both on the Prussia coat of arms (Wikimedia,
`Coat_of_Arms_of_the_Kingdom_of_Prussia_1873-1918.svg`, 11 MB; copy it to
`.agents/tmp/big.svg`):

```bash
cd .claude/skills/run-iron
GPU=1 UNTHROTTLED=1 PANEL_PIXELS=physical SIZE=1600x1000@1 bun run drive.ts < bench-large-svg.txt
GPU=1 UNTHROTTLED=1 PANEL_PIXELS=physical SIZE=1600x1000@1 bun run drive.ts < raster-pixels.txt
python3 raster-pixels.py screenshots 380 36 940 744
```

`bench-large-svg.txt` prints the interaction rows; at `wasm-bench` expect changed
frames p50 ≈ 1.2 ms for both pans, ≈ 3 ms for the deep drag and ≈ 0.5 ms for the zoom
gestures, with single-frame maxima up to ≈ 130 ms where a dense tile bakes (the
table is in `render-cache.md` §10). With `?gfx=webgl2` replaced by `webgpu` it runs
on the other backend. `raster-pixels.py` must print `pass` for `rp-deep`,
`rp-dragged`, `rp-fit`, `rp-group`, `rp-in`, `rp-meters` and `rp-undone`. It bounds
pixels off by more than 2 levels (0.2%) and by more than 8 (0.01%), not every
difference: each chunk frame (a group's position) is a raster group composited on its
own, so about 2–3% of pixels differ by 1–2 levels where antialiased edges overlap
(with the Prussia file imported as references, about 21,000 differ, ~500 by more than
2, 8 by more than 8; `rp-meters` 321, worst 1). The `eval` after the deep zoom reports
`evicted` > 0 (it runs in a 32 MB atlas). `RASTER=0 bench-render.sh …` runs the
stress benchmark with tiles off.

## Instance cache (plans/specs/instance-cache.md)

`?meters=N` (or `await __iron.meters(n)`) adds a `meters` composition and a Meters
tab: N dial meters from the standard library, their definition given 60 tick marks,
and a slider bound to the first one's `in`. SVG import turns every piece drawn twice
or more into a definition and its occurrences into references
(`__iron.importSvg(text)` returns `definitions` and `references`);
`__iron.importSvg(text, { share: false })` imports every copy in full. Three checks:

```bash
cd .claude/skills/run-iron
GPU=1 UNTHROTTLED=1 PANEL_PIXELS=physical SIZE=1600x1000@1 bun run drive.ts < bench-instances.txt
GPU=1 PANEL_PIXELS=physical SIZE=1600x1000@1 bun run drive.ts < instance-pixels.txt
PREFIX=ip- PAIR="shared flat" python3 raster-pixels.py screenshots 380 36 940 744
```

`bench-instances.txt` (build `PROFILE=wasm-bench`) measures the meters idle, scrubbed
twice and panned, then the Prussia near-whole group (g93169) dragged. Expect changed
frames p50 ≈ 0.4 ms for the scrubs with `baked: 0` on the second, the `eval` lines
printing `{"groups":2,"tiles":8}`, and the drag ≈ 7 ms p50 (0.6 ms with the file
imported unshared). `instance-pixels.txt` imports the file unshared and shared, each
in a fresh page, and screenshots both fitted and zoomed in; the comparison passes
(drawn from tiles, ~8,000 pixels differ from grouping; drawn live, with
`__iron.raster(false)` after `ready`, none). Natively (copy the file first):

```bash
BIG_SVG=$PWD/.agents/tmp/big.svg cargo test -p iron_svg --target aarch64-apple-darwin --release --test big_import -- --ignored --nocapture
BIG_SVG=$PWD/.agents/tmp/big.svg cargo test -p iron_spatial --target aarch64-apple-darwin --release --test references -- --ignored
cargo test -p iron_document --release --target aarch64-apple-darwin --test instance_cost -- --ignored --nocapture
```

`big_import` prints 14,407 nodes, 534 definitions and 1,832 references and checks that
the shared and unshared imports place every node alike; `references` checks the
spatial scene's bounds agree; `instance_cost` prints what expanded instances cost
(1,000 meters: a structural edit ≈ 160 ms).

## Spatial and picking regression checks

After a stable build with Vite up:

```bash
cd .claude/skills/run-iron
GPU=1 PANEL_PIXELS=physical bun run drive.ts < picking.txt
GPU=1 PANEL_PIXELS=physical SIZE=1600x1000@1 bun run drive.ts < spatial.txt
python3 check-spatial-images.py
GPU=1 PANEL_PIXELS=physical SIZE=1600x1000@2 bun run drive.ts < spatial.txt
python3 check-spatial-images.py
```

`picking.txt` verifies stationary-pointer invalidation and zero idle pick work.
Under SwiftShader its 500 ms settle waits are too short for the re-pick after a
same-version reload; raise them (`sed 's/^sleep 500$/sleep 2500/; s/^sleep 300$/sleep 1500/'`).
`spatial.txt` must pass every assertion: exact ordered chains in reference/indexed
modes, repeated/nested occurrences of hidden sources, clips, transformed paths,
negative/nonuniform scales, deletion/history and 2D-on-3D plane hits. The four
reference/indexed screenshot pairs must match across the complete canvas/editor
(the comparison excludes the bottom 100 CSS pixels and masks the Stats panel).

`await __iron.spatial(false)` selects exhaustive reference walks; `true` restores
indexed picking/culling. Bounds continue updating in both modes. The diagnostic
result contains current ordered hits; allow settled frames after switching before
comparing. This is a test switch, not persisted document state. Its reply also
reports `entities` (document nodes with a Bevy entity: every 3D node plus the
Live 2D set) and `live` (the Live set's size); with nothing selected or hovered a
2D composition of any size shows a handful of entities (vision 10 phase 3).

The smoke's Link step sleeps between the box click and `keydown Shift`: at a few
fps both can land in one frame, and the click then adds instead of replacing.
Likewise it rests after the length-slider drag and before the save/load roundtrip. Undo and redo themselves are
ordered with the other keys since 2026-10-17 (`engine_api::history_request`
queues them as tool keys), so a Backspace followed by Cmd+Z undoes the delete
even when both land in one slow frame. What still varies run to run is the
driver's pace: a drag can stop short (the slider at 1.18 instead of 1.2, the
ball at y 414 instead of 420) or, rarely, miss its press.

### Runtime signals and the piston demo (vision 10 phase 4)

`await __iron.signals()` lists the runtime layer: `producers` (`clips`, `network`
and every host producer, with `running` and how many leaves each holds),
`signals` (leaf, value, producer, stamp) and `errors` (a leaf two producers
asked for, or a bound leaf). `await __iron.producer('gas', 'freeze' | 'stop' |
'run')` commits what the gas producer holds as one `freeze gas` transaction and
stops it, stops it, or runs it again. `piston.txt` drives the Piston tab:

```bash
# piston.txt was retired 2026-11-09 (an exit demo for a finished feature; its logic has Rust tests). The walkthrough below is history.
```

Expected: `gas()` names `clips holds 0`, `network holds 8` (the thermal demo's
ports) and `gas running holds 1`, one `gas -> #….port.signal = 1.x`
signal among the network's, no errors; `bars()` reads pressure ≈ 55–75 and `pv`
≈ 90–115 at stroke 0.2; after the slider drag (to ≈ 0.83 here; the drag is
driver-paced) pressure is ≈ 130–180 and `pv` still ≈ 80–115; the piston mesh prints
`pos.bind="[1, 0, 0] * (1.3 - #….slider.value * 2.2) + [0, 1.2, 0]"` with `rot="0,0,-1.57…"`; the freeze
returns `[true, 1, false]`, logs `applied "freeze gas" (1 ops, Human)`, leaves
`gas stopped holds 0` with no signals and the bars unchanged; undo returns both
bars to 0 and `run` republishes. The gas's values are noisy (160 particles) and
differ run to run, and the gauge has a 0.4 s time constant in *gas* time: a
frame integrates at most 0.1 s, so at the container's 2–3 fps in this tab the
readings after a 6 s settle are ~90% converged (the compressed reading keeps
rising after `run`). On a display they settle within a second. The producer
publishes only while the Piston tab (or an embed of `cylinder`) is shown.

Native spatial/store checks:
`cargo test -p iron_document -p iron_spatial --target aarch64-apple-darwin`.
`iron_spatial` has five unit tests and six scene tests (`tests/scene.rs`): the
hierarchical scene against a flat composition-space reference when nothing is
rotated, a superset-of-exact check after rotation, and the 10k stress shape,
where rotating the field must update one entry (`--release -- --nocapture`
prints its timings: ~24 µs for the rotate update, ~40 ms for a rebuild).
Don't run performance measurements concurrently with other browser tests/builds.

## Display-paced interaction and independent renderer controls

These are confidence tests, documented with checked results and caveats in
`plans/rendering/renderer-sanity-checks.md`. Use the optimized build and run sequentially:

```bash
PROFILE=wasm-bench .claude/skills/run-iron/build.sh
GPU=1 PANEL_PIXELS=physical SIZE=1600x1000@2 \
  URL='http://localhost:5173/Ironfell/?gfx=webgl2&layout=default&stress=1000' \
  bun run .claude/skills/run-iron/drive.ts < .claude/skills/run-iron/bench-interaction.txt
GPU=1 PANEL_PIXELS=physical SIZE=2560x1440@2 \
  URL='http://localhost:5173/Ironfell/?gfx=webgl2&layout=default&stress=1000' \
  bun run .claude/skills/run-iron/drive.ts < .claude/skills/run-iron/bench-group-raf.txt
sh .claude/skills/run-iron/build-renderer-probe.sh
DPR=2 COUNTS=1000,10000 SCENARIOS=group-plain,group-drag,all-transform,all-geometry \
  bun run .claude/skills/run-iron/compare-renderers.ts run
```

Do not set `UNTHROTTLED` for these cadence checks. `bench-interaction.txt` adds
whole-field movement and verifies identity before/after, waits out the
second-press double-click window, and clears selection between cases.
`mdrag panel x0 y0 x1 y1 [steps]` drags with the middle button (pans a 2D view).
`scheme light|dark` sets the system appearance the page sees (the theme follows it).
`dpr <ratio>` changes devicePixelRatio live; emulation fires neither `resize` nor
the resolution media query, so follow it with `eval dispatchEvent(new Event('resize'))`.
`praf panel x0 y0 x1 y1 ms` generates frame-paced pointer events through the
normal DOM input bridge. `measure` now includes elapsed time, rAF percentiles and
buckets; callback FPS alone is not actual displayed-frame/GPU completion proof.

`compare-renderers.ts capture` exports the actual canonical Stress field.
`run` can instead restore the compressed archived DPR-2 fixtures and downloads
checksum-pinned ThorVG 1.1.2 into gitignored `.agents`. `preview` checks static
rendered output without new timed intervals. It adds no app dependency. The page
`/Ironfell/renderer-sanity.html` allows manual Canvas2D/ThorVG GL/WG comparisons.
Group/transform cases retain local geometry; `all-geometry` recreates every
Canvas2D Path2D, resets/rewrites ThorVG paths, and changes rectangle proportions,
circle radii and cubic curvature. ThorVG smart rendering is disabled. Vello
Hybrid 0.2 controls (`vello-gl`, `vello-wg`) rebuild one scene every frame without
Bevy; `build-renderer-probe.sh` writes only `.agents/render-perf/vello-probe`,
leaving served Iron artifacts unchanged. No custom bitmap cache exists. These
controls omit the editor/document/Bevy; synchronous submission and rAF cadence
are not full-frame GPU latency or proof of every presented frame.

## Run (human path)

```bash
bun run dev   # -> http://localhost:5173/Ironfell/ ; Ctrl-C to stop
```

Opens the WebGPU build in desktop Chromium on macOS or Windows, the WebGL2
build elsewhere; `build.sh` builds both. `./build-wasm.sh` makes the release
artifacts, which this skill does not run.

## Test

```bash
cargo test -p iron_document --target aarch64-apple-darwin
```

253 tests pass (verified 2026-11-03 after view zoom became CSS px and the
timeline's rows scroll on its axis, `tests/timeline_view.rs` 14; ~1 min warm), minus the two platform-precision tests noted above on Linux. The host target is required because
`.cargo/config.toml` defaults the whole workspace to wasm. The solver crate
has its own: `cargo test -p iron_solve --target aarch64-apple-darwin` (9
tests), and so does SVG: `cargo test -p iron_svg --target aarch64-apple-darwin`
(13 tests, verified 2026-10-15; `UPDATE_GOLDEN=1` rewrites `tests/golden/fixture.svg`).

## Gotchas

- Bevy 0.20 on this adapter logs `Clustered light or decal … had no assigned
  index!` twice a frame (bevy_pbr `cluster/mod.rs:527`, the scene's two point
  lights; CPU clustering under WebGL2). Rendering is unaffected; filter the
  line out of `logs` when reading the console. Not yet reported upstream.

- **Headless Chrome's own `--screenshot` flag is useless here.** It fires at
  page load, and neither `--virtual-time-budget` nor `--timeout` waits for
  the worker's wasm compile. Three attempts produced only "Loading..."
  frames. Drive it with Playwright and wait for `.loading` to detach.
- **`?gfx=webgl2` is required.** Headless Chrome on SwiftShader has no WebGPU
  adapter, and the loader would otherwise try it on macOS.
- **The FFI must not touch interaction state.** `left_bt_up` used to clear
  `DragState` directly, which silently prevented drag release from committing
  a transaction. Only events cross the FFI now; if drags stop producing
  "move …" transactions, look there first.
- **Worker console shows up on the page channel.** Playwright forwards it;
  a separate `worker` listener duplicates every line.
- **Software rendering is slow, and the first frames are the slowest.**
  Shader compilation makes the first frames take seconds. A drag sent before
  the loop is cycling collapses into one frame, where press and release cancel
  out and nothing moves, with no error anywhere. `ready` waits for the cadence
  probe to show frames advancing; always call it before interacting. The app
  then runs at roughly 8 fps, so `drag` paces itself in hundreds of
  milliseconds. Do not read performance off these runs.
- **Cmd+V does not paste in a driven browser.** A synthetic key press never
  runs the browser's paste command, so no `paste` event fires. Paste with
  `__iron.paste(text)`, which dispatches the event the shell listens for
  (`text` defaults to the tab's last copy, `__iron.copied()`). Cmd+C does work,
  because the driver grants clipboard permissions; read the result back with
  `navigator.clipboard.readText()`.

## Troubleshooting

- **`vite did not start`**: something else holds `:5173`. `serve.sh stop`
  then `start` again; it kills the listener first, so a stale Vite from a
  previous session is the usual cause.
- **Build fails with "no graphics backend selected"**: `cargo build` was run
  without `--features webgl2` (or `webgpu`). `build.sh` passes it; the old
  `run-wasm.sh` does not and is stale.
- **`wasm-bindgen` "schema version" mismatch**: CLI and crate versions
  differ. `build.sh` reinstalls the CLI at the `Cargo.lock` version.


## 2D appearance and inspector regression (2026-10-15)

The default inspector is now 280 px wide. Run the full `smoke-full.txt` at
`SIZE=1680x1000@1` so its existing 3D embed hunt retains the same 1020 px canvas;
its expected document-space results above are unchanged. Inspector field lookup now
uses `data-leaf` attributes, so human-readable section titles are independent of it.

After building and starting Vite, also run:

```sh
cd .claude/skills/run-iron
bun run appearance.ts
DPR=2 bun run appearance.ts
```

`appearance.ts` authors overlapping red/blue rectangles under group opacity, a linear
then radial gradient, an ellipse-clipped group, and an inspector target, through model
transactions. `appearance-pixels.py` (Pillow) checks actual rendered pixels. Real pointer
Alt-drags check that previews do not enter save, release commits one undo step, Escape
keeps selection and cancels the gesture, and a concurrent model edit wins over a stale
release. The inspector layout checks type and Alt-drag a linked corner radius, undo
it once, expand the individual corner grid without authoring a change, edit one corner
and show Mixed, scrub opacity from the input box's empty space, scrub stroke alpha
without changing RGB, switch a fill type in one undo,
and add/edit Drop shadow through Effects. Escape closes the detail popovers while
preserving selection. DPR 2 uses document/physical pixels, not CSS pixels, as the renderer does.
Screenshots are in `.agents/appearance/` (gitignored). Expected output for each run:

```text
pixels: isolated overlap, linear gradient and ellipse clip pass
inspector: preview, single undo, Escape and concurrent edit pass
inspector layout: linked corners, stroke alpha, paint types and effect settings pass
pixels: radial gradient pass
no page errors
```

On 2026-11-06 (Figma-style colour picker) the 1× run passes as above; `DPR=2`
stops at its first pixel check (`appearance-pixels.py`: both sample points white),
before any inspector step. Not yet investigated.

### Automatic embed frame / stroke regression

```sh
cd .claude/skills/run-iron
bun run stroke-clipping.ts
DPR=2 bun run stroke-clipping.ts
```

A hidden source composition has a rounded gradient rectangle with a thick stroke.
Two embeds show it: one without a frame, one with an explicit geometry-sized frame.
Pixel checks verify all four stroke outsets survive the automatic frame, including
after changing stroke width and shape size, while the explicit frame still clips.
Screenshots are saved under `.agents/stroke-clipping/` (gitignored).

```text
pixels: automatic embed preserves all four stroke outsets pass
pixels: automatic stroke bounds update after edits; explicit frame clips pass
no page errors
```

## Native feasibility target (2026-10-16)

See `src-native/README.md` for build, packaged self-test and opt-in concurrency
commands. The normal target is native Rust/Metal under the shared Svelte shell;
it does not load wasm. Keep the existing web smoke above as a regression gate.

Native pacing diagnostics are documented in that README. The normal window is
opaque and only the HTML overlay is transparent. `IRON_NATIVE_PRESENTATION`
confirms actual Metal layer settings: default FIFO/2 has display sync on and three
drawables; the opt-in Immediate/2 comparison has sync off and three. Timing traces
measure CPU scopes, not displayed input latency. Do not report the acquisition
improvement as an event-to-photon measurement or declare the latency waves fixed.

The native cursor probe (`IRON_NATIVE_SELF_TEST=1 IRON_NATIVE_CURSOR_TEST=1`) has
six passing checks: pen event → CSS, packaged cursor loading, native 64px raster,
usable keyword fallback, generated image loading and no image CSP violations.
Web cursor artwork remains 8x. The 2026-10-16 wheel tuning raises only the shared
per-event log-zoom cap from 0.05 to 0.065; ordinary smoke output is unchanged.
The complete smoke passed again after these changes against the rebuilt WebGL2
static preview at `GPU=1 SIZE=1680x1000@1`; log:
`.agents/cursor-zoom-web-smoke.log` (exit 0, both no-page-errors checks).

The 2026-10-16 native work ran the complete `smoke.txt` at
`GPU=1 SIZE=1680x1000@1` against a frozen Vite preview on port 5175. When testing
preview instead of the dev server, first copy
`src/iron_svg/tests/fixtures/figma.svg` to the same path under `dist/`: smoke's
relative fixture fetch otherwise receives the SPA HTML fallback. Expected web
behavior/output is unchanged; the successful log has no command failures or page
errors (`.agents/web-smoke-pass.log`). Both web backends also compile.

Baseline caveat observed before this port: the existing solver integration test
`three_link_chain_reaches_a_target` fails its iteration assertion at
`src/iron_solve/tests/phase1.rs:49` ("50 iterations"). Store, spatial and SVG tests
pass; this native attempt has not changed that solver test or its implementation.

## Text (2026-10-17)

`plans/specs/text.md`. After a build, with Vite up:

```sh
cd .claude/skills/run-iron && SIZE=1680x1000@1 bun run drive.ts < text.txt; cd -
```

T and a click make a text node and focus the hidden textarea (`[true,""]`).
Typing through it shows on the canvas, and Esc commits. The font picker shows 15
previews; picking Playfair Display fetches that file in the worker. Content bound
to `"level = " + fmt(#O.slider.value, 3)` follows the level slider. Four undos
remove everything, the last one undoing the creation and the typing as one entry
(the demo's own bound `readout` text, in the Optics composition, is listed and stays;
expected refreshed 2026-10-27 for the 298-op demo). Expected, minus screenshot and
`WARN` lines:

```
[true,""]
"Pressure"
["<text id=\"#3c\" name=\"readout\" x=\"560\" y=\"240\" rot=\"0\" sx=\"1\" sy=\"1\" content=\"0.00 mW\" content.bind=\"fmt(#3V.port.signal, &quot;mW&quot;, 2)\"","<text id=\"#3t\" name=\"Text\" x=\"260\" y=\"80\" rot=\"0\" sx=\"1\" sy=\"1\" w=\"100\" h=\"100\" content=\"Pressure\""]
clicked button[aria-label="Font family"]
15
clicked button[title="Playfair Display"]
clicked button[aria-label="Bind content"]
1
[["content=\"0.00 mW\"","family=\"Inter\""],["content=\"level = 0.200\"","family=\"Playfair Display\""]]
1
applied "open demo scene" (298 ops, System) -> v1
applied "add text" (1 ops, Human) -> v2
applied "add text" (1 ops, Human) -> v3
applied "style text" (1 ops, Human) -> v4
applied "bind text.content" (1 ops, Human) -> v5
applied "set slider" (1 ops, Human) -> v6
applied "undo: set slider" (1 ops, System) -> v7
applied "undo: bind text.content" (1 ops, System) -> v8
applied "undo: style text" (1 ops, System) -> v9
applied "undo: add text" (2 ops, System) -> v10
(no page errors)
```

Picking a font is a `text_style` call ("style text"): with no characters selected
it styles the whole node. The log shows "add text" twice (the creation, then the typed words folded into
it); history holds one entry, as for Alt-drag. The slider value depends on the
driver's pace (0.200 here). Native: `cargo test -p iron_text --target
aarch64-apple-darwin` (6 tests; 7 with `--features system`, which lays out an
installed font).

`text-styles.txt` covers the rest of plans/specs/text.md: an emoji text (Noto COLR v1 chunks
fetched on demand, drawn as colour glyphs), ⌘B on a selected range, the
inspector's Underline and colour on another range, a plain-text paste, and SVG
export. Expected: the runs JSON holds `weight` 700 on 0–5 and `color` plus
`underline` on 15–19; the paste makes a second text node ("paste text"); the
export has three `<tspan>`s for the first text (bold "Party", plain emoji, red
underlined "time") and one for the pasted text; no page errors.

## Snapping (2026-10-20)

`plans/specs/snapping.md`. `await __iron.snapping(enabled)` turns object snapping
on or off for the next move and returns the current move's guides (`x=…` or
`y=…`, with how many points each marks). After a build, with Vite up:

```sh
cd .claude/skills/run-iron && SIZE=1680x1000@1 bun run drive.ts < snapping.txt; cd -
```

The square dragged by (23, 3) keeps its right edge on rect #E's left (603) and
snaps its centre to rect #6's (465); with Cmd held it lands unsnapped; an Alt-drag
copy snaps its left edge to the original's right (580) and its bottom to an edge
at 501; with snapping off the first drag lands unsnapped. Then, snapping
on again: the square's bottom edge resized to 586 snaps to the small green
rect's bottom (587; `h` carries the resize's own float noise, unsnapped too);
the pen's second point, clicked 3 px low, lands level with the first; and in
point editing the third point dragged to x 873 snaps plumb under the first.
Inspect `screenshots/snapping-guides.png` (two red guides),
`snapping-resize.png` (crosses alone) and `snapping-point.png`. Expected, minus
screenshot, `nav` and `ready` lines:

```
{"enabled":true,"guides":["x=603 (4)","y=465 (2)"]}
"<rect id=\"#M\" name=\"square\" x=\"523\" y=\"425\""
{"enabled":true,"guides":[]}
{"enabled":true,"guides":[]}
"<rect id=\"#M\" name=\"square\" x=\"523\" y=\"423\""
{"enabled":true,"guides":["x=580 (4)","y=501 (4)"]}
["<rect id=\"#M\" name=\"square\" x=\"500\" y=\"420\"","<rect id=\"#3t\" name=\"square\" x=\"580\" y=\"421\""]
{"enabled":false,"guides":[]}
"<rect id=\"#M\" name=\"square\" x=\"523\" y=\"423\""
{"enabled":true,"guides":[]}
{"enabled":true,"guides":["y=587 (4)"]}
"<rect id=\"#M\" name=\"square\" x=\"500\" y=\"420\" rot=\"0\" sx=\"1\" sy=\"1\" w=\"80\" h=\"167.00000000000003\""
["<path id=\"#3v\" name=\"path\" x=\"870\" y=\"420\"","<point id=\"#3x\" x=\"0\" y=\"0\"","<point id=\"#3y\" x=\"120\" y=\"0\"","<point id=\"#3z\" x=\"66\" y=\"77\""]
{"enabled":true,"guides":["x=870 (2)"]}
["<point id=\"#3x\" x=\"0\" y=\"0\"","<point id=\"#3y\" x=\"120\" y=\"0\"","<point id=\"#3z\" x=\"0\" y=\"100\""]
applied "open demo scene" (298 ops, System) -> v1
applied "move rect" (2 ops, Human) -> v2
applied "undo: move rect" (2 ops, System) -> v3
applied "move rect" (2 ops, Human) -> v4
applied "undo: move rect" (2 ops, System) -> v5
applied "duplicate rect" (3 ops, Human) -> v6
applied "duplicate rect" (2 ops, Human) -> v7
applied "undo: duplicate rect" (5 ops, System) -> v8
applied "move rect" (2 ops, Human) -> v9
applied "undo: move rect" (2 ops, System) -> v10
applied "resize rect" (1 ops, Human) -> v11
applied "undo: resize rect" (1 ops, System) -> v12
applied "draw path" (3 ops, Human) -> v13
applied "add point" (1 ops, Human) -> v14
applied "add point" (1 ops, Human) -> v15
applied "move point" (2 ops, Human) -> v16
(no page errors)
```

The snap core's native tests: `cargo test -p iron_spatial --target
aarch64-apple-darwin --test snap` (9 tests).

## Layout (2026-10-26)

`plans/specs/layout-and-tables.md` phase 1: groups with a `layout` facet place their
children (`iron_layout`, on Taffy), published by the store's layout producer and
never stored. After a build, with Vite up:

```sh
cd .claude/skills/run-iron && SIZE=1680x1000@1 bun run drive.ts < layout.txt; cd -
```

A `stack_y` group made through the model surface places three texts at its padding,
each `x.by` its group; a second line in the middle text moves the footer down by one
line (69.14 → 86.08) and undo puts it back. Three rects shift-picked and laid out with
Shift+A become a `stack_x` group with the 20 px gap they were drawn with, nothing
moving; the inspector shows Auto layout; undo restores the rects. Inspect
`screenshots/layout-stack-grown.png` and `layout-shift-a.png`. Expected, minus
screenshot, `nav` and `ready` lines:

```
{"enabled":false,"guides":[]}
true
["name=\"title\" x=\"12\" x.by=\"#3t\" y=\"12\"","name=\"body\" x=\"12\" x.by=\"#3t\" y=\"44.19921875\"","name=\"footer\" x=\"12\" x.by=\"#3t\" y=\"69.138671875\""]
true
[" y=\"86.078125\""]
[" y=\"69.138671875\""]
true
["name=\"stack\" x=\"510\" y=\"280\" mode=\"stack_x\" gap=\"20\"","name=\"a\" x=\"0\" x.by=\"#40\" y=\"0\"","name=\"b\" x=\"60\" x.by=\"#40\" y=\"0\"","name=\"c\" x=\"120\" x.by=\"#40\" y=\"0\""]
true
["name=\"b\" x=\"570\" y=\"280\""]
applied "open demo scene" (298 ops, System) -> v1
applied "model edit" (4 ops, Model) -> v2
applied "model edit" (1 ops, Model) -> v3
applied "undo: model edit" (1 ops, System) -> v4
applied "model edit" (3 ops, Model) -> v5
applied "add auto layout" (10 ops, Human) -> v6
applied "undo: add auto layout" (10 ops, System) -> v7
(no page errors)
```

The text positions depend on Inter's metrics. Native: `cargo test -p iron_layout
--target aarch64-apple-darwin` (5 tests: tracks against hand calculations, a merged
cell, wrapping, stacks, fill) and `cargo test -p iron_document --target
aarch64-apple-darwin --test layout` (5 tests: reflow, undo and save, locking and
absolute, a grid of rects with fill, wrapping text, Shift+A keeping the arrangement).

## Tables (2026-10-26)

`plans/specs/layout-and-tables.md` phases 2–3: a `table` of `row`s of `text` cells,
laid out as a grid, drawing its own bands and lines; the model's `<table>` op and
compact read form; the select tool looking through rows. After a build, with Vite up:

```sh
cd .claude/skills/run-iron && SIZE=1680x1000@1 bun run drive.ts < tables.txt; cd -
```

In a fresh `slide` composition: the `<table>` op with a caption makes a stack of the
caption and the table, read back in rows (`emphasis="3"` as written, 1-based);
replacing one word keeps every cell's id (one op); a merge that is not a rectangle is
refused. A click on the captioned table selects its stack. On a second table: a click
selects the table and the inspector shows Table; + Row adds a row; a double-click
reaches the "113" cell (through its row), Shift-click adds "138", Merge joins them,
Esc goes back to the table; a double-click then selects "S (µm)", and a pasted
spreadsheet block fills from there (the vertical merge survives); a paste with
nothing selected makes a third table and the Table tool a fourth. Inspect
`screenshots/tables-op.png` and `tables-ui.png`. Expected, minus screenshot, `nav`
and `ready` lines:

```
{"enabled":false,"guides":[]}
1
"slide"
"<applied version=\"3\">\n  <table id=\"#3w\"/>\n</applied>"
["<table id=\"#3w\" name=\"table\" x=\"0\" x.by=\"#3u\" y=\"24.51953125\" y.by=\"#3u\" preset=\"light\" header=\"1\" columns=\"auto auto auto auto\" label-col=\"true\" emphasis=\"3\">","PI per side (µm) | 75  | 100 | 125","S (µm)           | 85  | 113 | 140","Gap W (µm)       | 153 | 138 | 125"]
[true,"<cells>#40 #41 #42 #43 / #44 #45 #46 #47 / #48 #49 #4A #4B</cells>"]
"<edit-failed applied=\"none\" version=\"4\">\n  <error op=\"0\" kind=\"table\">the merge at row 1 column 1 is not a rectangle</error>\n</edit-failed>"
["#3u"]
"<applied version=\"5\">\n  <table id=\"#4C\"/>\n</applied>"
["#4C"]
["Table","Position"]
clicked button[title="Add a row at the bottom"]
["113","138"]
clicked button[title="Merge the selected cells into one"]
["#4C"]
["#4K"]
undefined
["PI per side (µm) | 75 | 100      | 125","A                | B  | 113\\n138 | 140","C                | D  | ^        | 125","|    |          |"]
undefined
3
clicked button[title="Shape tools"]
clicked [role="menuitem"]:has-text("Table")
4
applied "open demo scene" (298 ops, System) -> v1
applied "model edit" (1 ops, Model) -> v2
applied "add table" (18 ops, Model) -> v3
applied "replace table" (1 ops, Model) -> v4
applied "add table" (16 ops, Model) -> v5
applied "add row" (5 ops, Human) -> v6
applied "merge cells" (3 ops, Human) -> v7
applied "paste into table" (4 ops, Human) -> v8
applied "paste table" (9 ops, Human) -> v9
applied "add table" (13 ops, Human) -> v10
(no page errors)
```

Cell positions depend on Inter's and JetBrains Mono's metrics. Native: `cargo test -p
iron_document --target aarch64-apple-darwin --test table` (11 tests: the reference
table, boxed lines, merges and the lines around them, a bound cell relaying, row and
column edits, merge and split, restyling, a deleted cell disturbing only its row, the
compact view, replace keeping ids, spreadsheet paste).

## Contexts (2026-10-26)

`plans/specs/contexts.md` §5 steps 0–1 (with `affordances.md` phase A1): behaviour
as context rules, the built-in `std:*` library, and a document overriding it. After a
build, with Vite up:

```sh
cd .claude/skills/run-iron && SIZE=1680x1000@1 bun run drive.ts < contexts.txt; cd -
```

In a fresh `slide` composition with a table: `__iron.contexts()` shows `std:table`'s
row rule; a click selects the table and a double-click its "113" cell (through its
row), whose computed values name their rules and start no text edit. The model's
`<context name="std:table">` verb adds `affords.enter="edit"` for cells; the view
prints it as markup, and the cell's computed `affords.enter` names `std:table rule 1
from #…`. The same click and double-click now edit the cell, its word selected
(`[true,"113","113"]`); one undo removes the context, and the double-click selects
the cell again. Inspect `screenshots/contexts-edit.png` (the cell in text edit, the
Text tool lit). The script reads the slide composition's id from its create (the
demo's size moves ids). Expected, minus screenshot, `nav` and `ready` lines (2026-11-09):

```
{"enabled":false,"guides":[]}
1
"slide"
"<applied version=\"3\">\n  <table id=\"#3y\"/>\n</applied>"
["<context name=\"std:table\" for=\"type:table\">","<rule match=\"type:row\" affords.select=\"through\"/>"]
["#48"]
["affords.select=\"target\" affords.select.by=\"std:free-2d rule 1\"","affords.enter=\"select\"","affords.edit=\"text\" affords.edit.by=\"std:free-2d rule 7\"","affords.grips=\"box\" affords.grips.by=\"std:free-2d rule 1\"","affords.drag=\"point\" affords.drag.by=\"std:free-2d rule 1\"","affords.drives=\"\"","affords.activate=\"none\"","affords.step=\"1\"","affords.axis=\"x\"","affords.delete=\"self\"","affords.cursor=\"auto\""]
"no textarea"
"<applied version=\"4\">\n  <context id=\"#4E\"/>\n</applied>"
["<context id=\"#…\" name=\"std:table\">","<rule match=\"type:text\" affords.enter=\"edit\"/>","</context>"]
["affords.select=\"target\" affords.select.by=\"std:free-2d rule 1\"","affords.enter=\"edit\" affords.enter.by=\"std:table rule 1 from #…\"","affords.edit=\"text\" affords.edit.by=\"std:free-2d rule 7\"","affords.grips=\"box\" affords.grips.by=\"std:free-2d rule 1\"","affords.drag=\"point\" affords.drag.by=\"std:free-2d rule 1\"","affords.drives=\"\"","affords.activate=\"none\"","affords.step=\"1\"","affords.axis=\"x\"","affords.delete=\"self\"","affords.cursor=\"auto\""]
[true,"113","113"]
false
false
1
(no page errors)
```

Native: `cargo test -p iron_document --target aarch64-apple-darwin --test contexts`
(7 tests: `std:*` prints back to itself, bad markup names the field or word, rows
passed through to cells, locality, a document override with its provenance and undo,
bad markup refused, the view's markup) and `--test registry_options` (every enum
field lists its values).

## View focus: palettes, keymaps, inspector sections (2026-10-27)

`plans/specs/contexts.md` §5 step 2: each view's focus context (its world, the
container its selection is open in, `editing-text`, `editing-points`, `main`) and
what it offers, its palette and keymap, as `match="view"` rules. The engine pushes
them per panel (`focus` events); the toolbar draws the palette, and plain letters
reach tools through the keymap of the view under the pointer
(`bevy_app/view_focus.rs`). After a build, with Vite up:

```sh
# focus.txt was retired 2026-11-09 (an exit demo for a finished feature; its logic has Rust tests). The walkthrough below is history.
```

The Main view's toolbar and `viewContext` are `std:free-2d`'s (the shapes share a
slot). Make component on the square puts a main in the scene the view embeds: the
focus gains `main` and the palette the port tool; undo takes both away. A `deck`
composition whose notation is defined only by a document context (`extends`
`std:free-2d`) shows Move, Text and Table (B); B over the deck picks the table and
R does nothing there; over Main, R picks the rectangle and B does nothing. (The
active tool is global, so Main's shape slot lights Table too.) The deck's context also
sets `inspector.sections="position fill"`: a rect made and clicked in the deck shows
only Position and Fill, while a click in Main (on the scene's embed, `#15`) shows
`std:free-2d`'s list, of which an embed has Position and its own section. Expected, minus
`nav` and `ready` lines:

```
[["Move (V)","Rectangle (R)","Pen (P)","Text (T)"]]
["<computed view=\"viewer\">","palette.tools=\"select rect|ellipse|table|empty pen text\" palette.tools.by=\"std:free-2d rule 9\"","keys.tools=\"v:select p:pen r:rect o:ellipse t:text\" keys.tools.by=\"std:free-2d rule 9\"","tolerance.slop=\"3\"","tolerance.double_click=\"400\"","tolerance.hit=\"1\"","inspector.sections=\"main port component table position text layout appearance fill stroke effects other add\" inspector.sections.by=\"std:free-2d rule 9\"","chrome.accent=\"#0d99ffff\"","chrome.grip=\"8\"","chrome.grip_shape=\"square\"","chrome.hover=\"on\"","snap.grid=\"0\"","chrome.component=\"on\"","</computed>"]
undefined
[[],"select rect|ellipse|table|empty pen text"]
clicked button[title^="Make a component"]
[["main"],"select rect|ellipse|table|port|empty pen text"]
[[],"select rect|ellipse|table|empty pen text"]
true
true
"deck"
[["Move (V)","Rectangle (R)","Pen (P)","Text (T)"],["Move (V)","Text (T)","Table (B)"]]
["<computed view=\"deck\">","palette.tools=\"select text table\" palette.tools.by=\"deck rule 1 from #…\"","keys.tools=\"v:select t:text b:table\" keys.tools.by=\"deck rule 1 from #…\"","tolerance.slop=\"3\"","tolerance.double_click=\"400\"","tolerance.hit=\"1\"","inspector.sections=\"position fill\" inspector.sections.by=\"deck rule 1 from #…\"","chrome.accent=\"#…\"","chrome.grip=\"8\"","chrome.grip_shape=\"square\"","chrome.hover=\"on\"","snap.grid=\"0\"","chrome.component=\"on\"","</computed>"]
["Table","Table (B)"]
["Table","Table (B)"]
["Rectangle (R)"]
["Rectangle (R)"]
true
["position","fill"]
["#15"]
["position","embed"]
applied "open demo scene" (298 ops, System) -> v1
applied "make component component" (6 ops, Human) -> v2
applied "undo: make component component" (6 ops, System) -> v3
applied "model edit" (1 ops, Model) -> v4
applied "context" (1 ops, Model) -> v5
applied "model edit" (1 ops, Model) -> v6
(no page errors)
```

Native: `--test contexts` covers view rules (palette and keys by focus, `main`, a
container in focus, a notation named by a document context that extends a built-in
one, view and node fields kept apart).

## Keyboard navigation by context (2026-10-27)

`plans/specs/affordances.md` §5.7 (phase B; `contexts.md` §5 step 5): a container's
`keys.nav` (`std:table`: `grid`; `std:row`/`std:column`, groups laid out in a line:
`list-x`/`list-y`) decides what arrows, Tab and Enter do to its items
(`iron_document::interaction::key`). After a build, with Vite up:

```sh
cd .claude/skills/run-iron && SIZE=1680x1000@1 bun run drive.ts < keys.txt; cd -
```

In a table's cells: Right from "113" to "140", Down to "125", Right at the edge stays;
Tab past the last cell adds a row ("add row") and selects its first, empty cell (4
rows); Esc goes back to the table. In a column stack in the scene: Down from "title"
to "body", Right (across the axis) stays, Alt+Down moves "body" past "footer" (one
"reorder text"), undone in one step. Expected, minus `nav` and `ready` lines:

```
{"enabled":false,"guides":[]}
1
"slide"
true
undefined
["113"]
["140"]
["125"]
["125"]
[[""],4]
["table"]
true
tab Main
undefined
undefined
["title"]
["body"]
["body"]
[["body"],["title","footer","body"]]
["title","body","footer"]
applied "open demo scene" (298 ops, System) -> v1
applied "model edit" (1 ops, Model) -> v2
applied "add table" (16 ops, Model) -> v3
applied "add row" (5 ops, Human) -> v4
applied "model edit" (4 ops, Model) -> v5
applied "reorder text" (1 ops, Human) -> v6
applied "undo: reorder text" (1 ops, System) -> v7
(no page errors)
```

Native: `--test contexts` (`a_tables_cells_navigate_as_a_grid`,
`a_stacks_items_navigate_as_a_list_and_alt_moves_one`).

## Reordering in sequence (2026-10-27)

`plans/specs/affordances.md` §6 (phase B; `contexts.md` §5 step 5): a stack's items
drag `sequence` (`std:row`/`std:column`'s rule for children). While held, the store
lays the stack out in a previewed order (`Store::set_layout_preview`, session state,
never an edit) with the dragged item floating at the pointer; release commits one
`Reorder`, a drop where it started writes nothing. After a build, with Vite up:

```sh
cd .claude/skills/run-iron && SIZE=1680x1000@1 bun run drive.ts < sequence.txt; cd -
```

A column stack's title (y 12) dragged down past the footer: mid-drag the title floats
at the pointer (78) while body and footer move up into its place (12, 37), the
document still at version 2; release lands it last (59) as one "reorder text"; undo
restores the order. Then motion as data (`motion.placement`, affordances.md §8.2;
`std:column`'s items glide "out 160 layout", the store easing what layout moves on
the frame clock, `document_bridge/motion.rs`): a document context slows it to 1.5 s,
so half a second after Alt+Down body is part-way (`true`) and then rests; with `none`
the next Alt+Up lands at once. Inspect `screenshots/sequence-mid.png`. Expected,
minus `nav` and `ready` lines:

```
{"enabled":false,"guides":[]}
true
undefined
["title 12","body 44","footer 69"]
["title"]
[["title 78","body 12","footer 37"],"2"]
["body 12","footer 37","title 59"]
["title 12","body 44","footer 69"]
true
true
["body 12","title 37","footer 69"]
true
["title 12","body 44","footer 69"]
applied "open demo scene" (298 ops, System) -> v1
applied "model edit" (4 ops, Model) -> v2
applied "reorder text" (1 ops, Human) -> v3
applied "undo: reorder text" (1 ops, System) -> v4
applied "context" (1 ops, Model) -> v5
applied "reorder text" (1 ops, Human) -> v6
applied "context" (1 ops, Model) -> v7
applied "reorder text" (1 ops, Human) -> v8
(no page errors)
```

Native: `--test contexts`
(`a_previewed_order_lays_the_stack_out_and_floats_the_dragged_item`).

## Controls made of data (2026-10-27)

`plans/specs/affordances.md` phase C, demo 3: plain nodes given behaviour by inline
`affords` facets (`affords.style="drag: ring; drives: transform2d.rot"`, CSS-style;
on a definition's template part it holds for every instance). A gesture drives one
of the node's own leaves through its binding (`Store::preview_target`): a bound leaf
moves its source, one that cannot follow is refused with a reason ("cannot follow").
After a build, with Vite up:

```sh
# controls.txt was retired 2026-11-09 (an exit demo for a finished feature; its logic has Rust tests). The walkthrough below is history.
```

In a fresh `kit` composition: a slider `level` (0.5); a needle whose rotation is
bound to it (`value × π`), a `ring`; a bar whose height is bound to it (`value ×
200`), a `track` along y; three texts that `toggle` italics, `step` their size by 4
and `cycle` their alignment. Turning the needle a quarter turn sets the slider to
1.00; dragging the bar's bottom up 50 sets it to 0.75; the clicks make italic, 28,
center; the computed view names the inline facet; five undos restore everything.
Expected, minus `nav` and `ready` lines:

```
{"enabled":false,"guides":[]}
1
"kit"
true
true
undefined
["0.50","false","24","left"]
" style=\"drag: ring; drives: transform2d.rot; step: 0.2618\""
["1.00","false","24","left"]
["0.75","false","24","left"]
["0.75","true","28","center"]
["affords.drives=\"font.italic\" affords.drives.by=\"inline on #…\"","affords.activate=\"toggle\" affords.activate.by=\"inline on #…\""]
["0.50","false","24","left"]
applied "open demo scene" (298 ops, System) -> v1
applied "model edit" (1 ops, Model) -> v2
applied "model edit" (1 ops, Model) -> v3
applied "model edit" (5 ops, Model) -> v4
applied "turn transform2d.rot" (1 ops, Human) -> v5
applied "set size.h" (1 ops, Human) -> v6
applied "toggle font.italic" (1 ops, Human) -> v7
applied "step font.size" (1 ops, Human) -> v8
applied "cycle font.align" (1 ops, Human) -> v9
applied "undo: cycle font.align" (1 ops, System) -> v10
applied "undo: step font.size" (1 ops, System) -> v11
applied "undo: toggle font.italic" (1 ops, System) -> v12
applied "undo: set size.h" (1 ops, System) -> v13
applied "undo: turn transform2d.rot" (1 ops, System) -> v14
(no page errors)
```

Native: `--test contexts`
(`a_definitions_part_declares_its_drag_for_every_instance_and_drives_through_its_binding`:
a rule on the standard meter's pivot reaches both placed meters, and turning one
moves its `in`).

## The contexts showcase (2026-10-27)

The app's own URL adds three tabs after the demo scene, one transaction "open
showcase" (`document_bridge::demo::showcase`): **Outline** (a notation whose texts
edit on entering: a table's cell takes words on the first double-click; a column
stack to drag, glide and Alt+arrow), **Pitch** (a notation with its own palette, keys
and inspector sections, and a card lit by its own interaction state), **Dials**
(controls made of inline `affords` facets; the needle shows its pivot as a dot) and **Data** (demo 4: three sliders shown
as a table of bound readouts and as bars; dragging a bar's top sets the data, 0.40 to
0.60, and typing 90 into its cell sets it to 0.90, the words inverting through their
`fmt` binding while the binding stays).
`drive.ts`'s default URL carries `showcase=0`, so every other script's ids follow the
demo scene as before; this script navigates without it. A saved layout from before
2026-10-27 lacks the tabs: open `?layout=default` or use the Document view's **Reset
layout**. After a build, with Vite up:

```sh
# showcase.txt was retired 2026-11-09 (an exit demo for a finished feature; its logic has Rust tests). The walkthrough below is history.
```

Inspect `screenshots/showcase-outline.png`, `showcase-pitch.png` and
`showcase-dials.png`. Expected, minus `nav` and `ready` lines:

```
["outline","pitch","dials"]
tab Outline
[["Move (V)","Rectangle (R)","Pen (P)","Text (T)"]]
tab Pitch
[["Move (V)","Text (T)","Table (B)"]]
undefined
0.45
1
tab Dials
["affords.drag=\"ring\" affords.drag.by=\"inline on #…\"","affords.drives=\"transform2d.rot\" affords.drives.by=\"inline on #…\""]
tab Data
undefined
["0.40","north  | {= fmt(#4e.slider.value * 100, 0)}"]
["0.60","north  | {= fmt(#4e.slider.value * 100, 0)}"]
"90"
["0.90","north  | {= fmt(#4e.slider.value * 100, 0)}"]
applied "open demo scene" (298 ops, System) -> v1
applied "open showcase" (176 ops, System) -> v2
applied "set size.h" (1 ops, Human) -> v3
applied "edit text" (1 ops, Human) -> v4
(no page errors)
```

Outline's chrome is touch-like (no hover light, 14 px grips reaching twice as far)
and Pitch's orange with round grips, from their contexts' view rules. A fifth tab,
**Circuit**, holds a divider in the schematic notation (`schematic.txt`).

## Chrome as data (2026-10-27)

`chrome.txt` (plans/specs/chrome.md's revision; contexts.md step 7): how selection
and hover look is a view's context. A notation `orangey` whose view rule sets
`chrome.accent`, `chrome.grip` and `chrome.grip_shape`; a selected rect's corner grip
is orange and round, with no blue; a group holding a needle whose inline style asks
for `chrome.handle: dot` shows the pivot dot while the group is selected; hovering
the rect lights its outline at half strength until the context adds
`chrome.hover="off"`. The driver's `tint <panel> x y w h #rrggbb` counts a
panel-relative box's pixels near a colour (each channel within 40). After a build,
with Vite up:

```sh
# chrome.txt was retired 2026-11-09 (an exit demo for a finished feature; its logic has Rust tests). The walkthrough below is history.
```

Inspect `screenshots/chrome-grips.png`, `chrome-handle.png`, `chrome-hover.png` and
`chrome-nohover.png`. Expected, minus `nav`, `ready` and screenshot lines:

```
{"enabled":false,"guides":[]}
true
true
"kit"
true
0
0
22
0
1
24
60
true
0
applied "open demo scene" (298 ops, System) -> v1
applied "context" (1 ops, Model) -> v2
applied "model edit" (1 ops, Model) -> v3
applied "model edit" (4 ops, Model) -> v4
applied "context" (1 ops, Model) -> v5
(no page errors)
```

## Schematics (2026-10-27)

`schematic.txt` (plans/specs/schematic.md): the Circuit tab, a divider in the
`schematic` notation built from the parts library (`std_parts.xml`) and wired pin to
pin, solved as one network. The slider sets R2 and the probe follows; R, Space and a
click place a turned resistor on the 10 px grid; W and three clicks wire it (one
"wire" transaction, the corner tidied); dragging the part moves it on the grid with
its wire rubber-banding; four undos. Then a probe wired onto a wire splits it at a
junction (J1) in one transaction; Delete on R2 takes its wires (`affords.delete=
"subject"`); a model's `<circuit>` edit places and wires a second divider and answers
with the netlist (`__iron.netlist`). After a build, with Vite up:

```sh
cd .claude/skills/run-iron && SIZE=1680x1000@1 bun run drive.ts < schematic.txt; cd -
```

Inspect `screenshots/schematic-wired.png`, `schematic-moved.png`,
`schematic-junction.png` and `schematic-model.png`. Expected, minus `nav`, `ready`
and screenshot lines:

```
tab Circuit
[["Move (V)","Wire (W)","Resistor (R)","Voltage source (E)","Ground (G)","Probe (P)","Text (T)"]]
undefined
undefined
undefined
undefined
"6 V"
["0.25","4.5 V"]
"<use name=\"R3\" x=\"500\" y=\"320\" rot=\"1.5707963267948966\" sx=\"1\" sy=\"1\" notation=\"schematic\" count=\"6\">"
[6,"500,290 300,290 300,250"]
["540,340","540,310 300,310 300,250"]
[5,false,"6 V"]
"* circuit (netlist from Iron)\nV1 N1 0 9 V\nR1 N1 N2 1 kΩ\nR2 N2 0 2 kΩ\n* P1 probes N2\n.end\n* 0 = 0 V\n* N1 = 9 V\n* N2 = 6 V"
[7,"9 V",true]
[5,true]
"<applied version=\"14\">\n  <circuit>placed V9 R8 R9 GND9 P9; 5 wires</circuit>\n</applied>\n<netlist sheet=\"circuit\">\n* circuit (netlist from Iron)\nV1 N1 0 9 V\nR1 N1 N2 1 kΩ\n* P1 probes N2\n* P2 probes N1\nV9 N3 0 5 V\nR8 N3 N4 10 kΩ\nR9 N4 0 10 kΩ\n* P9 probes N4\n.end\n* 0 = 0 V\n* N1 = 9 V\n* N2 = 9 V\n* N3 = 5 V\n* N4 = 2.5 V\n</netlist>"
"* circuit (netlist from Iron)\nV1 N1 0 9 V\nR1 N1 N2 1 kΩ\n* P1 probes N2\n* P2 probes N1\nV9 N3 0 5 V\nR8 N3 N4 10 kΩ\nR9 N4 0 10 kΩ\n* P9 probes N4\n.end\n* 0 = 0 V\n* N1 = 9 V\n* N2 = 9 V\n* N3 = 5 V\n* N4 = 2.5 V"
applied "open demo scene" (298 ops, System) -> v1
applied "open showcase" (176 ops, System) -> v2
applied "set slider" (1 ops, Human) -> v3
applied "place resistor" (9 ops, Human) -> v4
applied "wire" (9 ops, Human) -> v5
applied "move use" (2 ops, Human) -> v6
applied "undo: move use" (2 ops, System) -> v7
applied "undo: wire" (9 ops, System) -> v8
applied "undo: place resistor" (9 ops, System) -> v9
applied "undo: set slider" (1 ops, System) -> v10
applied "place probe" (3 ops, Human) -> v11
applied "wire" (37 ops, Human) -> v12
applied "delete use" (6 ops, Human) -> v13
applied "circuit" (68 ops, Model) -> v14
(no page errors)
```

## Scopes: durable documents (2026-11-02)

plans/specs/scopes.md phases 1–2 and §10.2. **Two pages, routed by the query**
(`src-ui/route.ts`): with no scope in the URL the page is the **scopes page**
(`ScopesPage.svelte`, `[data-scopes-page]`), which reads OPFS and loads no
engine, no wasm and no `__iron`; `?scope=<id>` opens the app on that scope (one
that is gone goes back to the page), `?scope=last` on the last one (with none, a
new demo), `?new=demo` / `?new=empty` makes one, and `&export` downloads the
scope once open. The address follows the open scope (`history.replaceState`),
so a reload reopens it. Once idle, the scopes page **warms the engine**
(`runtime/warm_engine.ts`): it fetches and compiles the wasm for the backend
the browser will use and imports the app's code; opening a scope from the page
swaps to the app in place (`openApp` in `main.ts`, `pushState`), so the engine
takes the compiled module rather than fetching (Back reloads the page). In dev
against localhost this barely shows (click to open ~1.6–2.1 s warm, ~1.2–2.6 s
cold); the saving is the download and compile on a real connection. **Without `&demo` the app keeps its
document**: every committed transaction is written
to the scope's log in batches (~500 ms, and at once when the tab is hidden), and
a reload replays it. `?demo`, `?stress=`, `?meters=`, the native build and a
browser without OPFS keep the old behaviour: an unsaved demo. **Every scripted
URL carries `&demo`** (drive.ts's default too), so scripts see the demo, not the
last scope; each driver run also starts with empty OPFS, being a fresh browser
context. In a normal browser, a page without `&demo` opens what you left, and
DevTools → Application → Storage → "Clear site data" forgets it. `__iron.scope()`
shows the open scope; `await __iron.flush()` writes now. Scripts that need a
stored scope navigate with `&scope=last`; the driver's `click` matches visible
text, so the page's controls are clicked with `clickel <selector>`.

`scopes.txt`: the empty scopes page (no engine), New demo, edit, undo, reload
(the edit is kept, the undone one stays undone, undo starts empty, same scope
id), an edit with no flush survives a reload, the page lists the scope and opens
it, then `&demo` opens unsaved. After a build, with Vite up:

```sh
cd .claude/skills/run-iron && SIZE=1680x1000@1 bun run drive.ts < scopes.txt; cd -
```

Expected, minus `nav`, `ready` and `clicked` lines:

```
"scopes page: No scopes yet.; engine loaded false"
"wasm prefetched on the scopes page: true"
"app opened in place; wasm fetched 1 time(s)"
"created Demo; address names it true"
"edited"
"flushed at v4"
"reopened at v4, edit kept, undone kept undone, nothing to undo"
"edited, waited"
"batch written without a flush, reopened at v5"
"listed [\"Demo\"], the same scope true"
"opened from the page: true"
"&demo: unsaved demo"
(no page errors)
```

Segments, compaction, a gap in the feed, a failed replay, two tabs and an engine
restart, the URL's boot targets and the page's storage-only operations
(`ScopeLibrary`: list, rename, duplicate with its log, import, export, delete
refused while another tab holds the lock) are covered by `bun test
tests/scopes.test.ts` (11 tests, a fake engine);
the Rust side (ids, schema 9, snapshot + log replay equals live through undo and
redo, the feed's gap) by `tests/scopes.rs` in `iron_document`.

### The scope menu (2026-11-03)

Phase 3: the button at the left of the top-left tab bar (the open scope's
name; `[data-scope-menu]`) opens a popover with the name, editable in place
(Enter or blur), **New** (an empty `main` composition), **New demo**,
**Duplicate**, **Import…** (a `.json`, as a new scope), **Export** (a download
named after the scope), the stored scopes newest first with sizes (click to
open; the bin asks once, then deletes; deleting the open scope opens the most
recent other, or a new empty one), and the site's storage use. A read-only tab
shows "Open here instead". On an ephemeral page (`&demo`) it says the page is
not saved. The same actions are `__iron.scopes.{list, create(kind, name),
open(id), rename(name), duplicate(), remove(id), exportText(),
importText(text, name), takeOver()}`; `scope_template(kind)` builds a new
scope's document in a fresh store.

`scope-menu.txt` drives each action, through the menu for New, delete and
rename, and the driver API for the rest. Expected, minus `nav`, `ready`,
`clicked`, `screenshot` and `sleep` lines:

```
"opened Demo, marked"
"menu lists [\"Demo\"]"
"New: Untitled, empty main composition"
"renamed; stored [\"Notes\",\"Demo\"]"
"opened Demo, marker back: true"
"duplicated: Demo copy, new id true, marker true"
"exported Demo copy (schema 9), imported as Imported"
"deleted Notes; stored [\"Demo\",\"Demo copy\",\"Imported\"]"
"open: Imported; menu marks Imported; name field Imported"
"first click asks: Delete?"
"deleted the open one; menu closed: true; now Demo copy; stored [\"Demo\",\"Demo copy\"]"
"renamed in the menu: Lab bench; trigger shows Lab bench"
"reload opens Lab bench; stored [\"Demo\",\"Lab bench\"]"
"scopes page lists [\"Lab bench\",\"Demo\"]; none held true; engine loaded false"
(no page errors)
```

The menu's **All scopes** (`[data-action=all-scopes]`) writes and compacts the
open scope, gives its lock back and goes to the scopes page. The page itself:
New, New demo and Import at the top; each scope a row (click to open) with a
menu for Open, Rename (in place), Duplicate, Export .json and Delete… (a
confirmation dialog; refused while another tab has it open, which the row
says). New and an Export whose log needs replaying open the app to do it.

The menu shifts the top-left group's tabs right; the smoke is unchanged by it.

## Layouts per scope and New composition (2026-11-04)

plans/specs/scopes.md phase 4. **The engine starts empty**: no demo is
queued at startup, so a stored scope opens with nothing applied before it.
On `enginePrepared` the shell gives the engine its document
(`DocumentClient.startDocument`): the scope the URL names, or on an unsaved
page (`&demo`, the benchmarks, the native build) the `demo` engine call, then
`stress`/`meters` and the `showcase`. **The app's own layout** is the Main view
(`viewer`), Timeline, Model, Library and Inspector (`baseLayout` in
`src-ui/views/workspace.ts`; since 2026-11-04 the Document view opens from the
add-view menu, there is no Stats view, and a fresh layout's left group shows the
document's first narrative, Lesson in the demo, in front). Once the document is installed,
the dock shows the scope's saved layout (`workspace.json` beside it in OPFS,
copied by Duplicate), or the app's plus **one tab per document composition and
narrative** (`addDocumentViews`): a new empty scope has no Pendulum tab, the
demo's tabs come from the demo document (they now include Cylinder, the
piston's embedded composition). Each scope saves its own layout as it changes;
switching scopes restores each one's. A tab onto a document composition names
it by **id** (`source: "#4"`), so renaming or undoing a rename keeps it; the
title follows the name. `compositions` lists `{id, name, dims, session}`.

**New composition…** in the "+" menu opens a dialog (`NewCompositionDialog.svelte`:
a unique name, 2D drawing, 3D scene or schematic) that applies one undoable
"new composition" (`author` verb `new_composition`) and opens its tab in that
group. **Double-click** a document composition's tab title to rename it
(`rename`); its **context menu** has Rename composition, Delete composition
(`delete_composition`, the composition and its contents in one entry, the tab
closed; Cmd+Z brings it back) and Close tab. A composition view onto a
composition the scope lacks (a tab from another layout, a deleted one) shows
`MissingComposition.svelte` instead of a canvas: Create (when the tab names
it) or Close tab; before, tools there drew previews that committed nowhere.

`workspace.txt` drives all of it. After a build, with Vite up:

```sh
cd .claude/skills/run-iron && SIZE=1680x1000@1 bun run drive.ts < workspace.txt; cd -
```

Expected, minus `nav`, `ready`, `clicked` and bare `1` lines:

```
applied "open demo scene" (302 ops, System) -> v1
"demo page: viewer scene assembly card pendulum controls optics schematic thermal cylinder piston components timeline layers model library selection"
(no new console output)
"empty scope: viewer timeline layers model library selection"
"suggested name: composition"
"made: Pendulum=#4:Pendulum; active Pendulum"
"renamed: Swing (#4)"
applied "new composition" (1 ops, Human) -> v2
applied "draw rectangle" (1 ops, Human) -> v3
applied "rename" (1 ops, Human) -> v4
"flushed"
(no new console output)
"reloaded: Pendulum layers library model selection timeline viewer; active Pendulum"
"after delete: tab open false"
applied "delete composition" (1 ops, Human) -> v5
applied "undo: delete composition" (2 ops, System) -> v6
"after undo: [\"main\",\"Swing\"]"
"missing view shown: true"
"after Create: missing view shown false"
applied "new composition" (1 ops, Human) -> v7
"new demo scope: viewer scene assembly card pendulum controls optics schematic thermal cylinder piston components outline pitch dials data circuit timeline lesson layers model library selection"
"back in Untitled: ghost layers library model selection timeline viewer"
(no page errors)
```

The `(no new console output)` after the reload is the point: nothing is
applied before a stored scope. Old WebGPU artifacts (built before this) still
queue the demo at startup and lack the `demo` call, so rebuild both backends
(`bun run wasm` / `wasm:fast`) after pulling this.

Updated 2026-11-06 for the Layers view in the default layout. The last two lines
had also drifted before that (no `stats` or `document` view opens by default now).

## The layers panel (2026-11-06)

plans/specs/layers-panel.md phase 1. The default layout's left group has a
**Layers** tab in front (`src-ui/views/html/LayersView.svelte`, notation `layers`,
source `follow` or a pinned composition id): a Compositions section, then one
composition's drawing tree. Rows come from the engine's `layers` call
(`__iron.call('layers', [source, JSON.stringify(openKeys)])` → `{ok, composition,
dims, rows, total}`); a row's `key` is the select tool's chain. `select`
(`__iron.call('select', [itemsJson, 'replace'|'add'|'toggle'])`, items `{node,
panel, chain?}`) sets the session selection from HTML. Rows are absolutely
positioned in a scroll container; only those in view are in the DOM. Driver
helpers in the fixture: `__row(name)`, `__sel()`, `__mark(name)` (then
`clickel [data-pick]` for a real click, with `keydown Meta`/`Shift` around it).

```sh
cd .claude/skills/run-iron && GPU=1 SIZE=1680x1000@1 bun run drive.ts < layers.txt; cd -
```

Expected, minus `nav`, `ready`, `tab`, `clicked` and quoted lines: twenty
`assertion passed` lines and `(no page errors)`. A full layout check of the
Prussia import (not in the repo; copy it to `.agents/tmp/big.svg` as for
`bench-large-svg.txt`) is manual: import into Controls, open `layer2`, scroll,
Cmd-click a path inside a shared piece and see its reference row revealed and
outlined. Measured 2026-11-06: every group opened gives 7,933 rows (the rest of
the 14,407 nodes are inside shared references), 19–20 ms and 1.1 MB per query;
28 rows mounted while scrolling.

## The narrative panel (2026-11-03)

plans/specs/narrative-panel.md §8. A narrative is a `narrative` root of `block`
nodes (`block` and `richtext` facets); the **Lesson** tab (Document group) is a
`narrative` view of the demo's `lesson`, which is part of the showcase, so a
`showcase=0` page (every other script) shows "No narrative named lesson" with a
New button instead. The panel is ProseMirror (`src-ui/narrative/`): typing is one
`narrative_edit` transaction per 400 ms pause, Cmd+Z is the store's undo, `#`
opens a mention menu, live values are atoms, a view block's header (or scrolling
it to the top, "follow views") shows its composition, and the bar switches
Authoring / Hybrid / Exploration. A model turn's changed blocks are marked with
Show / Keep / Revert. Driver: `__narrative.get('lesson')` →
`{editor(), mode(m?), flush(), askWith(xml, text), turns(), keep(i), revert(i),
status()}`; engine calls `narrative_list/read/edit/create/markdown/write/rewrite/
values/targets/probe` through `__iron.call`.

`narrative.txt` (no `showcase=0`, so ids differ from other scripts; masked
below as `#ID`). Expected, minus `nav`, `ready`, `tab`, `key` and `screenshot`
lines:

```
"12 blocks, mode authoring; values [\"500\",\"0.40\"]; mentions [\"square\",\"shadow\",\"level\",\"torus\"]"
"typed; store has it: true; versions advanced by 1"
"Cmd+Z: store undone true, editor undone true"
"menu offers [\"square\"]"
"heading now \"A tour of the demo with the square \"; mention of square"
"hover lights [\"#U\"] (#U); after: []"
"x is now shown as 612"
"active tab: pendulum; panel shows pendulum"
"hybrid: editor shown, outline hidden, target \"Writes at the cursor\"; exploration: editor hidden, outline shown, target \"Conversation only\"; authoring: editor shown, outline hidden, target \"Writes at the cursor\""
"model turn ok true, changed 1 block(s), marked 1"
"hybrid"
"reverted: reverted; model paragraph gone true"
"stale mentions: [\"shadow\"]"
"markdown: The :e[square]{#ID} sits at x = :val{#ID.transform2d.x digit"
"typed in scope Demo"
"after reload the heading reads \"A tour of the demo (kept)\""
(no page errors)
```

`bun test tests/narrative.test.ts` covers block ↔ editor conversion and the
block diff; `cargo test … --test narrative` and `narrative::tests` the store
side (edits as one transaction, undo, save/load, the span index and stale
mentions, markdown both ways, the model view).

## The timeline as a composition (2026-11-03)

plans/specs/timeline.md §11, §12. The Timeline tab is a **hybrid view**: its right
side is the session's generated `timeline` composition (`iron_document::timeline_view`,
notation `timeline`, context `std:timeline`), shown like any composition
(`{source: 'timeline', notation: 'composition'}`); its left column, `label_w` wide,
is DOM (`TimelinePanel.svelte`, `[data-timeline-column]`) over the canvas's plain
backdrop. Both are laid out from one plan: the column polls `timeline_rows(panel)`
for its rows and the axis's row scroll `y0`, and its wheel scrolls the rows
(`timeline_axis` `{"scroll": dy}`). The timeline's camera never pans or zooms: the
ruler and headers stay put. A clip is one property's animation: `timing.start`, `clip.prop` and
two or more `key` children (`t` after the start, `value`, `easing`).

Canvas layout in document px from the panel's top-left, which a 2D view at 100%
shows as CSS px at any pixel ratio: `label_w` 160, ruler 56 tall (13 px labels,
minor ticks, the playhead's head low on it), rows 22 apart from y 60, so time t is
at x 160 + t·100. The demo has an object row and a property row per animated
object: torus (`rot`, the spin clip, 1–4 s) and square (`x`, the slide clip, keys
at 0.5 s and 2.5 s, its bar from x 210 to 410 at y 126–148). Per property row a bar spans the clip's keys (`bar <clip> <target>`)
with a diamond per key (`key <key> <target>`, a rect turned π/4); per object row a
summary bar spans all its keys (display only).

- **What can be pressed is data**: the generator puts each piece in a role group
  (`backdrop`, `tracks`, `keys`, `headers`, `ruler`, `overlay`), and each group names
  a built-in context (`std:timeline-static`, `-tracks`, `-keys`, `-ruler`,
  `-overlay`) whose `children` rules say what its pieces afford. A bar is one
  target: a drag retimes its clip ("retime slide"), Delete deletes the clip
  (`affords.delete="subject"` follows `projects`). A diamond is a target: a drag
  moves its key ("move key of slide"; the handle rule prefers the leaf of what the
  piece `projects`), Delete removes it. A clip's first and last diamonds say
  `affords.cursor` `trim-start` / `trim-end` (the SVG cursors in
  `src-ui/cursors/`); `grips="ends"` shows the same. Backdrop, grid and headers are
  declared no target (a click clears the selection); labels and the playhead's line
  are `inert`, transparent to the pointer.
- **Press anywhere on the ruler** (y 0–56): the playhead jumps there and a drag
  scrubs (the ruler group is a `track` whose value is the clock's `t`; a session
  op, no transaction). A selected bar lights what its clip animates in every view,
  and a selected animated node lights its bars.
- **The column**: an object row's name selects its object (hover lights it); a
  property row carries ‹ ◇ › (`[data-key=prev|toggle|next]`): the previous key,
  add ("add key") or remove ("remove key") a key at the playhead, the next key.
- **A drag on an animated object** writes keys by the mode (§12.2, at commit,
  `timeline_view::key_ops`): on a key (within 1/60 s), that key's value; with
  **auto-keyframe** on, a new key at the playhead (an unanimated leaf gets a new
  clip: keys at 0 and the playhead); off, between keys or after the last, every key
  moves by the drag's delta. Auto-keyframe is the clock's session `autokey`: the
  transport's ◉ button (`[data-action=autokey]`) or Shift+K from anywhere but a text
  field; while on, the other 2D views wear a red frame and an "Auto-keyframe" chip.
- **Keys**, with the pointer over the timeline (the transport takes them before the
  engine): Space, Home, End, ← → (1/30 s; Shift, 1 s), I and O (loop ends), F (fit).
- **Wheel**: horizontal pans time (`axis.t0`), vertical scrolls the rows under the
  ruler (`axis.y0`), Ctrl/Cmd zooms `axis.pps` about the pointer. Middle-drag and
  Space-drag do the same as grabbing the content: x pans time, y scrolls the rows.
  Shift+1 fits, as F does; the other zoom keys leave the timeline alone.
- **Transport** (`TimelineTransport.svelte`, in the column's corner over the
  ruler): back to 0, play/pause, loop (all clips), fit, auto-keyframe, the time
  (click to type and seek), the end.
- **Engine calls**: `clock_read()` → `{ok, id, t, playing, autokey, rate, loop_in,
  loop_out, end}`; `clock_set(json)`; `timeline_axis(json)`, `{"fit": width}` or
  `{"pan", "zoom", "at"}` or `{"scroll": dy}`, → `{ok, t0, pps, y0}` (`'{}'`
  reads); `timeline_rows(panel)` → `{ok, rows: [{kind, label, y, target, prop, clip,
  error, keyed}], y0, zoom, label_w, ruler_h, row_h}` (a row's top is `(y − y0)·zoom`
  CSS px); `timeline_key(action, clip)` (`prev`, `next`, `toggle`).

`timeline.txt` (it turns smart guides off first). Expected, minus `nav`, `ready`,
`key`, `sleep` and `screenshot` lines:

```
"{}"
"clock at 0, auto-keyframe false, last key at 4; rows [\"o:torus\",\"p:rot\",\"o:square\",\"p:x\"]"
"scrubbed to 2.60 s; square at x 700; version 2"
"slide: start=\"1.5\""
"over the last key: cursor trim-end"
"keys after the drag: [\"0\",\"2.5\"]"
clicked [data-timeline-column] [data-row=object][data-target="#M"] .name
"name click selects [\"#M\"]"
"at 1 s"
clicked [data-timeline-column] [data-prop="transform2d.x"] [data-key=next]
"› to 2.50, ‹ back to 0.50"
"at 1.5 s"
clicked [data-timeline-column] [data-prop="transform2d.x"] [data-key=toggle]
"◇ added: 3 keys, button on"
clicked [data-timeline-column] [data-prop="transform2d.x"] [data-key=toggle]
"◇ again removed it: 2 keys"
"auto-keyframe off: keys [\"550\",\"750\"]"
"Shift+K: auto-keyframe true, button on"
"auto-keyframe on: keys [\"0@500\",\"2@700\",\"1@650\"]"
"after delete: slide clip gone"
"after undo: slide clip back"
applied "open demo scene" (302 ops, System) -> v1
applied "open showcase" (189 ops, System) -> v2
applied "retime slide" (1 ops, Human) -> v3
applied "undo: retime slide" (1 ops, System) -> v4
applied "move key of slide" (1 ops, Human) -> v5
applied "undo: move key of slide" (1 ops, System) -> v6
applied "add key" (1 ops, Human) -> v7
applied "remove key" (1 ops, Human) -> v8
applied "move rect" (2 ops, Human) -> v9
applied "undo: move rect" (2 ops, System) -> v10
applied "move rect" (1 ops, Human) -> v11
applied "undo: move rect" (1 ops, System) -> v12
applied "delete slide" (1 ops, Human) -> v13
applied "undo: delete slide" (3 ops, System) -> v14
(no page errors)
```

`TypeError: Cannot read properties of undefined (reading 'call')` mid-run, with
steps going wrong after it, was Vite re-optimising dependencies and reloading the
page. `vite.config.ts` now lists the two pages' entries and `bits-ui` in
`optimizeDeps` and excludes `@lucide/svelte` (icons are deep imports, AGENTS.md),
so a new icon no longer causes it; a new npm dependency still might. Rerun.

`cargo test … --test clock` (phase 1: the clock, the axis rule), `--test time`
(sampling keys) and `--test timeline_view` (the generator: rows, bars and diamonds
projecting their clips and keys, retiming and moving keys by drag, the §12.2 table of
what a drag on an animated object writes, ‹ ◇ ›, pan/zoom/fit without history, the
loop band, each piece's affordances and cursor from its group's context, the ruler's
track, delete following `projects`) cover the store side. The smoke's scrub step presses the ruler and drags to 3 s.

## Free running and per-view FPS (2026-10-18; Stats removed 2026-11-04)

`plans/specs/free-running.md`. The Stats view is gone (§4a): each composition
view's settings menu has **Show FPS** (`[data-show-fps]`), a readout in the
view's lower right corner (`[data-fps="<panel>"]`) of the frames that view
rendered per second, from the `frame_stats` engine call
(`__iron.call('frame_stats', [])`: per-panel render counts and `t`). Checked
2026-11-04: idle, Main and Timeline read 0; playing, both 60 and a hidden tab
0. Nothing in the default layout is a live source now. Each composition view has a settings button (top right,
`aria-label="Composition settings"`) with Run: Auto / Always / On demand;
`await __iron.runMode(source, mode)` and `await __iron.compositionInfo(source)`
do the same from the console (`source` is an id or a name).

Checked by hand: the Piston tab's gas moves with no input (two screenshots
2.5 s apart differ inside the chamber), stops when Piston is set to On demand
(identical) and moves again on Auto, each a "set run mode" transaction; the
menu reads `Running: cylinder` (bubbled out of the embed). With Stats set to
On demand and the pointer still, `__iron.timings` over 2 s shows only `frame`
and `propagate`: nothing renders.

The smoke's timeline scrub now ends later (the timeline is narrower beside
Stats), hence the torus's `rot="0,5.34…,0"`. The expected block was brought
up to date on 2026-10-20 with lines that had differed since at least
2026-10-18 and are what the script intends: the select tool's resize lands on
120×100 (553,392 rotated), the double-click enters the pendulum group (`["#a"]`,
the ball) before Esc climbs out, the toolbar reads `"Pen (P)"` after P, the
bend reads `oy="-63.4"`, and the rounded rectangle's corners read 30. The old
values came from driver-paced drags that stopped short and a double-click
that did not land.

## Shared document dispatch (2026-10-26)

Web worker, main-thread wasm and native now use `engine_dispatch::call` for
all document operations. Web exports only `document_call(handle, name, args_json)`;
`makeDocumentApi(call)` builds the model surface over either a synchronous or an
asynchronous transport. This does not complete the typed UI protocol project.

Fast transport-factory tests: `bun test tests/document-api.test.ts` (3 tests).
After rebuilding wasm and starting Vite, run the real browser contract in both
modes, from `.claude/skills/run-iron`:

```sh
GPU=1 SIZE=1680x1000@1 bun run drive.ts < document-calls.txt
GPU=1 SIZE=1680x1000@1 URL='http://localhost:5173/Ironfell/?gfx=webgl2&demo&contractRuntime=main' bun run drive.ts < document-calls.txt
```

The fixture sets the runtime in Settings, reloads, rejects unknown operations,
wrong arity and wrong argument types without changing authored state, applies an
edit, verifies atomic rejection, then loads both original and edited states and
checks canonical saves. Expected after nav/ready/runtime lines:

```
["unknown document operation","expected 1 arguments","argument 0"]
"apply/reject/save/load pass"
(no page errors)
```

Full `smoke.txt` was rerun after consolidation: exit 0, no page errors. The
render-inspector expectation now records 8 retained nodes and 1 chunk in view.
Canonical byte length and solver undo op counts can vary with floating-point
settling; the run saved 59,350 bytes and the round trip remained equal.

At the consolidation checkpoint, `bun run check` reported four font-catalog narrowing errors at
`TextSection.svelte:95–96` (an untouched component), plus ten warnings. The
refactored files had no reported errors. These errors are fixed by the typed protocol
checkpoint below; `tsc -p tsconfig.node.json` also passes.

The rebuilt packaged native workspace passes 19/19 checks. Its idle check sets
Stats to On demand first, since Stats is now a live composition. The first run
correctly found continuous rendering with Stats left on Auto; that is expected
product behavior, and the test setup has been updated.

The native workspace self-test exercises the same three dispatch errors and
unchanged-state check, followed by its existing editing/undo/redo/lifecycle checks
(19 total, or 21 with the runtime simulation fixture). The focused Rust dispatcher
test can run with `cargo test -p ironfell --lib --features native --target
aarch64-apple-darwin --profile dev-opt engine_dispatch::tests`.


## Typed UI protocol, lifecycle and scoped reads (2026-10-26)

The shared Rust `engine_protocol::Request` handles input/control and document
messages on all hosts. Browser adapters keep surface/bootstrap/frame management
and pointer-motion fast paths. A single `rustBridge.send_event_from_worker`
callback carries the serialized Rust event union in both browser modes; native
uses that event union on its channel. Fonts remain a host service.

`bun run protocol` regenerates named document commands and wire types
from Rust. `bun run check` checks freshness, negative type fixtures, all frontend
code and the Node config (zero errors, ten existing Svelte warnings).
`bun test tests/document-api.test.ts tests/protocol-lifecycle.test.ts` runs seven
fast tests for both transport kinds, schema retry, disposal/timeouts, stale
sessions/results, coalescing, stable identities and selective live updates.

`protocol-lifecycle.txt` checks actual UI read counts: changing the circle does
not reread the square inspector or composition summaries, changing the square
does, and history availability follows undo. It then leaves a request outstanding,
switches worker → main → worker, checks disposal and stale-event rejection, and
injects a runtime failure to verify teardown and restart. After a wasm rebuild:

```sh
cd .claude/skills/run-iron
GPU=1 SIZE=1680x1000@1 bun run drive.ts < protocol-lifecycle.txt
```

Expected (excluding nav/ready, successful edit outcomes and the first boolean):

```
assertion passed
assertion passed
assertion passed
assertion passed
assertion passed
"replacement settled pending work and rejected stale events"
assertion passed
assertion passed
"worker restored"
assertion passed
"failed runtime detached and restarted"
assertion passed
(no page errors)
```

The engine emits authored/session/resolved revisions and explicit history state,
coalesced once per frame. Consumers share scoped cached reads and release them
when hidden or unmounted. Read interests prevent unobserved Stats changes from
waking the shell. Whole-document reads use one authored-content interest rather
than copying every node id into a UI subscription. Replies identify queued,
applied or completed work; none promises that pixels have been presented.

Rust verification: `cargo test -p ironfell --lib --features native --target
aarch64-apple-darwin --profile dev-opt` (four tests). The native workspace fixture
also reloads the actual webview while retaining the native world, checking saved
state, existing undo availability and settled reads (22 checks; 24 with the
simulation fixture). All 22 packaged native checks pass, as do the two mailbox tests. The final native
package rebuild took 20 s warm; mailbox tests took 4 s and the
workspace/reload fixture took 20 s. All four shared-engine Rust tests pass.
The final full smoke took 210 s, main-thread document contracts 3 s, lifecycle
10 s, text styles 18 s, tables 23 s, and appearance 29 s, all without page errors.
`appearance.ts` now reads the current `facets` table instead of the obsolete
`components` key; pixel and inspector assertions are unchanged. Evidence lives
in `.agents/ui-protocol/` (gitignored).


### Browser runtime cleanup — done 2026-10-26

The worker and main-thread entry points now delegate browser lifecycle work to
`runtime/browser_runtime.ts`. Main-thread pointer motion is still immediate;
worker motion is sampled at a frame. Dead frame-gate counters are gone, and
frame errors stop RAF and enter the existing runtime-error path.

`bun run check`, the seven JS tests, `protocol-lifecycle.txt`, main-thread
`document-calls.txt`, and web/native frontend bundling pass. A dev-server smoke
lost its page-scoped `shapeView` helper midway (no page errors); a static-preview
run with the required SVG fixture copied under `dist/` passes in 210 s with no
page errors and unchanged semantic output. Web and native frontend builds pass.
The seven JS tests and zero-error frontend check pass. Rust and wasm artifacts
are unchanged; no Rust rebuild is needed for this cleanup. Evidence is in
`.agents/browser-runtime-cleanup/` (gitignored).

### Native latency experiments — 2026-10-27

`IRON_NATIVE_PACING=late bun run native:run` selects `IRON_NATIVE_RENDER_MODE=sync` and
`IRON_NATIVE_PACING=late` (parallel ECS retained, FIFO/2, three drawables). At the
time `native:run` defaulted to pipelined/display; since 2026-11-06 it defaults to
sync/adaptive. `IRON_NATIVE_RENDER_MODE=pipelined IRON_NATIVE_PACING=display bun run native:run` explicitly
selects that original scheduling in the same binary for human A/B testing. Save
and use the same document/view/window size; run one instance at a time.
For independent comparisons, set
both controls explicitly before `bun run native:run`. The late timer's margin
is `IRON_NATIVE_PACING_MARGIN_MS`, default 2 ms, accepted range 0.25–8 ms.
See `src-native/README.md` for capture commands.

The native workspace fixture verifies the requested render mode against the actual
render-thread state. Expected totals: **23 ordinary checks**, **25 with the 100k
simulation fixture**. All four mode combinations pass the workspace fixture;
pipelined/display and sync/late both pass the simulation fixture. The sync run
retains parallel ECS and has zero render/simulation overlap. Editing, canonical
reload, idle, resize, minimize/restore, JavaScript-stall independence and actual
webview reload pass. Do not enable `IRON_NATIVE_ALWAYS_RENDER` for the idle check.
Real native pointer dragging and keyboard undo were also inspected successfully.

Keep the macOS console **unlocked** and the test window visible. A locked/asleep
screen can suspend view-backed callbacks and webview timers; a visible NSWindow
flag alone is insufficient. `caffeinate -d -u <test binary>` is a process-scoped
sleep assertion, not an unlock operation. Do not count a capture with few/no
callbacks as a valid timing comparison. Trace schema 2 carries host frame IDs
across skipped renders; analysis warns about too-few callbacks or missing CPU
submissions. A final sparse trace matches all 460 submissions across 723 host
updates. CPU present return is not display scanout.

Seven native Rust tests (two mailbox, five budget), four shared-engine tests,
seven JS tests and three Python analysis tests pass. Both browser backends and
the native package compile; frontend checking has zero errors and ten existing
warnings. The full production-preview smoke on isolated port 5185 passes in
212 s with no page errors and unchanged semantic expectations. Native workspace
runs take 19–23 s; each 100k fixture took 15 s.

Final continuous-workspace timing: normal pipelined/display CPU present-return
p50/p95 18.32/18.87 ms; sync/late 4.19/7.55 ms, acquisition p95 0.10 ms, nine CPU
target misses out of 1201 steady frames. Sync alone's earlier 2.63 ms median did
not repeat (16.58 ms in the final capture). A two-drawable pool worsened tails;
retain three. Full results and limits: vision 08's “Native latency controls” and
`plans/rendering/benchmarks/native-latency-2026-10-27.json`. Evidence is in the
latency worktree's `.agents/latency/`. Mouse-to-photon, 120 Hz and multi-display
measurements remain open; these checks do not close the native latency gate.


### Optical filming scene — 2026-10-27

Apply this skill when building or checking the opt-in filming scene. From the
worktree root, `IRON_NATIVE_FILM=1 IRON_NATIVE_PACING=late bun run native:run` opens sync/late and `IRON_NATIVE_FILM=1 IRON_NATIVE_RENDER_MODE=pipelined IRON_NATIVE_PACING=display bun run native:run`
opens pipelined/display. Browser URLs append `latencyFilm=1`. Normal workspace
launches are unchanged. Usage/recording protocol: `src-native/README.md`, “Filming
visible pointer delay”. Keep the cursor visible, grab the yellow square at its
centre and move horizontally; cyan/magenta centres are offset −96/+96 CSS px.
Continuous defaults off. Reset after resize/display-scale changes.

Browser integration uses the worktree server and real Playwright pointer input:

```sh
cd .claude/skills/run-iron
URL='http://127.0.0.1:5185/Ironfell/?gfx=webgl2' bun latency-film.ts
URL='http://127.0.0.1:5185/Ironfell/?gfx=webgpu' bun latency-film.ts
cd ../../..
python3 .claude/skills/run-iron/check-latency-film.py
```

Each backend prints three passing runtime/DPR cases (worker 1×, worker 2×, main 1×).
Checks cover single fixture, target/guide alignment, normal drag/undo, always/auto,
exported actual backend, reload and reset with no page errors. Pixel checking prints
six `all three target centres/offsets pass` lines after both backends. Images and
manifests live under `.agents/latency/`; run from the indicated directory because
the browser script's output path is relative to it.

After native packaging, run each preset separately with `IRON_NATIVE_FILM=1` and
`IRON_NATIVE_SELF_TEST=1` on the packaged binary. Set `IRON_NATIVE_RENDER_MODE` and
`IRON_NATIVE_PACING` explicitly to sync/late or pipelined/display; keep the screen
unlocked (use process-scoped `caffeinate -d -u` if needed). The app auto-exits and
prints `IRON_NATIVE_TEST` with **14 checks, all pass, errors: []**. It verifies the
native pointer path, actual render mode/caption, normal transaction edits, CSS-sized
fixture and qualified reset leaves, guide alignment, no duplicates, run modes and
actual webview reload preserving the target ID/canonical document.

The ordinary production-preview full smoke still passes with its existing semantic
expectations and `(no page errors)` (211 s in the filming-mode run). Both backends
and native package compile; frontend check remains zero errors/ten existing warnings.
These are functional/pixel checks, not measured optical latency. Setup export records
camera sampling rate but does not calculate a photon-latency value.


### Latency view and measured latency — 2026-11-05

Owning plan: [`plans/rendering/latency-view.md`](../../../plans/rendering/latency-view.md).
A developer view (Settings → Developer, then "+" → Developer → **Latency**) with a
yellow square dragged through the select tool and seven pointer trackers (Vello
engine ring, Canvas2D in the event, Canvas2D on rAF, SVG, and the controls Konva,
raw WebGPU and raw WebGL2), all concentric at rest.
`?latency=solo|panels|full` opens it in a fixed layout and does not save the
layout; `?runtime=worker|main` overrides the stored runtime for one page; native:
`IRON_NATIVE_LATENCY=solo|panels|full` with any native launch.

Functional check (headless Chrome, no OS input), from `.claude/skills/run-iron`
with the worktree's Vite on 5185. A worktree under `.agents/` is outside Vite's
watch (`watch.ignored`), so **restart Vite with `--force` after every edit** and
check the served file with curl; otherwise every run measures the old code:

```sh
bun run latency-view.ts          # DPR=1, RUNTIMES=worker, LAYOUTS=panels narrow it
```

Expected (six lines):

```text
PASS LATENCY VIEW worker/solo/dpr2: opens, concentric at rest and held, drag outside history, recentre; no page errors
PASS LATENCY VIEW worker/panels/dpr2: opens, concentric at rest and held, drag outside history, recentre; no page errors
PASS LATENCY VIEW main/solo/dpr2: opens, concentric at rest and held, drag outside history, recentre; no page errors
PASS LATENCY VIEW main/panels/dpr2: opens, concentric at rest and held, drag outside history, recentre; no page errors
PASS LATENCY VIEW menu/developer-on: opens from the + menu; no page errors
PASS LATENCY VIEW menu/developer-off: not offered; no page errors
```

Measured latency drives the **real OS pointer** and records the screen, so the
calling app needs Accessibility (macOS 27: "Device Control and Data Access") and
Screen Recording, the display must be awake, and nobody may touch the mouse
(~15 s a run). The probe captures the built-in display only (with a second
display attached too), reads the cursor back and warns `the cursor was moved by
something else` when someone did. Run on AC power: on battery at or below 20%
Chrome's Energy Saver throttles the page to 30 Hz and the driver warns
`page rAF … throttled`. Never script System Events for it: that raises an
Automation prompt over the screen. Build the probe once, then from
`.claude/skills/run-iron`:

```sh
../../../scripts/latency/build.sh
URL='http://127.0.0.1:5185/Ironfell/?demo' REPEAT=2 bun run latency-measure.ts      # Chrome (own profile), worker+main × solo+panels
BROWSER=safari URL='http://127.0.0.1:5185/Ironfell/?demo' bun run latency-measure.ts
BROWSER=native MODES=baseline,adaptive LAYOUTS=solo,panels bun run latency-measure.ts   # after native:ui + cargo build -p iron-native
../../../scripts/latency/summarize.py ../../../.agents/latency/measure/*.json
```

`CONTINUOUS=1` turns on the view's Continuous rendering first (Chrome); `TAG=x`
suffixes labels; `OUT=dir` changes the output directory; `SESSION=k` measures k
times in one browser session. Chrome runs also print the engine's own tick timing
(`engine tick: start … after rAF … pointer age at tick …`). Chrome comes from
`latency-chrome.ts` (own temporary profile). `latency-trace.ts` (`URL=… NAME=x`)
records a Chrome trace of one measurement to `.agents/latency/trace/`. Page
parameters for experiments: `?workerTick=raf` (the worker's old rAF-only tick),
`?latencyBusy=N` (N ms of extra work in the page's rAF); driver settings `CHROME_WINDOW=w,h` (window size in points, default 1710,1080) and `RENDER_SCALE=1|1.5|2|auto|panel4k` (the Settings view's render scale) for large-display runs, where the probe's interference warning also fires from load alone. On a large window add `&latencyTrackers=light` (engine, square, SVG ring): the five canvas trackers are full-view layers that, at 6K, load the GPU more than Iron and make the whole page drop frames. The probe's `--no-capture=1` (`NO_CAPTURE=1` for `latency-trace.ts`) drives the pointer without recording, and `PLAIN_CHROME=1` launches Chrome without Playwright's default switches; dropped frames then come from the trace's presentation feedback. Safari runs launch a fresh Safari per run when it was not already running, and close all its windows and quit after each (`scripts/latency/latency-key --only com.apple.Safari`, a key press only while Safari is frontmost), so no test tabs are restored later; the first run after a launch can be slow for the square, so add a discarded warm-up. `URL=http://127.0.0.1:5185/Ironfell/scripts/latency/bare/index.html` measures a plain page with no Iron (DOM square, Canvas2D, SVG, Konva) as each browser's baseline (2026-11-05: Chrome ≈36 ms, Safari and Firefox ≈52 ms). `BROWSER=firefox` runs a separate Firefox instance per run with a temporary profile in `/private/tmp` (under `~/Documents` macOS asks for folder access; spawned directly from a sandboxed shell Firefox cannot load the profile, so it goes through `open -na`), quit by profile path after the run. Playback smoothness without input: `latency-anim.ts` (`CASES=off,on,always`, `CHROME_WINDOW`, `REPEAT`, `RATE`, `DURATION`, `TRACE=1`) loops the demo square's clip recoloured yellow while `latency-probe --watch=1` records it; `scripts/latency/judder.py` reports repeated and doubled steps (worker-frame-hold.md §6f). `latency-churn.ts` (`SELECT=1` selects the square first) lists DOM mutations by dock panel during playback and Chrome's layout/style/script ms/s with each panel hidden in turn. With the built-in display open, run on a monitor with `CHROME_POS=x,y` (global top-left points; 1710,0 for the LG right of a 1710-pt built-in) and `PROBE_ARGS=--display=largest` (the probe's `--display=largest`; default is the built-in display). Frame modes (plans/rendering/worker-frame-hold.md §6g): with cross-origin isolation (`bun run dev`, or `IRON_COI=1 bunx vite …`) Chromium runs the mode controller by default (`frameHold=auto`: early per interaction, late as the fallback, the clock ahead between interactions; the FPS readout shows `late`/`ahead`, and in developer mode always the mode (`early`, `off` with the reason in its tooltip), with the engine build and the frontend's commit in the tooltip; `__frameHold.stats()` has the counts, `events` (mode changes and why) and `skips`); `&frameHold=off` is free running, `&frameHold=late` fixed late, `&halfRate=1` the half-rate experiment, `&holdOrder=first` the old hold order (wait, then the page's work; the default works first, then waits), `&pageLayout=a-b` a random a–b ms of real layout per frame (off-screen text, steered to the target; `__frameHold.stats()` shows `layoutMs` and, in development, `lateWrites`, DOM changes after the hold began), `&pageBusy=a-b` a random a–b ms of main-thread work per frame (a busy UI; it changes nothing on screen, since a per-frame 1 px change alone made Chrome draw the page later). `scripts/latency/flips.py run.csv` classifies a drag's displayed frames as Vello in the same frame as the DOM, one behind or further, with flips (the judder); `scripts/latency/animtrace.py trace.json` reads a `TRACE=1` playback trace per vsync (worker frame made the draw or not, draw and dispatch times). `latency-measure.ts` takes `URL=…&workerTickBusy=N&frameHold=…` per configuration (`TAG` names it). The prototype's options: `&frameHold=on` (Chromium only; `always`: hold without recent input; `force`: in any engine; `&holdCap=ms`, the starting limit, default 4.5; `&holdAdapt=0`, the fixed cap) and `&workerTickBusy=N` for heavier ticks; the driver prints `frame hold: … held … released … skipped …` per run, `&with=…`/`&without=…`
(views added to or closed from the measurement layout); see the plan's §6. Each run prints `label: p50 ms …` and writes JSON and per-frame CSV
to `.agents/latency/measure/`; a run that cannot find the square saves the still it
searched (`<label>-still.png`). Reference values (60 Hz built-in display, 2026-11-05),
p50 ms, DOM / engine ring / square, after the worker tick-on-input fix: Chrome
worker ≈36 / 36 / 36 in every layout (with `?workerTick=raf`: ≈36 / 50 / 52 in
`solo` and `panels`); Chrome main ≈36 in `solo`, ≈52 for everything with panels
(the whole page a frame late); Safari worker ≈52 / 52 / 52; native baseline
≈52 / 52 / 52 (varies by a frame between runs and layouts); native adaptive
≈52 / 38 / 36. The five page-thread trackers agree within 0.4 ms. All trackers must report
`concentric at rest: yes`.

### Native layer / presentation controls — 2026-10-27

`IRON_NATIVE_FILM=1 IRON_NATIVE_LAYER_PROBE=1 IRON_NATIVE_RENDER_MODE=pipelined IRON_NATIVE_PACING=display bun run native:run` adds a green native Bevy sprite at −192 CSS px on
the final window camera, bypassing Vello. `IRON_NATIVE_PROBE_WEBVIEW=detached` removes WKWebView
after fixture setup, freezing the native drawable rectangle first. Use unheld
cursor motion in the detached case; HTML button ownership is absent. Restart to
restore the webview. Both shortcuts select pipelined/display; use
`IRON_NATIVE_LAYER_PROBE=1 IRON_NATIVE_FILM=1 IRON_NATIVE_PACING=late bun run native:run` for sync/late. Normal app launch
is unchanged. Idle marker updates do not dirty unchanged sprite/visibility state.

After native packaging, run `python3 src-native/test/validate-layers.py` from the
worktree root. Expected: six cases (overlay/hidden/detached × pipelined/display
or sync/late), **12 available GPU readbacks passing per case**, zero difference
between corrected direct-sprite and Vello marker input positions. It checks actual
attachment/hidden state and rejects any native viewport resize after detachment.
These correctness controls force continuous rendering and scripted pointer positions.
Capture windows temporarily stay above other apps, then close. Keep the screen
unlocked. Fully transparent surface readbacks are recorded as unavailable and
retried; they do not establish a stale frame or a successful check. Fewer than
12 available captures fails the suite. Readback/PNG work perturbs frame timing.

For presentation timing, omit readback and use the process-local Metal HUD flags
in `src-native/README.md`. Show `presentdelay`, not just CPU frame time. Generate a
five-second report using the HUD menu; set `MTL_HUD_REPORT_URL` to a writable path.
`MTL_HUD_LOG_ENABLED=1` is the current logging flag (the old
`MTL_HUD_LOGGING_ENABLED` spelling did not enable it). Save reports beneath
`.agents/latency/hud-reports/`, with matching `present-<case>.log` native logs.
`python3 src-native/test/analyze-presentation.py` extracts metrics and source hashes
into `plans/rendering/benchmarks/native-presentation-2026-10-27.json`.

Completed reports: sync/late FIFO/2 with overlay **40.60 ms** average Present Delay,
webview detached **35.81 ms**, FIFO/1 (two drawables) **30.57 ms**; sync/display with
native display sync disabled **46.38 ms**. GPU averages are **2.22–2.72 ms**. These
are separate five-second exploratory captures, not randomized repeated trials.
The smaller queue also increases CPU acquisition waits. Live HUD showed Composited
with the webview removed, and Direct in the VSync-off control; neither removal nor
VSync off eliminated the long post-submit wait. Pipelined/display and sync/display
FIFO report attempts did not produce completed saved reports and are excluded from
quantitative report comparisons. The baseline's live HUD observation is recorded
separately. No absolute input-to-photon or 3D/standalone-Bevy result is inferred.

Optional `IRON_NATIVE_GPU_DONE_TRACE=1` adds queue completion callback receipt to
CPU traces. Receipt can wait for device polling: it is not a GPU timestamp or actual
presentation feedback. Early detached measurements that changed the viewport from
2560×1636 to 2560×1700 were excluded; the diagnostic now freezes the original view.
Owning record: vision 08, “Native layer / presentation controls”.


Final control validation: **72/72** GPU captures in 24.89 s, zero unavailable
readbacks, zero corrected sprite/Vello position difference, all attachment/viewport
checks pass. The native package builds; seven Rust tests and frontend checking
(zero errors, ten existing warnings) pass. Ordinary native workspace **23/23**
and native filming **14/14** regressions pass in 18.44/2.99 s. No new full browser
smoke is claimed for these native controls; the prior filming-mode full smoke passes.

The native filming suite also passes 14/14 with the direct Bevy sprite enabled.


### Standalone Bevy/winit control — 2026-10-27

`src/iron_latency_probe/run.sh` runs the isolated Cargo project in
`src/iron_latency_probe`. It enables native winit in its own workspace, not the
shared Iron workspace, and reuses the worktree target cache with the same dev-opt
profile. Its dependency graph has no Tauri, Vello or Iron engine. Use the ordinary
pipelined/FIFO default first; `IRON_BEVY_RENDER_MODE=sync` is an optional comparison.

Expected startup `IRON_BEVY_PROBE`: runner `Bevy DefaultPlugins / winit`,
`renderThreadEnabled:true`, `presentMode:Fifo`, logical size `[1280,818]`, physical
size `[2560,1636]`, DPR 2 on the tested screen, requested maximum frame latency null
(backend default). Green diamond follows the pointer at −192 logical px; yellow
48 logical px square drags, R resets. Keep only one comparison app running and match
the hosted control's continuous-rendering setting to the focused Bevy app.

`bundle-macos.sh` in that project creates the minimal macOS app wrapper under
`.agents/latency/standalone/` around the same executable, for native GUI automation.
The unbundled executable works normally but has no bundle ID in the app inventory.
Input trace (`IRON_BEVY_INPUT_TRACE=1`) is observational and off by default. It
records original OS event timestamp, monitor receipt and PostUpdate, plus cursor,
button and target state. Live validation shows held target motion and release;
these input ages are not presentation/photon measurements. No hosted-versus-standalone
latency conclusion is claimed yet. Details: the standalone project's README.

### Shared point controls / fiber and separate shadow — 2026-11-03

With a WebGL2 build and a Vite server running, from this skill directory:

```sh
URL='http://localhost:5186/Ironfell/?gfx=webgl2&demo&showcase=0&layout=default' bun run attachments.ts
```

The fixture creates two ordinary paths and two empty controls, makes a main,
then uses the inspector task to attach both paths' starts and ends. It exposes
input/output ports, moves an endpoint through the document inspector API,
edits an offset through the task, detaches and undoes, and saves/reloads the
canonical document. Reload must close the task even when node IDs are reused.
Light/dark screenshots go to `.agents/attachments/` in the worktree.
Expected output:

```text
PASS: two paths, two shared controls, ports, movement, offsets, detach/undo, save/reload; no page errors
```

The movement step sets a transform through `inspect_patch`; this fixture does
not simulate a pointer drag. Native authoring tests cover transformed parent
frames, rotation/scale following, preserved tangent vectors, contour ownership,
instance overrides, port exposure, a plugged laser moving both paths, unplug
placement, atomic rejection, detach/undo, deletion/cycle rejection and rejection
of attachment-metadata removal while its position bindings remain.

Validation in the isolated `fiber-authoring` worktree: cold WebGL2 build and
final warm rebuild pass; the full native document suite passes **254 tests**, with
one pre-existing ignored test; frontend checking has **0 errors / 10 existing
warnings**. The full GPU browser `smoke.txt` completes with its expected semantic
outputs and `(no page errors)`. After the final lifecycle/removal guards, the
native suite, frontend checking and attachment browser fixture pass again.
Only WebGL2 was built here; the WebGPU artifact is a loader stub, so use
`?gfx=webgl2` when trying this worktree until a real WebGPU build is made.

Integration with current `master`, 2026-11-04: **258 native document tests pass**
(one ignored), frontend checking remains at **0 errors / 10 existing warnings**,
and the attachment fixture passes at both **1× and 2×** (`DPR=2`). The fixture now
waits for a queryable demo document, avoiding the startup race where `.loading`
has not mounted yet. The new fast default smoke passes all six assertions in
**4.72 s**; the original broad script is preserved byte-for-byte as
`smoke-full.txt`. No additional broad-suite run is claimed for this integration.
