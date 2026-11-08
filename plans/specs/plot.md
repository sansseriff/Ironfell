# Plot: series values, bound paths, sized instances and `std:plot`

**Proposed 2026-11-08.** Waits on the user. State, appear and disappear are in the
companion spec [`machines.md`](machines.md); this one ends where the plot first needs
them (§8).

## 0. Why, and decisions at a glance

The agent log of 2026-11-07 (`.agents/agent-logs/2026-11-07.jsonl`, the Young's double
slit turn) spent 412 s and 49.7k output tokens on one edit. 61% of that edit's text was
the same op repeated with one number changed: 61 rects for a brightness strip, 61 bound
`point`s for the intensity curve, 31 for the envelope, ten wavelet ellipses, seven
stacked glows standing in for a colour function. A plot primitive removes most of it;
the engine pieces it needs (series values, a path drawn from an expression, an instance
that knows its size) also serve hand-built components and the agent generally.

| # | Decision |
|---|---|
| D1 | A **series** is a new value: numbers with one unit, `Value::Series(Arc<[f64]>, Dim)`. Not a general list. |
| D2 | **Floor functions broadcast** over series, as NumPy's do: `pow(cos(pi * #s.xs / d), 2)` is a series. A function plot is a `ys` bound to an expression over `xs`. **No lambdas**, so dependencies stay static and nothing new can hide a reference. |
| D3 | **`path.d` may be bound** to an expression that returns **SVG path data**, a string (`'M ' + points(xs, ys)`), which the engine parses after each change. Agents already write SVG fluently; helpers only write path-data text. A bound path is a third path form beside packed and expanded: drawn from its resolved value, never point-edited. Draw-on is SVG's own `pathLength` plus dash trick (`stroke.path_length`). |
| D3b | **Functions are values**: `x => body`, JavaScript's arrow. A lambda's references are ordinary dependencies of the binding that holds it; its parameter is not. A plot given `f` samples it at its own resolution (one sample per 2 px), so resizing or rescaling an axis resamples. (Decided 2026-11-08.) |
| D4 | **An instance can have a size.** A template may carry `size` (its default box); its parts bind to the template's `size.w/h`, which expansion maps to the `use`'s own `size`. Dragging a corner of such a use writes the use's `size`, not its parts. Text never stretches, because only positions bind to the size. |
| D5 | **`std:plot` is a standard component**, not a node type (registry.rs: "domain things such as a bar chart … are definitions"). It is built from primitives in `standard.rs`, like `std:meter`. |
| D6 | Phase 1 of the plot has a **fixed number of label slots** per axis (hidden past the tick count); tick marks and grid lines are one bound path each. Variable structure (a repeat node) is a later, separate primitive (§9). |
| D7 | The plot's **ports are `xs` and `ys`**, not `x` and `y`, which `<place>` already uses for position. |

## 1. Series values

### 1.1 The value

`Value::Series { data: Arc<[f64]>, dim: Dim }`: numbers in SI base units, one dimension
for all of them (a series of wavelengths, of watts, or plain numbers). `Arc` keeps
`Value` at 48 bytes (packed-paths.md §3) and makes clones free; equality has an
`Arc::ptr_eq` fast path, then compares elements. `ValueKind::Series` joins `ALL`.

Not a general list (`Arc<[Value]>`): mixed lists cost every consumer a case, and the
plot, the agent's repeated geometry and tables-as-data all need numbers with a unit.
Colours as a series (a colour map, a density strip) are a later question (§10).

### 1.2 Writing one

| Where | Form |
|---|---|
| An attribute constant (edit language, inspector) | space-separated numbers with an optional unit at the end, as `stroke.dash` already reads: `ys="0 0.4 0.9 1.6 2.5"`, `xs="400 450 500 nm"` |
| An expression | `series(0, 0.4, 0.9)`; `range(a, b, n)` (n evenly spaced, both ends included); `step(a, b, d)` (a, a+d, … ≤ b) |
| A file (`serial.rs`) | `{"ser": [0, 0.4, 0.9], "u": "nm"}`, a tagged object; a bare JSON array already means Vec2/Vec3 (value.rs:231) |
| The agent's view | in full up to 16 elements; beyond, a summary: `series(121 · nm, 400…700)`, as packed geometry prints `N contours, M anchors`. `<look values="#e.args.ys"/>` prints one in full |

`[a, b]` stays a vector in expressions (expr.rs:976, and its tests). Adding a variant is
additive, so whether this bumps `SCHEMA` (9) is the serial policy's call (§10 Q1).

### 1.3 Operations

- **Broadcasting.** Arithmetic and comparisons between a series and a number apply per
  element; between two series of equal length, elementwise; unequal lengths are an
  error that names both lengths. Every numeric floor function (`sin` … `hypot`, `abs`,
  `clamp`, `lerp`, `scale`, `pow`) broadcasts over any series argument. Units follow
  `quantity_op` per element, so `xs * 1 mm` and `ys / 1 mW` work and `nm + mW` is still
  an error.
- **New functions** (each also documented for the agent, §7):

| Function | Result |
|---|---|
| `series(a, b, …)`, `range(a, b, n)`, `step(a, b, d)` | a series |
| `len(s)`, `at(s, i)` (0-based; clamps), `first(s)`, `last(s)` | a number |
| `min(s)`, `max(s)`, `sum(s)`, `mean(s)` | one-argument forms reduce; the two-argument `min(a, b)` is unchanged |
| `interp(xs, ys, x)` | y at x, linear between samples (the "dot on the curve") |
| `ticks(lo, hi, n)` | about n "nice" values covering [lo, hi] (1, 2, 5 × 10ᵏ), the rule `timeline_view.rs` uses for its ruler, moved to `expr.rs` so both share it |
| `nice_lo(lo, hi, n)`, `nice_hi(lo, hi, n)` | the first and last of those ticks, for auto ranges |
| `log10`, `ln`, `exp`, `sinc` | scalars and broadcast (`sinc(0) = 1`) |
| `where(cond, a, b)` | elementwise `cond ? a : b` |
| `rgb(r, g, b[, a])`, `hsl(h, s, l[, a])`, `mix(c1, c2, t)`, `spectrum(nm)` | colours; `spectrum` is visible light's hue for a wavelength (the seven-glow workaround) |
| `x => body`, `(x, y) => body` | a function value (§1.4) |
| `map(s, f)`, `sample(f, a, b, n)` | `f` applied to each element; `f` at n points from a to b, as a series |

`fmt` takes a number as now; `fmt` of a series prints it comma-separated (for a readout
or a table cell), elided past 16.

### 1.4 Functions (lambdas)

`x => pow(cos(pi * x / #F.port.signal), 2)` is a function value. It fits the reactive
graph unchanged:

- **Dependencies stay static.** `deps()` walks the body, so `#F.port.signal` is a
  dependency of the binding holding the lambda; `x` is a local, bound only while the
  function is applied, and is no dependency.
- **Evaluating a lambda closes it.** The result, `Value::Fn`, holds the body with every
  reference replaced by its current value. When `#F` changes, the binding re-runs and
  yields a new, unequal function, so whatever samples it re-runs too.
- **Nothing new is saved.** A function exists only as a resolved value; the document
  stores the binding's text. A constant function (from a file or an edit) is refused.
- **Pure and bounded.** No recursion (a lambda has no name to call), no side effects,
  sample counts capped (100,000).

Not injective in the strict sense (`x => 2 * x` and `x => x + x` draw one curve), but
what matters holds: every pixel has one source, a node whose binding reads one stored
line of text, and the picture is a pure function of the document. A curve drawn by a
function cannot be dragged into a new shape (a nonlinear binding locks the drag, as
today); its parameters can.

### 1.5 What it touches

`value.rs` (variant, kind, JSON tag, Display summary, equality), `expr.rs` (floor
dispatch: a `broadcast` helper wraps the scalar arms; the new functions), `schema.ts`
`Kind` and `coerce` (the `any` arm must stop turning `"1 2 3"` into anything but a
series), `text::display`, the inspector's value editor (a series shows its summary and
opens a small table). `document_bridge/chunks.rs:470` hashes `format!("{v:?}")` for its
share keys; with summaries that collides, so `Value` gets a real content hash first.

## 2. Paths drawn from an expression

### 2.1 SVG path data

A bound `path.d` evaluates to a string of **SVG path data** (`M 0 0 L 10 5 …`, every SVG
command, absolute and relative), parsed after each change. A constant `path.d` written
as path data in an edit is parsed the same way, so
`<create type="path" d="M 0 0 C 20 0 20 40 40 40"/>` works. Helpers only write
path-data text, so the engine reads one format:

| Function | Writes |
|---|---|
| `points(xs, ys)` | `x,y x,y …`, SVG's `points` list: `'M ' + points(xs, ys)` is a polyline |
| `smooth(xs, ys)` | `M`, then `C` segments through the points (Catmull-Rom) |
| `circles(xs, ys, r)` | one closed circle per point (two `A` arcs each): a scatter plot as one path |
| `segments(x0s, y0s, x1s, y1s)` | `M x0 y0 L x1 y1` per index (any argument may be a number): ticks, grid lines, error bars |
| `bars(xs, ys, base, w)` | one closed rectangle per point |
| `area(xs, ys, base)` | the polyline closed down to `y = base` |

Non-finite samples break the line (a new `M`), as plotting libraries do with NaN.
Numbers print with at most 4 decimals, so a 2,000-point curve is about 30 kB of text,
parsed in well under a millisecond.

**Draw-on** is SVG's own idiom: `stroke.path_length="1"` (SVG's `pathLength`, new)
with `stroke.dash="1 1"` and `stroke.dash_offset="{= 1 - t}"`.

### 2.2 Lifting "constant only"

`path.d` is refused as a binding today (facet.rs:791, op.rs:542; packed-paths.md §3
"Constant only … no expression produces geometry"). That reason goes away, so the
refusal does too, with a **third path form**:

| Form | Holds | Drawn from | Point editing | Resize |
|---|---|---|---|---|
| packed | constant `path.d` | the constant | expands first | moves anchors |
| expanded | `contour`/`point` children | the children | yes | moves points |
| **bound** | `path.d` bound | the resolved value | **no**: the chrome says "drawn by an expression" and the inspector shows the binding | by its transform, or by what it binds to (a sized instance, §3) |

The places that decide the form by `p.d.constant()` change to the three-way test:
`contour::packed` (contour.rs:48), `check_path_form` (op.rs:417: contour children beside
a bound `d` are refused), `select.rs:1957` (not point-editable) and `:1168` (not
"packed" for resize). Gestures never preview `path.d` over a binding
(manipulate.rs:519). Rendering already follows the resolved value (`contour::geometry`)
and `geom::effect_of` already maps `path.d` to a `Local` change. A recomputed curve is a
new `Arc<Geometry>`, so its outline cache rebuilds once per change: at 200–2000 anchors
that is well under a millisecond, against one graph node and one `Local` revision per
bound `point` today.

Instance cache (instance-cache.md): a template `path.d` bound to an argument is keyed
when the argument is constant and live when wired (analysis.rs:356), which is what it
should be.

## 3. Instances with a size

### 3.1 The model

- A `template` may carry a `size` facet: the picture's **default box** (the plot's is
  360 × 240). A template without one behaves as today.
- `use` gains an optional `size` facet. `<place>` seeds it from the template's size (or
  from `w=`/`h=`), so every sized picture stores its own box; two pictures of one plot
  can be different sizes, which is right, since size is a picture's, not the entity's.
- Template parts bind to the **template's own** `size.w`/`size.h`
  (`{= #T.size.w - 48}`). Expansion's `follow` (expand.rs:222) gains one arm: a
  reference to the template node itself relocates to the use. The scope rule (§8 of
  component-model.md) already allows a definition's bindings to read the definition's
  own nodes.
- Analysis: a part reading the size is keyed on the use's size (two uses of one size
  share a drawing).

### 3.2 Dragging a corner

The select tool treats a use whose template has `size` as **one sized box**, like a rect:
its `local_box` is `(0, 0, size.w, size.h)`, and grips write the use's `size.w/h` (and
`transform2d.x/y` for the top and left grips), not its parts (select.rs:1102 and
`Part::apply` :1306 gain a `SizedUse` kind). This also sidesteps today's rule that an
anchor `empty` makes a whole instance unresizable (select.rs:1184), since parts are not
visited. Shift keeps the aspect ratio, Alt resizes about the centre, as for rects.

**Reflow, not stretch.** Each part decides how it follows: the plot area and axes bind
to `w/h`, tick positions and the curve rescale through `scale`, text keeps its font size
and moves. A template may give `min` sizes (`size.min_w`, `size.min_h`, new optional
fields) that the grips clamp to; the plot's are 160 × 100.

Layout groups inside a template work today (layout.rs scans expanded groups), so a part
may also be a fixed-size layout group whose `size` binds to the template's.

### 3.3 Not in scope

Scaling a picture uniformly (`transform2d.sx/sy` on the use, text and strokes scaling
too) stays possible for unsized templates and is not what a plot wants.

## 4. `std:plot`

### 4.1 Ports

| Port | Kind | Default | Meaning |
|---|---|---|---|
| `xs` | series | `range(0, 10, 11)` | x samples |
| `ys` | series | a parabola over the default `xs` | the first series |
| `ys2`, `ys3` | series | empty | further series on the same `xs`; empty hides them |
| `style` | enum `line \| smooth \| scatter \| area \| bars \| line+markers` | `line` | how series are drawn |
| `x_min`, `x_max`, `y_min`, `y_max` | number or quantity | auto | the axes' range; auto is `nice_lo/nice_hi` of the data (§10 Q3 on how "auto" is spelt) |
| `x_scale`, `y_scale` | enum `linear \| log` | `linear` | |
| `x_label`, `y_label`, `title` | text | from the series' unit (`λ (nm)`), empty | |
| `cursor` | number, NaN = none | none | a vertical marker at x with a dot at `interp(xs, ys, cursor)` and its value |
| `grid` | bool | true | |
| `color`, `color2`, `color3` | colour | the palette (§6) | |
| `f` | function | none | instead of `ys`: sampled at one point per 2 px over the x range (§1.4) |
| `reveal` | number 0..1 | 1 | the draw-on fraction (sets the series' dash offset), driven by the machine (§8) |
| out `y_at_cursor` | as `ys` | | what the dot reads, for other components to use |

`xs` and `ys` are in ports, so they can be **wired**: a model component's output series
feeds a plot through an ordinary connection, and the plot redraws live.

### 4.2 Parts (all one template, `notation="free"`)

1. Frame rect (`w × h`, corner radius, fill, stroke): bound to the size.
2. Plot area group: inset by margins (left 48 for labels, bottom 32, top 24 when titled,
   right 12); `group_clip` to the area so curves never spill.
3. Grid: one bound path, `segments` at `ticks(…)` for both axes, faint.
4. Axes: one bound path (two lines).
5. Tick marks: one bound path, `segments` at the ticks.
6. Tick labels: 8 text slots per axis. Slot `i` is placed at `at(ticks, i)` and reads
   `fmt(at(ticks, i))`; past `len(ticks)` its opacity is 0. `ticks` asks for at most 8,
   and fewer when the axis is short (`n = clamp(floor(axis_px / 64), 2, 8)`), so labels
   thin out as the plot is resized smaller.
7. Series 1–3: one bound path each, the drawing chosen by `style` (`points`, `smooth`,
   `circles`, `area`, `bars`); the stroke is a constant width, so resizing never fattens
   lines.
8. Cursor: a line and a dot, hidden when `cursor` is NaN; the dot's readout text.
9. Axis titles and the title: text bound to their ports and placed by the size.

About 30 nodes, against the hundreds the agent wrote by hand, and constant in the
sample count: a plot of 10,000 samples is still one path per series.

### 4.3 Interaction (what the engine gives for free, and what it needs)

| Ability | How | Phase |
|---|---|---|
| Corner resize, reflow | §3 | 2 |
| Drag the cursor along the curve | the cursor line has `affords.drag="track"`, `drives="…cursor"`, its `track` mapping pointer x to data x (the slider mechanism, select.rs:626); the dot follows by `interp` | 3 |
| Live from sliders and the model | `ys` bound or wired; the curve recomputes | 3 |
| Animate over time | bind `ys` to the clock's `t`, or key a port on the timeline | 3 |
| Appear and disappear (draw-on, fade) | the `vis` machine, machines.md | 4 |
| Hover readout | §8.1 interaction state plus a view `pointer`, not built (affordances.md §8.1) | later |
| Pan and zoom the axes by dragging them | `track` on `x_min/x_max` | later |
| Drag a data point | only for constant `ys`: a track on `at(ys, i)` writes back an element; needs element writes | later |
| SVG export | bound paths export their resolved geometry already | 3 |

### 4.4 For the agent

```xml
<edit><place def="std:plot" in="#main" x="600" y="800" w="420" h="260" name="Screen I(y)"
  xs="{= range(-15, 15, 241)}"
  ys="{= pow(cos(pi * $self.xs / #F.port.signal), 2) * pow(sinc(pi * $self.xs / #E.port.signal), 2)}"
  x_label="y (mm)" y_label="I / I₀" style="area"/></edit>
```

That replaces the 61 rects and the 92 bound points of the 2026-11-07 turn. (`$self.xs`
is how an argument reads another argument of the entity being placed; §10 Q4.)

## 5. Units on axes

A series' dimension names its axis: `x_label` defaults to the quantity's name and unit
(`λ (nm)`); tick labels are numbers in that unit (`fmt` with the unit dropped, since the
title carries it). A log axis refuses a range that reaches 0 or below, with a message.

## 6. Look

Defaults match the app's light and dark themes through a palette: three series colours
that read on both (the dataviz validator's rule: contrast against the frame, distinct to
colour-blind viewers), a frame one step off the canvas, grid at low contrast. Phase 3
holds them as the plot's default arguments. When contexts gain a palette domain
(contexts.md), the defaults move there, and a lesson's or a document's theme restyles
every plot at once.

## 7. Documenting it for the agent

The prompt's EXPRESSIONS list is hand-written (`src-ui/model/manifest.ts:136`) and
already lags the floor (`fmt`, `sci`, `str` are missing; that gap made the model believe
text can't show a live number). The floor gains a table of names, signatures and one-line
descriptions that the manifest prints, so a new function is documented by being added.
`std:plot`'s ports print in the standard component list as the others do.

## 8. Where state machines come in

The plot's `reveal` and `fade` are arguments in phases 1–3, so a slider, a clip or a
binding can drive them. Phase 4 gives the definition a `vis` machine (`hidden`, `shown`;
`appear`, `disappear`) whose channels drive them, specified in
[`machines.md`](machines.md).

## 9. Later: a repeat node

The label slots (D6) and anything with a data-dependent count of *nodes* (one text per
tick, one card per row) want a primitive that stamps a child per element of a series,
with `i` and `item` readable in its bindings and derived ids `#rep/i/…` like expanded
instances. It is the general answer to the agent's repeated ops, but it makes
structure depend on resolved values, which the graph does not do today (only
`timeline_view.rs` generates structure, from code). It gets its own spec once the plot
shows how often slots fall short.

## 10. Open questions

1. Bump `SCHEMA` for the series value, or treat a new tag as additive?
2. Broadcast `==`: elementwise (NumPy) or whole-value equality (today's `Value` `==`)?
   Proposed: whole-value, with `where` for elementwise tests.
3. How "auto" is written for a range port: NaN as "auto", a separate `x_auto` bool, or an
   empty series. Proposed: NaN, shown as "auto" in the inspector.
4. How an argument's binding reads a sibling argument (`ys` from `xs`): today
   `#entity.args.xs`, which needs the entity's id, unknown inside one `<place>`. Proposed:
   `$self.xs` in `<place>`, rewritten to the new entity's id.
5. Colour series (multi-stop gradients, density strips, heat maps): a `ColorSeries`, or
   gradients whose stops bind to a number series through a colour map?
6. Several plots sharing one x axis (small multiples): a layout of sized plots with
   bound ranges, or a plot that takes many series?

## 11. Phases and exit criteria

| Phase | Builds | Exit |
|---|---|---|
| 1. Series and functions | §1 (lambdas included), the content hash, the manifest table (§7) | expression tests for broadcasting, units, `ticks`, reductions; a series round-trips through a file and an edit; the view prints a summary |
| 2. Bound paths and sized uses | §2, §3 | a bound `'M ' + points(…)` renders, hit-tests, exports to SVG, refuses point editing; a sized test template resizes from each grip, text keeping its size, and undo restores the box in one step |
| 3. `std:plot` | §4–§6 | placed from the Library tab and by `<place>`; a slider-driven `ys` redraws live; corner resize reflows ticks and labels; the cursor drags along the curve; the double-slit example of §4.4 in one short edit; run-iron smoke updated |
| 4. Appear and disappear | machines.md phases 1–2 | §8 |

## 12. Done

**2026-11-08: phase 1, and phase 2's bound paths.**

- `Value::Series { data, dim }` and `Value::Fn` (`value.rs`): series written as
  `"0 0.4 0.9"` or `"400 450 500 nm"` (`Value::parse_series`; the UI's `coerce` sends
  `{ser: text}`), stored as `{"ser": [[si…], "unit"]}`, shown in full up to 16 and as
  `series(121 · nm, 400…700)` beyond (`units::display_unit` picks one unit for all).
  A function cannot be loaded as a constant.
- Expressions (`expr.rs`, new `series.rs`): broadcasting arithmetic and every scalar
  floor function; comparisons give 1/0 per element; `==` stays whole-value (§10 Q2).
  `series range step len at first last sum mean min(s) max(s) interp ticks nice_lo
  nice_hi where log10 ln exp sinc map sample rgb hsl mix spectrum` and the path-data
  writers `points smooth circles segments bars area`. Ticks use d3's step thresholds.
  `sinc(x) = sin(x)/x` (unnormalised).
- Lambdas: `x => body`, `(x, y) => body`, `f(x)`; parameters are `Expr::Var`, scoped by
  the parser; evaluating a lambda closes it (`Closure`), and its references stay
  dependencies of the binding that holds it.
- `'…'` (an enum literal) joins with `+` and serves as `fmt`'s unit like `"…"`.
- `path.d`: a constant written as path data is parsed (`Geometry::from_svg_path`, via
  kurbo); a binding that yields text is parsed in the graph (`graph::as_leaf_kind`), a
  parse failure being a binding error. `contour::bound` is the third form; `op.rs`
  refuses contour children beside it; the select tool frames it as `Kind::Fixed` and
  never point-edits it. packed-paths.md's "constant only" is superseded.
- `stroke.path_length` scales dashes on paths (`materialize::stroke_style`) and
  exports as `pathLength`. Other shapes ignore it for now.
- Not yet: the generated function table (§7;
  the prompt is updated by hand), `<look values=…>`, the share-key content hash
  (§1.5; series hash by `Debug`, which is complete for them).
- Follow-up seen in the app: a binding's quantity literals print in SI (`400 nm` reads
  back as `0.00000040000000000000003 m` in views and files). Printing the prefixed
  form when it round-trips exactly is a small change to `write_lit`, but it changes
  saved binding text, so it is left for a deliberate format decision.

**2026-11-08: phase 2's sized instances (§3).**

- `template` and `use` accept `size` (registry.rs). `definition::use_ops` seeds a new
  picture's `size` from its template's, as constants; `<place>` takes `w=`/`h=` (or
  `size.w=`/`size.h=`; bare `w`/`h` set arguments instead when the definition has
  inputs of those names) and refuses them for an unsized template.
- Expansion (`expand::want_use`): a reference to the template's own `size` relocates to
  the use's when the use is sized; an unsized use keeps reading the template's default.
- Render analysis keys any part reading the box on it (`analysis::classify`, input
  `size`), so equal-size pictures may share a drawing and others never do.
- The select tool's `local_box` gives a sized `use` a box of its size with
  `Kind::Sized`, so grips write the use's `size` and position instead of resizing
  parts. Min sizes (§3.2's `min_w`/`min_h`) are not built: a drag may shrink a box to
  nothing, as it may a rect.
- Static instances (`instantiate_static`) still read the template's default size.

**2026-11-08: phase 3, `std:plot` (§4), without the cursor drag and log axes.**

- Built in `standard.rs` (`plot`), listed in the Library tab. Ports as §4.1 except
  `x_scale`/`y_scale` (log axes, not built). Inputs `x_min x_max y_min y_max cursor`
  default to NaN ("auto"/"none"); an input with a NaN default accepts any number or
  quantity (`definition::check_kind`), and `<place>` reads `x_min="400 nm"` as one.
- Outputs `x_lo x_hi y_lo y_hi y_at_cursor` are the ranges in use and the curve at the
  cursor. They do not depend on the picture's box (an entity's ports are shared by its
  pictures), so the y range is "nice" for 5 ticks whatever the size; the picture's own
  tick count follows its size (about one per 70 px across, 40 px up, 2 to 8).
- Template (360 × 240): frame, title, a clipped plot area (grid, series 3, 2, 1, cursor
  line, dot and value), unclipped axes, tick marks and 8 label slots per axis (D6), and
  axis titles (the label given, else the data's unit; tick labels are in that unit).
  With `f` the curve is `sample(f, …)` at one point per 2 px; the range output samples
  200 points.
- `<place>`: `$self.k` reads the entity's own argument `k` (§10 Q4), series, enum and
  `#rrggbb` arguments parse. Floor additions: `isnan kind concat unit_of in_unit sig`.
  A quoted word (`'…'`, an enum literal) bound to a text leaf becomes text in the graph.
- Not built: dragging the cursor (§4.3), log axes, colour series, the palette domain
  (§6; colours are the defaults of `color`, `color2`, `color3`). The `vis` machine (§8) was added the same day (machines.md §10).
- Seen in the app: a selected plot's authoring chrome shows a pin for each of its 19
  inputs down its left edge; a quieter chrome for many-input components is a chrome.md
  question.

