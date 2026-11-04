# Ironfell

Technical and scientific communication in age of AI

## Run it

```sh
bun install
bun run wasm:fast   # optimized build (opt 3, SIMD): use this to judge performance
bun run wasm        # dev build (opt 1, debug info): faster to compile, ~3x slower to run
bun run wasm:smallfast  # the shipped build (LTO, wasm-opt -Oz, SIMD), both backends: judge size with this
bun run dev         # serves http://localhost:5173/Ironfell/
```

`bun run dev` serves whichever wasm build ran last. The Stats view (right of the
timeline) says which: `Build wasm-bench (opt 3, SIMD)`, or `Build dev-opt … SLOW`.
After switching builds, hard-reload the page.

**Settings:** the gear beside **+** in any tab bar. Render scale (canvas pixels per
CSS pixel; "Match a 4K panel" for a 4K display in a scaled macOS mode) applies at
once, as does a move to a screen of another density. The 2D renderer and worker
mode restart the app, keeping the document. Settings are kept per browser.
The app follows the system's light/dark appearance unless Settings forces one.

**UI components:** dropdowns, menus and switches come from
[bits-ui](https://bits-ui.com), icons from [@lucide/svelte](https://lucide.dev),
imported per icon (`@lucide/svelte/icons/<name>`). Colours are the tokens in
`src-ui/app.css`; shared component styles are in `src-ui/ui.css`.

## Shared controls for a fiber and its shadow

Attach points from separate paths to the same empty controls so a fiber and its
shadow move together. The paths keep their own contours, styling and tangent
handles; each attached point keeps an editable offset from its control.

1. Draw the fiber and shadow as separate paths, select both, and choose **Make
   component**. Enter the component's main and add two **Empty** controls from
   the shape toolbar. Name them endpoint A and endpoint B.
2. Select the main or one of its parts. In the inspector, click **Attach points
   to control**. The task stays pinned to this main while you select geometry.
3. Choose endpoint A. Pick the corresponding start point on each path using
   the numbered point buttons, or edit a point on the canvas and click
   **Collect canvas point**. Click **Attach 2 points** to apply both together.
   For a packed path, first click **Enable point attachments** on that path.
4. Choose endpoint B and repeat for the two end points. Move either empty to
   move both paths' attached points. Rotation and scale also affect the offsets;
   tangent handles retain their authored directions in the path's frame.
5. To make the endpoints connectable, choose **Expose as input port** for A and
   **Expose as output port** for B. Select each empty to set its signal kind,
   connector and signal expression in the existing inspector. Duplicate the main
   to create linked instances, then connect compatible ports with port drags.

Use **Edit offset** to adjust a point relative to its control, or **Detach** to
release it while preserving its current position. Detach points before deleting
their control. Each applied operation is undoable. Pending picks change nothing
until **Attach**; **Done** or **Cancel pending picks** closes the task while
keeping edits already applied. Attachments are authored on the main and shared
by its instances; nested-instance attachment authoring is not supported yet.

See the [authoring workflow specification](plans/specs/authoring-workflows.md#10-first-implementation-shared-point-controls-2026-11-03)
for implementation details and remaining workflow work.

**Stress test:** click **+** in any tab bar and pick a size under *Stress test*
(1,000–20,000 shapes). It opens a Stress tab there; the slider at its top rotates
the whole field. Picking another size replaces the scene. `?stress=N` in the URL,
or `await __iron.stress(N)` in the console, does the same.
