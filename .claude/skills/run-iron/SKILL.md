---
name: run-iron
description: Build, run, drive, screenshot, and test the Iron app (Bevy + Vello in wasm, Svelte shell). Use when asked to start Iron, build the wasm, take a screenshot of the running app, drag something in the viewer, check the console for applied transactions, or run the document store tests.
---

Iron is a Bevy app compiled to wasm, running in a web worker behind a Svelte
shell served by Vite. Drive it with `.claude/skills/run-iron/drive.ts`: a
stdin-scripted Playwright driver that launches the installed Google Chrome
headless (SwiftShader, WebGL2), waits for the engine, and takes screenshots,
drags, and console dumps. All paths below are relative to the repo root.

Verified on macOS (Apple Silicon) on 2026-09-18. Linux is untested; the only
platform-specific piece is the Chrome path (`CHROME` env var overrides it).

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

```bash
.claude/skills/run-iron/build.sh
```

Builds the `dev-opt` profile with the `webgl2` feature, binds it into
`src-ui/wasm/webgl2/`, and stubs `src-ui/wasm/webgpu/` so Vite's import of
both artifact URLs resolves. About 2.5 minutes cold, 15 seconds warm. The
wasm is ~128 MB because the profile keeps debug info; that is expected.

## Run (agent path)

```bash
.claude/skills/run-iron/serve.sh start
cd .claude/skills/run-iron && bun run drive.ts < smoke.txt; cd -
.claude/skills/run-iron/serve.sh stop
```

`smoke.txt` opens the app, waits for the engine, screenshots, drags the
square and the torus, undoes both with Cmd+Z, redoes one with Cmd+Shift+Z,
scrubs the slider and checks the bar bound to it, scrubs the timeline and
checks the clip driving the torus, applies one good and one bad edit in the
model's markup, round-trips the document through save and load, and prints
the transactions the store applied. Expected output, minus screenshot lines:

```
"<bar id=\"#P\" name=\"bar\" x=\"780\" y=\"370\" y.bind=\"640 - #O.slider.value * 300\" … h=\"270\" h.bind=\"#O.slider.value * 300\" …/>"
"<bar id=\"#P\" name=\"bar\" x=\"730\" y=\"370\""
"<mesh id=\"#Q\" name=\"torus\" pos=\"0,1.5,0\" rot=\"0,3.63…,0\" … <driven-by ref=\"#S\" rel=\"animates\" prop=\"transform3d.rot\"/>"
{"ok":true,"version":9,"aliases":{"$n":"#U"}}
"<edit-failed applied=\"none\" version=\"9\">\n  <error op=\"0\" kind=\"unknown_node\">no node #zz</error>\n  <error op=\"1\" kind=\"cycle\">binding #N.radius.r would depend on itself</error>\n</edit-failed>"
"<rect id=\"#U\" name=\"from-model\" x=\"300\" y=\"120\" … w=\"90\" w.bind=\"#O.slider.value * 100\" h=\"40\" color=\"#22aa55ff\" …/>"
{"roundtrip_equal":true,"bytes":7533}
"<document version=\"9\"> |   <group id=\"#1\" name=\"scene\" count=\"25\"> | …"
applied "open demo scene" (31 ops, System) -> v1
applied "move rect" (2 ops, Human) -> v2
applied "move mesh" (1 ops, Human) -> v3
applied "undo: move mesh" (1 ops, System) -> v4
applied "undo: move rect" (2 ops, System) -> v5
applied "redo: move rect" (2 ops, System) -> v6
applied "set slider" (1 ops, Human) -> v7
applied "move bar" (1 ops, Human) -> v8
applied "model edit" (2 ops, Model) -> v9
loaded document v9 (30 live nodes)
(no page errors)
```

`__iron.edit(xml)` applies `<edit>` markup exactly as a model reply would be
applied (see `src-ui/model/edit.ts` for the verbs); `__iron.ask(text)` runs a
real model turn and needs an Anthropic API key entered in the Document
panel, so the smoke script does not call it.

`window.__iron` is the document API for `eval`: `await __iron.save()` returns
the canonical JSON, `await __iron.load(text)` replaces the document, and
`await __iron.view({fidelity:'summary', depth: 2})` renders a view. The
Document panel on the left shows the full tree view and refreshes on every
applied transaction.

Screenshots land in `.claude/skills/run-iron/screenshots/<name>.png`. Look at
them; a frame showing only "Loading..." means the engine never came up.

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
| `nav [url]` | open the app; default `http://localhost:5173/Ironfell/?gfx=webgl2` |
| `ready` | wait until the `.loading` overlay leaves the DOM and the frame loop is cycling (cadence probe frame count advancing between polls), then read the viewer panel rect. About 6 s under SwiftShader |
| `shot <name>` | screenshot to `screenshots/<name>.png` |
| `move x y`, `down`, `up`, `drag x0 y0 x1 y1 [steps]` | pointer in window pixels |
| `dmove x y`, `ddrag x0 y0 x1 y1 [steps]` | the same in document 2D space: viewer-panel top-left origin, y-down, the space `transform2d.x/y` are in |
| `pdrag <panel> x0 y0 x1 y1 [steps]` | drag relative to a named DOM panel (`viewer`, `timeline`, `sidebar`); a drag along the timeline scrubs the playhead over its 30 s |
| `key <Key>` | press a key (Playwright key names); `Meta+z` undoes, `Meta+Shift+z` redoes |
| `eval <js>` | evaluate in the page and print the JSON result; an async IIFE is awaited |
| `logs [regex]` | print console output since the last `logs`, optionally filtered; Rust `info!`/`warn!` lines from the worker appear here |
| `errors` | print uncaught page errors |
| `panel` | print the viewer panel rect |
| `sleep ms` | wait |
| `quit` | close the browser |

Where things are in the demo scene, in document space: the draggable square
covers (500,420)–(580,500); the circle is centred at (160,330); the slider
track runs (100,600)–(400,624) and scrubbing along it previews the value
through the store; the bar at x=780 has its height and top bound to the
slider; the torus sits near the viewer centre, about (435,350) at a
1600×1000 window. The timeline holds a "slide" clip (0.5–2.5 s, square x
500→700) and a "spin" clip (1–4 s, torus rotation). Any `<attr>.bind` in a
view is a binding and any `<driven-by>` names a clip; an axis whose leaf is
bound, or clip-driven at the current time, is locked for the whole drag (the
bar moves in x only), and a node with both axes locked does not drag.

## Run (human path)

```bash
bun run dev   # -> http://localhost:5173/Ironfell/ ; Ctrl-C to stop
```

Opens the WebGPU build in a real browser when one exists; the human path
needs `./build-wasm.sh` for the release artifacts, which this skill does not
run.

## Test

```bash
cargo test -p iron_document --target aarch64-apple-darwin
```

21 tests pass (13 unit, 8 integration). The host target is required because
`.cargo/config.toml` defaults the whole workspace to wasm.

## Gotchas

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

## Troubleshooting

- **`vite did not start`**: something else holds `:5173`. `serve.sh stop`
  then `start` again; it kills the listener first, so a stale Vite from a
  previous session is the usual cause.
- **Build fails with "no graphics backend selected"**: `cargo build` was run
  without `--features webgl2` (or `webgpu`). `build.sh` passes it; the old
  `run-wasm.sh` does not and is stale.
- **`wasm-bindgen` "schema version" mismatch**: CLI and crate versions
  differ. `build.sh` reinstalls the CLI at the `Cargo.lock` version.
