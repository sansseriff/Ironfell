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
square (step by step, with a mid-drag screenshot and view showing the
shadow rect bound to it following) and the torus, undoes both with Cmd+Z,
redoes one with Cmd+Shift+Z, scrubs the slider and checks the bar bound to
it, drags the bar so its bound y inverts onto the slider, scrubs the
timeline and checks the clip driving the torus, applies one good and one
bad edit in the model's markup, runs the pendulum through every
constraint-system phase 3 exit (drag the ball, drag beyond reach, undo,
a locked arm, an out-of-reach model edit adjusted then refused with
exact, the length slider, the fix moved to the ball and the platform
dragged, `.by` markers), round-trips the document through save and load,
and prints the transactions the store applied. Expected output, minus
screenshot lines and the per-frame `solve:` lines:

```
["<rect id=\"#M\" name=\"square\" x=\"620\" y=\"340\"","<rect id=\"#T\" name=\"shadow\" x=\"720\" x.bind=\"#M.transform2d.x + 100\" y=\"340\" y.bind=\"#M.transform2d.y\""]
redraw (document): 2 of 35 nodes changed [#M #T]; damage 499,379–741,501
redraw (document): 2 of 35 nodes changed [#M #T]; damage 559,339–801,461
"<rect id=\"#q\" name=\"bar\" x=\"780\" y=\"370\" y.bind=\"640 - #p.params.level * 300\" … h=\"270\" h.bind=\"#p.params.level * 300\" …>"
"<rect id=\"#q\" name=\"bar\" x=\"730\" y=\"400\""
"value=\"0.8\"/>"
"<mesh id=\"#P\" name=\"torus\" pos=\"0,1.5,0\" rot=\"0,3.63…,0\" … <driven-by ref=\"#R\" rel=\"animates\" prop=\"transform3d.rot\"/>"
{"ok":true,"version":9,"aliases":{"$n":"#r"}}
"<edit-failed applied=\"none\" version=\"9\">\n  <error op=\"0\" kind=\"unknown_node\">no node #zz</error>\n  <error op=\"1\" kind=\"cycle\">binding #N.radius.r would depend on itself</error>\n</edit-failed>"
"<rect id=\"#r\" name=\"from-model\" x=\"300\" y=\"120\" … w=\"80\" w.bind=\"#O.slider.value * 100\" h=\"40\" color=\"#22aa55ff\" …/>"
["<circle id=\"#Z\" name=\"ball\" x=\"440.43…\" x.by=\"#U\" y=\"298.93…\" y.by=\"#U\""," rot=\"-1.11…\""," h=\"238.93…\""]
"move circle" by Human: cluster #U re-solved; moved 2 node(s): #X #Y
"<circle id=\"#Z\" name=\"ball\" x=\"32….\" x.by=\"#U\" y=\"359.9…\" y.by=\"#U\""
["<circle id=\"#Z\" name=\"ball\" x=\"320\" x.by=\"#U\" y=\"360\" y.by=\"#U\"","<circle id=\"#X\" name=\"j1\" x=\"320\" x.by=\"#U\" y=\"160\" y.by=\"#U\""]
#a locked for this drag: #a.transform2d.x is bound to 4 free leaves (#W.transform2d.x, #W.transform2d.y, #X.transform2d.x, #X.transform2d.y)
#a locked for this drag: #a.transform2d.y is bound to 4 free leaves (…)
"<applied version=\"14\">\n  <solver clusters=\"#U\" moved=\"#X #Y #Z\"/>\n  <unmet leaf=\"#Z.transform2d.x\" requested=\"900\" solved=\"619.99…\">constrained by cluster #U: the constraints allow 619.99… at most</unmet>\n</applied>"
"<edit-failed applied=\"none\" version=\"14\">\n  <error op=\"0\" kind=\"unsatisfiable\">exact transaction not honoured: #Z.transform2d.x requested 900 but constraints allow 619.99…</error>\n</edit-failed>"
"<constraint id=\"#h\" name=\"link1\" kind=\"distance\" value=\"120.00000000000001\" value.bind=\"#e.slider.value * 100\" enabled=\"true\">"
true
"<circle id=\"#Z\" name=\"ball\" x=\"320\" y=\"420\""
["<circle id=\"#Z\" name=\"ball\" x=\"320\" y=\"420\"","<rect id=\"#V\" name=\"platform\" x=\"280.9…\" x.by=\"#U\" y=\"38.1…\" y.by=\"#U\""]
"x.by=\"#U\""
{"roundtrip_equal":true,"bytes":14263}
"<document version=\"18\"> |   <group id=\"#1\" name=\"scene\" count=\"27\"> | …"
applied "open demo scene" (65 ops, System) -> v1
applied "move rect" (2 ops, Human) -> v2
applied "move mesh" (1 ops, Human) -> v3
applied "undo: move mesh" (1 ops, System) -> v4
applied "undo: move rect" (2 ops, System) -> v5
applied "redo: move rect" (2 ops, System) -> v6
applied "set slider" (1 ops, Human) -> v7
applied "move rect" (2 ops, Human) -> v8
applied "model edit" (2 ops, Model) -> v9
applied "move circle" (3 ops, Human) -> v10
"move circle" by Human: cluster #U re-solved; moved 2 node(s): #X #Y
applied "move circle" (3 ops, Human) -> v11
"move circle" by Human: cluster #U re-solved; moved 2 node(s): #X #Y
applied "undo: move circle" (7 ops, System) -> v12
applied "undo: move circle" (7 ops, System) -> v13
#a locked for this drag: … (twice)
applied "model edit" (1 ops, Model) -> v14
"model edit" by Model: cluster #U re-solved; moved 3 node(s): #X #Y #Z; unmet: #Z.transform2d.x wanted 900 got 619.99…
applied "undo: model edit" (7 ops, System) -> v15
applied "set slider" (1 ops, Human) -> v16
"set slider" by Human: cluster #U re-solved; moved 3 node(s): #X #Y #Z
applied "model edit" (2 ops, Model) -> v17
applied "move rect" (3 ops, Human) -> v18
"move rect" by Human: cluster #U re-solved; moved 3 node(s): #W #X #Y
loaded document v18 (53 live nodes)
(no page errors)
```

Solved values carry solver noise in their last digits (shown as `…`
above); everything else is exact. Each drag frame through a cluster logs
`solve: cluster #U, 12 vars, N iterations, M evaluations, T ms`; T is
0.0–0.7 ms here, with a single ~2 ms frame where the chain straightens.

Any `locked for this drag` line names an axis the drag held still and why;
any `differs from Bevy` line means the store's world transform disagrees
with the entity's and is a bug. Every rebuild of the 2D layer logs one
`redraw (<reason>): N of M nodes changed [ids]; damage x0,y0–x1,y1` line:
the reason is `document` (the reconciler's changed-node set, with
`, structure` when the tree's shape changed), `panel layout`, or
`selection`; the damage is the union of the changed nodes' old and new
document-space bounds.

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
| `peek [regex]` | like `logs`, without clearing the buffer |
| `errors` | print uncaught page errors |
| `panel` | print the viewer panel rect |
| `sleep ms` | wait |
| `quit` | close the browser |

Where things are in the demo scene, in document space: the draggable square
covers (500,420)–(580,500); the circle is centred at (160,330); the slider
track runs (100,600)–(400,624) and scrubbing along it previews the value
through the store; the bar at x=780 has its height and top bound to the
slider through a definition: the library (a root) holds a `bar` definition
with one parameter, `level`, and a template rect; the scene holds an entity
instantiated from it (its `level` bound to the slider) and the rect that
depicts it, whose y and h read the entity's params. The template itself is
never drawn. The "shadow" rect at (600,420) has x and y bound to the square's;
the torus sits near the viewer centre, about (435,350) at a 1600×1000
window. The timeline holds a "slide" clip (0.5–2.5 s, square x 500→700)
and a "spin" clip (1–4 s, torus rotation). Any `<attr>.bind` in a view is a
binding and any `<driven-by>` names a clip.

The pendulum (constraint-system.md phase 3) is a group with a fixed
platform (240,40)–(400,60), joints every 100 px below its hinge at (320,60),
a ball (r=14) at (320,360), arms drawn between the joints by bindings, a
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

74 tests pass (31 unit, 43 integration). The host target is required because
`.cargo/config.toml` defaults the whole workspace to wasm. The solver crate
has its own: `cargo test -p iron_solve --target aarch64-apple-darwin` (9
tests).

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
