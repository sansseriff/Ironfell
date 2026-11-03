# The narrative panel (outline)

**Status:** Outline 2026-11-02; **decisions taken by best guess 2026-11-03** (§7), with
the user away, and **built 2026-11-03** (§8).
**Background:** [`../notes/narrative-layer-and-storage-primitives.md`](../notes/narrative-layer-and-storage-primitives.md)
§5 (text as the surface, the formalisation ladder, how text is stored) and §6 (moves,
the narrative clock). Canvas text, drawn by the engine, is a different thing and stays
in [`text.md`](text.md).

---

## 1. What it is

An HTML panel in the dock that shows and edits one **narrative**: flowing document text
whose spans refer to entities, values and views in the scope's compositions. It is step
2 of the note's build order. It proves the hardest new piece first: **text kept stable
against the store through ops**, while people and the LLM both edit it.

**First version: level 1 of the formalisation ladder only.** References that need no
new objects:

| Reference | Shows | Does |
|---|---|---|
| entity mention (`:e`) | a chip with the entity's name | hover lights its figures in every visible composition (cross-probing exists); click selects it |
| live value (`:val`) | the value, formatted (`fmt`, as text readouts already are) | updates as state changes |
| view change (`::view`) | a block naming a composition and framing | clicking it, or scrolling it to the top of the panel, shows that composition in its panel at that framing |

Not in the first version: moves (`:go`), claims, goals, the narrative clock beyond
"scroll into view", audio.

## 2. Data

Blocks are nodes in the scope's store (note §5.3), so text gets ops, undo, history and
durability through `iron_document` and `scopes.md` with nothing new.

- **A `narrative` node** is the root. **`block` nodes** are its children (paragraph,
  heading, list item, view, quote, code), ordered by the existing order keys.
- **A `richtext` facet** holds a block's content: text plus marks, each mark with its own
  id and a target (a node id, later a global reference).
- **A derived span index** in the store maps target → marks, like the `byTarget` index
  vision 05 §3.1 describes for animation. It answers "which paragraphs mention `#fiber`"
  and lets a deleted entity mark its mentions stale.
- **Ops in v1 are per block:** insert, move, delete, set content. Typing replaces one
  block's content, debounced, as one transaction per pause. A text CRDT inside the
  facet waits for collaboration (note §10 Q4).

## 3. The editor

**ProseMirror**, with or without TipTap on top. Its document model maps closely:

| ProseMirror | Iron |
|---|---|
| doc | the `narrative` node |
| top-level node, with an `id` attribute | a `block` node |
| mark (or inline node, for chips), with an `id` attribute | a mark in the `richtext` facet |

Two directions of sync:

- **Editor → store.** Editor transactions become block ops, debounced per block, so an
  edit is one undoable step per pause, not per keystroke.
- **Store → editor.** Changes from elsewhere (the LLM, undo, another panel) arrive as
  block-level patches keyed by block id and are applied as editor transactions, so the
  cursor and selection survive. The store is the source of truth.

**Markdown** is the exchange format: for import, export, and the LLM's view.
Prior art worth reading before choosing a syntax: Curvenote's editor packages, which
convert markdown to ProseMirror and back, and **MyST markdown** (the Jupyter Book and
Curvenote markup), which already has *roles* (inline) and *directives* (block) for
scientific writing. MyST may be a better base than the generic-directives syntax the
note sketched; deciding is §6 Q1.

## 4. The LLM

**The model edits the narrative as it edits compositions today.** Its view of a
narrative is markdown with block ids. Its writes are `<edit>` verbs on blocks
(`src-ui/model/edit.ts`): create, set content, move, delete. That way every LLM change
is a transaction: undoable, attributed to the model as actor, and visible in history.

**Where the conversation lives** is the open design question. Existing tools put a chat
panel beside the document. That is v1, since the Model view already exists. The goal is
for the **document, not the chat, to be the focus**. Candidates, roughly from cheapest
to most novel:

1. **Chat beside, changes in place.** The side-by-side chat, but its edits appear in
   the document as tracked changes (insertions and deletions marked in place), accepted
   or rejected per change, not applied silently. The chat becomes a place to give
   instructions; the document is where you see results.
2. **Ask in place.** Select text or put the cursor somewhere, press a key, and type an
   instruction in a small popover ("tighten this", "add the measured value", "explain
   why this lens"). The answer arrives as a tracked change right there. No separate
   transcript to read.
3. **The LLM as a margin commenter.** Its suggestions appear as margin notes anchored to
   text, as a colleague's comments would. Examples: "this reads like a hypothesis about
   #fiber; mark it?", "this value is stale since the model changed", "mention #lens
   here?". Each note is a small thread: reply, accept or dismiss. The conversation is
   spread across the places it is about, instead of in one long stream.
4. **Intent blocks.** A block can hold an instruction instead of prose ("explain the
   mode-matching problem; show the beam sketch"). The LLM fills it, and the intent stays
   attached so the block can be regenerated when the scene changes. Hand edits become
   overrides of the generated text, the component-instance pattern again: the
   generated part is derived, and only the author's changes are stored.
5. **Writing to the agent in the document.** A line beginning `@iron` (or a hotkey) is
   an instruction. The reply is a block you keep, edit or delete. In a lab notebook,
   where the record of what was asked and done *is* the content, the chat and the
   notebook become the same document, and the agent's actions become recorded moves.
6. **Commands on state, not text.** "Rotate the camera to show the lens" produces a
   change to the scene, and offers it to the narrative as a `::view` or, later, a
   `:go`.

**Leaning:** build 1 first, as it is mostly the existing Model view plus tracked
changes. Then 2 and 3, which make the document the focus. Explore 4 and 5 as the most
distinctive, since they fit Iron's derived-plus-overrides model and its notebooks.
Ask in place is expected to produce most first drafts.

### 4.1 Layout modes

The panel always holds two parts, the **document** and the **conversation**. Its modes
are layouts of those parts, not different features; switching keeps the conversation,
the document and the cursor. Their defaults are view fields resolved for the narrative
view's focus context ([`contexts.md`](contexts.md) §2.7), so a notebook-like and a
lesson-like narrative can default differently; the mappings onto the contexts system
are in the note's §6.6.

| Mode | Layout | For |
|---|---|---|
| **Authoring** | document fills the panel; a small composer docked below | drafting by ask in place, little back and forth |
| **Hybrid** | document above, conversation below (side by side when wide); split adjustable | revising with some discussion |
| **Exploration** | conversation fills the panel; document collapsed to an outline or hidden | freeform work with the model |

**The composer shows its target**: "Writes at cursor, §3" or "Conversation only", with a
toggle. Authoring defaults to the cursor, Exploration to conversation only. This keeps
"where you ask decides" (note §6.5) visible in a stacked layout.

### 4.2 The cursor as context, not a rule

In Authoring mode, LLM output lands at the cursor unless the request implies another
place ("fix the units in the intro"). The cursor is a strong suggestion that supplies
context, not a rule about where text appears. Proposed guard rails, since this is
unexplored and unpredictability is the risk:

| What the author has | Scope of the LLM's edit |
|---|---|
| a selection | hard: confined to the selection unless the request names somewhere else |
| a cursor | soft: insert at the cursor; edits elsewhere allowed |
| nothing (Exploration) | the whole narrative, or none in conversation-only |

- **Insertions at the cursor apply directly**, as tracked changes.
- **Edits away from the cursor arrive as proposals** to accept, with a notice listing
  where they are ("edited §2 and §5") and links to jump there. Nothing changes off
  screen unseen.

Prior art: editors that scope an inline instruction to the selection, against agent
modes that may edit anywhere; this table puts both in one panel, chosen by what the
author has selected.

## 5. Phases (sketch)

1. Block and narrative node types, the `richtext` facet, block ops, the span index;
   Rust tests.
2. The panel: ProseMirror over one narrative, two-way sync, undo through the store.
3. References: mention autocomplete (`#`), chips with cross-probing, live values, view
   blocks.
4. Markdown import and export; the model's view and `<edit>` verbs on blocks.
5. The LLM in place: tracked changes from the chat (option 1), then ask in place
   (option 2).

## 6. Open questions

1. **Syntax:** MyST roles and directives, or the generic-directives sketch?
2. **TipTap or plain ProseMirror?**
3. **Is a narrative a composition** (shown through the view registry like any other),
   or its own panel kind? Leaning: its own notation in the view registry, so it opens
   from "+" like compositions do.
4. **Chips or marks** for mentions: an inline node (atomic, cannot be partly selected)
   or a mark over the original words (keeps the author's wording)? Probably both: the
   author's words with a mark, and a bare chip when the text is just the name.
5. **Per-block debounce versus finer ops:** when does undo granularity in prose start to
   matter?
6. **Which LLM options** (§4) to prototype after the first.
7. **The cursor rule in practice** (§4.2): do off-cursor proposals feel safe or slow?
   Should small off-cursor fixes (a unit, a typo) apply directly?
8. **How a linked view appears** when the narrative panel drives a composition (note
   §6.5): the frame, the breadcrumb, and the link toggle's place.

## 7. Decisions (best guesses, 2026-11-03)

The user asked for best guesses on §6 and for the build to proceed. Each is cheap to
revisit.

1. **Syntax: generic directives** (`:e[words]{#id}`, `:val{#id.facet.field fmt="…"}`,
   `::view{show=name focus=#id}`), as the note sketches; remark-directive parses them.
   MyST stays possible: the store holds marks, not syntax.
2. **Plain ProseMirror**, no TipTap: the schema is small and every behaviour (sync,
   undo through the store, chips) is custom anyway.
3. **Its own notation**, `narrative`, in the view registry. Source: the narrative's
   name or id. It opens from "+" like a composition.
4. **Mentions are marks over the author's words**; a mention whose words are the
   entity's name reads as a chip. **Live values are inline atoms** (their text is
   derived, so it cannot be typed into).
5. **One transaction per block per pause** (400 ms), as the outline said.
6. **LLM, first:** chat beside (option 1) and **ask in place** (option 2), both landing
   as **reviewable changes**: the blocks a model turn changed are marked, each with
   Keep and Revert, rather than full tracked changes inside the text.
7. **The cursor rule** as §4.2 proposes; small off-cursor fixes are not special-cased.
8. **A linked view** is shown by the view block itself (highlighted while its
   composition is the one shown); the frame and breadcrumb of note §6.5 wait for moves.

### 7.1 Storage

- **Types.** `narrative` (a root; children: `block`) and `block` (no children).
- **Facets.** `block { kind, level, show, focus }`: `kind` is `paragraph`, `heading`,
  `bullet`, `ordered`, `quote`, `code` or `view`; `level` a heading's; `show` and
  `focus` a view block's composition and optional node to frame. `richtext { text,
  marks }`: `marks` is JSON, `[{"type":"mention","start":0,"end":5,"target":"#4"}]`,
  as `text.runs` already is. Offsets count UTF-16 code units (what the editor counts).
  Types: `mention` (`target` a node), `value` (an atom: one U+FFFC in `text`; `target`
  a leaf, `fmt` optional), `strong`, `em`, `code`, `link` (`href`).
- **The span index** is computed from the marks on demand (`narrative::mentions`):
  target → (block, mark). A mention whose target is not live is **stale**, shown struck
  through, as stale overrides are.
- **Limit:** targets inside a JSON string are not remapped by copy and paste between
  documents, and the validator does not see them; a relation per mark is the cleaner
  form once collaboration needs it.

## 8. Built (2026-11-03)

Phases 1–5 of §5 in a first form, on the §7 decisions.

- **Store (`iron_document`).** Types `narrative` and `block`; facets `block` and
  `richtext` (§7.1). `narrative.rs`: read blocks; `plan` turns block edits
  (`insert` with a key, `set`, `delete`, `move`, `after` naming an id or an earlier
  key) into ops, one transaction, with order keys between neighbours; the span index
  (`references`, `references_to`, stale when the target is gone); markdown out
  (`to_markdown`, `block_markdown`, `inline_markdown`) and in (`parse_markdown`,
  `parse_inline`) with generic directives; `value_expr`. The model's view prints a
  narrative as markdown, each block's first line `[#B] `. `narrative` and `block`
  are structural (never drawn, not copied, not loose roots). Tests:
  `tests/narrative.rs` (5) and `narrative::tests` (3).
- **Engine calls.** `narrative_list`, `narrative_read` (with each target's label and
  staleness), `narrative_edit`, `narrative_create`, `narrative_markdown`,
  `narrative_write` and `narrative_rewrite` (the model's verbs), `narrative_values`
  (keyed by expression), `narrative_targets` (the `#` menu), `narrative_probe`
  (hover and select from HTML; `SelectionState::probe` keeps a hover lit while the
  canvas hovers nothing).
- **Panel (`src-ui/narrative/`, `views/html/NarrativeView.svelte`).** The
  `narrative` notation; a Lesson tab in the default layout (layout key v7) and
  "Narratives" in the "+" menu. ProseMirror schema (`code_block` for the store's
  `code`, since a mark is called `code`), conversion with UTF-16 offsets
  (`convert.ts`, tested in `tests/narrative.test.ts`), the block diff (a longest
  increasing run of kept blocks stays; the rest move; split blocks and `~n`
  temporary ids are new), and `editor.ts`: debounced flush, read-and-reconcile on
  every document change (in place when the block list is unchanged, whole document
  otherwise, the cursor kept by block and offset), Cmd+Z through the store, input
  rules (`# `, `- `, `1. `, `> `, ```` ``` ````), Backspace at a block's start makes a
  paragraph, the `#` menu (its keys before every keymap), chips and stale mentions as
  decorations, value atoms and view blocks as node views.
- **Modes** (§4.1): Authoring, Hybrid, Exploration, per view (kept in
  `localStorage`); Exploration shows an outline of headings and view blocks.
- **The model.** `<write narrative after>` and `<rewrite block>` in `edit.ts` (in an
  `<edit>` of their own); the manifest's NARRATIVES section; the composer adds where
  the cursor is (block, heading, selection) to the request (§4.2), or says
  "conversation only". A turn's changed blocks are marked; Revert applies the
  inverse block edits as one transaction.
- **Demo.** The showcase adds `lesson`, a tour of the demo with mentions, two live
  values and three view blocks.

Not built: tracked changes inside a block's text (the review is per block); margin
comments, intent blocks and `@iron` lines (§4 options 3–5); `:go` moves and the
narrative clock beyond scroll-to-top; mark targets as relations (§7.1's limit);
remapping targets on paste between documents.

