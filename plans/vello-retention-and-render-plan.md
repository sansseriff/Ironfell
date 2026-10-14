# Vello retention: current behavior and the missing presentation layer

Date: 2026-10-13. Research clarification, not a renderer migration.
Read with `renderer-sanity-checks.md`, `vello-hybrid-migration-scope.md`, and
`vision/01_layered-architecture-overview.md`.

For the fuller Zulip research and a source-audited implementation experiment,
read [the retained subtree renderer proposal](vello-retained-subtree-renderer-proposal.md)
(2026-10-14). It distinguishes the proposed outer frame planner from Vello's
existing internal raster-job scheduler.

## Decision

Iron already retains document state, derived data, complete Vello view scenes and
view textures. Its dirty-view rendering remains coarse: a changed target resets
and re-encodes all its visible content. Describe that as **immediate-style scene
recording with whole-view retention**, rather than claiming everything is
immediate or that raster caching would change Vello's API into a retained graph.

The intended next architecture is a retained presentation/render-plan layer above
the rasterizer. It determines dependencies, painter-ordered partitions, live
versus cached content, surface allocation and invalidation. Raster caching is one
representation available to that layer; it is not its definition or a substitute
for avoiding unrelated document/geometry work.

## 1. What the pinned API actually does

App version: `vello_hybrid 0.2.0`, published from
`7073a85d61ee099d1d3595651ddf92302b074348`. The local Cargo registry's `scene.rs`
and `lib.rs` were compared with that commit and match exactly.

- Calls such as `set_transform`, `set_paint`, `fill_path` and `stroke_path` record
  new draws. They do not address an existing document object by id.
- Filled paths are transformed and processed into strips during scene recording.
  Keeping an unchanged Scene retains that preprocessing result between renders.
- `set_transform` affects subsequently recorded draws. It is not a setter for an
  existing group's transform. `reset` clears strips/paints and recorded draws.
- The public Scene API has no per-object replace/remove operation or public
  `append` of independently prepared Hybrid scenes. A retained snapshot therefore
  differs from an incrementally editable scene graph.

Sources: [path processing at recording time](https://github.com/linebender/vello/blob/7073a85d61ee099d1d3595651ddf92302b074348/sparse_strips/vello_hybrid/src/scene.rs#L339),
[transform setter](https://github.com/linebender/vello/blob/7073a85d61ee099d1d3595651ddf92302b074348/sparse_strips/vello_hybrid/src/scene.rs#L910),
[scene reset](https://github.com/linebender/vello/blob/7073a85d61ee099d1d3595651ddf92302b074348/sparse_strips/vello_hybrid/src/scene.rs#L956).

Retaining the Scene does not imply all GPU preparation is retained. The renderer
still constructs a schedule and prepares/uploads data when asked to render it.
Sources: [renderer preparation](https://github.com/linebender/vello/blob/7073a85d61ee099d1d3595651ddf92302b074348/sparse_strips/vello_hybrid/src/render/wgpu/mod.rs#L440).

## 2. Iron's existing retention and its granularity

| Level | What Iron keeps | Limitation |
|---|---|---|
| Document | Stable nodes, hierarchy, geometry, properties | Retaining author intent alone saves no raster work |
| Derived presentation | Bounds/index and some selection/picking geometry | Some dependency stamps remain global; movement can cause descendant work |
| Vello Scene | One prepared scene per target | A dirty target resets/re-emits its visible layers |
| Raster output | One retained texture per view | One changed shape can invalidate that view's content |

Evidence: `src/vector/hybrid.rs` `prepare_scene` skips clean targets at lines
685–688, then resets dirty scenes at line 697 and emits their visible layers.
`TargetScene` holds its scene and rendered flag. Rendering skips finished targets
at lines 900–901 and 1092–1093. This is useful retention already; the missing
capability is granular update within a dirty target.

Example: moving B between unchanged painter-ordered A and C should preserve A/C's
prepared work. The current dirty-view path records all visible A/B/C again.
Raster caching could substitute images for A/C; a more capable backend could
instead retain reusable vector encodings or GPU geometry. Either way, partitions
and dependencies must preserve order and compositing semantics.

## 3. What the maintainers actually discuss

These are dated discussions and proposals, not guarantees that the capabilities
are implemented in 0.2.0 or any later release.

- **2026-01-26:** In Andrew Mueller's own hierarchical-scene-compilation thread,
  Raph Levien recommends Understory for a mostly retained document with minimal
  work per rendered frame. This directly connects the earlier Understory plans
  to the current question.
  [Raph's reply](https://xi.zulipchat.com/#narrow/channel/197075/near/570195961).
- **2026-02-15:** Bruce Mitchener distinguishes scene graph, imaging graph and
  render plan, including surfaces, cached tiles, compositor layers and specialized
  fast paths. He suggests Vello CPU/Hybrid should render subtrees into surfaces.
  The retained orchestration is a separate layer, not something the authors have
  failed to consider.
  [Bruce's explanation](https://xi.zulipchat.com/#narrow/channel/197075/near/574033580).
- **2026-03-11:** Raph describes a possible higher-level orchestration layer that
  selects pipelines per sub-scene and manages textures for static content.
  [Render-plan discussion](https://xi.zulipchat.com/#narrow/channel/197075/near/578760890).
- **2026-04 / May:** The old recordings API was removed because it was unused and
  complicated other work. Its authors explicitly leave open reconsidering it.
  Memory cost was discussed, not unanimously established as the sole problem.
  [Removal rationale](https://xi.zulipchat.com/#narrow/channel/197075/near/590147396),
  [Alex's qualification](https://xi.zulipchat.com/#narrow/channel/197075/near/592180594),
  [merged removal PR](https://github.com/linebender/vello/pull/1611).

This corrects the migration-scope headline's earlier claim that no upstream
cross-frame caching would ever be provided. The pinned API gap remains real.

## 4. Why retention is not just a missing setter

Hybrid's strips contain device-space coverage. Moving a shape fractionally,
rotating it or changing scale changes which pixels it covers and their alpha.
Reusing prepared coverage as if it were an object-space vertex mesh does not
preserve exact vector output. A bitmap bake can be transformed cheaply, accepting
resampling and a resolution policy; a retained vector renderer still needs to
invalidate transform-dependent preparation when necessary.

It can preserve local geometry, unaffected siblings and resources while doing
that necessary work. An immediate-style API does not prohibit internal caches,
and a retained API does not guarantee fast updates. The useful requirement is:
**a local edit should cause only dependent preparation to run**, with a backend
that can reuse the remaining work efficiently.

Low-level rasterization and retained orchestration can sensibly be separate
libraries. That choice nevertheless leaves real work for an editor developer:
Vello alone is not the complete retained compositor desired for Iron.

## 5. Implementation priorities

1. Keep author authority in `iron_document`; retain stable local/subtree geometry
   and eligibility by their actual dependencies, separate from placement.
2. Separate feedback into a retained target. Measure dense leaf-outline rendering
   independently: the matched 10k A/B already shows a renderer-only cost increase
   from ~14.1 to ~36.5 ms in Hybrid GL.
3. Introduce painter-ordered presentation partitions with explicit dependency
   revisions and render-plan entries. Their representation can be live geometry,
   a prepared backend artifact where supported, or a retained surface.
4. Add selective subtree surfaces using existing offscreen views, with explicit
   edit/live transitions, scale quality, memory budgets and invalidation. Keep
   fully changing content on the live path; do not describe this as making the
   Vello API itself retained-mode.
5. Validate sparse edits, group transforms, zoom, embedded occurrences and history
   with encode/raster counts and presentation traces. Consider a backend with a
   richer retained contract if the required live workload still misses budget.

Understory currently supplies indexing through `iron_spatial`. Bounds/query
infrastructure alone does not supply a complete render-plan compiler, GPU cache
or compositor. Evaluate the evolving higher-level crates against these concrete
requirements instead of importing all of them as a substitute for a design.

## 6. Reproduce the Zulip research

The JavaScript spectator client is read by the new, read-only helper:

```sh
bun run .claude/skills/run-iron/read-zulip.ts \
  'https://xi.zulipchat.com/#narrow/channel/197075-vello/search/retained' \
  'Blit rect pipeline'
```

It opens a fresh anonymous browser, captures the actual message endpoint, then
uses its public-channel narrow to read up to 1,000 matching messages. Bare REST
requests without the public-channel narrow return 401; no credentials are used.
An optional second argument selects a topic. JSON snapshots with message ids,
authors, dates and direct links are saved under `.agents/research/zulip/`.
The helper creates no account and sends no messages. It reads public discussions;
if a response reports limited history, treat the transcript as incomplete.
