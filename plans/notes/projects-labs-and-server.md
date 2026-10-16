# Projects, the lab twin, and a server

**Status:** Note — an idea under consideration, not a plan. Nothing here should be built
until a vision doc or spec adopts it. The open questions in §8 come first, and several
of them depend on which product Iron is meant to be (§7).
**Date:** 2026-10-15
**Context:** A design conversation about a project system, prompted by the
project-container task in [`../handoff-product.md`](../handoff-product.md) §4 and
[`../specs/clipboard-and-assets.md`](../specs/clipboard-and-assets.md) §6. Grounded in
vision 01 (§3, layer 1), vision 09 (entities, compositions, ports), the document spine
(§5.1 identity, §6 ops, §7 serialisation, §15 deferred), ECM §15 and
[`time-machines-and-scenarios.md`](time-machines-and-scenarios.md).

---

## 1. The tension

Iron is meant for pedagogy and for modelling research labs and experiments. Two pulls
point at different shapes of "project":

- **Project-scoped.** One project per experiment, thesis chapter, or lesson. Owned by
  one student or a small group, changing daily, forkable, its own history.
- **Model everything.** One model of the whole lab: every instrument, and everything
  that uses each one. Many users and many semantic projects in one workspace that acts
  as a digital twin of the laboratory, a "monorepo" of lab state.

A single shared document serving everyone breaks down: edits collide, one student's
change ripples into everyone's experiment, permissions and history have no natural
boundary, and the file grows without limit. Many separate projects lose the global
view: nothing can answer "what uses this laser?".

## 2. The reconciling idea

The tension comes from using one concept for two jobs:

- **units of meaning:** "everything about this laser", "this experiment", "this lesson";
- **units of ownership and change:** who edits it, how often, with whose approval, with
  what history.

A code monorepo works because these are separate: one namespace and cross-references
everywhere, while packages, owners, review and releases keep it from being one file
everyone edits. Figma does the same with team spaces, files and published libraries.

**Proposal:** draw document boundaries by owner and rate of change. Get the "model
everything" view through references and queries, not containment.

## 3. Two levels

### 3.1 Projects

Experiments, analyses, lessons. Each is a set of compositions (vision 09: schematic,
3D assembly, cartoon, scope) plus its model tree and lessons. Projects have states and
are forkable: branches, with a **master** branch.

### 3.2 The universal lab state (the twin)

Above the projects. It "contains", or references, existing projects, and specifically
their **master** versions, which represent how things are set up in reality right now.

Only some compositions in a project are **reflective** of the universal state: for
example a 3D view showing where equipment sits on the table is an explicit diagram about
the state of the world, and represents a sub-part of the lab's universal state. Other
compositions are project-oriented: story-like, ephemeral, or about something more
nuanced than what exists where.

### 3.3 A three-tier reading of the same idea

One way to make the two levels concrete (offered in the conversation, not decided):

1. **The lab workspace**: identity, people, and the curated library:
   - equipment *definitions* (a laser model, a fiber type, as ECM definitions);
   - the *facility*: rooms, and the specific physical instruments as *instances* ("the
     laser on bench 3", its calibration, its schedule).

   Changes rarely and deliberately, and has owners.
2. **Projects**, owned by a student or group, changing daily, forkable. They *reference*
   the lab's instruments rather than copying them, and keep experiment-specific settings
   as local overrides, like Figma component overrides.
3. **Compositions and views** inside each, as now.

"Everything that uses this piece of equipment" is then a **reverse-reference query**
across the workspace (which projects reference bench-3-laser?), not one document holding
everything.

### 3.4 What this would require

- **Library changes are published, and projects pin a version.** When the lab updates
  the laser definition, projects see "update available" rather than a silent change.
  This answers vision 09 §13.3 and ECM I2.
- **Stable identity across documents**: a document id plus a node id. The missing
  `doc_uid` (clipboard-and-assets §6).
- **Shared physical state belongs to the lab, not to experiments.** A real instrument is
  one object many projects observe. Projects hold intent and settings; the lab holds
  facts such as calibration and bookings.
- **Most work is single-owner.** Collaboration pressure concentrates in the slowly
  changing lab layer, which changes by publishing; live multi-user editing is rarely
  needed in project documents.

### 3.5 Pedagogy

A lesson is a project template. Students fork it; an instructor sees the forks. Real lab
equipment becomes teaching material by being referenced from the lab library.

## 4. The server

Iron will soon need a server. Import and load of JSON project objects stays supported,
but projects will quickly become large and hard to handle as JSON once they reference
heavy things: SolidWorks parts, videos, and so on.

**Organizations.** Users form organizations. Every member has access to the
organization's universal lab state and to its individual projects.

**One monolith to start.** A single server serves many organizations, each with its own
universal state and sub-projects.

Shape suggested in the conversation (§8 Q6–Q8 decide it):

- **Documents** stay canonical JSON of authored intent (spine §7); derived data (signals,
  solved values, obligations) is never stored.
- **Assets** live in a content-addressed blob store, apart from document JSON;
  documents reference them by hash, so a project's JSON stays small however heavy its
  media. This is the bundle shape clipboard-and-assets §6 already sketches
  (`document.json` plus `assets/<hash>`).
- **Transactions** are the unit of sync: the op log the spine reserves for
  collaboration (§15) is what a client would push or stream.

## 5. What the existing design already gives

| Need | Already in place |
|---|---|
| Stable identity | `NodeId`, never reused; the newtype leaves room for `(actor, counter)` (spine §5.1) |
| Sync and history | Every mutation is an op in a transaction; an append-only op log (spine §6–7) |
| Branching and diffing | Flat, canonical JSON; a reparent is a two-line diff (spine §7) |
| Small stored state | Authored versus derived: signals, solutions and obligations are never serialised |
| A thing versus its pictures | Entity versus figure (vision 09 §2): "the laser on bench 3" is separate from any drawing of it |
| A lab library | Definitions and instances (ECM) |
| Reference rather than copy | Embeds draw their source by reference (ECM §7.4) |

**Missing:** anything above one document. No document identity, no cross-document
references, no definition versioning (ECM I2), no durable storage (the app boots into the
demo; saving downloads one JSON file), no notion of user or organization.

## 6. Where to start, as suggested

Don't build the lab twin first. Build the single-experiment project, and make the seams
real from day one:

- a document bundle with durable local storage (OPFS);
- a `doc_uid`;
- definitions that can come from a separate library document by reference.

If that works for one student's experiment, the lab workspace is mostly a library
document plus a cross-document index added on top. If identity and references are
skipped now, the twin becomes a migration later.

## 7. Product questions behind the technical ones

Which product Iron is for changes which answers in §8 matter, and how soon. Candidate
first users, each pulling differently:

| First user | What they need first | What can wait |
|---|---|---|
| **An instructor building lessons** | Projects as templates, forking, publishing to students, assets, possibly public sharing across institutions | The twin, live data, merges |
| **One research group modelling its experiments** | Projects, a small shared library of its equipment, branches as "what if" setups | Multi-organization, approval workflows |
| **A lab or facility running a twin** | The lab layer, ownership of facts, approvals, history of physical changes, maybe live instrument data | Rich lessons |
| **Several collaborating labs** | Cross-organization references and sharing | Everything else |

Two questions sit across all of them:

- Is the twin a **model** of the lab (what exists and how it connects), or also a
  **record** (what happened, when, with which settings)? The record side pulls toward an
  electronic lab notebook and an append-only history per instrument, a different data
  shape. Suggested: keep it out of v1 deliberately (Q5).
- Is the buyer the person who uses it (a student, an instructor) or an institution (a
  lab manager, a department)? Institutions bring single sign-on, data residency and
  self-hosting requirements (Q8).

## 8. Open questions

Ordered roughly by how much each one decides.

1. **Who owns a fact such as "the laser is on bench 3, 40 cm from the isolator"?** §3.2
   has the universal state made from the master branches of projects, through their
   reflective compositions. Two projects can both place the same physical laser on their
   master 3D view: which is true? Iron's rule is one owner per value. Leaning: the lab
   layer owns reflective facts (instruments as entities, their placement), and project
   compositions depict them by reference, so a project never holds reality, only a view
   of it plus its own story. Or should reality be assembled from project masters?
2. **Is "reflective" a property of a composition, or of what is in it?** A project's 3D
   bench view will mix real equipment with hypothetical additions ("what if we add an
   amplifier here"). Per composition, one view cannot hold both. Per entity or figure
   (bound to a lab instrument or not), it can, and a proposed or ghost state shows
   naturally.
3. **Does merging to master mean "this changed physically in the lab"?** If so, the twin
   is a record of physical changes, merging needs approval (a PI or lab manager), and a
   project branch is a *proposal* to change the lab. Strong and useful, but it decides
   who has authority over what.
4. **Real merges in v1, or fork and propose?** Two students branching and both merging
   needs a three-way merge of a structured document. Op logs make it possible, but it is
   the hardest piece here. Branches as proposals, applied one at a time against the
   current master, avoid most of it.
5. **Is the twin "now" only, or historical?** An experiment's data was taken with the lab
   set up a certain way on a certain day. If an experiment can pin "lab state as of
   version N", reproducibility comes nearly free from versions and the op log. If the
   twin is only "now", that link is lost. This is where an electronic lab notebook
   begins (§7).
6. **Server: authoritative and live, or storage and sync?**
   - (a) Figma-style: the server holds the authoritative document; clients stream ops
     over a websocket; several people edit one project live.
   - (b) The server stores versioned bundles and assets; clients load, edit locally, and
     push transactions or snapshots; one editor per branch at a time.

   (b) is much smaller and fits branching. (a) is real-time collaboration, which the
   spine deferred (§15). Is simultaneous editing of one project needed soon?
7. **Heavy assets and CAD conversion.** Blobs by hash (§4) keep documents small. Who turns
   a SolidWorks part into something Iron renders (STEP or glTF)? Server-side conversion
   needs a geometry kernel (OpenCascade) and a job queue, a real piece of the server.
   Requiring users to export glTF pushes it onto them. Vision 09 §13.5, still open.
8. **Hosted only, or self-hostable?** One multi-tenant service is fine as a design. Must
   it also run inside one university's network? Universities often require on-premises
   or in-country data, and some lab data is export-controlled. This decides login
   (university single sign-on), storage backends, and whether the browser build must
   work against a local server or offline.
9. **Can projects cross organizations?** Collaborations between labs, and lessons
   published publicly: a lesson built on lab A's equipment used by a course elsewhere.
   If yes, cross-organization references need a sharing model. If no, organizations are
   hard walls, which is much simpler.
10. **How live is "reality right now"?** The scenarios note lists live instrument data as
    a possible driver. Is streaming instrument readings into the twin in scope
    eventually? That moves the server from storing documents toward handling time
    series, which affects its design even if built much later.

Related questions already open elsewhere, which this decision would answer or reshape:

- vision 09 §13.3 and ECM I2: where definitions live and how they are versioned;
- ECM I3: whether a 3D composition is in metres, needed before CAD and 3D assets;
- ECM I6: one undo stack per project versus per composition, which branches and multiple
  documents make concrete;
- clipboard-and-assets §6: the bundle, durable storage, document identity, and where the
  workspace is saved.

## 9. Appendix: SVG import and export

Discussed in the same conversation. SVG does not wait on the project system, except for
raster images embedded in an SVG, which need asset storage.

**Iron's 2D vocabulary today:** groups, rects with per-corner radii, ellipses with arcs,
paths of points with handles, fill, stroke, drop shadow, and a transform (position,
rotation, scale). No text, gradients, images, clipping, masks or dashes.

**Export, about 1–2 days.** A walk over a composition writing resolved values. A transform
becomes an SVG `transform`; equal corners become `<rect rx>`; unequal corners and arcs
become `<path>`; the shadow becomes a drop-shadow filter. Bindings and constraints flatten
to current values. Iron's own data can ride in `data-` attributes so an Iron→SVG→Iron
round trip keeps rects as rects.

**Import, about 3–5 days for the core**, with the `usvg` crate, which handles CSS, `<use>`,
units and nested transforms and reduces every shape to a path. Import is one transaction
of create ops, like paste; pasting SVG copied from Figma comes almost free. Decisions:

1. **Paths with holes.** An Iron path is one outline, so a donut or the letter "O" cannot
   be represented. A path needs several outlines plus a fill rule: the one change to the
   document format. Decide before import.
2. **Skew.** SVG transforms can skew; Iron's cannot. Bake skew into path points: easy,
   lossy.
3. **Primitives.** `usvg` turns rects and ellipses into paths, losing their radius and
   arc handles. Re-detect them, or read Iron's own `data-` attributes.
4. **The long tail.** Gradients, text, images, masks and dashes are approximated (a
   gradient becomes one colour) and reported, as shape-to-path conversion reports what
   it cuts. Each is a rendering feature of its own.
