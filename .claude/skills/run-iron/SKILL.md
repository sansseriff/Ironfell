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

The shell is a dock of views (`src-ui/views/`, plans/panels-and-session.md):
tabs and splits, each a view onto a composition or an HTML view. The
document-space commands below address the **Main** tab, whose view id is
`viewer`; it shows the `main` composition, which embeds the scene, the
pendulum, the controls and the 3D assembly where they sat before the split.
The other tabs (Scene, Pendulum, Controls, Assembly, Card) show one
composition each. A hidden tab has no panel rectangle (`panel <id>` prints
zeros). Each driver run starts with a fresh browser profile, so the default
layout; in a normal browser the layout is saved, and `?layout=default` or
the Document view's **Reset layout** restores the default.

`smoke.txt` opens the app, waits for the engine, screenshots, drags the
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
runs the select tool (stage E, plans/select-tool.md): click, a corner
resize, a Shift-snapped rotation, undo of both, a group's click /
double-click / Esc / Esc, a marquee, arrow nudges, and delete of the
pendulum undone in one step. Expected output, minus screenshot lines,
`nav`/`ready` lines, and the per-frame `solve:` lines:

```
["<rect id=\"#M\" name=\"square\" x=\"620\" y=\"340\"","<rect id=\"#U\" name=\"shadow\" x=\"720\" x.bind=\"#M.transform2d.x + 100\" y=\"340\" y.bind=\"#M.transform2d.y\""]
redraw (document): 2 of 39 nodes changed [#M #U]; damage 499,379–741,501
redraw (document): 2 of 39 nodes changed [#M #U]; damage 559,339–801,461
"<rect id=\"#11\" name=\"bar\" x=\"780\" y=\"370\" y.bind=\"640 - #10.params.level * 300\" rot=\"0\" sx=\"1\" sy=\"1\" w=\"60\" h=\"270\" h.bind=\"#10.params.level * 300\" color=\"#3380e6ff\" opacity=\"1\">"
"<rect id=\"#11\" name=\"bar\" x=\"730\" y=\"400\""
"value=\"0.8\"/>"
"<mesh id=\"#Q\" name=\"torus\" pos=\"0,1.5,0\" rot=\"0,3.64…,0\" scale=\"1,1,1\" asset=\"torus\"> <driven-by ref=\"#S\" rel=\"animates\" prop=\"transform3d.rot\"/>"
{"ok":true,"version":9,"aliases":{"$n":"#16"}}
"<edit-failed applied=\"none\" version=\"9\">\n  <error op=\"0\" kind=\"unknown_node\">no node #zz</error>\n  <error op=\"1\" kind=\"cycle\">binding #N.radius.r would depend on itself</error>\n</edit-failed>"
"<rect id=\"#16\" name=\"from-model\" x=\"300\" y=\"120\" rot=\"0\" sx=\"1\" sy=\"1\" w=\"80\" w.bind=\"#O.slider.value * 100\" h=\"40\" color=\"#22aa55ff\" opacity=\"1\"/>"
["<circle id=\"#a\" name=\"ball\" x=\"440\" x.by=\"#V\" y=\"300\" y.by=\"#V\""," rot=\"-1.12…\""," h=\"240\""]
"move circle" by Human: cluster #V re-solved; moved 2 node(s): #Y #Z
"<circle id=\"#a\" name=\"ball\" x=\"323.71…\" x.by=\"#V\" y=\"359.97…\" y.by=\"#V\""
["<circle id=\"#a\" name=\"ball\" x=\"320\" x.by=\"#V\" y=\"360\" y.by=\"#V\"","<circle id=\"#Y\" name=\"j1\" x=\"320\" x.by=\"#V\" y=\"160\" y.by=\"#V\""]
#b locked for this drag: #b.transform2d.x is bound to 4 free leaves (#X.transform2d.x, #X.transform2d.y, #Y.transform2d.x, #Y.transform2d.y)
#b locked for this drag: #b.transform2d.y is bound to 4 free leaves (#X.transform2d.x, #X.transform2d.y, #Y.transform2d.x, #Y.transform2d.y)
"<applied version=\"14\">\n  <solver clusters=\"#V\" moved=\"#Y #Z #a\"/>\n  <unmet leaf=\"#a.transform2d.x\" requested=\"900\" solved=\"619.99…\">constrained by cluster #V: the constraints allow 619.99… at most</unmet>\n</applied>"
"<edit-failed applied=\"none\" version=\"14\">\n  <error op=\"0\" kind=\"unsatisfiable\">exact transaction not honoured: #a.transform2d.x requested 900 but constraints allow 619.99…</error>\n</edit-failed>"
"<constraint id=\"#i\" name=\"link1\" kind=\"distance\" value=\"120.00…\" value.bind=\"#f.slider.value * 100\" enabled=\"true\">"
true
"<circle id=\"#a\" name=\"ball\" x=\"320\" y=\"420\""
["<circle id=\"#a\" name=\"ball\" x=\"320\" y=\"420\"","<rect id=\"#W\" name=\"platform\" x=\"281.00…\" x.by=\"#V\" y=\"38.20…\" y.by=\"#V\""]
"x.by=\"#V\""
"#s"
found at … (a point on the card dot; varies with the sweep)
true
["#l","#Q"]
clicked Link
applied "link" (4 ops, Human) -> v21
"<entity id=\"#18\" name=\"box\" def=\"#17\" definition=\"thing\">\n    <figure id=\"#l\" type=\"rect\" name=\"box\" in=\"#1\" composition=\"scene\"/>\n    <figure id=\"#Q\" type=\"mesh\" name=\"torus\" in=\"#P\" composition=\"assembly\"/>\n  </entity>"
"<mesh id=\"#Q\" name=\"torus\" … asset=\"torus\" of=\"#18\">"
clicked Show in assembly
clicked Show in scene
focus #Q: panel viewer pans to centre it
focus #l: panel viewer pans to centre it
false
true
"<edit-failed applied=\"none\" version=\"23\">\n  <question kind=\"merge\" a=\"#11\" b=\"#Q\">\n    <entity id=\"#10\" name=\"bar\" definition=\"bar\" level=\"{= #O.slider.value}\" figures=\"#11\"/>\n    <entity id=\"#1A\" name=\"box\" definition=\"thing\" figures=\"#l #Q\"/>\n    Both already picture an entity. Resend <link-figures> with keep=\"#10\" or keep=\"#1A\"; the other entity is deleted and its figures move to the one kept.\n  </question>\n</edit-failed>"
[true,"<entity id=\"#10\" name=\"bar\" def=\"#w\" level=\"0.8\" level.bind=\"#O.slider.value\" definition=\"bar\">\n    <figure id=\"#11\" type=\"rect\" name=\"bar\" in=\"#1\" composition=\"scene\"/>\n    <figure id=\"#l\" type=\"rect\" name=\"box\" in=\"#1\" composition=\"scene\"/>\n    <figure id=\"#Q\" type=\"mesh\" name=\"torus\" in=\"#P\" composition=\"assembly\"/>\n  </entity>"]
{"roundtrip_equal":true,"bytes":17779}
"<document version=\"25\"> |   <composition id=\"#1\" name=\"scene\" notation=\"free\" dims=\"2d\" count=\"26\"> |     <rect id=\"#2\" x=\"111\" y=\"231\" rot=\"0\" sx=\"1\" sy=\"1\" w=\"30\" h=\"30\" color=\"#986ecaff\" opacity=\"1\"/> |     <rect id=\"#3\" x=\"406\" y=\"241\" rot=\"0\" sx=\"1\" sy=\"1\" w=\"30\" h=\"30\" color=\"#9d399aff\" opacity=\"1\"/>"
applied "open demo scene" (82 ops, System) -> v1
applied "move rect" (2 ops, Human) -> v2
applied "move mesh" (1 ops, Human) -> v3
applied "undo: move mesh" (1 ops, System) -> v4
applied "undo: move rect" (2 ops, System) -> v5
applied "redo: move rect" (2 ops, System) -> v6
applied "set slider" (1 ops, Human) -> v7
applied "move rect" (2 ops, Human) -> v8
applied "model edit" (2 ops, Model) -> v9
applied "move circle" (2 ops, Human) -> v10
"move circle" by Human: cluster #V re-solved; moved 2 node(s): #Y #Z
applied "move circle" (3 ops, Human) -> v11
"move circle" by Human: cluster #V re-solved; moved 2 node(s): #Y #Z
applied "undo: move circle" (7 ops, System) -> v12
applied "undo: move circle" (6 ops, System) -> v13
#b locked for this drag: #b.transform2d.x is bound to 4 free leaves (#X.transform2d.x, #X.transform2d.y, #Y.transform2d.x, #Y.transform2d.y)
#b locked for this drag: #b.transform2d.y is bound to 4 free leaves (#X.transform2d.x, #X.transform2d.y, #Y.transform2d.x, #Y.transform2d.y)
applied "model edit" (1 ops, Model) -> v14
"model edit" by Model: cluster #V re-solved; moved 3 node(s): #Y #Z #a; unmet: #a.transform2d.x wanted 900 got 619.99…
applied "undo: model edit" (7 ops, System) -> v15
applied "set slider" (1 ops, Human) -> v16
"set slider" by Human: cluster #V re-solved; moved 3 node(s): #Y #Z #a
applied "model edit" (2 ops, Model) -> v17
applied "move rect" (3 ops, Human) -> v18
"move rect" by Human: cluster #V re-solved; moved 3 node(s): #X #Y #Z
applied "move circle" (2 ops, Human) -> v19
applied "undo: move circle" (2 ops, System) -> v20
applied "link" (4 ops, Human) -> v21
applied "undo: link" (4 ops, System) -> v22
applied "link" (4 ops, Model) -> v23
applied "link" (5 ops, Model) -> v24
applied "undo: link" (5 ops, System) -> v25
loaded document v25 (70 live nodes)
(no page errors)
undefined
["#M"]
{"x":500,"y":420,"w":120,"h":100,"deg":0}
{"x":553,"y":392,"w":120,"h":100,"deg":45}
{"x":500,"y":420,"w":80,"h":80,"deg":0}
["#V"]
["#a"]
["#V"]
["#14"]
[]
["#o","#14","#15"]
{"x":501,"y":430,"w":80,"h":80,"deg":0}
true
true
tab Pendulum
[true,false]
tab Main
[false,true]
applied "open demo scene" (82 ops, System) -> v1
applied "resize rect" (2 ops, Human) -> v2
applied "rotate rect" (3 ops, Human) -> v3
applied "undo: rotate rect" (3 ops, System) -> v4
applied "undo: resize rect" (2 ops, System) -> v5
applied "nudge rect" (1 ops, Human) -> v6
applied "nudge rect" (1 ops, Human) -> v7
applied "delete group" (1 ops, Human) -> v8
applied "undo: delete group" (16 ops, System) -> v9
(no page errors)
```

Solved values carry solver noise in their last digits (shown as `…`
above); everything else is exact. After the platform drag the pendulum is
not always fully settled, so a later transaction (the links) may log a
`"link" by …: cluster #V re-solved; moved 1 node(s): #W` line and its undo
one op more; that is the under-converged solve noted in
`constraint-system.md` §14, not the link. Each drag frame through a cluster logs
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

Every view renders into its own texture and the window only composites
(`plans/view-textures.md`), and a frame that changes nothing visible is not
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
| `nav [url]` | open the app; default `http://localhost:5173/Ironfell/?gfx=webgl2` |
| `ready` | wait until the `.loading` overlay leaves the DOM and the frame loop is cycling (cadence probe frame count advancing between polls), then read the viewer panel rect. About 6 s under SwiftShader |
| `shot <name>` | screenshot to `screenshots/<name>.png` |
| `move x y`, `down`, `up`, `drag x0 y0 x1 y1 [steps]` | pointer in window pixels |
| `dmove x y`, `ddrag x0 y0 x1 y1 [steps]` | the same in document 2D space: viewer-panel top-left origin, y-down, the space `transform2d.x/y` are in |
| `pdrag <panel> x0 y0 x1 y1 [steps]` | drag relative to a named DOM panel (`viewer`, `assembly`, `timeline`); a drag along the timeline scrubs the playhead over its 30 s |
| `key <Key>` | press a key (Playwright key names); `Meta+z` undoes, `Meta+Shift+z` redoes |
| `keydown <Key>`, `keyup <Key>` | hold and release a key, e.g. `Shift` around a `pclick` to add to the selection |
| `pclick <panel> x y` | click inside a named panel, panel-relative |
| `tab <title>` | activate the dock tab with that title (`Pendulum`, `Assembly`, …) |
| `dragtab <title> x y` | drag a dock tab to a window point: a group's edge splits, its centre or tab bar stacks |
| `dclick x y`, `ddblclick x y` | click or double-click at a document point in the viewer (the select tool: a double-click enters a group or embed) |
| `click <text>` | click the shell button with that accessible name (`Link`, `Show in assembly`) |
| `hunt x0 y0 x1 y1 step <js>` | sweep the pointer over a document-space grid until `<js>` is truthy (e.g. `__iron.hovered().includes(id)`); prints where. For targets whose screen position is hard to predict, such as a shape on a plane in 3D |
| `dragfound dx dy` | drag from the last `hunt` hit by (dx, dy) document px |
| `eval <js>` | evaluate in the page and print the JSON result; an async IIFE is awaited |
| `logs [regex]` | print console output since the last `logs`, optionally filtered; Rust `info!`/`warn!` lines from the worker appear here |
| `peek [regex]` | like `logs`, without clearing the buffer |
| `errors` | print uncaught page errors |
| `panel [name]` | print a panel's rect (default the viewer) |
| `sleep ms` | wait |
| `quit` | close the browser |

Where things are in the demo scene, in document space: the draggable square
covers (500,420)–(580,500); the circle is centred at (160,330); the slider
track runs (100,600)–(400,624) and scrubbing along it previews the value
through the store; the bar at x=780 has its height and top bound to the
slider through a definition: the library (a root) holds a `bar` definition
with one parameter, `level`, and a template rect; the `model` root holds an
entity instantiated from it (its `level` bound to the slider) and the scene
holds the rect that figures it, whose y and h read the entity's params. The
template itself is never drawn. `scene` (#1) is a 2D composition; the torus
lives in a 3D composition, `assembly`, seen through `main`'s embed, near
document (875,200), beside the `card` composition (a 2D card with a dot and
a bar) shown in the assembly as a plane in perspective; the purple cartoon `box` covers (520,590)–(630,660);
the pendulum group is a loose root, drawn in the viewer. The "shadow" rect
at (600,420) has x and y bound to the square's. The timeline holds a "slide" clip (0.5–2.5 s, square x 500→700)
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
