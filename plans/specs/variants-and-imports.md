# Component variants and imports

**Proposed 2026-11-08.** Waits on the user. Settles the syntax now; building waits on
narrative moves (narrative-panel.md) for variants used by lessons, and on the server
(`notes/projects-labs-and-server.md` §4) for remote libraries.

## 1. Decisions at a glance

| # | Decision |
|---|---|
| V1 | A lesson that changes a component makes a **variant**: a definition that `extends` another and stores only the differences (part overrides, added parts, added or changed states, transitions, tweens). It lives in the lesson's library. |
| V2 | **Structure never lives in narrative text.** The narrative holds references, moves (`#p1.vis → compare`) and edited progress bands (machines.md §4.2). Anything reusable or structural is a variant. |
| V3 | **Imports are declared, pinned and copied in.** `<import from="iron://owner/package@range" …/>` resolves to an exact version and a content hash, recorded like a lockfile; the definition is copied into the document with that provenance (as `std:` components already are). Updating is explicit, with a diff. |
| V4 | Entering a component from a lesson asks Figma's question: **edit for this lesson** (makes or edits the variant) or **edit the original**. |

## 2. Variants

```xml
<library name="lesson: two slits">
  <definition name="Plot · two-slit" extends="plot">
    <override target="series 1" stroke.width="3"/>
    <add parent="area"><text name="note" x="8" y="8" content="d/a = 4"/></add>
    <machine name="vis">
      <state name="compare" fade="1" reveal="1" lift="0"/>
      <transition name="sweep" from="shown" to="compare" ms="1500">
        <tween channel="reveal" at="0" ms="1500" ease="in_out"/>
      </transition>
    </machine>
  </definition>
</library>
```

- `extends` names a definition (local, `std:`, or an import's name). The variant's
  ports, template and machines are the base's, then its own changes in order: overrides
  of base parts by name or id (component-model.md's override, at the definition level),
  added parts under named base parts, machines merged by name (states and transitions
  added, or replaced by name).
- It is expanded like any definition; instances of the variant are linked instances.
- **When the base changes** (an import is updated, the original is edited), the variant
  re-applies its changes; any that no longer apply (a part or state that's gone) are kept
  as stale and listed, as instance overrides are today.
- A variant can be **flattened** into an independent definition (detached) when the
  author wants no further updates.

For the agent: `<variant of="plot" name="Plot · two-slit" as="$v"> …edits addressed to
the base's part names… </variant>`, then `<place def="$v" …/>`.

## 3. Where variants live

Each narrative may hold a `library` child: **the lesson's library**. Its definitions are
visible to that narrative's compositions and moves, and to nothing else unless exported
(moved to the document library). This keeps one model teachable by several lessons
(narrative note §6.7: "the model never holds the lesson") while letting a lesson
customise freely.

## 4. Imports

```xml
<imports>
  <import from="iron://std/plot@^1" as="plot"/>
  <import from="iron://physics-lab/optics@1.4" names="laser fiber mzm"/>
</imports>
```

- Address: `iron://<owner>/<package>@<semver range>`, then a component by name (`names`)
  or the package's default (`as`). `std` is the built-in owner: today's `std:<key>`
  components become `iron://std/<key>`, resolved locally with no server.
- Declared at the top of a document or a lesson (a narrative's `imports` child). A
  lesson's imports are visible to the lesson only.
- Resolving writes the **lock**: the `import` facet already has `source` and `version`;
  it gains `hash` (the content hash of the definition as published). The copy is the
  definition as fetched, marked with that facet; documents work offline and never change
  underneath their author.
- **Update available**: when the library publishes a newer version within the range, the
  Library tab offers it, showing a diff (ports added or removed, parts changed); applying
  it is one transaction, undoable, after which variants re-apply (§2).
- Names: an imported component is referred to by its import name (`plot`,
  `optics:laser` for a `names` import from a package aliased `optics`) in `<place def=…>`,
  `extends=` and the Library tab.

## 5. Open questions

1. Scope identity: an import's copy could instead stay a cross-scope reference
   (scopes.md §3.2) rather than a copy. Proposed: copy (offline, immutable) in v1.
2. Publishing: who can publish to an owner, and review (projects-labs note §3.4).
3. Assets that travel with a component (a mesh, an image): with the package, cached per
   scope (scopes.md phase 5).
