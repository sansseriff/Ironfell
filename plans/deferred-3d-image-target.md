# Plan: render the 3D viewer to a panel-sized image target

## Motivation

The `MainCamera3D` currently targets the window with a viewport sub-rect. Bevy sizes
a camera's intermediate render textures (main HDR/LDR target, depth, MSAA samples) by
the **full window** (`camera.physical_target_size`), not the viewport rect. So the 3D
view pays for full-window (5K) intermediates even though it only draws into a panel.

We measured the fallout: enabling the active 3D camera at 5K drove SoC temps past 100C.
`Msaa::Off` mitigated it (removed the 4x multisample amplifier on 5K color+depth), and
that's what currently ships. This plan is the structural fix: render the 3D view into an
`Image` sized to the viewer panel, then composite that image into the canvas. Cost then
scales with the *panel*, not the monitor — and MSAA 4x becomes affordable again because
it multisamples a panel's worth of pixels.

Defer until the current branch is merged/released; this is a follow-up.

## Current state (what this replaces)

- `scene3d.rs::setup_3d_scene` spawns `MainCamera3D` with `is_active: false`, targeting
  the window (default `RenderTarget::Window`).
- `mod.rs::apply_viewer_viewport` (Update) mirrors the `viewer` panel rect from the
  `Panels` resource onto `camera.viewport`, and toggles `is_active`.
- `picking.rs::camera_ray_from_window_px` maps window-space cursor px through
  `viewport_to_world`. (After the offset fix, it passes window-space `screen` directly.)

## Target design

### 1. A panel-sized render-target image
- Create an `Image` with `TextureUsages::RENDER_ATTACHMENT | TEXTURE_BINDING |
  COPY_DST`, `RenderAssetUsages::RENDER_WORLD`, sized to the viewer panel rect in
  physical px (fallback 1x1 until the panel exists).
- Store its `Handle<Image>` in a resource, e.g. `ViewerTarget { image: Handle<Image>,
  size: UVec2 }`.
- Point the camera at it: `Camera { target: RenderTarget::Image(handle.into()), .. }`,
  drop the `viewport` field entirely. Restore `Msaa::Sample4` (now panel-sized).

### 2. Resize the image when the panel changes (replaces `apply_viewer_viewport`)
- New system `resize_viewer_target` (Update): read the `viewer` rect from `Panels`;
  when its physical size changed, reallocate the image to the new size (`image.resize`)
  and update `ViewerTarget.size`. Toggle `camera.is_active` on rect presence.
- **Debounce during drags**: SplitPane drags fire rect changes every frame; reallocating
  a GPU texture per frame is the exact churn we're avoiding. Options, cheapest first:
  - Round the target size up to a bucket (e.g. next multiple of 128) so small drags
    don't reallocate.
  - Or reallocate only after the rect has been stable for N frames / a short timer.
  - Also cap at the window physical size so a mis-measured rect can't allocate huge.

### 3. Composite the image into the canvas
The image is off-screen; it must be drawn into the visible canvas at the panel rect.
Two viable routes (pick one):

- **A — Bevy UI ImageNode (recommended).** Spawn a `Node` positioned/sized to the panel
  (absolute, px from the same rect), with `ImageNode::new(handle)`, on the vello/UI
  camera's render layer. `apply_viewer_viewport`'s DOM->rect mirroring becomes
  "position this UI node." Pros: stays in the existing UI camera pass; no z-fighting
  with vello. Cons: another full-window UI pass already exists, this adds one node.

- **B — vello scene blit.** Draw the image into the vello scene as an image primitive at
  the panel rect. Pros: single composited layer with the rest of the vello UI. Cons:
  routing a Bevy `Image`/GPU texture into a `VelloScene` is more plumbing than a UI node.

Route A is less code and lower risk. Use it unless the vello layer must own the 3D view
for clipping/ordering reasons.

### 4. Picking against the image target
`camera_ray_from_window_px` currently maps window-space cursor px. With an image target
the camera has **no window viewport**, so:
- Convert the window cursor px to **image-local px**: subtract the panel rect's
  top-left (where the composite node/quad is drawn) — same rect used for compositing.
  Keep the bounds rejection against the panel rect.
- Pass image-local px to `viewport_to_world`. With an image target, the camera's
  `logical_viewport_rect().min` is `(0,0)` and its size is the image size, so
  image-local is exactly what it wants (no offset to double-count — the offset bug
  cannot recur here because the target origin is genuinely zero).
- Feed the compositing rect and the ray-mapping rect from **one source** (the `viewer`
  panel rect) so they can never disagree.

## Migration steps (in order, each independently testable)

1. Add `ViewerTarget` resource + create the image (1x1) at startup; keep camera on the
   window for now. No behavior change.
2. Switch `MainCamera3D.target` to the image; add the ImageNode composite (route A).
   Expect the 3D view to appear in the panel again; picking will be wrong until step 4.
3. Replace `apply_viewer_viewport` with `resize_viewer_target` (+ debounce) and the
   ImageNode positioning.
4. Update `camera_ray_from_window_px` (or its caller) to map window px -> image-local px
   using the panel rect. Verify select + drag land on the torus with no offset, at
   several panel sizes and after SplitPane drags.
5. Restore `Msaa::Sample4` on the 3D camera; confirm temps stay civil at 5K (panel-sized
   MSAA). Re-measure with `__probe()`.

## Verification
- Torus select/drag lands exactly under the cursor at: default layout, after dragging the
  left panel wider/narrower, after resizing the window, at DPR 1 and 2.
- `__probe()` cadence healthy at full-screen 5K; SoC temps well under the MSAA-off
  baseline even with `Sample4` restored.
- No per-frame texture realloc during a SplitPane drag (watch for wgpu
  "configuring/allocating" churn or a hitch while dragging the divider).

## Risks / notes
- Reallocation churn is the main hazard — the debounce/bucketing in step 3 is not
  optional at 5K.
- HDR: if the 3D camera enables `hdr`, the image format must be `Rgba16Float`; match the
  camera's tonemapping expectations or tonemap into an LDR image.
- The offset bug just fixed (double viewport-offset subtraction) is a cautionary tale:
  keep the compositing rect and the pick rect derived from a single value.
