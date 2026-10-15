# Architecture improvements from the infinite canvas tutorial

**Status:** Phase 3 implemented (§13); phase 1 first pass done 2026-10-15 (§4,
"First pass"); phases 2, 4 and 5 open
**Date:** 2026-10-13
**Scope:** Adapt relevant patterns from `xiaoiver/infinite-canvas-tutorial` while
preserving Iron's authored document, Bevy runtime, and Svelte host boundaries.
**Research baseline:** Tutorial commit
`ef879525f47cfdf2c4cab6366039cba41694d0fc`; source inspection, not comparative
performance measurements. Iron's implementation is evolving; verify the current
code before starting each phase.

## 1. Decision

Keep `iron_document` as authority. Borrow the tutorial's more developed spatial,
gesture, UI lifecycle, and capability patterns around it.

Iron's atomic semantic transactions, editable handles, units, constraints, and
entity/figure/composition distinction are particularly suited to the intended
scientific explanation tool. Moving authority into Svelte or directly into Bevy's
ECS would weaken those foundations.

Phases 3–5 below are rendering work: phase 3 is implemented (§13), and the order of
phases 4–5 is owned by [`handoff-render.md`](../handoff-render.md) §4. Phases 1–2
(gesture ownership, typed UI protocol) are independent hardening work, listed as
candidates in [`handoff-product.md`](../handoff-product.md).

Capability providers and DOM hosting arrive with their first concrete product use.
They do not block phases 1–4.

## 2. Relationship to existing plans

This plan refines existing work rather than replacing the roadmap.

| Work | Existing home | Contribution here |
|---|---|---|
| Gesture ownership | [Select tool](select-tool.md), [constraints](constraint-system.md), roadmap E | Define what happens when human previews overlap model edits, undo, or view changes |
| UI API and subscriptions | [Document spine](document-spine.md), [panels and session](panels-and-session.md) | Typed transport, scoped subscriptions, stale-response protection, explicit disposal |
| Bounds and spatial queries | [Roadmap H](../roadmap.md), [layered architecture](../vision/01_layered-architecture-overview.md) | Independent derived subsystem shared by rendering and interaction |
| Editor overlays | [Rendering architecture](../vision/07_current-2d-backends-and-hybrid-architecture.md) | Keep hover and selection from invalidating retained content |
| Chunks and raster caching | [View textures](view-textures.md), [Hybrid design](../archive/bevy-vello-hybrid-design.md), vision 07 | Feed existing cache direction with precise changes and spatial work sets |
| External Models | [Computation boundary](../vision/06_the-computation-boundary.md), [substrate](../vision/02_substrate-model-projection-control.md) | Named capabilities with explicit providers and stale-result handling |
| DOM-hosted nodes | Document spine §4, panels and session | Materialize hosted-node projections in Svelte |

The performance gate for roadmap H remains. Correctness work on gestures and
request lifecycle can proceed without committing to a tile cache. Record actual
implementation completion in the owning plan and roadmap when it lands.

## 3. Architectural baseline

### Iron

- `iron_document` holds authored nodes, components, relations, operations,
  history, and resolved values. It has no Bevy dependency.
- `document_bridge` maps stable `NodeId`s to Bevy entities with `Provenance`, and
  reconciles transactions and resolved changes.
- Gestures preview through the store and commit one semantic transaction.
- `DisplayList` and `VectorLayer` form the renderer-facing data seam. They are
  renderer-neutral but currently Bevy-dependent, including image handles.
- Renderer plugins consume draw intent and retain per-view textures. Clean views
  can be reused; dirty targets still require broad encoding within the target.
- Svelte owns browser chrome and dock layout. `BevyPanel` registers a DOM
  rectangle; Rust renders the corresponding view. Hidden canvas tabs unregister
  their regions. Views can be canvas, HTML, or hybrid.

### Tutorial

The repository contains multiple generations: earlier shape-oriented lesson/core
packages, a Becsy ECS editor, Web Components UI, a React integration, and a separate
Rust Vello plugin. Its mesh renderer and Vello renderer have different retention
behavior; findings about one must not be attributed to all backends.

The relevant implemented patterns are independent bounds, spatial culling,
geometry/material dirtiness, gesture context validation, scoped UI stores,
cancelable scheduled edits, resource scopes, and named capability providers.

Its Vello content redraw calls `clearShapes()` and resubmits shapes. Camera-only
updates avoid that resubmission, but this is not evidence of a completed
damage-bounded raster tile cache. Likewise, its scheduled `edit()` boundary does
not guarantee rollback after a callback failure. Preserve Iron's stronger atomic
transaction semantics. See [Vello submission](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/plugin-vello/src/VelloPipeline.ts#L856) and [failed-edit rollback test](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/__tests__/ecs/api.edit.spec.ts#L122).

## 4. Phase 1 — Gesture ownership and conflicting edits

### Tutorial evidence and adaptation

- [gizmoDocumentKey and gizmoViewKey](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/ecs/src/systems/pick3d/gizmo-context.ts#L7) capture source/ancestor spatial fields and the projection used to interpret a gesture.
- [GizmoSession](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/ecs/src/systems/pick3d/gizmo-session.ts#L13) checks whether the captured source context remains current.
- [GizmoSourceFrame](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/ecs/src/systems/pick3d/gizmo-source-frame.ts#L37) distinguishes accepted preview writes from external layout changes; `restoreOrigin` preserves newer document coordinates.

**Iron adaptation:** extend this spatial-context pattern to leaf write targets,
bindings, constraint fingers, and embedded reaches. The tutorial does not implement
Iron's constraint/handle semantics or prescribe a revision-based dependency set.

### Problem

A gesture captures a starting pose, handles, and a view mapping. A model edit,
undo, reparent, binding change, or camera change can make those assumptions stale
before pointer release. A stale release must not overwrite newer authored intent.

The tutorial's 3D gizmo implementation captures document and view context,
distinguishes its preview writes from external changes, and permits unrelated
name/material edits while canceling conflicting spatial edits.

### Work

1. Give each active gesture an explicit identity and dependency set. Include the
   manipulated leaves and their resolved write targets, relevant ancestors and
   embed placements, constraint membership, binding/handle configuration, and
   the panel/view mapping used to interpret the pointer.
2. Classify changes as gesture-owned preview changes, compatible external changes,
   or conflicts. Use touched fields and relevant dependency revisions rather
   than canceling on every document-version change.
3. Initially cancel on conflicts; defer rebasing until a concrete interaction
   requires it. Unrelated edits continue normally.
4. On cancellation, remove gesture-owned previews and fingers, then resolve from
   the current document. Never restore an obsolete authored snapshot over newer
   state. Review the current shared overlay API: ownership must prevent one
   gesture's cancellation from clearing another interaction's preview.
5. Define cancellation for document replacement, deletion, tool/session disposal,
   panel removal, and view-mapping changes. Decide explicitly how undo during a
   gesture behaves before applying history.
6. Use one lifecycle contract for move, resize, rotate, point/handle edits, sliders,
   and 3D manipulation, while retaining their different value semantics.

**Affected modules:** `src/bevy_app/select.rs`, `interaction.rs`, `pen.rs`,
`src/document_bridge/mod.rs`, `src/iron_document/src/lib.rs`, `graph.rs`, and
`handle.rs`. Exact types follow an audit of existing gesture paths.

### Exit criteria

- A model pose edit during a drag survives cancellation and subsequent release.
- An unrelated caption or color edit does not interrupt the drag.
- Reparenting, deleting the target, changing its binding, or replacing its view
  cancels cleanly with no stale commit.
- Gesture preview writes do not invalidate their own context.
- Cancel produces no authored transaction; successful release produces one.
- Bound-source and constraint-driven gestures obey the same ownership rules.

### First pass (2026-10-15)

**In the store** (`iron_document::gesture`, `Store`):
- **A gesture has an id and watched nodes.** `begin_gesture(nodes)` starts one; a
  preview or `begin_drag` without one starts it implicitly and adds its node. One
  gesture at a time: there is one pointer, and a newer gesture cancels the older.
- **A transaction from outside the gesture is classified before it applies.** It
  conflicts if it:
  - deletes or reparents a watched node or an ancestor;
  - sets a geometry leaf of one, or binds any of its leaves;
  - sets a leaf the gesture drives (a handle's source, such as the slider behind
    a bound bar);
  - links or unlinks a constraint or a clip on a watched node;
  - makes the solver move a watched node (`derived_ops`).

  Anything else is compatible.
- **Every outside transaction applies without the gesture's previews.** It applies
  and solves against the document alone, with the drag's fingers set aside. The
  previews are then put back, or dropped on a conflict. This fixed a latent bug: an
  unrelated edit during a cluster drag used to solve with the drag's finger and
  previewed pose, and wrote the dragged pose into the document under the other
  transaction's label.
- **A cancelled gesture commits nothing.** Its previews and fingers are dropped
  and the document, with the edit, shows. `gesture_status(id)` says why.
- **Undo and redo cancel a running gesture first** (the decision item 5 asked for).
- **One-off edits no longer share the overlay.** `set_through_handle` is one
  transaction through a leaf's handle with no previews. The inspector's
  `document_inspect_set` used to preview into the shared overlay and commit
  everything in it, including a drag in progress.

**In the select tool:**
- It holds the store gesture behind its current one (`SelectTool::owned`). When the
  store has ended it, the tool goes idle and the release commits nothing.
- **Box gestures and marquees follow their view's current pan and zoom** while the
  button is down, and end if the view closes.
- **The legacy drags** (3D, planes, sliders) begin their gesture explicitly in
  `begin_legacy`.

**Exit criteria:**
- Met, natively, in `tests/gestures.rs`:
  - a model pose edit survives cancel and release;
  - a colour or another node's edit leaves the drag running;
  - deleting the node or its parent, reparenting, moving the parent, rebinding or
    resizing cancels;
  - previews never cancel their own gesture;
  - cancel writes nothing and a release writes one transaction;
  - a bound-source drag and a cluster drag obey the same rules.

  The conflict and suspension tests fail with suspension and classification
  disabled: checked by mutation.
- Met in the browser by the smoke script's gesture ownership section.
- **View replacement** is handled for 2D box gestures.
- **Not handled:**
  - a camera change during a 3D drag, which still uses the camera it started with;
  - rebasing: deliberately cancel-only, as item 3 says.

**Deviation:** item 1's dependency set is coarser than proposed. It watches nodes
with their ancestors and a fixed list of geometry components, rather than
per-leaf revisions. Enough for every exit; revisit if a compatible edit is ever
wrongly cancelled.

**Not yet (item 6):** one lifecycle contract in the tools themselves. The store's
lifecycle is shared, but `select.rs`'s `Gesture` enum and `interaction.rs`'s
`DragState` are still two state machines.

### 2D and 3D: what to share

Three layers, shared to different degrees:

1. **Lifecycle and ownership: shared, and done.** Identity, watched nodes, previews,
   commit and cancel, conflicts. It is in the store and knows nothing of dimension.
2. **The pointer state machine: should be shared, and is not yet.** Press, slop,
   drag, release, Esc, modifiers, cursors, which tool owns the pointer. Today
   `select.rs` runs 2D box gestures and hands 3D, plane and slider drags to
   `interaction.rs`'s `DragState`. The proposal is one state machine with a
   *manipulator* per kind, each answering three questions:
   - `begin`: which nodes it watches;
   - `update`: the leaf values for this pointer;
   - its label.

   The manipulators: box transform (`Part::apply`), path point and handle, shape
   grip, slider, drag through a plane, and the 3D gizmo below.
3. **Value semantics: per dimension.** A 2D manipulator maps the pointer through
   the panel affine and the embed chain; a 3D one through a camera ray. Constraint
   drags go through `begin_drag`/`drag_to`, whose fingers are 2D world points; 3D
   mates (CS phase 6) need 3D fingers.

**A 3D transform gizmo** (move, rotate, scale handles) is a manipulator in this
scheme. `transform-gizmo` 0.11 fits the Bevy 0.19 workspace, and comes in two
forms:
- **`transform-gizmo-bevy`** writes the `Transform` of entities marked `GizmoTarget`
  directly. That bypasses the store, and the reconciler would fight it.
- **The core crate `transform-gizmo`** is framework-agnostic. According to its
  documentation, it takes the targets' transforms, the camera's view and
  projection, and the pointer, and returns updated transforms plus screen-space
  triangles to draw. (Verify the exact API when importing.)

**Recommendation:** use the core crate as a manipulator:
- read the selected meshes' resolved `transform3d`;
- preview its result through `Store::preview`, so ownership, conflicts and the
  single commit apply unchanged;
- draw its triangles in the panel's retained overlay target (stage H).

Its dependencies include egui's `epaint`, `emath` and `ecolor`, so measure the wasm
size when adding it. Not imported yet.

## 5. Phase 2 — Typed UI protocol, subscriptions, and lifecycle

### Tutorial evidence and adaptation

- [createCanvasStore](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/react/src/store.ts#L49) retains unchanged node snapshots and gives mounted owners `claim`, `attach`, and `release` lifecycles.
- [useCanvasSelector and useCanvasActions](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/react/src/CanvasProvider.tsx#L25) separate state subscriptions from stable editing actions.
- [createCanvasActions](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/react/src/actions.ts#L58) reads the current API and cancels queued edits when API ownership changes.
- [API subscriptions](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/ecs/src/API.ts#L388) expose committed state and history availability separately.
- [API.edit contract](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/ecs/src/API.ts#L3682) distinguishes edit completion from rendering and rollback; [EditQueue](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/ecs/src/EditQueue.ts#L1) settles canceled/disposed work.
- [edit lifecycle tests](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/__tests__/ecs/api.edit.spec.ts#L77) exercise destruction, abort handling, and failed edits.

**Iron adaptation:** typed worker messages, query generations, leaf-scoped reads,
and separate authored/session/resolved versions are proposed for Iron's transport
and authority model. The React store uses broad snapshot notifications; it is not
an existing fine-grained worker subscription protocol to copy verbatim.

### Problem

`DocumentClient` combines document reads, model requests, selection, tools, and
generic string-based FFI calls. Document-version changes trigger broad rereads.
Request IDs correlate replies, but do not establish whether a reply is still
current for a component or session.

### Work

1. Define discriminated request/response types and explicit command payloads for
   the UI operations currently routed through `documentCall`. Share them across
   worker and main-thread adapters. Keep raw FFI dispatch behind this boundary.
2. Give the document client an explicit attach/dispose lifecycle. On disposal or
   replacement, settle pending requests, detach subscriptions, and invalidate the
   session generation. An API advertised as a promise must not remain pending
   forever after its runtime disappears.
3. Tag requests and responses with session identity and relevant versions. Use
   per-query generations where users can change selection, fidelity, or scope
   while an older query is in flight. Only the current result publishes UI state.
4. Add scoped subscriptions for selection, inspected nodes, composition summaries,
   document views, and history availability. Keep content version, session version,
   and resolved-value changes distinct: a live binding can change without an
   authored transaction.
5. Coalesce refreshes and use touched-node/leaf information where available.
   Subscribe only while a consumer needs the data; retain stable UI object identity
   for unchanged results without mirroring the whole document by default.
6. Define acknowledgment semantics per command. Distinguish accepted/queued,
   applied, reconciled, and rendered where callers need that distinction.
   A successful apply does not promise that a frame has been presented.
7. Preserve one semantic implementation behind both adapters. Cancellation can
   prevent pending work; it does not undo a transaction already applied.

Svelte holds a read projection, not a second authority. User edits remain semantic
commands into `iron_document`, and inspector edits continue through handles.

**Affected modules:** `src-ui/runtime/document_client.svelte.ts`,
`session_adapter.ts`, `src-ui/worker.ts`, `main-thread-adapter.ts`, the HTML views,
`src-ui/model/api.ts`, and `src/web_ffi.rs`.

### Exit criteria

- Both adapters satisfy the same command contract.
- Session replacement/disposal settles every pending request.
- Rapid selection changes cannot publish the previous selection's inspector.
- Changing one node does not reread unrelated inspectors or composition summaries.
- Resolved-value updates reach subscribed inspectors without inventing document
  transactions.
- History controls update from explicit state; components do not infer availability.

## 6. Phase 3 — Independent bounds and spatial queries

### Tutorial evidence and adaptation

- [ComputeBounds queries and updateBounds](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/ecs/src/systems/ComputeBounds.ts#L82) prepare bounds independently of draw submission, distinguish geometry from render bounds, propagate group changes, and compute world/selection bounds.
- [ViewportCulling](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/ecs/src/systems/ViewportCulling.ts#L17) maintains an RBush per camera from changed bounds and queries viewport candidates.

**Iron adaptation:** per-composition indexes, embedded-occurrence identity,
independent interaction bounds, and old/new damage are the proposed Iron design.
They are not all supplied by the tutorial. Its bounds code sometimes folds hit-area
expansion into render bounds, and camera changes still enumerate descendants;
copying it does not establish bounded work for every update.

### Problem

`Bounds2d` is currently maintained during the drawing traversal, while picking
walks composition content separately. Bounds should be available even when a view
does not draw, and should support rendering, picking, marquee, fit-to-content,
and damage from one derived source.

### Work

1. Extract bounds preparation from `render_document_layer`. Distinguish geometry,
   painted/render, and interaction bounds. Selection decoration must not silently
   enlarge cached document-content bounds.
2. Track local geometry/style changes separately from world-placement changes.
   Parent changes propagate to descendants; point changes update owning paths;
   deletes retain old painted bounds long enough to report damage.
3. Maintain a derived spatial index per composition, keyed by stable document
   identity. Evaluate Understory against a small native implementation; select an
   index from measurements, not from the tutorial's use of RBush alone.
4. Expose shared `hit`, `hits_in`, visible-set, and content-bounds queries. Spatial
   candidates still require precise shape tests, clipping, and painter ordering.
5. Represent embedded occurrences explicitly. One source node may appear through
   multiple embed chains and panels. Keep canonical composition-space bounds and
   occurrence placement/clip information separate; a `NodeId` alone cannot identify
   an on-screen occurrence.
6. Produce conservative old/new damage in the affected composition/view spaces.
   Include descendants moved by ancestors, stroke joins/caps, effects when supported,
   antialiasing margins, and resource changes. Preserve embed occlusion and the
   current distinction between shape hits and embed-background hits.

**Affected modules:** `src/document_bridge/mod.rs`, `materialize.rs`, `render.rs`,
`nest.rs`, `panel_views.rs`, `src/bevy_app/picking.rs`, and select-tool query callers.
Keep renderer-specific cache policy out of the document crate.

### Exit criteria

- Picking and fit-to-content work without first rendering the composition.
- Indexed queries agree with the existing full traversal on representative scenes.
- Move/delete/reparent damage includes old pixels and affected descendants.
- Two embeds of one composition return distinct reaches while sharing source identity.
- Pan/zoom/rotation, clips, nested embeds, and 2D-on-3D picking remain correct.
- Report candidates examined, bounds updated, nodes traversed, and query duration.

## 7. Phase 4 — Editor overlays independent of content

### Tutorial evidence and adaptation

- [RenderHighlighter](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/ecs/src/systems/RenderHighlighter.ts#L53) owns hover/highlight decoration as a separate ECS system.
- [RenderTransformer](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/ecs/src/systems/RenderTransformer.ts#L94) owns selection-transformer UI independently of authored shape data.
- [BatchManager](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/ecs/src/systems/BatchManager.ts#L113) retains shape draw-call resources; [MeshPipeline dirty handling](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/ecs/src/systems/MeshPipeline.ts#L1053) distinguishes geometry and material changes.

**Iron adaptation:** a separately invalidated overlay target that leaves content
raster counters unchanged is our proposed extension. Separate tutorial systems
alone do not prove separate retained raster targets or zero content redraw on hover.

### Work

1. Move hover/selection outlines, handles, marquee, and guides into an editor layer
   or retained target with its own invalidation. Preserve exact visual geometry and
   coordinate mapping across panels and embeds.
2. Route cross-probe highlighting through that editor path where it is editor
   feedback. Authored highlights, lesson annotations, and other document content
   retain their intended painter order and authored semantics.
3. Keep overlay bounds and revisions separate from document-content bounds.
4. Retain content textures when only editor feedback changes. Do not promise that
   an overlay frame avoids composition/presentation work; the goal is to avoid
   re-encoding and re-rasterizing unchanged content.

**Affected modules:** `src/document_bridge/highlight.rs`, `render.rs`, select/pen
overlay code, `src/vector/backend.rs`, and renderer target integration.

### Exit criteria

- Hovering and selecting leave content display-list and raster counters unchanged.
- Handles remain aligned during pan, zoom, resize, and embedded-view interaction.
- Cross-probe feedback updates every visible counterpart without rebuilding its
  unchanged content.
- Authored annotations retain correct ordering and clipping.

## 8. Phase 5 — Presentation chunks and bounded raster work

### Tutorial evidence and adaptation

- [BatchManager retained draw-call caches](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/ecs/src/systems/BatchManager.ts#L113) demonstrates retention below scene semantics, with explicit resource ownership.
- [VelloPipeline.renderCamera](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/plugin-vello/src/VelloPipeline.ts#L800) calls `clearShapes` at line 856 and gathers shapes again for content submission.
- [Vello camera-only update handling](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/plugin-vello/src/VelloPipeline.ts#L1527) explains why camera changes avoid resending all shapes across JS/WASM.

**Iron-origin work:** presentation chunks, damage-bounded raster jobs, tile/subtree
caches, zoom proxies, gutters, and eviction come from Iron's existing vision 07
and view-textures plans. The tutorial motivates finer retention boundaries; it
is not a reference implementation of this phase's cache architecture.

Implement this under roadmap H and vision 07, after the earlier measurements.

1. Give derived painter-contiguous chunks stable identity and separate content,
   placement, style/effect, and resource revisions. Do not equate semantic groups,
   chunks, and cache entries: they have different responsibilities.
2. Rebuild only dirty chunks. Keep command comparison as a temporary correctness
   check, rather than the primary way to discover unchanged output.
3. Use spatial damage to select all contributors to a dirty region in painter
   order. A moved object damages its old and new locations; rebuilding only that
   object's commands is insufficient to restore the pixels behind it.
4. Add bounded raster targets, world tiles, or isolated subtree images according
   to measured reuse. Preserve blend/clip/isolation semantics; arbitrary groups
   cannot always be replaced by independently composited textures.
5. Apply the existing zoom-bucket, proxy, gutter, memory-budget, eviction, and
   progressive-refinement policies from vision 07 and view-textures.

**Exit:** a local edit does not traverse or encode the whole document; clean cache
entries incur no raster job or upload; pan reuse and zoom refinement preserve
painter order and remove old pixels. Count every full-view fallback and measure
CPU stages, GPU work where available, allocation/upload bytes, and cache memory.

## 9. Conditional phase — Capability providers for external Models

### Tutorial evidence and adaptation

- [CapabilityRegistry](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/ecs/src/CapabilityRegistry.ts#L1) implements named providers, explicit selection on ambiguity, errors for unavailable selected providers, unregister functions, and disposal.
- [ResourceScope](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/ecs/src/resources/ResourceScope.ts#L1) owns idempotent cleanup and releases dependants before dependencies.

**Iron adaptation:** scientific input/output schemas, cost policy, input revisions,
and publishing only current results into derived state are additions required by
Iron's computation boundary. The tutorial registry itself serves image-processing
capabilities and does not implement that scientific invocation protocol.

Start with the first real external scientific computation, using the existing
one-method LLM transport abstraction as a precedent.

- Authored invocation data names a stable capability and typed inputs/outputs.
- Runtime configuration selects a named provider: local WASM, worker, remote
  service, or eventually native process. Separate portable invocation semantics
  from per-user endpoint/credential configuration.
- Multiple providers require explicit selection; a selected unavailable provider
  reports an error rather than silently changing computation.
- Providers declare cost, cancellation, and lifecycle. Results carry input
  revisions and invocation/session identity. Discard stale results.
- Publish results into derived state, never as ordinary authored property edits.
  Keep expensive invocations outside continuous manipulation and bound any
  in-process floor computation.

**Exit:** two providers satisfy one capability contract; changing inputs or the
session prevents an older result from publishing; unregister/dispose cleans up
pending work; document save/load does not serialize credentials or runtime output.

## 10. Conditional phase — DOM-hosted document nodes

### Tutorial evidence and adaptation

- [RenderHTML](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/ecs/src/systems/RenderHTML.ts#L25) tracks HTML/embed transforms, culling, editability, DOM creation, and removal.
- [HTMLPlugin](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/ecs/src/plugins/HTML.ts#L1) registers DOM hosting as a scheduled subsystem.
- [Lesson 29: embedding HTML content](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/site/docs/guide/lesson-029.md#L18) explains the HTML container, camera synchronization, culling, and interaction routing.

**Iron adaptation:** Rust publishes resolved hosted-node data to main-thread
Svelte, which owns keyed components, measurements, and handle-based edits. The
tutorial's direct DOM access cannot be moved unchanged into Iron's render worker.

Start with one native text/number control that genuinely belongs to a composition
or generated inspector, rather than migrating all Svelte chrome.

- Rust publishes a hosted-node projection: stable identity, resolved props,
  placement, clipping, visibility, and editability.
- Svelte uses keyed components from a host registry. DOM lifetime belongs to the
  main thread, including worker mode.
- Values return through semantic operations and handles; intrinsic measurements
  return as derived inputs to layout. Define feedback convergence and avoid
  repeated layout/measurement oscillation.
- Define focus, keyboard/IME ownership, pointer routing, and disposal. Preserve
  the known restriction that DOM content above the canvas cannot interleave
  arbitrarily with canvas painter order.
- Keep dockview's current chrome-layout ownership until a separate workspace
  migration is implemented. Hosted content layout does not imply two competing
  owners for dock rectangles.

**Exit:** one hosted control works in worker and main mode, edits through its
handle, follows view placement and clipping, and disposes without stale listeners
or replies. Locked values explain why, and focus does not leak canvas shortcuts.

## 11. Validation and boundaries

Use native document tests for gesture/operation semantics, protocol tests for
request ordering and disposal, and browser smoke checks for displayed placement,
input routing, and both runtime modes. Build on the existing run-iron workflow.
Performance claims require measurements on representative content; uniform-shape
fixtures alone do not cover bindings, constraints, embeds, or editor overlays.

Keep these decisions intact:

- One renderer-independent authored authority and one atomic operation/history path.
- Stable document IDs rather than durable Bevy entity IDs.
- Editable handles resolve to authored leaves or explicit solver/inversion policies.
- Svelte consumes projections and sends commands.
- Retained view textures are already implemented; this plan refines their
  invalidation granularity rather than proposing that migration again.
- Collaboration remains deferred. The tutorial's adapters demonstrate useful
  origin suppression and per-field synchronization, but do not solve Iron's
  selective undo, constraints, semantic relations, or collaborative identity policy.
  Evidence: [bindDocument and diffNodes](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/site/docs/collaboration/document.ts#L1) and [Yjs provider origins and field patches](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/site/docs/collaboration/yjs.ts#L10).
- Do not recreate the tutorial's graphics engine or 3D GPU cache where Bevy already
  owns equivalent resources. Borrow invalidation and ownership contracts at Iron's
  actual seams.

## 12. Source references

Each phase above links its motivating implementation and identifies extensions
that belong to Iron. Treat the linked symbols and tests as evidence of the specific
pattern, not proof that every proposed exit criterion already holds upstream.

Tutorial links are pinned to the inspected commit so later repository changes do
not silently change the evidence for this plan.

- [Bounds preparation](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/ecs/src/systems/ComputeBounds.ts)
  and [spatial culling](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/ecs/src/systems/ViewportCulling.ts).
- [Gesture context](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/ecs/src/systems/pick3d/gizmo-context.ts)
  and [preview ownership](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/ecs/src/systems/pick3d/gizmo-source-frame.ts).
- [Scoped UI store](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/react/src/store.ts),
  [actions](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/react/src/actions.ts),
  and [edit lifecycle tests](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/__tests__/ecs/api.edit.spec.ts).
- [Capability registry](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/ecs/src/CapabilityRegistry.ts)
  and [resource scope](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/ecs/src/resources/ResourceScope.ts).
- [HTML hosting](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/ecs/src/systems/RenderHTML.ts).
- [Mesh batching](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/ecs/src/systems/BatchManager.ts)
  and [Vello content submission](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/plugin-vello/src/VelloPipeline.ts).
- [Collaboration adapter](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/site/docs/collaboration/document.ts)
  and [Yjs implementation](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/site/docs/collaboration/yjs.ts).


## 13. Implementation checkpoint — change-driven picking and spatial foundation (2026-10-13)

The user authorized these three increments, after the SIMD baseline and cached
selection/hover geometry: (1) change-driven 2D picking, (2) independent bounds and
an index adapter, (3) indexed picking and per-view culling. This is a slice of
Phase 3 above, not completion of every phase in this document.

- **Picking invalidation:** cache the base 2D hits by physical pointer, current
  panel/view mapping, document version, projection generation, settled transform
  generation, spatial generation and reference/indexed mode. Embedded-view changes
  invalidate separately. Restore base 2D hits before the 3D pass, which can replace
  them with plane hits. The 3D ray pass still runs each frame.
- **Spatial authority:** `src/iron_spatial` is a native-testable adapter around
  `understory_index`, pinned to
  [`2ac7efbc010c8ee4daa316c34eadc0bafacfe83b`](https://github.com/forest-rs/understory/tree/2ac7efbc010c8ee4daa316c34eadc0bafacfe83b).
  Understory's recycled keys stay private; query results carry Iron `NodeId`.
  Bevy's propagated globals supply composition-space transforms. No box-tree
  transform hierarchy or renderer dependency was added to `iron_document`.
- **Bounds:** geometry, painted and interaction rectangles are distinct. Local
  stroke/miter and path-hit margins are transformed with the shape, including
  nonuniform/negative scale. Editor outlines are still in content, so painted
  bounds conservatively include their possible extent for now. Slider knob
  overhang and device-space antialiasing margins are covered too.
- **Preparation:** `document_bridge/spatial.rs` prepares visible and hidden source
  compositions after transform propagation, independently of render submission.
  An unchanged projection returns immediately. Working updates scan Bevy change
  flags and recompute only changed leaves, with embeds refreshed afterward.
  Structural edits/replacement rebuild the derived indexes and retain old damage.
  Pure style changes report damage even if their AABB stays identical.
- **Queries:** one R-tree per canonical composition, sorted by cached document paint
  keys once per query (one scope needs no merge sort). Sparse edits update
  individual entries; batches moving over half the entries in scopes of at least
  256 entries use Understory's packed bulk builder, with handles remapped privately. Picking retains precise geometry, embed-region clipping, opaque 3D
  occlusion and background-hit rules. Each embed occurrence retains its prefix
  chain and view transform while sharing its source's index. Rendering maps each
  view/occurrence's clip conservatively into composition space, then submits only
  painted candidates; exact clips remain in the display list.
- **Reference switch:** `await __iron.spatial(false)` uses the original exhaustive
  walks while bounds continue updating; `true` restores indexed queries. Its
  diagnostic result includes ordered hit chains for comparison. This switch is
  for testing, not persisted document state.

Tutorial motivation remains the pinned
[ComputeBounds](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/ecs/src/systems/ComputeBounds.ts#L82)
and [ViewportCulling](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/ecs/src/systems/ViewportCulling.ts#L17)
patterns. Iron's implementation uses canonical compositions and occurrence chains
rather than the tutorial's per-camera indexes.

**Still separate work:** migrate marquee/fit/selected-group geometry to shared
subtree summaries without changing their oriented group semantics; introduce
subtree dependency revisions; separate editor feedback into retained overlay
**targets**; form painter-contiguous presentation chunks; then measure selective
subtree baking using the existing offscreen-view path. `understory_box_tree` and
`understory_presentation` are not dependencies of this implementation. World-space
raster tiling and atlas allocation remain later, evidence-driven choices.

Measured outcomes and verification are in `render-benchmark.md` §7 and
`handoff-render.md`; the benchmark will also quantify maintenance cost when
all descendants move. Indexing reduces query work; it does not promise that a
large rotating subtree becomes constant-time.


## 14. Confidence tests before further caching (2026-10-13)

The owner's 1k interaction concern prompted display-paced build comparisons and
isolated Canvas2D/ThorVG replays of exported Iron geometry. Optimized SIMD restores
near-60 callback cadence locally, including selected group movement at 5K
backing; the owner confirmed smoother Helium interaction. Dev-opt was ~19 FPS.
Canvas2D supplies a useful uncached-raster reference, but these isolated controls
exclude the editor/document host and do not identify Bevy's share of the cost.

The standalone same-version Vello replay is now built;
`renderer-sanity-checks.md` §7 records the all-changing geometry tests. Vello
Hybrid can reach 60-Hz callback cadence for 10k independently moving/resizing
leaves here (~13.8 ms GL), but all-leaf outlines push it to ~36.5 ms without Bevy.
That identifies feedback rendering as its own bottleneck. Baking is not the only
remaining solution. Split local/subtree bounds and eligibility revisions from
placement, and retain the independent-overlay-target priority from §7. The
underlying motivation remains independent retained geometry from
[ComputeBounds](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/ecs/src/systems/ComputeBounds.ts#L82)
and independent editor feedback from
[RenderHighlighter](https://github.com/xiaoiver/infinite-canvas-tutorial/blob/ef879525f47cfdf2c4cab6366039cba41694d0fc/packages/ecs/src/systems/RenderHighlighter.ts#L53).


## 15. Upstream retained render-plan clarification (2026-10-13)

The new `vello-retention-and-render-plan.md` distinguishes a retained document,
prepared Vello scene snapshot, view texture, and incrementally compiled render
plan. Raster caching is one representation in that plan, not a switch that turns
Vello's drawing API into a mutable retained graph. The existing implementation
already retains whole views; a dirty target still rebuilds its visible content.

This is also an explicit upstream direction. In Andrew's January thread,
[Raph recommends Understory](https://xi.zulipchat.com/#narrow/channel/197075/near/570195961).
[Bruce's February explanation](https://xi.zulipchat.com/#narrow/channel/197075/near/574033580)
separates scene graph, imaging graph and render plan. Do not equate importing the
index crate with implementing all of that orchestration. The tutorial's dirty
bounds and independent feedback patterns remain useful components of this layer.
