# The demo

**Status:** Built 2026-11-13 (branch `t3/demo-refresh`).

What a first visit (`?demo`, or a new scope from the `demo` template) shows, how it is
seeded, and how its gallery pages were made. Owning code: `src/document_bridge/demo.rs`
(`demo_scene`, `showcase`), `src/document_bridge/gallery/*.xml`.

## 1. Two transactions

- **`demo_scene`** ("open demo scene", 282 ops): the scene the run-iron scripts drive.
  Main, Scene, Assembly, Card, Pendulum, Controls, Optics, Schematic, Thermal, Cylinder,
  Piston, Components, the library, the model and the timeline's two clips. Scripts load it
  alone (`?showcase=0`, drive.ts's default), and know its nodes by id (`#M` the square,
  `#U` its shadow, `#Q` the torus…). The twenty random rects it used to open with are
  gone; their twenty ids are still drawn, so every later id is unchanged.
- **`showcase`** ("open showcase"): everything a visitor sees beyond that, added on the
  app's own page unless `?showcase=0`. In order:
  1. **Theme colours.** Every constant `fill.color` and `stroke.color` in the scene becomes
     its nearest theme token (`themed`, palette.rs's built-ins; a translucent colour is the
     token mixed into `@surface`), so the scene reads in light and dark. This happens here,
     not in the scene, because a token joins the document as nodes of its own (the
     palette), which would move every id the scripts know.
  2. **Outline** and **Data**, as before (contexts.md, affordances.md demo 4).
  3. **The gallery** (§2): eight science pages, their tabs placed before every root of the
     scene, so they come straight after Main.
  4. **Circuit**, the divider `schematic.txt` drives, unchanged.
  5. **RC charging** and its **rc circuit** (§2.1).
  6. **Lesson**, the narrative.
  7. **Notes** (`gallery/notes.xml`): titles, captions and labels appended to the scene's
     compositions (Main, Scene, Pendulum, Controls, Optics, Schematic, Thermal, Piston,
     Components, Data). Appended last in each, so no scene id moves; scripts run without
     them.

Deleted 2026-11-13: **Pitch** (a notation with its own toolbar and an orange chrome; a
title and a card, nothing else) and **Dials** (controls made of `affords` facets, drawn as
a bare needle and bar). Outline keeps a notation with its own context and Data keeps the
affords-driven bars, so neither feature lost its demo. `document_showcase` now checks for
`circuit` to know the showcase is in.

## 2. The gallery

Each page is one composition written as node markup with `<place>`
(`markup::compose`), `include_str!`d from `src/document_bridge/gallery/`:

| Page | Shows |
|---|---|
| Oscillator | Damped spring: a mass drawn by bindings, a spring as a bound path, two `std:plot`s (x(t) with its envelope and a cursor; the phase portrait), a marker placed on the portrait through the plot's derived axis ports (`$phase_portrait.x_lo`) |
| Refraction | Snell's law: rays and angle arcs bound to three sliders, total internal reflection, the Fresnel reflectance as the reflected ray's opacity, θ₂ against θ₁ with a cursor |
| Interference | Young's double slit: wavefronts drawn from the sliders, the intensity on the screen (an area series), the pattern itself as a gradient band, the fringe spacing as a readout |
| Wave packet | Group and phase velocity: ψ(x, t) with its envelope; a timeline clip sweeps `t` from 0 to 8 s, so Play on the timeline runs it |
| Kepler | An orbit drawn from a and e, the planet placed by its eccentric anomaly, the swept wedge (equal areas), vis-viva speed with a cursor, readouts of r, v, M and T |
| Lorenz | The attractor integrated live by `accumulate` nodes, a Reset button (an `on` writing a `let`), the x–z projection and x against time |
| Blackbody | Planck curves at three temperatures, the visible band with Wien's peak on it, a star in the colour of T with a glow, absolute or per-peak scale from two buttons |
| Enzyme | Michaelis–Menten with a competitive inhibitor, the Lineweaver–Burk lines, a cartoon of the active site, a readout bar |
| RC charging | A capacitor charging: the circuit (an `embed` of `rc circuit`), V_C and I against t with τ marked, a table of V_C after n time constants |

### 2.1 RC charging

The circuit is built in Rust (`parts::place_ops`, `wire_ops`) in a schematic composition
of its own, `rc circuit`, because wires are relations and connections that markup does
not write. The page embeds it (`embed.source="#CIRCUIT"`, replaced with its id), and the
resistor's and capacitor's arguments are bound to the page's sliders (`built.named`). Its
parts are named without numbers (`V`, `R`, `C`, `GND`, `P`), so designators elsewhere
count as if it were not there (`schematic.txt` expects to place R3 and P2). The solve is a
DC operating point: the probe reads 5 V, the charged state; the curves show the transient.

### 2.2 Markup that reads a placed component

`markup::compose` (markup.rs) builds a page in one transaction: a `<place id="plot">`
names the place, and another node may read it before it exists: `$plot.transform2d.x` (the
`use`'s own leaf) or `$plot.x_lo` (an output port of the placed instance, an expanded
node `#entity/spec`). Leaves that read a place are set after the places are composed
(`compose::compose_els`). The places are composed after the nodes, so the siblings around a
place are then put back in markup order: a mark written after a plot paints over it. The store checks a `Set` whose binding reads an expanded node at
the end of the transaction, after instances expand (op.rs `reads_expanded`), as it already
did for a `Create`.

## 3. How the pages were made

The in-app agent wrote them (`agent-probe.ts`, `PROVIDER=claude MODEL=claude-opus-5-5`),
one run per topic, with a shared style prompt (`.agents/prompts/style.txt`: a new 2D
composition inside x 40–1000, y 30–680; a 26 px title and a 14 px muted caption; theme
tokens only; `std:plot` for charts; sliders with labels and `fmt` readouts) and three
turns: build; look at the picture, critique and fix; move every slider, check, polish.

| Run (2026-11-13) | Topic | Turns |
|---|---|---|
| 22-33-18 | oscillator | 3 of 3 |
| 22-33-53 | interference | 3 of 3 |
| 22-33-55 | kepler | 3 of 3 |
| 22-33-57 | blackbody | 3 of 3 |
| 22-33-59 | refraction | 2 of 3: the probe timed out reading the third turn's chat row; the document was rebuilt from the log (`replay-edits.ts`) |
| 22-34-01 | enzyme | 3 of 3 |
| 22-39-23 | wave packet | 1 of 3: turns 2 and 3 failed at the provider (three runs at once) |
| 22-39-25 | lorenz | 1 of 3, the same |
| 22-39-27 | rc | 1 of 3, the same |
| 2026-11-14 04-07-19 | wave packet, refined | 2 of 2: a critique of the seeded page (its marker tracks read as stray rules): the crest and envelope markers moved onto the plot, a "the packet travels this way" note, the sliders realigned |

All six complete runs came out polished; the second turn's self-critique fixed overlaps
and off-scale axes. The refinement round ran on the demo itself (the gallery already
seeded), prompted with what was wrong (`.agents/prompts/wavepacket-refine.txt`), and its
result replaced the page. Lorenz and RC charging keep their first-turn result, which
needed no fixing at the size they show. A turn's saved document (`turn-N-after.json`) becomes markup with the
`demo_markup` example (iron_document), then by hand: colours a table preset wrote as hex
became tokens, two compositions were renamed, the wave packet's clip moved to Rust.

## 4. Engine fixes made for it

- **Bindings to a placed instance's ports in the same transaction** (§2.2).
- **`<place id=…>` in node markup**, and `$place.port` / `$place.leaf` references.
- **Tables follow the theme:** the Light and Boxed presets colour cells and rules with
  tokens (`@ink`, `@ink-2`, `@muted`, `@series-2`, `@grid`, `@axis`); `table::compact`
  compares slots, so a token cell is not reported as restyled.
- **Schematic parts follow the theme:** symbols and wires `@ink`, labels `@ink-2`.
- **A simulation in a hidden tab holds still:** an `accumulate` in a composition no open
  view reaches does not step (`Store::advance_steps(now, paused)`, `liveness::reached`).
  With the Lorenz page in the background the page ran at 24 fps instead of 60.
- **A NaN position no longer panics the spatial index.** Past the critical angle the
  refraction plot's curve is NaN (the line ends there), so `std:plot`'s value at the
  cursor was too, and its readout's `y` became NaN: `BoundsIndex::upsert` asserted
  finite bounds and took the app down. The index now leaves a node with non-finite
  bounds out until they are finite again (`Scene::bounds` is `None` meanwhile), and the
  plot hides its cursor dot and value where the curve has no value
  (`a_plot_cursor_past_the_end_of_its_curve_stays_finite`).
- **`&#10;`** in a markup attribute is a newline.
- **Drivers wait for the whole document:** `ready` (drive.ts) waits for
  `__documentOpened`, set once the demo and its showcase are in.

## 5. Limits found

- `std:plot` has no dashed or reference-line series style: the enzyme page draws its V_max
  guide as scatter dots.
- Every placed `std:plot` adds a `vis` row to the timeline (its Motion port), so the
  timeline lists thirteen plots below the clips.
- Markup writes no relations: a page that needs a clip, a wire or a constraint gets it in
  Rust. `demo_markup` warns about each.
- The showcase takes about 4 s to apply in a dev build (thirteen plots expanded); the
  gallery tabs arrive after Main.
- The Lorenz page, on screen, costs the most per frame (three 400-sample histories
  rebuilt each step): about 18 fps under SwiftShader.
- The RC circuit's probe shows the DC operating point (5 V), not the charging transient.
- `schematic.txt` fails at its Space-to-turn step on master as well (checked against the
  main checkout's build, 2026-11-13); not caused by the demo.
