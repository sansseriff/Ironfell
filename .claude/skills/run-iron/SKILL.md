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

Wall-clock on the Apple Silicon dev machine, from timed runs on 2026-10-22. Per
AGENTS.md, check in with the user before running a step marked ⏱ (over a minute).

| step | typical time |
|---|---|
| `BACKENDS=webgl2 build.sh`, warm, after a source change | ~45 s (cargo ~25 s) |
| ⏱ `build.sh` (both backends), warm | ~1.5 min |
| ⏱ `build.sh` after a dependency or feature change | ~3.5 min |
| ⏱ `build.sh` cold | ~5 min |
| ⏱ full `smoke.txt` (`GPU=1`) | ~3.5 min |
| `piston.txt` | ~1 min |
| `snapping.txt` | ~30 s |
| `layout.txt` | ~35 s |
| `tables.txt` | ~45 s |
| ⏱ `raster-pixels.txt` (with the Prussia import, then the meters) | ~1.5 min |
| ⏱ `bench-instances.txt` | ~1.5 min |
| `instance-pixels.txt` | ~1 min |
| ⏱ `instance_cost` (native, release) | ~1.5 min |
| `bench-large-svg.txt` | ~1 min |
| a short ad-hoc driver script (a few commands) | 20–40 s |
| ⏱ `cargo test -p iron_document`, after a dependency change | a few minutes (not yet timed) |

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
build) for both backends. The Stats view, docked right of the timeline, names
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
`nav`/`ready` lines, and the per-frame `solve:` lines:

```
{"enabled":false,"guides":[]}
["<rect id=\"#M\" name=\"square\" x=\"620\" y=\"340\"","<rect id=\"#U\" name=\"shadow\" x=\"720\" x.bind=\"#M.transform2d.x + 100\" y=\"340\" y.bind=\"#M.transform2d.y\""]
redraw (document): 2 of 36 nodes changed [#3b #3i/3b]; damage 539,114–661,269
redraw (document): 2 of 36 nodes changed [#M #U]; damage 560,340–800,460
"<rect id=\"#12\" name=\"bar\" x=\"780\" y=\"370\" y.bind=\"640 - #11.port.signal * 300\" rot=\"0\" sx=\"1\" sy=\"1\" w=\"60\" h=\"270\" h.bind=\"#11.port.signal * 300\" color=\"#3380e6ff\" opacity=\"1\">"
"<rect id=\"#12\" name=\"bar\" x=\"730\" y=\"400\""
"value=\"0.8\"/>"
"<mesh id=\"#Q\" name=\"torus\" pos=\"0,1.5,0\" rot=\"0,5.34…,0\" scale=\"1,1,1\" asset=\"torus\"> <driven-by ref=\"#S\" rel=\"animates\" prop=\"transform3d.rot\"/>"
{"ok":true,"version":9,"aliases":{"$n":"#3t"}}
"<edit-failed applied=\"none\" version=\"9\">\n  <error op=\"0\" kind=\"unknown_node\">no node #zz</error>\n  <error op=\"1\" kind=\"cycle\">binding #N.radius.rx would depend on itself</error>\n</edit-failed>"
"<rect id=\"#3t\" name=\"from-model\" x=\"300\" y=\"120\" rot=\"0\" sx=\"1\" sy=\"1\" w=\"80\" w.bind=\"#O.slider.value * 100\" h=\"40\" color=\"#22aa55ff\" opacity=\"1\"/>"
["<ellipse id=\"#a\" name=\"ball\" x=\"440\" x.by=\"#V\" y=\"300\" y.by=\"#V\""," rot=\"-1.12…\""," h=\"240\""]
"move ellipse" by Human: cluster #V re-solved; moved 2 node(s): #Y #Z
"<ellipse id=\"#a\" name=\"ball\" x=\"323.71…\" x.by=\"#V\" y=\"359.97…\" y.by=\"#V\""
["<ellipse id=\"#a\" name=\"ball\" x=\"320\" x.by=\"#V\" y=\"360\" y.by=\"#V\"","<ellipse id=\"#Y\" name=\"j1\" x=\"320\" x.by=\"#V\" y=\"160\" y.by=\"#V\""]
#b locked for this drag: #b.transform2d.x is bound to 4 free leaves (#X.transform2d.x, #X.transform2d.y, #Y.transform2d.x, #Y.transform2d.y)
#b locked for this drag: #b.transform2d.y is bound to 4 free leaves (#X.transform2d.x, #X.transform2d.y, #Y.transform2d.x, #Y.transform2d.y)
"<applied version=\"14\">\n  <solver clusters=\"#V\" moved=\"#Y #Z #a\"/>\n  <unmet leaf=\"#a.transform2d.x\" requested=\"900\" solved=\"619.99…\">constrained by cluster #V: the constraints allow 619.99… at most</unmet>\n</applied>"
"<edit-failed applied=\"none\" version=\"14\">\n  <error op=\"0\" kind=\"unsatisfiable\">exact transaction not honoured: #a.transform2d.x requested 900 but constraints allow 619.99…</error>\n</edit-failed>"
"<constraint id=\"#i\" name=\"link1\" kind=\"distance\" value=\"120.00…\" value.bind=\"#f.slider.value * 100\" enabled=\"true\">"
true
"<ellipse id=\"#a\" name=\"ball\" x=\"320\" y=\"420\""
["<ellipse id=\"#a\" name=\"ball\" x=\"320\" y=\"420\"","<rect id=\"#W\" name=\"platform\" x=\"281.00…\" x.by=\"#V\" y=\"38.19…\" y.by=\"#V\""]
"x.by=\"#V\""
"#s"
found at … (a point on the card dot; varies with the sweep)
true
["#l","#Q"]
clicked Link
applied "link" (4 ops, Human) -> v21
"<entity id=\"#3v\" name=\"box\" def=\"#3u\" mode=\"linked\" definition=\"thing\">\n    <figure id=\"#l\" type=\"rect\" name=\"box\" in=\"#1\" composition=\"scene\"/>\n    <figure id=\"#Q\" type=\"mesh\" name=\"torus\" in=\"#P\" composition=\"assembly\"/>\n  </entity>"
"<mesh id=\"#Q\" name=\"torus\" … asset=\"torus\" of=\"#3v\">"
clicked Show in assembly
clicked Show in scene
focus #Q: panel viewer pans to centre it
focus #l: panel viewer pans to centre it
false
true
"<edit-failed applied=\"none\" version=\"23\">\n  <question kind=\"merge\" a=\"#12\" b=\"#Q\">\n    <entity id=\"#10\" name=\"bar\" definition=\"bar\" level=\"{= #O.slider.value}\" figures=\"#12\"/>\n    <entity id=\"#3x\" name=\"box\" definition=\"thing\" figures=\"#l #Q\"/>\n    Both already picture an entity. Resend <link-figures> with keep=\"#10\" or keep=\"#3x\"; the other entity is deleted and its figures move to the one kept.\n  </question>\n</edit-failed>"
[true,"<entity id=\"#10\" name=\"bar\" def=\"#w\" mode=\"detached\" level=\"0.8\" level.bind=\"#O.slider.value\" definition=\"bar\">\n    <port id=\"#11\" name=\"level\" kind=\"value\" dir=\"in\" signal=\"0.8\"/>\n    <figure id=\"#12\" type=\"rect\" name=\"bar\" in=\"#1\" composition=\"scene\"/>\n    <figure id=\"#l\" type=\"rect\" name=\"box\" in=\"#1\" composition=\"scene\"/>\n    <figure id=\"#Q\" type=\"mesh\" name=\"torus\" in=\"#P\" composition=\"assembly\"/>\n  </entity>"]
{"roundtrip_equal":true,"bytes":59348}
"<document version=\"25\"> |   <composition id=\"#1\" name=\"scene\" notation=\"free\" dims=\"2d\" run=\"auto\" count=\"26\"> |     <rect id=\"#2\" x=\"111\" y=\"231\" rot=\"0\" sx=\"1\" sy=\"1\" w=\"30\" h=\"30\" color=\"#986ecaff\" opacity=\"1\"/> |     <rect id=\"#3\" x=\"406\" y=\"241\" rot=\"0\" sx=\"1\" sy=\"1\" w=\"30\" h=\"30\" color=\"#9d399aff\" opacity=\"1\"/>"
applied "open demo scene" (298 ops, System) -> v1
applied "move rect" (2 ops, Human) -> v2
applied "move mesh" (1 ops, Human) -> v3
applied "undo: move mesh" (1 ops, System) -> v4
applied "undo: move rect" (2 ops, System) -> v5
applied "redo: move rect" (2 ops, System) -> v6
applied "set slider" (1 ops, Human) -> v7
applied "move rect" (2 ops, Human) -> v8
applied "model edit" (2 ops, Model) -> v9
applied "move ellipse" (2 ops, Human) -> v10
"move ellipse" by Human: cluster #V re-solved; moved 2 node(s): #Y #Z
applied "move ellipse" (3 ops, Human) -> v11
"move ellipse" by Human: cluster #V re-solved; moved 2 node(s): #Y #Z
applied "undo: move ellipse" (7 ops, System) -> v12
applied "undo: move ellipse" (6 ops, System) -> v13
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
applied "move ellipse" (2 ops, Human) -> v19
"move ellipse" by Human: cluster #V re-solved; moved 1 node(s): #W
applied "undo: move ellipse" (3 ops, System) -> v20
applied "link" (4 ops, Human) -> v21
"link" by Human: cluster #V re-solved; moved 1 node(s): #W
applied "undo: link" (5 ops, System) -> v22
applied "link" (4 ops, Model) -> v23
"link" by Model: cluster #V re-solved; moved 1 node(s): #W
applied "link" (5 ops, Model) -> v24
applied "undo: link" (5 ops, System) -> v25
loaded document v25 (290 live nodes)
(no page errors)
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
["#3t"]
"<point id=\"#3x\" x=\"130\" y=\"220\""
"pen-add"
[4,"<point id=\"#3y\" x=\"86.9\" y=\"-22.4\" ix=\"-47.2\" iy=\"7.5\" ox=\"47.6\" oy=\"-7.5\"/>"]
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
["power=\"10 mW\"","signal=\"7.94… mW\"",["<connection a=\"laser.out\" b=\"fiber.in\" kind=\"optical_power\"/>","<connection a=\"fiber.out\" b=\"meter.in\" kind=\"optical_power\"/>","<connection a=\"heater.out\" b=\"thermal link.a\" kind=\"heat_flow\"/>","<connection a=\"thermal link.b\" b=\"ambient.p\" kind=\"heat_flow\"/>","<connection a=\"source.out\" b=\"fiber.in\" kind=\"optical_power\"/>","<connection a=\"fiber.out\" b=\"meter.in\" kind=\"optical_power\"/>"]]
[75.9,28.5]
["power=\"15.78… mW\"","signal=\"4.99… mW\"",["<connection a=\"laser.out\" b=\"fiber.in\" kind=\"optical_power\"/>","<connection a=\"fiber.out\" b=\"meter.in\" kind=\"optical_power\"/>","<connection a=\"heater.out\" b=\"thermal link.a\" kind=\"heat_flow\"/>","<connection a=\"thermal link.b\" b=\"ambient.p\" kind=\"heat_flow\"/>","<connection a=\"source.out\" b=\"fiber.in\" kind=\"optical_power\"/>","<connection a=\"fiber.out\" b=\"meter.in\" kind=\"optical_power\"/>"]]
"<edit-failed applied=\"none\" version=\"27\">\n  <error op=\"2\" kind=\"bad_connection\">#…: cannot connect rf_voltage (#…) to optical_power (#…); a connection joins ports of one kind</error>\n</edit-failed>"
tab Thermal
undefined
["out 303.15 K -5 W","a 303.15 K 5 W","b 293.15 K -5 W","p 293.15 K 5 W"]
["out 313.15 K -10 W","a 313.15 K 10 W","b 293.15 K -10 W","p 293.15 K 10 W"]
tab Controls
undefined
clicked .toolbar button[aria-label="Rectangle"]
"Rectangle (R)"
["<rect id=\"#3z\" name=\"rectangle\" x=\"500\" y=\"100\" rot=\"0\" sx=\"1\" sy=\"1\" w=\"140\" h=\"80\"","Move (V)"]
"<ellipse id=\"#40\" name=\"ellipse\" x=\"740\" y=\"140\" rot=\"0\" sx=\"1\" sy=\"1\" rx=\"40\" ry=\"40\""
"<ellipse id=\"#40\" name=\"ellipse\" x=\"770\" y=\"140\" rot=\"0\" sx=\"1\" sy=\"1\" rx=\"70\" ry=\"40\""
4
"<ellipse id=\"#40\" name=\"ellipse\" x=\"770\" y=\"140\" rot=\"0\" sx=\"1\" sy=\"1\" rx=\"70\" ry=\"40\""
tab Main
{"cuts":["its h is bound to #11.port.signal * 300; the path keeps the current value"],"ok":false}
["its h is bound to #11.port.signal * 300; the path keeps the current value"]
clicked Cancel
null
tab Controls
"<rect id=\"#3z\" name=\"rectangle\" x=\"500\" y=\"100\" rot=\"0\" sx=\"1\" sy=\"1\" w=\"140\" h=\"80\" tl=\"30\" tr=\"30\" br=\"30\" bl=\"30\""
clicked Add effect
clicked [data-effect-type="shadow"]
" shadow.color=\"#00000040\" dx=\"0\" dy=\"4\" blur=\"4\""
"<ellipse id=\"#40\" name=\"ellipse\" x=\"770\" y=\"140\" rot=\"0\" sx=\"1\" sy=\"1\" rx=\"70\" ry=\"40\" start=\"0\" sweep=\"1.57…\" inner=\"0\""
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
"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"741\" height=\"527\" viewBox=\"99 98 741 527\" data-iron=\"1\">\n  <defs>\n    <filter id=\"shadow-1\" filterUnits=\"userSpaceOnUse\" x=\"494\" y=\"98\" width=\"152\" height=\"92\" color-interpolation-filters=\"sRGB\">\n      <feDropShadow dx=\"0\" dy=\"4\" stdDeviation=\"2\" flood-color=\"#000000\" flood-opacity=\"0.25\"/>\n    </filter>\n  </defs>\n  <g id=\"level\" data-iron-type=\"slider\" data-iron-name=\"level\" transform=\"translate(100 600)\">\n    <rect y=\"9\" width=\"300\" height=\"6\" rx=\"3\" fill=\"#bfc2cc\"/>\n    <rect y=\"9\" width=\"120\" height=\"6\" rx=\"3\" fill=\"#3380e6\"/>\n    <circle cx=\"120\" cy=\"12\" r=\"12\" fill=\"#ffffff\" stroke=\"#4d4d59\" stroke-width=\"1.5\"/>\n  </g>\n  <path id=\"path\" data-iron-type=\"path\" data-iron-name=\"path\" transform=\"translate(200 200)\" d=\"M0 0 C0 0 160 -60 220 0 C280 60 130 220 130 220\" fill=\"none\" stroke=\"#000000\" stroke-width=\"2\" stroke-linejoin=\"round\" stroke-linecap=\"round\"/>\n  <g filter=\"url(#shadow-1)\">\n    <rect id=\"rectangle\" data-iron-type=\"rect\" data-iron-name=\"rectangle\" transform=\"translate(500 100)\" width=\"140\" height=\"80\" rx=\"30\" fill=\"#d9d9d9\" data-iron-shadow=\"#000000 0.25 0 4 4\"/>\n  </g>\n  <path id=\"ellipse\" data-iron-type=\"ellipse\" data-iron-name=\"ellipse\" transform=\"translate(770 140)\" d=\"M0 0 L70 0 C70 22.0914 38.6599 40 0 40 Z\" data-iron-rx=\"70\" data-iron-ry=\"40\" data-iron-arc=\"0 1.57… 0\" fill=\"#d9d9d9\"/>\n</svg>\n"
["slider level is exported as a picture of itself; it imports as shapes"]
4
{"anchors":2,"cuts":["filters are not imported"],"definitions":0,"nodes":4,"ok":true,"references":0}
[8,4]
["<rect id=\"#4M\" x=\"10\" y=\"10\" rot=\"0\" sx=\"1\" sy=\"1\" w=\"100\" h=\"60\" tl=\"8\" tr=\"8\" br=\"8\" bl=\"8\"","<ellipse id=\"#4N\" x=\"150\" y=\"40\" rot=\"0\" sx=\"1\" sy=\"1\" rx=\"30\" ry=\"30\"","<path id=\"#4O\" x=\"0\" y=\"0\" rot=\"0\" sx=\"1\" sy=\"1\" fill_rule=\"nonzero\" d=\"1 contour, 2 anchors\"","<rect id=\"#4P\" x=\"140\" y=\"80\" rot=\"0\" sx=\"1\" sy=\"1\" w=\"40\" h=\"40\""]
" d=\"1 contour, 2 anchors\""
[true,4]
4
undefined
8
4
{"ok":true,"version":60,"summary":"<applied version=\"60\">\n  <imported roots=\"#4U #4V #4W #4X\"/>\n  <cut>filters are not imported</cut>\n</applied>"}
8
4
tab Components
undefined
["fiber detached","meter detached","source linked","fiber linked","meter linked","spare laser linked","short fiber linked","long fiber linked"]
undefined
["#…/3V","7.94… mW"]
{"derivedNodesSaved":0,"connectionsToPorts":4}
[0,5]
["#…/3V","15.88… mW"]
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
[["power value in","out optical_power out"],"#….slider.value * 0.00… W",["fiber detached","meter detached","meter linked","spare laser linked","short fiber linked","long fiber linked"]]
["#…/3V","15.88… mW"]
undefined
[2,[[240,175],[545,175]]]
[2,[[240,175],[665,435]]]
["fiber detached","meter detached","source linked","fiber linked","meter linked","spare laser linked","short fiber linked","long fiber linked"]
"kind=\"out_of_scope\""
undefined
undefined
[[545,175],[240,475],["meter 10 mW","short fiber 0 W"]]
[[340,475],[340,475]]
[[545,175],[545,175],["meter 15.88… mW","short fiber 0 W"]]
[[240,475],["meter 15.88… mW","short fiber 10 mW"]]
["meter 15.88… mW","short fiber 0 W"]
clicked button[title^="Make a component"]
"component use #4y"
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
[[600,300],[600,300],["meter 15.88… mW","short fiber 0 W"]]
tab Library
["slider level 18","laser power 18 mW"]
tab Controls
"controls"
["Render inspector watching controls","Chunks","1 0 in view","Nodes","25"]
applied "open demo scene" (298 ops, System) -> v1
applied "resize rect" (2 ops, Human) -> v2
applied "rotate rect" (3 ops, Human) -> v3
applied "undo: rotate rect" (3 ops, System) -> v4
applied "undo: resize rect" (2 ops, System) -> v5
applied "nudge rect" (1 ops, Human) -> v6
applied "nudge rect" (1 ops, Human) -> v7
applied "delete group" (1 ops, Human) -> v8
applied "undo: delete group" (16 ops, System) -> v9
applied "draw path" (3 ops, Human) -> v10
applied "add point" (1 ops, Human) -> v11
applied "add point" (1 ops, Human) -> v12
applied "close path" (1 ops, Human) -> v13
applied "undo: close path" (1 ops, System) -> v14
applied "move point" (2 ops, Human) -> v15
applied "add point" (5 ops, Human) -> v16
applied "undo: add point" (5 ops, System) -> v17
applied "bend segment" (4 ops, Human) -> v18
applied "undo: bend segment" (4 ops, System) -> v19
applied "delete point" (1 ops, Human) -> v20
applied "undo: delete point" (1 ops, System) -> v21
applied "delete point" (1 ops, Human) -> v22
applied "undo: delete point" (1 ops, System) -> v23
applied "set size.w" (1 ops, Human) -> v24
applied "set transform2d.x" (1 ops, Human) -> v25
applied "set slider" (1 ops, Human) -> v26
applied "resize rect" (1 ops, Human) -> v27
applied "set slider" (1 ops, Human) -> v28
applied "draw rectangle" (1 ops, Human) -> v29
applied "draw ellipse" (1 ops, Human) -> v30
applied "resize ellipse" (2 ops, Human) -> v31
applied "convert ellipse to path" (7 ops, Human) -> v32
applied "undo: convert ellipse to path" (7 ops, System) -> v33
applied "round corners" (1 ops, Human) -> v34
applied "add shadow" (1 ops, Human) -> v35
applied "edit arc" (1 ops, Human) -> v36
applied "duplicate 2 nodes" (7 ops, Human) -> v37
applied "paste 2 nodes" (7 ops, Human) -> v38
applied "cut 2 nodes" (2 ops, Human) -> v39
applied "paste 2 nodes" (4 ops, Human) -> v40
applied "undo: paste 2 nodes" (4 ops, System) -> v41
applied "undo: cut 2 nodes" (2 ops, System) -> v42
applied "undo: paste 2 nodes" (7 ops, System) -> v43
applied "undo: duplicate 2 nodes" (7 ops, System) -> v44
applied "model edit" (1 ops, Model) -> v45
applied "model edit" (1 ops, Model) -> v46
applied "move rect" (2 ops, Human) -> v47
applied "undo: move rect" (2 ops, System) -> v48
applied "undo: model edit" (1 ops, System) -> v49
applied "undo: model edit" (1 ops, System) -> v50
applied "duplicate rect" (5 ops, Human) -> v51
applied "duplicate rect" (2 ops, Human) -> v52
applied "undo: duplicate rect" (7 ops, System) -> v53
applied "move mesh" (1 ops, Human) -> v54
applied "undo: move mesh" (1 ops, System) -> v55
applied "import SVG" (4 ops, Human) -> v56
applied "undo: import SVG" (4 ops, System) -> v57
applied "import SVG" (4 ops, Human) -> v58
applied "undo: import SVG" (4 ops, System) -> v59
applied "import SVG" (4 ops, Model) -> v60
applied "undo: import SVG" (4 ops, System) -> v61
applied "set slider" (1 ops, Human) -> v62
applied "detach instance" (6 ops, Human) -> v63
applied "undo: detach instance" (6 ops, System) -> v64
applied "model edit" (1 ops, Model) -> v65
applied "undo: model edit" (1 ops, System) -> v66
applied "model edit" (2 ops, Model) -> v67
applied "undo: model edit" (2 ops, System) -> v68
applied "model edit" (2 ops, Model) -> v69
applied "model edit" (1 ops, Model) -> v70
applied "undo: model edit" (1 ops, System) -> v71
applied "undo: model edit" (2 ops, System) -> v72
applied "make component component" (24 ops, Human) -> v73
applied "model edit" (2 ops, Model) -> v74
applied "undo: model edit" (2 ops, System) -> v75
applied "undo: make component component" (24 ops, System) -> v76
applied "disconnect in" (3 ops, Human) -> v77
applied "disconnect in" (2 ops, Human) -> v78
applied "connect in to out" (2 ops, Human) -> v79
applied "connect in to out" (3 ops, Human) -> v80
applied "move use" (2 ops, Human) -> v81
applied "undo: move use" (4 ops, System) -> v82
applied "undo: connect in to out" (10 ops, System) -> v83
applied "connect in to out" (3 ops, Human) -> v84
applied "undo: connect in to out" (3 ops, System) -> v85
applied "draw ellipse" (1 ops, Human) -> v86
applied "draw rectangle" (1 ops, Human) -> v87
applied "make component component" (9 ops, Human) -> v88
applied "add port" (2 ops, Human) -> v89
applied "wrap in empty" (4 ops, Human) -> v90
applied "bind transform2d.rot" (1 ops, Human) -> v91
applied "duplicate use" (3 ops, Human) -> v92
applied "disconnect in" (3 ops, Human) -> v93
applied "disconnect in" (2 ops, Human) -> v94
applied "disconnect in" (2 ops, Human) -> v95
applied "undo: disconnect in" (7 ops, System) -> v96
applied "disconnect out" (3 ops, Human) -> v97
applied "disconnect out" (2 ops, Human) -> v98
applied "disconnect out" (2 ops, Human) -> v99
applied "undo: disconnect out" (7 ops, System) -> v100
applied "move use" (2 ops, Human) -> v101
applied "undo: move use" (2 ops, System) -> v102
applied "add slider" (8 ops, Human) -> v103
applied "add slider" (3 ops, Human) -> v104
applied "add laser" (7 ops, Human) -> v105
applied "add laser" (3 ops, Human) -> v106
applied "connect value to power" (3 ops, Human) -> v107
applied "set slider" (1 ops, Human) -> v108
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
`redraw (<reason>): N of M nodes changed [ids]; damage x0,y0–x1,y1` line:
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
| `nav [url]` | open the app; default `http://localhost:5173/Ironfell/?gfx=webgl2` |
| `ready` | wait until the `.loading` overlay leaves the DOM and the frame loop is cycling (cadence probe frame count advancing between polls), then read the viewer panel rect. About 6 s under SwiftShader |
| `shot <name>` | screenshot to `screenshots/<name>.png` |
| `move x y`, `down [button]`, `up [button]`, `drag x0 y0 x1 y1 [steps]` | pointer in window pixels; `down right` … `up right` around `move`s orbits a 3D panel's camera |
| `dmove x y`, `ddrag x0 y0 x1 y1 [steps]` | the same in document 2D space: viewer-panel top-left origin, y-down, the space `transform2d.x/y` are in |
| `pdrag <panel> x0 y0 x1 y1 [steps]` | drag relative to a named DOM panel (`viewer`, `assembly`, `timeline`); a drag along the timeline scrubs the playhead over its 30 s |
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
at (600,420) has x and y bound to the square's. The timeline holds a "slide" clip (0.5–2.5 s, square x 500→700)
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
cd .claude/skills/run-iron && SIZE=1680x1000@1 bun run drive.ts < piston.txt; cd -
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

130 tests pass (35 unit, 95 integration; verified 2026-10-17 after vision 10
phase 4 added `tests/signals.rs`, minus the two platform-precision tests noted
above on Linux). The host target is required because
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

The default inspector is now 280 px wide. Run the full `smoke.txt` at
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
remove everything, the last one undoing the creation and the typing as one entry.
Expected, minus screenshot and `WARN` lines:

```
[true,""]
"Pressure"
["<text id=\"#33\" name=\"Text\" x=\"260\" y=\"80\" rot=\"0\" sx=\"1\" sy=\"1\" w=\"100\" h=\"100\" content=\"Pressure\""]
clicked button[aria-label="Font family"]
15
clicked button[title="Playfair Display"]
clicked button[aria-label="Bind content"]
1
[["content=\"level = 0.200\"","family=\"Playfair Display\""]]
0
applied "open demo scene" (233 ops, System) -> v1
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

## Free running and the Stats view (2026-10-18)

`plans/specs/free-running.md`. The Stats view right of the timeline is the
session's `stats` composition: four Vello text lines the `stats` producer
publishes (`FPS … avg … ema …`, `2D vello_gpu (sparse strips)`, the build,
the canvas size). It is a live source, so while it shows, the window renders
every frame. Each composition view has a settings button (top right,
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
