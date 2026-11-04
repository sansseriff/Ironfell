# Authoring workflows: from a drawing to a linked, computed component

**Status:** design investigation requested 2026-10-28; first shared point-control
implementation built 2026-11-03 (§10), integrated into `master` 2026-11-04. The
broader workflow and panel design remains open. No new panels, APIs, schema or computational backend
are approved merely by their appearance here. The direction is agreed: make the
semantic operations behind the demos explicit and usable from the UI and model surface.

**Owns:** end-to-end workflow discovery and the next bounded authoring slice. Extends
[`component-authoring.md`](component-authoring.md), not a replacement for its built
mains/ports/links. Reuses [`component-model.md`](component-model.md),
[`entity-composition-model.md`](entity-composition-model.md),
[`panels-and-session.md`](panels-and-session.md), and [`contexts.md`](contexts.md).
[`interaction-lifecycle.md`](interaction-lifecycle.md) owns shared interaction and
command contracts. [`model-interop.md`](model-interop.md) remains a separate, unadopted
implementation proposal: discussing it here does not authorize its backend work.
Work order belongs to [`../handoff-product.md`](../handoff-product.md).

## 1. Why this work exists

The demos contain authored structures and computational connections whose creation
flows are missing or obscure. Packaging selected content is only part of authoring.
The user should be able to understand and build the dependencies that make an object
behave, create another representation of it, and inspect what remains unresolved.
Do not equate a runnable fixture or model macro with an available human workflow.

Use one real application to settle operations; use a second to test whether the
contracts generalize. First investigate an endpoint-driven fiber because it directly
exposes the missing steps between a simple path and a working component. Compare
multi-composition component placement and the gas bottle's computation loop. Nanowire
work with external computational Models is another candidate application, not a
requirement to implement a scientific stack in this pass.

## 2. Keep the nouns distinct

| Concept | Responsibility |
|---|---|
| Definition/component | Reusable interface, behavior and templates |
| Entity | A particular thing with identity and arguments |
| Representation/figure | That thing shown in a composition, often through a `use` |
| Composition | A 2D/3D world containing pictures and their authored placements |
| Context | Reusable rules governing interaction and presentation |
| View/embed | A composition presented in a panel, another 2D world or on a 3D plane |
| Computational Model/producer | Computes outputs from identified inputs; a producer publishes runtime values |

Two compositions can share entities and use different contexts. Two compositions
using the same context need not share entities. Therefore investigate "show this
entity elsewhere" and composition correspondence rather than an ambiguous blanket
"link contexts" command. Embedding another composition and drawing the same entity
in another composition are distinct workflows.

## 3. First investigation: make a fiber from a path

Start with a newly drawn ordinary path, not a standard fiber or generated demo. Write
a concrete interaction script with each click/menu/control, the operation it invokes,
and the authored versus derived data after it. UI form is a design question; avoid
inventing a large new inspector framework before walking the example.

The script must answer:

1. How does a person package the path into a definition and enter/edit its main? Verify
   current limitations rather than assuming the Make component label completes this.
2. How are its two ends identified and exposed? Distinguish a geometric endpoint, a
   port anchor, and a signal terminal. Are endpoint position and facing inputs, derived
   outputs of anchors, or a defined combination? Make ownership explicit.
3. How does the user declare that the path's end anchors follow those positions, and
   that its control handles or interior points depend on them? Express the mapping
   through normal bindings/geometry mechanisms; do not add a fiber-only updater.
4. Which geometry stays authored: an initial curve, endpoint offsets, bend parameters,
   free intermediate points? What can be dragged after the endpoint dependency exists?
5. How are connector direction and physical follow/plug behavior specified separately
   from signal flow? Connecting ports must not silently invent propagation equations.
6. If this is an optical fiber, how are signal ports, length/loss inputs and output
   behavior added and inspected? Do not assume drawn length equals physical length;
   distinguish geometry, units and an explicit mapping.
7. How is an instance placed, connected to two other objects, disconnected and edited?
   What follows when the partner moves? What happens when both ends are free or when
   a binding cannot be inverted? Explain each lock or editable source.
8. Can the component be reused, its definition changed, and an instance overridden,
   saved/reloaded and undone without losing identity or dependencies?

Packed paths are an explicit design constraint. Identify whether selecting an endpoint
for a persistent binding expands only the necessary geometry under today's mechanisms,
requires current whole-path expansion, or needs a future addressing primitive. Record
that trade-off. Do not silently defeat packed geometry for every imported path.

The deliverable is a causal diagram that a person can inspect: endpoint/anchor values
→ curve geometry; signal inputs and parameters → signal outputs; authored inputs versus
runtime/derived values. A component's authored behavior must be readable from its main
and interface, not reconstructible only from the demo source.

### 3.1 Empties and shared geometric controls — required fiber case

**User clarification, 2026-10-28:** the fiber may require several shapes: a main
stroke and a separate blurred/offset shadow stroke. Individual points of both must
follow common movable endpoint controls. A single-path example is insufficient to
establish the authoring machinery. Blur here describes the intended content; this
investigation does not authorize a new rendering effect implementation.

Use two endpoint controls, A and B. Each drives corresponding geometry in both paths:

```
control A → fiber start point + shadow start point (with a local offset)
control B → fiber end point   + shadow end point   (with a local offset)
```

This exposes a distinction between **structural ownership** and **transform/dependency
parenting**. Today path/contour child order defines geometry, and ordinary world
transforms follow structural parents. Moving a point under an empty would remove it
from the contour whose geometry reader enumerates it. One structural parent therefore
cannot simultaneously express contour membership and a shared external transform frame.
Do not promise that Wrap in empty already solves this case.

An empty is a candidate visible authoring handle/local coordinate frame. A port anchor
is an empty associated with an interface terminal, a separate role. A reusable rig
control need not create a full semantic `entity` or a port for every pivot. Conversely,
an actual component endpoint may expose a port using that same geometric control.
Define the minimum reusable concept before adding a new node kind.

Investigate three alternatives against the actual code: ordinary bindings from points
to control-frame values; an explicit attachment relation that derives those bindings;
or a transform-parent reference independent of containment. Record which mechanism is
needed beyond what exists. Prefer one shared dependency mechanism over a second
transform authority, a multi-parent containment tree or fiber-specific propagation.
A user-facing attachment action may author normal bindings even if no new persistent
relation is needed. Decide that from the workflow, including detach and inspection.

The proposed authoring action is "attach these geometric features to this control,
keeping their current placement". The UI must allow selecting points on different
paths while keeping their contour membership, choose the common control, display the
attachments and inspect or edit offsets. Do not substitute grouping whole paths: each
endpoint influences only part of each path. A shared centerline with a derived shadow
is a useful alternative, but must not hide the need to control independently authored
shapes and points together.

Settle the following in the interaction script and dependency diagram:

- Placement and offsets are expressed in named coordinate frames, with the necessary
  conversion into each point's path space. Test different translated, rotated and
  scaled path parents; copying raw x/y values is not general attachment.
- Define whether attachment follows position only, position plus orientation, or the
  whole frame, and what happens to tangent handles versus point positions. A shadow
  offset may be local to the fiber or fixed in the composition; this must be explicit.
- A control move previews all attached geometry in the same frame and commits one undo
  step. Directly dragging an attached point must have a defined result: move the
  control, edit its local offset, detach, or refuse with the controlling source named.
- Binding/attachment cycles are rejected. Moving the whole component applies its
  transform once; definition editing, nested instances, overrides, copy and reload
  preserve control references and offsets.
- Detach preserves the currently visible geometry. Deleting a control must not silently
  destroy the paths or leave broken references: choose an explicit supported policy,
  present its impact through the shared operation planner, and verify undo.
- Packed path feature addressing and any required point expansion follow the trade-off
  recorded above. Do not turn every anchor into a node just to attach two endpoints.

**Acceptance for the design investigation:** show how a user constructs, inspects,
moves and detaches A/B for a fiber plus a separate shadow without code or LLM-only
steps. Identify today's missing operations honestly. Include one middle control that
influences both curves if endpoint-only control would conceal a limitation. The next
implementation slice must name the chosen attachment representation and ownership
rules; this plan does not preselect one.

## 4. General operations to investigate

Names here are provisional. For each, inventory existing store/model support, identify
missing shared semantics, and specify the minimum human entry point.

| Operation | Questions the workflow must settle |
|---|---|
| Create/edit a definition | Main entry/exit, exposed inputs, endpoint/part targeting, behavior authoring |
| Create entity / place representation | Can identity be created without a placed picture? How are defaults chosen? |
| Show entity in another composition | Choose destination and notation; use the same entity, not a duplicate |
| Create/open/embed composition | 2D in 3D: plane placement and picking; 3D in 2D: camera/framing and viewport size |
| Connect ports / realize connection | Create topology separately from its wire, path or other visible picture |
| Link existing pictures | Identify subjects, preserve or merge identities deliberately, show affected representations |
| Inspect missing representations | Unplaced parts and unrealized connections, with an explicit intentional-omission action |
| Remove picture / delete subject | Shared operation planning and scope warnings from the interaction plan |
| Attach geometric features to a control | Points across several paths share a frame; preserve contour ownership, offsets and explicit detach behavior |
| Bind a control to an input | Select a target leaf/input, mapping, bounds and refusal/lock explanation |
| Attach/run computation | Named inputs/outputs, lifecycle and accepted output revision; no hard-coded scene names |
| Freeze/commit result | Deliberate authored transaction, distinct from ongoing runtime publication |

For representation creation, distinguish derived symbol geometry from authored position,
routing, camera and annotations. Repeating "show here" should not silently create a
second entity, duplicate the same required occurrence or reset a person's layout.
Some applications allow several occurrences intentionally; define that choice rather
than enforcing one universal representation count.

## 5. Related compositions and unresolved work

Walk one component from schematic creation to an unplaced board representation and
then placement. This is a workflow probe, not authorization to build a PCB application.
Show which entity is shared and which placement/geometry belongs to each representation.

Review ECM §8's planned obligations, tray, ghost lines and `omits`. Identify what can
be reused and what is missing. A requirement to represent an entity must arise from an
explicit participating design/scope and representation role; sharing a context is not
sufficient. Not every entity needs a picture in every composition.

Distinguish missing pictures from missing semantics. A connection may exist while its
wire is unplaced; conversely a drawn line does not necessarily establish a connection.
A notation may depict a connection implicitly. The inspector/model should explain
these cases rather than count every absence as an error or invent duplicate content.

## 6. Computation workflow: the gas bottle as evidence

Trace the existing loop before proposing a general UI:

```
authored stroke input → bindings → resolved chamber/piston poses
  → Bevy gas simulation → published pressure → bound readouts/pictures
  → explicit freeze commits a value, when requested
```

`document_bridge/gas.rs` currently discovers scene targets by name. The general workflow
needs explicit input/output bindings and an inspectable lifecycle. Identify which
inputs are parameters, which are geometry snapshots, which outputs are scalar signals
and which are bulk renderable data. Runtime particles do not become authored nodes
merely because they are visible.

Specify start/pause/reset/stop/freeze meanings; clocks, visibility/run modes, revision
handling and output ownership; and what happens to a running computation after input
edits, load, undo or producer failure. Do not claim that arbitrary feedback cycles are
solved by the reactive graph. A dynamic simulation may need explicitly ordered steps
and initial state, unlike a one-way binding or an algebraic constraint network.

For a proposed external nanowire workflow, describe one input geometry/parameter snapshot,
one job or live computation, its result representation and any explicit commit/export.
Keep scientific algorithms and backend adapter choices separate from the general
input/output/result lifecycle. No PHIDL/SuperScreen integration or external-model
implementation is part of this design investigation.

## 7. Authored controls and pedagogical contexts

Explain the slider to an author in three steps: "this part receives a drag", "the drag
changes this input through this mapping", "these pictures read that input". Compare a
slider, ring and endpoint-driven curve using the same vocabulary. Existing sliders
already participate in the general policy direction but still have a specialized
mechanism; a universal context-switching system is not already implemented.

Investigate an explicit **application scope** for contexts, separate from the context
rules and from the component's content. Begin with a composition/container or a named
lesson stage: which rules apply, to which occurrences, for how long, and why? Preserve
bounded resolution and named roles. Do not introduce arbitrary overlapping spatial
selectors as a prerequisite for authoring a control.

A later time/space scope must name its clock or coordinate space, precedence and entry/
exit behavior. It must also specify what happens to an acquired gesture when the scope
changes, using the interaction plan's ownership rules. The user wants unusual teaching
interactions to remain possible, with discoverable effects.

Record explanation requirements for cross-composition restrictions: an input limited
elsewhere should name the limiting source and allow navigation/highlighting. Warnings
and visual anchors are future presentation options; the complete limitation/callout
system needs a later design. Do not bury its source facts in UI-only tooltip strings.

## 8. First design deliverable: decision inventory and proposed UI

**Direction clarified, 2026-10-28:** proceed from the existing fiber-plus-shadow
example. The user does not need to specify a complete fiber or brainstorm more
features before this investigation can begin. Surface missing decisions through a
concrete proposal, then ask for preferences where alternatives materially change the
result. More examples are useful later as tests of the proposed machinery.

For every authored fact/reference in the finished example, trace where the demo
programmer supplied it. Classify it as inferred from current context/selection, an
engine invariant, a user choice, or a derived value. Only actual unresolved choices
need to be requested from the person. Record these inventory columns:

| Decision | Already known / proposed default | How supplied | Authored operation / result | Validation and cancel/undo | Open preference |
|---|---|---|---|---|---|
| Component owner | Open main, when available | Existing scope or explicit target pick | Definition/template edit | Refuse incompatible scope; show owner | Entry when no main is open |
| Endpoint control | Selected empty, or a new control | Canvas selection/placement | Create/control assignment | Preserve placement; cancel without partial attachment | Meaning of free versus connected endpoint |
| Affected geometry | Selected points across fiber and shadow | Guided canvas picking | Attach features while retaining contour ownership | Highlight all targets; reject cycles | Which tangent/interior features also follow |
| Offsets and frame following | Keep current visible positions | Inspector fields plus canvas handles | Authored mapping/bindings or attachment data | Explain coordinate space; detach preserves geometry | Position/orientation/scale and shadow-offset policy |
| Port interface | Known definition and chosen endpoint | Compact inspector form | Create/expose port and associate anchor | Validate kind, units and compatibility | Connector and follow policy |
| Value-to-picture mapping | Selected input and visual property, if available | Pick source/target; preview range mapping | Authored binding | Show existing driver, limits and inversion/refusal | Mapping shape and range |
| Reuse | Current definition and destination composition | Place an instance on canvas | Entity/representation placement | No accidental duplicate subject; undo/reload | Initial placement defaults |

These are starter hypotheses to refine, not approved UI or attachment semantics. The
inventory must distinguish a completed operation from a caption/button that conceals
an unavailable capability. Do not ask authors to enter values the engine can derive,
and do not invent unexplained defaults merely to eliminate a real choice.

### 8.1 Proposed presentation model to sketch

Separate context, workflow and presenter. Context offers named capabilities and
supplies defaults/eligible targets. A workflow tracks known inputs and remaining
decisions, invoking shared semantic operations. Svelte and canvas tools present those
decisions. The same operation can complete immediately from a suitable selection or
enter a guided picking flow when its targets are not yet known.

Sketch a persistent authoring inspector and temporary canvas instructions as the
initial hypothesis: enter the main; place/select control A; select points on both
paths; attach and inspect offsets; expose the endpoint port; repeat for B; place and
test an instance. Use direct manipulation for position/facing/geometry, compact forms
for names/kinds/ranges, and a focused side panel when several dependency choices need
explanation. Choose a covering dialog only when its task benefits from that treatment;
do not assume a wizard or modal is required for every multi-step action.

For each sketch, show what remains visible/selectable, current workflow state, Back/
Cancel/Finish behavior, committed versus provisional edits, stale-target handling, and
how the workflow resumes after picking on the canvas. Decide transaction boundaries
per semantic action; do not automatically make the entire authoring session one giant
transaction or save every pointer movement as an op.

This does not introduce an arbitrary document-authored workflow language or dynamic
Svelte code. First identify repeated decision/picking patterns worth expressing as data
through registered capabilities. Preserve the distinction between an authored context
and the engine mechanisms capable of carrying out its offered actions.

### 8.2 Review and stop point

Present the inventory, an annotated UI walkthrough and the dependency/ownership diagram
for the fiber-plus-shadow case before broadening into all other journeys. Recommend
one smallest complete implementation slice and list consequential unresolved choices.
Ask for concrete preferences with visible alternatives, not a request that the user
specify the whole component from scratch. This first deliverable is sufficient to
review the approach; later journeys test its generality.

### 8.3 Reactive panels, source views and workflow targets

**Design discussion, 2026-10-28:** include the default inspector and a proposed
composition layers panel in the UI sketches. The user questions whether the document
markup panel is still a useful default; a layers stack for 2D compositions is a candidate
replacement, not an implemented or finalized workspace change. There is no privileged
"main" viewport: multiple views may show different compositions or occurrences of one.

The existing panels/session plan already proposes an inspector generated in the session
and bound to its target, with follow-selection or pinned targeting. The current HTML
inspector is narrower: `SelectionView.svelte` subscribes to `doc.selection[0]`, and
`engine_api::document_inspect` resolves section order from that node's selection reach/
source view, falling back to its composition. Svelte still implements section widgets
and eligibility checks. These are reactive host subscriptions, not yet an authored
inspector composition using ordinary document controls.

Investigate an explicit panel-source contract: follow the current editing view, follow
a particular view's selection, pin an object/occurrence, or present an active workflow.
An occurrence includes its view/embed chain; node identity alone does not tell us which
context or placement is being inspected. Separate keyboard/UI focus from the source
being inspected: clicking a property field must not make the inspector follow itself.
Pointer hover alone must not silently replace an acquired workflow or edit target.
Existing per-frame global selection is not a completed per-view selection system.

**Required occurrence example, 2026-10-28:** selecting the schematic representation
of a resistor and selecting its representation in a 3D assembly refer to the same
entity but must be able to produce different panel UI. Shared resistance/designator
values still belong to that entity; symbol placement, wiring actions and pin
presentation belong to the schematic occurrence/context, while 3D placement and
assembly/mate actions belong to the 3D occurrence/context. This is a design fixture,
not a claim that a resistor's 3D authoring workflow is already built. Resolve panels
from entity + selected representation/occurrence + source view/open scope/context,
not entity type alone. Keep the target level of each field/action inspectable.

Treat source resolution, context policy, target capabilities and presentation as distinct
inputs to a derived panel description. Resolve availability and operations through
shared engine/query mechanisms, then let Svelte host the controls. Preserve the route
toward authored panel templates and session projections; do not make the host's own
conditional rendering the only source of meaning. Set-valued selection queries and
aggregation are reactive computations, not automatically expressible as one scalar
`Expr` binding. Identify the minimal common machinery rather than assuming it exists.

Define multiple-selection behavior explicitly. A section can require all targets, apply
to a compatible subset, inspect an explicit primary target, or group targets by
capability. Choose and label its policy; never silently edit the first selected object
while appearing to edit all. Distinguish an absent property, a mixed value and a locked
value. Shared editing must plan its target set, handle source inversion, deduplicate
writes to shared sources, validate atomically and explain partial eligibility. Different
representations of one entity may share semantic parameters while retaining distinct
local appearance/placement; aggregate at the intended level.

For workflows, sketch a stable authoring area/task section rather than assuming the
entire inspector is replaced on every selection change. Capture the workflow owner and
source occurrence explicitly. Picking an endpoint or property may update workflow
inputs without changing its owner. Show what the inspector will edit, how to inspect
another object deliberately, and how Cancel/Finish restores normal following. Compare
a side-panel tab/section, a second dock view and temporary pinned targeting; these
are presentation alternatives to review, not mandates for a single global workflow.

For the layers candidate, make its source the chosen composition/open scope, with
selection/highlight synchronization through the same session data. A row is a projection
of a document object, not a duplicate object; its edits use shared select/reorder/remove
operations. Distinguish the drawing hierarchy from the semantic entity/connection
structure. Source following and pinning should work for both inspector and layers.
Specify which panel-source/pinning state is session-only versus workspace-persisted;
reusable authored templates and derived inspector instances have different lifetimes.

The first UI deliverable must demonstrate two composition views, clicking inside the
inspector, mixed selection, an explicitly pinned target and the fiber workflow's target
picking. Record current limitations and recommend a bounded source/aggregation contract
before implementing a general authored panel system.

### 8.4 Workflow discovery and a reusable workflow presenter

**User clarification, 2026-10-28:** the supplied SolidWorks Mate example illustrates
an action starting a workflow and a side panel becoming its task UI. Explore that
separation for Iron, without copying a fixed permanent tab for every workflow. Iron
should accommodate a growing catalog and eventually user-authored workflow versions.
The host location and tab arrangement remain design choices.

Separate four responsibilities in the first UI proposal:

| Responsibility | Contract to design |
|---|---|
| Workflow definition | Stable identity, title/description, applicability, required inputs, supported steps/operations and explanation data |
| Discovery/launch | Context/selection resolves eligible workflows; search/list/contextual actions can launch them with known inputs populated |
| Workflow invocation | A session instance with an owner and source occurrence, acquired targets, missing inputs, progress, preview and completion/cancellation state |
| Presenter | Reusable task area renders the current invocation through supported controls/pickers; it is not a new permanent tab type for each definition |

These are proposed responsibilities to validate, not a new approved workflow language
or final registry schema. Context may offer/filter workflows and provide defaults;
a shared capability check still validates current targets and invocation inputs.
Authored versions must invoke the same semantic operations as built-ins. Do not solve
extensibility by requiring a new hand-coded Svelte tab or arbitrary runtime Svelte
execution for every authored workflow.

Sketch one reusable workflow/task host with a title, description of affected subjects,
remaining decisions, target-picking controls, preview and explicit completion/cancel
controls. Let the invoked definition supply its content through supported presenters.
A stable Tasks/Authoring category or a dedicated dock view is compatible with this;
an ever-growing permanent tab strip of Mate/Fiber/Port/etc. is not the default design.
Do not let starting a workflow silently discard a pinned inspector or lose the layers
view: show how the person returns, compares ordinary properties or opens another view.

Compare discovery through a searchable action list, contextual actions and component/
notation-specific offers. The operation should launch the same invocation regardless
of entry point. Test a large hypothetical workflow catalog and two authored variants;
show meaningful names, applicability and unavailable-action reasons without designing
hundreds of individual tabs. Discovery labels are data; an author-defined definition
still needs supported executable mechanisms, not just a name or ID.

Specify whether another launch replaces, suspends or runs alongside an invocation;
which unfinished inputs/targets survive switching views; and what happens when a target
is deleted or the source context changes. Do not impose one global active workflow
merely because one default task area is visible. Distinguish persistent reusable
definitions/workspace host placement from ephemeral invocation state and authored
transactions. A workflow panel's own focus must not replace its captured source context.

**Additional review examples:** show an action launching a Mate-like workflow from a
3D occurrence, the fiber attachment workflow using the same presenter contract, and
schematic versus 3D inspection of the same entity. Include a user-authored workflow
variant as a design sketch; explain what existing primitives suffice and what new
capability is missing. Pair the screen sketches with session/operation state so the
proposal demonstrates extensibility rather than only relocating bespoke forms.

## 9. Deliverables and exit for the investigating agent

1. First produce §8's decision inventory and annotated proposed UI for the fiber case,
   including §8.3's occurrence-aware panels and §8.4's scalable workflow presenter.
   Inventory the operations above as built, model-only, demo/code-only, partially built,
   or absent; support the distinctions with code and an attempted user journey.
2. Produce the fiber-plus-shadow interaction script, shared endpoint/control attachment
   dependency diagram and empty/control ownership decision, plus small sketches of
   the author-facing controls. Identify ambiguity and offer concrete alternatives.
3. Walk one 2D↔3D embedding journey, one shared-entity representation journey and the
   gas computation journey. Explain how the same operations enable each.
4. Recommend one bounded implementation slice with acceptance criteria. Separate shared
   machinery from domain-specific content and explicitly list what it will not build.
5. Record decisions still requiring the user's preference, including UI form, curve
   parameters, representation requirements and any new computation/lesson contracts.

The agent may inspect and prototype reversible workflow sketches. It should not
silently implement the entire table or reopen settled transaction, component, context
locality and rendering contracts. Implementation slices become concrete after the
investigation; their work order is recorded in the product handoff. Record completed
investigation here and in roadmap Status without marking unbuilt UI complete.

## 10. Shared point controls — first implementation, 2026-11-03

The user resumed this work after reviewing the October 18 and October 28 chats and
explicitly requested implementation in an isolated worktree. Branch `fiber-authoring`
is based on `0d8e416`; concurrent uncommitted timeline work in the main checkout is
not part of this slice. This implements the shared geometric-control and port-exposure
journey; it does not mark the general workflow presenter or §8.3 panel system complete.

### Decisions and ownership

| Decision | Implemented choice | Author supplies |
|---|---|---|
| Owner | A main component, captured when the task starts | Select the main or one of its parts |
| Control | An existing empty inside that main | Choose an empty; the existing Empty tool creates one |
| Geometry | Points from any number of the main's paths, retaining contour parents and order | Collect canvas points or pick numbered points in the task |
| Placement | Preserve every point's current position when attaching | No manual offset required |
| Frame | Offsets in the control's local 2D frame; rotation and scale carry positions | Editable X/Y offsets |
| Tangents | Keep existing handle vectors in the path's frame | Edit handles separately with the existing point tool |
| Existing driver | Refuse an already bound/runtime-driven position or existing attachment | Explicitly detach/remove its driver first |
| Port | Add a port spec and anchor facet to the same empty, default loose-end behavior | Choose input/output, then kind/connector/signal in the existing inspector |
| Detach | Bake resolved position in the point's path coordinates, remove attachment | Detach one point or all points on a control |
| Delete/reparent | Refuse deleting a referenced control or changing an attached point's transform ancestry without detaching | Detach first; deleting the whole rig is valid |
| History | Each Attach, Detach, exposure and offset edit is one document transaction | Pending picks and Cancel create no history |

The containment tree remains `template → path → contour → point`, with sibling
empties. `point.attachment` records the control reference and numeric `dx/dy` offsets.
The attachment operation writes ordinary `transform2d.x/y` bindings: control-local
offset → composition coordinates → point-parent coordinates. It reuses the same
expression builders as port anchors, not another transform evaluator or a fiber
producer. Instance expansion rewrites control and point references normally.

A control need not be a port. Exposing it later retains its identity and every
attachment. Signal kind, connector compatibility and output equations remain separate
from geometric follow behavior. The standard optical transfer expression can still be
entered in the Port value field; no fiber-specific signal editor was added.

### Walkthrough

1. Draw the fiber and a separate shadow path. Make them a component; enter its main
   and place two empties using the existing Empty tool. Name them endpoint A/B.
2. With the main or one of its parts selected, choose **Attach points to control** in
   the inspector. The task captures the main and remains open as selection changes.
   Ordinary properties remain below it rather than being replaced.
3. Choose endpoint A. Collect its corresponding points across both paths. Canvas
   point editing offers **Collect canvas point**; the path list also offers numbered
   point buttons, so points on overlapping strokes remain individually addressable.
4. **Attach 2 points** preserves placement and commits once. Pending picks do not
   move geometry. The Controlled points list shows each dependency and its offset.
5. Repeat with endpoint B. Move the empties: both paths respond through the graph.
6. Expose A/B as input/output ports. Select the main/empty to edit port kinds and
   connector properties in the existing inspector. Duplicate the main to obtain an
   instance, then plug its ends into compatible components using existing port drags.
7. Edit offsets, detach individual points, or detach all from a control. Undo restores
   the attachment. **Done** or **Cancel pending picks** closes the task; previously
   applied actions remain normal undoable transactions.

For a packed path, **Enable point attachments** explicitly expands only that chosen
path using the existing whole-path expansion operation. It is its own undoable step;
there is no automatic expansion of other imported paths or new packed-point addressing.

### Boundaries and follow-ups

This is one focused task presenter using shared document operations, not the proposed
catalog of authored workflow definitions. It pins a main identity; per-view occurrence
pinning, mixed-selection aggregation, layers, workflow search and simultaneous task
invocations remain open. Point positions follow rotation/scale but tangent vectors do
not. Automatic bend-distance mappings, tangent-follow policies, new blur effects,
3D controls and nested-instance attachment authoring remain open. Geometry is authored
on a main; placed instances consume it and can move/override their endpoint controls.

Store APIs live in `attachment.rs` and `author::expose_control`; the existing `author`
command exposes `attach_points`, `detach_points`, `expose_control`,
`expand_path_points`, and the read-only `attachment_info` query. The Svelte task uses
Iron's Bits UI wrappers and theme tokens, retains pending point IDs explicitly, checks
stale targets on refresh and clears on engine/document session replacement.

Validation: `tests/authoring.rs` covers shared controls, frame following, contour
ownership, tangent preservation, detach/undo, deletion/cycle and unsafe metadata-removal rejection, instance
expansion, save/reload and a plugged laser moving both paths without an unplug jump.
The `attachments.ts` browser fixture exercises both endpoints through the task UI,
port exposure, offset editing, detach/undo and canonical reload with task reset, and captures light/dark
screens. Full regression results are recorded in the run-iron skill.
