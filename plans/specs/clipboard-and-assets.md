# Clipboard, duplicate and assets

**Status:** phase 1 (copy closure, Cmd+D, Cmd+C/X/V within the app) done 2026-10-15.
Phase 2 (Alt-drag) waits on gesture ownership. Phases 3 and later (assets, drag and
drop, cross-document paste) wait on the project-container decision (§6).

This spec came out of a conversation on 2026-10-15 about how deep copy and paste must
go, how it meets bindings and ports, how much should cross the app boundary, and how
images should eventually be dropped into 2D and 3D views. The conclusion was that
copy and paste inside one document needs no new foundations, while Alt-drag needs
gesture ownership and assets need a decision about the project container.

## 1. Copy is a store function, not an op

`iron_document::copy`:

- `fragment(store, roots, origin) -> Fragment`: the closure of the roots (§2), as a
  small document holding the copied nodes under their original ids, plus how to
  place them back.
- `paste(store, fragment, Placement, here) -> Pasted`: ordinary `Create` and `Link`
  ops with fresh ids, ascending in creation order. One transaction, so undo, the
  model surface and the reconciler need nothing new.
- `duplicate(store, roots)`: both, each copy placed right above its original.

`definition::instantiate` copies a template the same way and could move onto this;
it has not yet, because its parameter rewriting is specific to it.

## 2. What a copy takes along

A reference inside the copied set always follows the copy. What happens to each
reference that leaves it:

| Reference | Rule |
|---|---|
| `figures` (shape → entity) | The entity comes too, with its ports and equations: a copied laser is a second laser, placed after the first in the model. A whole subtree of figures of one entity makes one new entity |
| `connection` (two `connects`) | Copied when both ports are copied; otherwise left behind and reported, and the copy's port is unconnected |
| `constraint` (`constrains`) | Pulled in when every operand is copied, although users never select constraint nodes; otherwise left behind and reported. A pin's world point moves with the paste offset |
| clip (`animates`) | Pulled in when every target is copied, into the original's timeline |
| `after`, `during`, `aligns_to`, `references` | Copied when both ends are |
| A binding reading an outside leaf | Kept within the same document and session. Elsewhere, replaced by the value it read at copy time, and reported |
| `instance.def`, `embed.source`, `embed_camera.camera` | Kept: definitions and embedded compositions are shared |
| `selects` (session tier) | Never copied |

Roots of the tree (compositions, the model, the library, timelines), the session and
the parts of a definition are never copied on their own; ports, equations and points
come with their owner. A root inside another root is copied once.

**Cut, then paste** moves rather than copies: the first paste after a cut relinks the
figures to the entities they pictured (which a cut does not delete), so connections
survive. Later pastes of the same cut copy as usual. Ids are never reused, so the
pasted figures have new ids either way.

`Store::encapsulate` (ECM §9) asks the same boundary question of a selection, and
should reuse this classification when it is built.

## 3. Placement

- **Duplicate (Cmd+D)** puts each copy in its original's container, right above it
  in painter order, at the same position, as Figma does. The copies are selected.
- **Paste (Cmd+V)** goes into the 2D view under the pointer (else the selection's,
  else the first 2D view): beside the selection in that view, else into the
  artboard under the pointer, else into the view's composition. It lands at the
  copied position when that is on screen, else centred in the view. An embed is
  never pasted inside the composition it shows.
- **A root moved between containers keeps its place on screen**: its transform is
  re-expressed in the new parent's space, shear dropped. A bound position stays
  where its binding puts it, and that is reported.

## 4. The clipboard

The worker has no `navigator.clipboard`, and the store lives in the worker.

- **Copy and cut** start the write in the main thread's keydown handler, inside its
  user activation, with a `ClipboardItem` whose content is a promise. The worker
  answers with the fragment's text (`send_clipboard_from_worker`) and the promise
  resolves. `src-ui/runtime/clipboard.ts`.
- **Paste** is read from the DOM `paste` event, which needs no permission prompt;
  Cmd+V itself is left to the browser. `clipboard_paste` hands the text to the
  select tool.
- **Cmd+C belongs to the canvas** only when something is selected there and neither
  a text field nor a page text selection has the focus.
- **When the system clipboard refuses the write**, the tab keeps the text and the
  next paste uses it.
- **The text** is the canonical file format of the fragment's small document,
  wrapped with the paste metadata (`Fragment::to_text`, format 1). Old fragments
  therefore migrate as old files do.
- **Same document or not** is decided by an origin token (`DocumentOrigin`): random
  per session, renewed on every load. A paste in another tab, or after a reload,
  freezes outside reads rather than guess that equal ids mean the same nodes.

## 5. Phases

1. **Done 2026-10-15.** The copy closure, Cmd+D, and Cmd+C, X and V through the
   system clipboard, within the app (any tab of the same origin).
   - Tests: `tests/copies.rs`, the copy tests in `tests/ports.rs` and
     `tests/constraints.rs`.
   - The smoke script's Optics section: duplicate, copy to the system clipboard,
     paste, cut and paste, four undos.
2. **Alt-drag to duplicate.** It creates structure in the middle of a gesture and
   removes it again if Alt is released, so it waits on gesture ownership
   (`infinite-canvas-architecture-patterns.md` §4) and is one of its exit tests. It
   needs history to merge the duplicate and the move into one entry, an exception
   to spine §6.5 that must be recorded there.
3. **Outbound formats:**
   - `text/plain` as the model's XML view of the selection, so a paste into a
     conversation with Claude reads well, with the fragment moved to a `text/html`
     carrier;
   - `image/png` of the selection;
   - "Copy as SVG".
4. **Assets (needs §6):** the content-addressed blob store; an `asset` node in the
   library (`{hash, mime, w, h, name}`); images as a rect's image fill, as in Figma
   (decision pending, §7); drop and paste of PNG and JPEG into 2D views. Images
   draw through `DrawCmd::Texture`, which embeds already use; tell the render track,
   because images change stage H's cache keys.
5. **3D drops (needs ECM I3, the 3D length unit):** image planes placed where the
   pointer's ray meets a surface, and `.glb` as a `mesh` asset.
6. **Inbound SVG** through `usvg`, and cross-document paste that brings definitions
   along.

## 6. The project container (decide before phase 4)

Assets, cross-document paste and saving the workspace all need the same decision:

- **No durable project exists.** The app boots into the demo on every reload, and
  saving downloads one JSON file.
- **What it must settle:**
  - a bundle format: `document.json` plus `assets/<hash>`, a zip on the web, a folder
    natively;
  - durable storage (OPFS);
  - a document identity, so `DocumentOrigin` can become a property of the document
    rather than of the session;
  - where libraries live and how they are versioned (vision 09 §13.3, ECM I2);
  - where the workspace is saved (panels-and-session open question).

## 7. Open questions

1. **Copying a figure of an entity**: a new entity (built) or another picture of the
   same one? Built as a new entity, because a copied laser should be a second laser.
   The other behaviour is "Paste linked", or the existing Link gesture afterwards.
   `Placement::share_entities` already does it; only a command is missing.
2. **Images**: a rect with an image fill (recommended; corners, masks and shadows
   then apply unchanged) or an `image` node?
3. **Clips**: built to come along with the nodes they animate, as keyframes do in
   After Effects. Confirm.
