# SVG import and export

**Status:** Phases 1–4 done 2026-10-15 (§4, §7): export, import with rect and ellipse
recognition, paths with holes (schema 5), stroke style. Initial 2D appearance pass
2026-10-15: isolated opacity, two-stop gradient fills, group clip frames, SVG export
and exact Iron SVG restoration (§8). Gradient strokes and general foreign clip/gradient
import remain open.
**Background:** the assessment in
[`../notes/projects-labs-and-server.md`](../notes/projects-labs-and-server.md) §9.
Does not wait on the project container, except for raster images inside an SVG.

## 1. Goal

- **Export** a composition, or the selection, as SVG: a file to download, and SVG text
  on the clipboard (Figma's "Copy as SVG").
- **Import** SVG by pasting SVG text, dropping or opening an `.svg` file, or from the
  model surface, as **one transaction** of create ops placed like a paste.
- Report what could not be carried over, as shape-to-path conversion already reports
  its cuts. Never drop things silently.

## 2. Where it lives

A new workspace crate, **`iron_svg`**, depending on `iron_document` and `usvg`. It keeps
`iron_document` free of the SVG dependency, and it builds and tests natively
(`cargo test -p iron_svg --target aarch64-apple-darwin`) like the store.

- `export(store, root, opts) -> (String, Vec<Cut>)`: resolved values to SVG text.
- `import(svg_text) -> Result<(Fragment, Vec<Cut>), String>`: SVG to an
  `iron_document::copy::Fragment`, so placement, ids and the transaction go through the
  existing paste path (`copy::paste`, `select.rs` `paste_placement`).
- `Cut`: one human-readable line per dropped or approximated feature.

Use `usvg` with default features off (no text, no fontdb). Measure the wasm size before
and after, and record the change in this spec.

## 3. Mapping

Iron's 2D space already matches SVG: y-down, origin top-left, and `transform2d` applies
scale, then rotate, then translate (`geom.rs`). A rect spans (0,0)–(w,h) in its own
space, and an ellipse is centred on its origin.

| Iron | SVG out | SVG in (after `usvg`) |
|---|---|---|
| `transform2d` x, y, rot, sx, sy | `transform="translate(x y) rotate(deg) scale(sx sy)"` | Decompose the matrix; if it skews, bake it into the path's points and use an identity transform |
| `group` | `<g>` | `<g>` → `group` |
| `rect` with equal corners | `<rect width height rx>` | (paths only, see note) |
| `rect` with unequal corners | `<path>` (from `iron_document::shape::to_path`) | — |
| `ellipse`, full | `<ellipse rx ry>` centred | — |
| `ellipse` with an arc | `<path>` | — |
| `path` + `contour`s of `point`s, `fill_rule` | `<path d>`: per contour M, then L where both handles are zero, else C; Z if closed; `fill-rule` | One `path`, each subpath a `contour`; the fill rule kept. Quadratics are raised to cubics; `usvg` already turns arcs into cubics |
| `fill` color, opacity | `fill`, `fill-opacity` | Solid fill → `fill`. A gradient → its first stop's colour, plus a `Cut` |
| `appearance.opacity` | Isolated SVG `opacity` | Preserved as isolated group/shape opacity |
| `gradient` two-stop linear/radial fill | `linearGradient`/`radialGradient` in local user space | Exact for Iron metadata; foreign gradients still approximate with a cut |
| `group_clip` frame | A group's `clip-path` with rect/rounded rect/ellipse | Exact for Iron metadata; general foreign clips remain a cut |
| `stroke` color, width, join, cap, miter, dash | `stroke`, `stroke-width`, `stroke-linejoin`, `-linecap`, `-miterlimit`, `-dasharray`, `-dashoffset` | All of it (`miter-clip` → `miter`, a cut) |
| `shadow` | `<filter>` with `feDropShadow` (`stdDeviation` = blur ÷ 2) | Not read back, unless from Iron's own data attributes |
| `name` | `id` (made unique) and `data-iron-name` | `id` → `name` |
| `embed` (2D) | `<g>` with the embed's transform and a `clipPath` of its frame | — |
| `embed` (3D), sliders, meshes | Left out, plus a `Cut` | — |

**Iron's own data.** Export writes `data-iron-type` and the shape's parameters
(`data-iron-w`, `data-iron-corners`, `data-iron-arc`, …). Import reads them with a light
pass over the raw XML (`roxmltree`, which `usvg` already uses), keyed by element `id`,
and recreates rects and ellipses as rects and ellipses. Without them, `usvg` gives only
paths. Re-detecting rects and ellipses from foreign paths is optional, phase 2 at the
earliest.

**Not carried, reported as `Cut`s:** text, images, gradients and patterns (beyond the
first-stop approximation), masks, clip paths other than an embed's frame, filters other
than Iron's shadow, blend modes, bindings, constraints, clips, entities and links.
Export flattens bindings to their current resolved values.

## 4. Phases

### Phase 1: export (done 2026-10-15, §7)

1. `iron_svg::export` with the mapping above, from resolved values (`Store::resolved`).
2. FFI and shell:
   - `__iron.exportSvg(scope)` returns the SVG text, where scope is a composition or
     `'selection'`;
   - an **Export SVG** command (the Document view, or the tab's + menu) downloads it;
   - **Copy as SVG** puts it on the clipboard as text (Figma has no default shortcut
     for it; pick one that does not clash with Cmd+C/X/V/D).
3. Tests: native golden files for a fixture composition covering every row of §3, and
   one parse of each golden with `usvg` to prove it is valid SVG.

**Exit:** the demo's Controls composition exports, opens correctly in a browser, and
matches its golden file. The smoke script checks `__iron.exportSvg` for a small
composition.

### Phase 2: import (done 2026-10-15, §7)

1. `iron_svg::import` to a `Fragment`, plus `Cut`s.
2. Entry points, all through the paste path, so the import lands under the pointer or at
   the view's centre, in the artboard under it, as one transaction "import SVG":
   - **paste:** `clipboard_paste` currently ignores text that is not an Iron fragment;
     try SVG when the text starts with `<svg` or `<?xml`;
   - **file drop or open** of an `.svg` in the shell, read as text and sent the same
     way;
   - `__iron.importSvg(text)` for the driver and the model surface.
3. Show the `Cut`s to the person: a toast or the console for now. The model surface
   returns them in its edit result.
4. Tests: native fixtures (Figma's "Copy as SVG" output, an Inkscape file, a file with
   nested transforms and `<use>`); **the round-trip law:** export → import → export
   gives identical SVG for Iron-made content.

**Exit:** pasting Figma's "Copy as SVG" of a few shapes gives the same picture in Iron,
editable: points and handles in edit mode; rects and ellipses keep their handles when
the SVG came from Iron. Undo removes the import in one step. The smoke script imports a
fixture and checks node counts and one point's handles.

### Phase 3: paths with holes (done 2026-10-15)

Decided by the user 2026-10-15: change the format to carry as much of SVG as is
reasonable, holes first. A `path` now holds `contour` children, each one outline with
its `point`s and its own `contour.closed`; `path.fill_rule` is `nonzero` (default) or
`evenodd`. Schema 4 → 5; an older file's path gets one contour holding its points and
its `closed` (`serial::migrate_contours`). Only closed contours fill; every contour is
stroked. `iron_document::contour` reads and builds them for everyone: the renderer
(`Shape2d::Path` carries the stroked outline, the filled one and the rule), hit
testing (winding by the rule), the select tool's edit mode and the pen (segments,
neighbours, insert and remove per contour; the pen draws and closes a path's last
contour), `shape::to_path` (a ring now converts: two contours), copy, the demo and the
stress fixture. A point's or contour's edit redraws its path.

### Phase 4: stroke style (done 2026-10-15)

`stroke.join` (`round`, `miter`, `bevel`), `stroke.cap` (`round`, `butt`, `square`),
`stroke.miter` (limit, default 4), `stroke.dash` (SVG dasharray text, empty for solid)
and `stroke.dash_offset`. Defaults are round joins and caps, Iron's only style before,
so old drawings look the same. Drawn through `kurbo::Stroke`, which both renderer
backends take; culling bounds pad by the miter limit. SVG carries all of it both ways,
so dashes, caps and joins are no longer cuts.

### Appearance features (initial pass 2026-10-15, §8; follow-ups remain)

5. **Opacity** on groups and shapes (SVG `opacity`, Figma's layer opacity): a layer in
   the display list. Import stops approximating a group's opacity per shape.
6. **Gradients**, linear and radial, for fill and stroke: `Brush` is solid-only in both
   backends today, and the inspector and the model's values need a way to say a
   gradient (likely a gradient node or a record value).
7. **Clipping a group to a shape** (Figma's "clip content", SVG `clip-path` on a
   group): Iron already clips embeds; generalise it.

**Not planned:** text (fonts and shaping are their own project), images (they wait on
the project container's asset storage), masks, filters other than the drop shadow, and
blend modes.

## 5. Decisions to raise, not take

1. ~~Phase 3's format change and its shape.~~ **Decided 2026-10-15:** change the format
   to carry as much of SVG as is reasonable; contour nodes (§4).
2. ~~Whether import should re-detect rects and ellipses in foreign SVG.~~ **Decided
   2026-10-15: yes** (§7, "Recognised shapes").
3. ~~Where Export SVG lives in the UI until there is a File menu.~~ **Decided
   2026-10-15: the Document view** (the left panel), for now; it will be remade.

## 6. Conventions

Per [`../../AGENTS.md`](../../AGENTS.md) and
[`../handoff-product.md`](../handoff-product.md) §6:
- every document change is ops in a transaction;
- add each phase's checks to the smoke script, and keep the expected output in
  `.claude/skills/run-iron/SKILL.md` current;
- record each phase as "Done" here and in `roadmap.md`'s Status table;
- don't push.

Imports mint ids, so later ids in the smoke's expected output shift.

## 7. Done

**Phases 1 and 2, 2026-10-15.** `src/iron_svg` (`export.rs`, `import.rs`), wired into
the select tool's paste path, the FFI, the shell and the model surface.

- **API.** `export(store, &Scope) -> Result<Export { svg, cuts }, String>`, where
  `Scope` is `Composition(id)` or `Nodes(ids)` (the selection: each node in its
  composition's space, wrapped in a `matrix()` group when its container moves it).
  `import(text) -> Result<Imported { fragment, cuts, nodes }, String>`. A cut is a
  `String`; repeats are counted (`… (×20)`) rather than listed.
- **Numbers.** Coordinates, transforms and paint are written to four decimals. The
  shape parameters in Iron's data attributes (`data-iron-w`, `-h`, `-corners`, `-rx`,
  `-ry`, `-arc`) are written exactly (shortest round-trip f64): the outline beside
  them is computed from them, and a rounded sweep (4.7124 rather than 3π/2) draws a
  different outline on the next export. The round-trip test caught this.
- **Strokes** carry their join, cap, miter limit and dashes (phase 4); before phase 4
  every stroke was written with round joins and caps, Iron's only style then.
- **Shadow:** the shape inside `<g filter>` with an `feDropShadow` whose region is
  `userSpaceOnUse`, computed as the renderer bounds it, so a large blur is not
  clipped; the shape carries `data-iron-shadow="#rrggbb alpha dx dy blur"`.
- **2D embed:** a `<g data-iron-type="embed">` holding the embedded composition's
  content, clipped to the frame (or to the content's outline bounds, as
  `nest::content_bounds` does). It imports as a group of copies, with a cut.
- **Deviation: sliders are exported as a picture** of themselves (track, fill,
  knob), with a cut, rather than left out: the Controls composition holds only a
  slider, so leaving it out made the exit's export empty. It imports as shapes.
- **Import space.** Coordinates are the SVG's user space (its `viewBox`): Iron's
  own files then land in composition space; an icon with `viewBox="0 0 24 24"
  width="48"` imports 24 units wide.
- **Structure.** usvg makes an anonymous group for every transformed shape, and for
  opacity, filters and clips; one with a single child is folded into it. A group
  with an `id`, or with several children, becomes a `group`, its `id` the name.
- **Iron's own elements** (those with `data-iron-type`) are read again from the raw
  XML by `id`, in f64: transforms from `translate() rotate() scale()`, path points
  from `d`, rect and ellipse parameters from the data attributes. usvg's tree is
  f32, which is not enough for an exact round trip. The f32 values of foreign files
  are rounded to four decimals.
- **Deviation: skew.** A skewing transform leaves the node its translation and bakes
  the rest into points (or passes it to a group's children); the plan said an
  identity transform. Keeping the translation keeps the node's origin where it was.
- **Paths.** Each subpath is a `contour` of `point`s in one `path`, with the fill rule
  (phase 3); quadratics are raised to cubics. A closing curve back to the start
  merges into the first point; a closing line does too in foreign files (in Iron's,
  it is a real point). An open subpath with a fill and no stroke is closed (SVG fills
  it as if closed; Iron fills only closed contours); with a stroke, a cut. Export
  reports a filled path that mixes open and closed contours, since SVG fills the
  open ones too.
- **Recognised shapes** (decision 2, 2026-10-15). A foreign closed outline, in its
  own axes and not skewed, that is a rect (each corner rounded or sharp, radii read
  from where the edges' anchors stop) or a full ellipse comes in as a `rect` or an
  `ellipse`, so it keeps its handles. Each guess is drawn with Iron's own outline
  (`shape::rect_outline`, `ellipse_outline`) and kept only if the two coincide
  within 0.1% of the size, both ways; the ellipse is tried first, so a circle is not
  read as a rect rounded to half its size. Elliptical corners (`rx` ≠ `ry`), skewed
  rects, pies and compound paths stay paths.
- **Not imported, each a cut:** text (scanned in the raw XML, since usvg without
  its font stack drops it), images, patterns, masks, clip paths, filters other than
  Iron's shadow, blend modes; gradients become their first stop's colour;
  a group's opacity is multiplied into its shapes (a cut when it has several).

**Entry points.** Paste: `clipboard_paste` text that is SVG (also a clipboard's
`image/svg+xml`) imports through `select.rs`'s paste placement as one "import SVG".
Dropping an `.svg` on the canvas moves the pointer to the drop point and pastes its
text. The Document view's Load opens an `.svg` as an import. `__iron.importSvg(text)`
imports on the next frame and answers `{ok, cuts, nodes}`. The model surface's
`<import-svg to="#c" x y>…</import-svg>` (alone in its `<edit>`) applies at once,
placed where it says, and returns `<imported roots>` and `<cut>` lines in the edit
result's `summary`; the manifest lists the verb. Export:
`__iron.exportSvg(scope)` (a composition's id or name, a node, or `selection`) and
`__iron.exportSvgCuts(scope)`; the Document view has a row "SVG of [selection |
composition] · Export SVG · Copy SVG" (decision 3: here until the Document view is remade). **Copy as SVG is
Alt+Shift+C** (by key code): Cmd+Shift+C and Cmd+Alt+C open the browser's
developer tools; it writes the clipboard inside the key event, as Cmd+C does.

**Tests** (`cargo test -p iron_svg --target aarch64-apple-darwin`, 12; the fixture and
the round trip include a dashed, mitred stroke and an even-odd frame with a hole): the fixture
composition (every row of §3, plus a binding, a constraint, a slider, a framed 2D
embed and a 3D one) against `tests/golden/fixture.svg`, parsed by usvg; each row's
markup; the selection's wrapper; **the round-trip law** on Iron-made shapes (and
rects and ellipses come back as themselves); Figma's "Copy as SVG" (rect, circle,
line, drop-shadowed square: a rounded rect, an ellipse, a path, a rect); near
misses that stay paths (a trapezoid, a half disc, elliptical corners, a skew); an Inkscape file (mm
units, a layer, a gradient, a dashed stroke, text); nested transforms with `<use>`,
skew, an even-odd donut, a quadratic and an arc; refusals; undo in one step.

**Exit.** The demo is built by the app crate (Bevy), so the Controls export is
checked by the smoke script's expected output rather than a native golden file;
the native golden is the fixture composition. The smoke script exports Controls,
imports the Figma fixture (node counts; the rounded rect, the ellipse, and the
line's two points),
copies it as SVG through the system clipboard, undoes it in one step, imports it
again by a paste event and by `<import-svg>`, each undone. The exported Main
composition, opened as an image in Chrome, matches the live view, less the 3D
embed (a cut).

**Observed, not changed.** A semi-transparent fill looks lighter in Iron than in a
browser (`#333333` at 0.35 over white: Iron about `#d4d4d4`, Chrome `#b8b8b8`):
Iron's renderer appears to composite in linear light, browsers in sRGB. Export
writes the stored sRGB values.

**Wasm size** (shipped build, `build-wasm.sh webgpu`: wasm-release, `wasm-opt
-Oz`): 19,097,817 → 19,429,157 bytes, **+331 KB (+1.7%)**;
gzipped 6,256,646 → 6,404,900, **+148 KB (+2.4%)**. Measured 2026-10-15 by building
the same tree with and without the app's use of `iron_svg`, before
`document_import_svg_at` (a thin wrapper) was added. usvg is built without text, so
no font stack.



## 8. Initial 2D appearance and inspector pass (2026-10-15)

- `appearance.opacity` (0–1) is separate from `fill.opacity`. `PushOpacity` in the
  display list composites a whole shape or group once, preserving overlapping children.
  The render walk restores ancestor scopes around the spatial query's candidates;
  embeds retain their own placement and clips. No new transform hierarchy.
- `gradient` is an optional two-stop **fill**, overriding the solid fallback:
  `kind=linear|radial`, `start`/`end` colours with alpha, `x0/y0/x1/y1` fractions of the
  local outline's bounds. Radial's first point is its centre; the distance to the second
  is its radius. Each field is a normal bindable/animatable leaf. Fill opacity multiplies
  both stops. Removing the component reveals the retained solid fill.
- `group_clip` is an optional local frame on a group: `enabled`, `shape=rect|ellipse`,
  `w`, `h`, `radius` (uniform rounded rect). Drawing and picking use the same resolved
  frame, including previews and transformed ancestors. Dimensions default to 200×200;
  set them explicitly for the intended frame. This initial representation does not yet
  reference an arbitrary child path as a clip. Selection/marquee bounds remain
  conservative around clipped descendants; there are no on-canvas clip handles yet.
- SVG writes ordinary `opacity`, linear/radial gradient definitions and `clipPath`s.
  Resolved `data-iron-appearance` metadata preserves the exact optional components and
  solid fallback on Iron exports. Import restores these without cuts, and keeps foreign
  multi-child group opacity as an isolated layer rather than multiplying each shape.
  General foreign gradients still approximate to their first stop with a cut; foreign
  clips still report a cut. Gradient strokes and multiple stops remain follow-ups.
- Inspector details and its composition status live in `panels-and-session.md`.

Validation: native appearance tests cover transformed/rounded/elliptical clip picking,
preview cancellation, single-transaction scrubbing, undo/redo and file round trips. SVG
coverage includes both gradient types with alpha, clip frames and nested opacity under
the exact export→import→export law, plus foreign isolated group opacity. The run-iron
`appearance.ts` regression checks rendered pixels and real Alt-drag pointer gestures.

**Automatic embed frame correction (2026-10-15).** Unframed 2D embeds previously
clipped their source to geometry-only content bounds, cutting the outer half of a
centred stroke at the source composition's edges. Automatic regions now include the
same local stroke/join, shadow and slider-knob padding used by spatial bounds,
expanded before mapping each node's transform. SVG export applies the same policy.
Explicit authored frames retain their intended clipping. The run-iron
`stroke-clipping.ts` pixel regression covers a rounded gradient shape, all four
stroke outsets, later width/size edits, explicit clipping and DPR 1/2; a native SVG
regression checks nonuniform transforms, shadow padding and explicit frames.
