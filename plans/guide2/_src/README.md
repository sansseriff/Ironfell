# Guide 2 sources

**Status:** refreshed 2026-10-28 against `5334ab8`. All four Foundations,
Parts 1–14, the index and field guide rebuild together. The guide describes the
current implementation; the roadmap and handoffs still own status and work order.

`plans/guide2/*.html` is generated. Edit `src/` and the field-guide table in
`entries.py`, then run from the repo root:

```sh
python3 plans/guide2/_src/build.py
python3 plans/guide2/_src/check.py
```

A page can be built alone (`build.py 10-boundary`), but run the full build before
considering an update complete: that also checks the field guide and every reference.
The builder validates named `<ref has="…">` targets and field-guide symbols. Each
`<snip>` also carries `first` and `last` boundary text, checked against the cited
range. When code moves, inspect the function and update both range and boundaries;
do not refresh the numbers without checking the explanation. No generated page is
written if validation fails. `check.py` checks local links/fragments, duplicate ids,
one main heading per page, and leftover source tags.

`guide.css`, `guide.js` and `highlight.min.js` (vendored highlight.js 11.9.0) in the
parent folder are hand-maintained. The guide works offline with fallback fonts.
Editor links point into the checkout where the guide was built.

## October 28 audit

The original nine-part guide landed in `b6e21bf` (October 13), Foundations in
`91a4454`. `3132ca1` included the engine-boundary chapters and earlier corrections;
`f318fad` added Part 14, built separately because old source locations had drifted.
`24c2fe8` extended that part for the schematic. This refresh reviewed the intervening
history through `5334ab8`, then checked mechanisms against the current source rather
than treating commit messages or plans as implementation evidence.

| Changes reviewed | Current source checked | Guide coverage |
|---|---|---|
| Shapes, SVG/appearance, clipboard, gesture ownership, pointer/gizmo, view textures and native feasibility (Oct 13–17) | `iron_document/{op,copy,gesture,shape,contour,composition,solve}`, `iron_svg`, bridge, tools and vector backend | Foundations, Parts 3–11 and 13; retain the useful earlier explanations, relocate excerpts and correct frame timing |
| `2169681`–`47950f2`: facets, inputs/args, linked instances, nested paths, overrides, anchors, plugging, mains and authoring | `facet`, `definition`, `expand`, `make`, `author`, `anchor`, `plug`, store apply/run, canvas selection/links | F1–F2, Parts 2, 5, 7 and 9; replace default copy-instantiation with linked expansion and explain authored versus derived edits |
| `3132ca1`, `6cb7ec2`, `67b25aa`: text, snapping, free running, layout and tables | `iron_text`, `iron_layout`, document `text/layout/table`, `iron_spatial::snap`, text tools, bridge `liveness/stats` | Parts 7–8 and 12; text measurement/editing, table structure, layout producer 2, host ids from 3 and Stats as live composition |
| `bc8791f`–`2489333`: packed paths, chunks, raster tiles, render inspector, instance cache | `geometry`, `contour`, `analysis`, bridge `chunks/render/inspector`, vector `display_list/hybrid/raster`, spatial scene | Parts 6–7 and 10; packed versus expanded geometry, rigid chunk movement, tile fallbacks/damage, static/keyed/live sharing and lightweight SVG references |
| `fc895d3`, `5621f65`, `65157a1`: dispatch, typed protocol, lifecycle and scoped reads | `engine_api/engine_dispatch/engine_protocol/host_events`, bridge `ui`, `BrowserRuntime`, document client, subscriptions, pending requests, model API | F4, Parts 8 and 11; three runtimes, both sync barriers, acknowledgment limits and stale-result protection |
| `5b1c05d`–`24c2fe8`: contexts, affordances, motion, focus, chrome and electrical schematic | `contexts`, built-in XML, `interaction`, motion/layout, `view_focus`, `parts/std_parts.xml`, wiring and model macros | Parts 1, 7, 9 and 12–14; relocate Part 14 excerpts and connect the earlier guide to the new rule mechanisms |
| `3771040`, merge `34085a5`, synthesis `5334ab8`: native presentation pacing | native pacing/feedback/budget modules, host launch scripts, standalone probe and `rendering/native-frame-pacing.md` | Part 11; distinguish CPU completion, GPU work and presentation, explain late-fifo/adaptive and preserve measurement limits |

The 267-entry field guide now indexes these mechanisms as well as the original spine. Examples
that abbreviate file/view output are labelled illustrative. Historical performance
results retain their fixture context and point to the owning benchmark records.

Validation for this documentation refresh: full generator and structural/link checker
pass; repeated generation is byte-identical; a deliberately wrong excerpt boundary is
rejected without replacing the last valid output. Browser preview was unavailable in
this session. No app behaviour changed, so no wasm/native rebuild or application smoke
was run or claimed.
