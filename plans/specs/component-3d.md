# Components in 3D compositions

**Status:** Proposed 2026-11-06, not built; waiting on the user's go-ahead. Amends
[`component-model.md`](component-model.md) §5.1 (what a `use` stores) and the
registry. The agent verbs it would extend, `<place>` and `<make-component>`, are built
(component-model.md §14, phase 7).

## 1. What is missing (verified 2026-11-06 against `f84579d`)

- A `use` requires `transform2d` and cannot carry `transform3d`
  (`registry.rs`), so `definition::instantiate` and `place_figures` always make a 2D
  picture. `compose.rs` refuses a 3D `in` and points the model at `<link-figures>`.
- `group` has no `transform3d` either. A 3D composition today is a flat set of
  `mesh`, `camera` and `embed` nodes; the demo's assembly hangs meshes off groups
  whose 3D transform is the identity.
- Expansion (`expand.rs`) is dimension-agnostic: it copies a template's subtree
  under the `use`, whatever the node types. `geom::world_affine3` already composes
  every ancestor's `transform3d`. So the expansion and the transform math work as soon
  as a `use` can hold a 3D placement.
- The only 3D picture of an entity today is a plain mesh joined to it with
  `<link-figures>` (the demo's torus). It does not follow the definition.

## 2. Proposal

1. **A `use` is placed by its composition's dimensions**, as an `embed` is: both
   `transform2d` and `transform3d` become optional on `use`, and a `use` in a 3D
   composition has `transform3d`. Validation refuses a `use` with the wrong one for its
   composition (and one with neither). `group` gains an optional `transform3d`, so a
   template can hold a hierarchy.
2. **A template for 3D is a template with a 3D notation** (`notation="3d"` by
   convention, the composition's notation deciding as in 2D). Its children are meshes
   and groups. `template_of` is unchanged.
3. **Instantiation picks the transform**: `use_ops` takes the composition's dims and
   writes `transform3d` (pos, rot, scale) for 3D. `<place>` in a 3D composition takes
   `x y z`, or `pos="x,y,z"`, and `rot` as Euler radians, as `transform3d` does.
4. **Materialisation**: check that `document_bridge::materialize` spawns expanded
   meshes (compound ids `#u/7`) under a `use` and reads their world transform through
   the `use`. Selecting a derived mesh selects its `use` (the select tool's chain, as in
   2D).
5. **Ports and anchors** stay 2D (`empty` with `anchor` is 2D only). A 3D picture
   carries no anchors in this phase; connections are drawn in 2D compositions.

## 3. Phases and exits

- **3D-1, registry and placement.** Items 1 and 3. *Exit:* `tests/compose.rs` places
  one definition, with a `free` and a `3d` template, in a 2D and a 3D composition, as one
  transaction; the 3D picture's mesh has the world transform of `use` ∘ template mesh;
  save and load keep it; undo removes both.
- **3D-2, drawn.** Item 4. *Exit:* the Assembly tab shows an instance's mesh moving
  when its `use`'s `transform3d.pos` is set; an edit to the definition's 3D template
  reaches every instance; the smoke passes.
- **3D-3, the agent.** The manifest documents 3D `<place>`; a fixture puts one
  component in a 2D and a 3D composition.

## 4. Open questions

1. Should a 3D `use` take a uniform `scale` only, as a component's picture, or the
   full `transform3d`?
2. Does `make-component` from meshes in a 3D composition make a `3d` template and a
   main there, as from 2D shapes (component-authoring.md §2)? That needs a 3D main.
3. Whether a template should declare its dims, rather than infer them from its
   children's types.
