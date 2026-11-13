# Figma cursors, as mined

Raw CSS `cursor` values captured from Figma in Chrome with [`mine.js`](mine.js)
(paste it in the DevTools console, use Figma, run `saveCursors()`). Each `.txt`
is `image-set(url("data:image/png;base64,…") 8dppx) <hotspot x> <y>, auto`: a
256 px PNG for 8 device px per CSS px. `png/` holds the decoded images. The app
uses copies in `src-ui/cursors/` (registered in `src-ui/cursors/index.ts`).

In use (`src-ui/cursors/`): `2` default, `1` grab, `drag_*` grabbing, `3` pen,
`4`–`7` resize and rotate (turned at runtime), `adjust_point_1` point,
`other` bend, `new_*`/`close_drag`/`remove_anchor_i_think`/`soemthing_else`
pen variants, `break_handle` break (Alt over a path handle).

Kept for later (not used yet):

| file | looks like | hotspot | likely Figma use |
|---|---|---|---|
| `move.txt` | four-way arrows | 16 16 | moving a selection (keyboard nudge, move tool) |
| `move-selection.txt` | arrow with a four-way diamond | 4 4 | dragging an already-selected object |
| `resize-ns.txt` | vertical double arrow | 16 16 | an edge resize, upright (ours turns `resize.png`) |
| `resize-ew.txt` | horizontal double arrow | 16 16 | an edge resize, level |
| `rotate-angles/` | the rotate arrow at ten angles | 16 16 | reference only; `rotate.png` is turned at runtime |
