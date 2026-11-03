# Scopes: durable local storage and global identity

**Status:** Approved 2026-11-02: the user accepted every default in §2 and §9.
**Phases 1–2 done 2026-11-02** (§10, with the deviations); phases 3–5 not started.
**Replaces:** the project-container section of
[`clipboard-and-assets.md`](clipboard-and-assets.md) §6.
**Background:** [`../notes/narrative-layer-and-storage-primitives.md`](../notes/narrative-layer-and-storage-primitives.md)
§2.2 (scopes, and projects as views) and
[`../notes/projects-labs-and-server.md`](../notes/projects-labs-and-server.md) §2 and §6
(meaning versus ownership; build the single local project first with real seams).
The store's rules are [`document-spine.md`](document-spine.md) §5–7.

---

## 1. What this is for

Today the app boots into the demo on every reload, and saving downloads one JSON file
(`document_client.svelte.ts`, `download()`). Nothing persists except the dock layout and
settings in `localStorage`. This spec makes work survive a reload and gives every node
an identity that means something outside its file.

A **scope** is the unit with one op log, one permission set and one history: what
today's `iron_document` document already is, made durable and named. Projects,
notebooks and lessons are *views* over scopes (the note's §2.2) and are not built here.

**In scope:** a scope id; durable storage in the browser (snapshot plus op log); boot
into the last scope; a small menu to create, open, rename, duplicate, delete, import and
export scopes; the dock layout saved per scope; an assets directory.

**Out of scope, deliberately:** a server, sync, collaboration, permissions between
people, a database, partial loading. Browser storage is the right home for now and is
not scalable; §8 lists the few constraints that keep the later designs open, and
otherwise nothing is built for them.

## 2. Decisions

| # | Question | Decision | Why |
|---|---|---|---|
| D1 | Scope id | a random 128-bit id, written as 26-character Crockford base32 (ULID-style, so ids created later sort later), with a short prefix in views | Random ids need no coordination, so a future server or another device can mint them. Time-ordered text makes directory listings readable |
| D2 | Node ids | unchanged. The per-scope `u64` counter stays; a global reference is the pair (scope id, node id) | `id.rs` already keeps the integer private and leaves room for an `(actor, counter)` form if collaboration arrives. Nothing in existing files changes meaning |
| D3 | Cross-scope references | representable in v1, resolved later. A relation endpoint may name another scope; an unresolved endpoint is a normal state, drawn as a placeholder | Libraries (component-model phase 5) and the narrative layer both need references across scopes. Making "not loaded" ordinary now is the same mechanism streaming needs later (§8) |
| D4 | File format | schema 9 adds `"scope": "<id>"` and `"name"` to the header. An imported file is given a fresh scope id. **Only schema 9 loads** (2026-11-02: no saved files from earlier versions exist, so the migrations for 1–8 were removed) | One field; no migration code to keep |
| D5 | Storage API | OPFS (the Origin Private File System), through the async `navigator.storage.getDirectory()` API, behind a small `ScopeStorage` interface | `localStorage` holds about 5 MB of strings and blocks the main thread; OPFS is files, large, and works in both the worker and the main thread. IndexedDB would also work; files map directly onto snapshot plus log |
| D6 | Layout on disk | one directory per scope holding versioned snapshot files, the log as one small segment file per batch of transactions, metadata, the workspace and assets (§4) | Spine §7 already describes a snapshot plus newline-delimited JSON of transactions. Separate files avoid OPFS's costly appends and its uneven support for renaming (§4.2) |
| D7 | Who writes | the TypeScript host, beside the engine (the worker when the engine runs there). Rust provides the snapshot and the committed transactions; it does not touch storage | Keeps `iron_document` free of I/O and of the browser; a server-backed storage later implements the same interface |
| D8 | Undo across reloads | not in v1. The log is persisted for durability and provenance; the undo stack is cleared after a reload's replay | Persisted undo needs gestures and session state in the log. It can come later without changing the format |
| D9 | One scope per what? | in v1 each thing a user creates is one scope ("a project is one scope with everything pinned") | The simplest case of the views model. Views spanning scopes come with the narrative layer |
| D10 | Workspace tier | saved per scope, beside it (`workspace.json`), replacing the global `iron.workspace.v1` key | Answers the panels-and-session open question in favour of "beside it per user". Locally there is one user |
| D11 | Boot | open the last scope. With no scopes, create one from the demo. `?demo` opens a fresh, unsaved demo scope, so the smoke script stays deterministic | The demo stops being the only thing the app can show, without breaking the regression suite |
| D12 | Assets | `assets/<sha256>` inside the scope directory, referenced by hash from facets, garbage-collected by reachability at compaction | The bundle sketch in clipboard-and-assets §6; unblocks clipboard phase 4 (images, drag and drop) |
| D13 | Export | download the snapshot JSON as today when a scope has no assets; a zip of the directory when it has some | No new format for the common case |

## 3. Identity

### 3.1 Scope ids

`ScopeId` is a new type in `iron_document` (`id.rs`): 128 bits, minted by the host
(`host_services`, which already makes origin tokens) and passed into the store. It is
rendered in full in files, and as a short alias in views where ambiguity allows (the
model's XML view, inspectors).

`DocumentOrigin` (`document_bridge/mod.rs`), today a fresh token per load, becomes the
scope id. The copy rule in `iron_document::copy` then gains a meaning across reloads: a
fragment pasted back into the scope it was copied from keeps its references even after
a reload. A fragment pasted into another scope freezes them as now; turning them into
cross-scope references instead is a later option (§9).

### 3.2 Global references

A reference inside a scope stays a bare node id (`#a3`), exactly as now. A reference to
another scope is `{"scope": "<id>", "node": "#a3"}` in a relation endpoint. Proposed
text form for views and the model: `#a3@<alias>`, where `@<alias>` is the short scope
alias. (The narrative note uses `@name` for states; that syntax is a sketch and can
change.)

Resolution in v1: if the target scope is the open one, resolve normally; otherwise the
endpoint is **unresolved** and the bridge draws a placeholder. No v1 feature creates
cross-scope references except import tests; libraries and narratives will.

### 3.3 Schema 9

```json
{"schema": 9, "scope": "01JC8Z3V5Q4M7W2N6R0T9X1K8E", "name": "Fiber coupling",
 "next_id": 41, "next_rel": 7, "version": 812,
 "nodes": { … }, "facets": { … }, "relations": [ … ]}
```

Loading a file is an import: the caller supplies a fresh scope id and a name (the file
name). Only schema 9 loads; the readers and migrations for schemas 1–8 were removed
(2026-11-02), since no saved files from earlier versions exist. A later format change
bumps the schema and drops the old reader the same way, until there are files worth
keeping.

## 4. Storage

### 4.1 Layout

```
<OPFS root>/iron/
  index.json                        scope ids, names, last opened, schema
  scopes/<scope id>/
    meta.json                       name, created, modified
    snapshot-<version>.json         the canonical document (schema 9); normally one
    log/<first version>.ndjson      one segment per batch of committed transactions
    workspace.json                  dock layout, each view's pan and zoom
    assets/<sha256>                 binary assets by content hash
```

`index.json` is a cache of the `meta.json` files and can be rebuilt by listing
directories.

### 4.2 Writing

1. Each committed transaction is written to the log. The store's `History` already
   keeps every applied transaction, undo and redo included, in a log bounded by a
   horizon, and its comment names "the append-only file" as the durable log; the feed
   in §5 hands the host whatever is newer than the last version it wrote.
   Transactions are already `Serialize`. Writes are batched: on idle, at most every
   ~500 ms, and immediately on `visibilitychange` to hidden and on `pagehide`. **Each
   batch is a new segment file**, `log/<first version>.ndjson`. An OPFS writable commits
   atomically when closed, but appending to an existing file copies it first, so
   segments keep each write small and whole.
2. **The feed must keep up with the horizon.** `History`'s log keeps only the last 200
   transactions (`DEFAULT_HORIZON`). The host drains the feed every frame it receives
   events; if it ever finds a gap (the oldest transaction offered is newer than the last
   one written), it compacts at once, so nothing is lost: the snapshot holds the state
   the missing transactions produced.
3. **Compaction** writes a new snapshot when the log passes a threshold (2 MB or 2,000
   transactions), on explicit save, and when switching scopes. It writes
   `snapshot-<version>.json`, then deletes the older snapshot and the segments it
   covers. No rename is needed: a crash at any point leaves at least one whole snapshot,
   and segments older than the newest snapshot are skipped and later deleted.
4. History retention: v1 keeps only the tail since the last snapshot (§9).
5. The session tier and expanded nodes are never written, as today. Session changes
   (`apply_session`) do not bump the document version and are not in `History`, so the
   log holds content changes only.

### 4.3 Loading

Read the newest snapshot that parses, `Store::load` it, then `Store::replay` the
transactions in the segments, in version order, skipping any not newer than the
snapshot. `replay` applies each transaction's recorded and derived ops without solving
(CS §7), so a reload reproduces exactly what was saved; it is already tested
(`tests/constraints.rs`, `tests/spine.rs`). A transaction that fails to replay stops the
replay, keeps what loaded, moves the remaining segments to `log-broken-<time>/`, and
reports it. The scope is never silently discarded.

`replay` pushes each transaction onto the undo stack, so after loading, the store's
history is cleared (a new `Store::clear_history`), giving D8's consistent "undo starts
empty" rather than undo reaching back only as far as the last snapshot.

### 4.4 Two tabs on one scope

Two tabs writing one log would interleave versions and corrupt it. Each open scope
holds a Web Lock (`navigator.locks.request("iron-scope-<id>")`). A second tab opening the
same scope gets it read-only, with a notice and an "open here instead" action that asks
the first tab to release it.

### 4.5 The interface

```ts
interface ScopeStorage {
  list(): Promise<ScopeMeta[]>;
  create(meta: ScopeMeta, snapshot: string): Promise<void>;
  read(id: ScopeId): Promise<{ snapshot: string; log: string[]; workspace?: string }>;
  append(id: ScopeId, transactions: string[]): Promise<void>;
  compact(id: ScopeId, snapshot: string, version: number): Promise<void>;
  writeWorkspace(id: ScopeId, json: string): Promise<void>;
  putAsset(id: ScopeId, bytes: Uint8Array): Promise<string>;   // returns the hash
  getAsset(id: ScopeId, hash: string): Promise<Uint8Array | undefined>;
  remove(id: ScopeId): Promise<void>;
}
```

The web implementation is OPFS, developed and tested on Chromium. A memory
implementation serves tests, `?demo` and the native build. No filesystem implementation
for Tauri and no work for other browsers' OPFS support: when a server arrives, local
storage becomes a cache in front of it (§8), and the interface is where a server-backed
implementation plugs in.

Request persistent storage (`navigator.storage.persist()`) on first save, so the browser
does not evict scopes under storage pressure, and show `navigator.storage.estimate()`
in the scope menu.

## 5. Engine calls

Following the typed protocol (new call: an `engine_api` function, a dispatch arm, then
`bun run protocol:generate`):

| Call | Does |
|---|---|
| `scope_open(id, name, snapshot, log[])` | load and replay; set the scope id; replaces `document_load` for scopes |
| `scope_import(text, id, name)` | load any readable schema as a new scope |
| `document_save` | unchanged, now writes schema 9 |
| a committed-transactions event | the host's feed for appends: the serialised transactions since the last event, with their versions |
| `scope_info` | id, name, version, transactions since the last snapshot |

`document_load` stays for tests and the smoke script.

## 6. The scope menu

Not a project page yet (see the note's "What to build first"). One menu in the shell's
title area:

- the current scope's name, editable in place;
- **New** (empty, or from the demo), **Open** (recent scopes, with sizes),
  **Duplicate**, **Delete** (with confirmation, as it cannot be undone);
- **Import** (a `.json` or bundle `.zip`; becomes a new scope) and **Export**;
- storage used and available.

The model surface gains nothing in v1; the model works on the open scope as it does on
the document now.

## 7. Phases

| Phase | Builds | Exit |
|---|---|---|
| **1. Identity** | `ScopeId`; schema 9 header; import of ≤ 8 as a new scope; `DocumentOrigin` from the scope id | Rust round-trip and migration tests; every golden view unchanged; smoke unchanged |
| **2. Durable scopes** | `ScopeStorage` over OPFS and memory; transaction feed; segments, compaction, load with replay; `Store::clear_history`; the per-scope lock; boot into the last scope; `?demo` | Edit, reload, see the edit; kill the tab mid-edit and lose at most the last batch; a second tab opens read-only; a Rust test that load + replay of a saved log equals the live store (including after undo and redo); a JS test of segments, compaction and a gap in the feed; `drive.ts`'s default URL gains `&demo`, so scripts that reload still see the demo, and the smoke passes |
| **3. The scope menu** | New, open, rename, duplicate, delete, import, export | Driving script covering each action; `SKILL.md` updated |
| **4. Workspace per scope** | `workspace.json`; each view's pan and zoom (the panels open item) | Switching scopes restores each one's layout and views |
| **5. Assets** | `assets/<sha256>`, `putAsset`/`getAsset`, reachability GC at compaction, zip export | Unblocks clipboard-and-assets phase 4 |

Phases 1–2 are the core and are worth doing together; 3–5 can follow in any order.

## 8. Keeping later designs open

**When a server exists, local storage stays, with a different job.** Server-backed
editors commonly keep a browser copy (Figma's local cache for edits made offline,
Notion's WASM SQLite on OPFS as a cache, Linear's and tldraw's IndexedDB stores): the
server holds the truth, the browser holds a cache and an outbox. This design maps onto
that directly: the snapshot becomes the cached copy, the log segments become the outbox
of transactions the server has not yet acknowledged, and reconnecting sends the tail and
receives what others wrote. So the work here is kept, but nothing beyond Chromium's OPFS
is built for it.

Large or complex scopes may eventually have to be **streamed**: loaded in part,
depending on what the narrative is at or what is on screen. A server and sync come
later too. This spec builds none of that, but keeps to five constraints so none of it
is ruled out:

1. **Identity never encodes location.** A row can be named, fetched and cached alone
   (scope id + node id). Already true of node ids; scope ids keep it true across scopes.
2. **"Not loaded" is an ordinary state.** Unresolved cross-scope endpoints are drawn as
   placeholders, not treated as errors (D3). Partial loading inside a scope would reuse
   the same state.
3. **The log is per scope, append-only and versioned.** Sync later ships log tails;
   nothing here assumes the local copy is the only one.
4. **Storage is keyed by scope and async**, with no call that means "everything".
5. **Assets are content-addressed and fetched on demand**, as fonts already are
   (`text.md` §4).

What streaming would additionally need, so it is recorded rather than designed: a
loading unit smaller than a scope (a composition subtree, a chunk as in
`render-cache.md`, or "what a narrative position needs"), and ops that apply to a
partially loaded store. The second is the hard part, and is a reason to keep scopes
reasonably small until then.

## 9. Settled defaults and remaining questions

Settled for v1 (user, 2026-11-02, accepting the proposals):

1. **History retention:** only the tail since the last snapshot. Keeping old segments
   as provenance waits for a notebook view that reads them.
2. **Persisted undo:** no (D8).
3. **Pasting between scopes:** references freeze, as now.
4. **One open scope at a time.** Unresolved cross-scope endpoints show as placeholders.
5. **The scope alias:** the first characters of the id that are unique among local
   scopes.
6. **The demo:** "New from demo" copies the demo into a new scope; templates in general
   come later.

Still open, not blocking phases 1–2:

- How old segments would be kept if provenance becomes wanted, and their size limit.
- Whether pasting between scopes should create cross-scope references once two scopes
  can be open.

## 10. Done: phases 1–2 (2026-11-02)

**Rust.** `iron_document::ScopeId` (`id.rs`): 128 bits as 26 Crockford base32
characters, case-insensitive, `I L O` read as `1 1 0`. `Document` carries an optional
scope; schema 9 writes `"scope"` second in the header, and a file without one loads with
none. `Store::set_scope` (not an op: no version bump, no history), `Store::clear_history`,
and `Store::log_since(version)`, which returns the logged transactions after `version`
or `None` when the horizon has dropped some. Engine calls (`engine_api.rs`):
`scope_open(id, snapshot, log)` loads, replays each line newer than the snapshot with
`Store::replay` (no solving), stops at the first that fails or does not follow on,
clears history, names the scope and queues the store; `scope_assign(id)` names what is
open (or the load waiting to install); `scope_log(since)` is the feed. `PendingLoad`
holds a `Store`, so replay happens before the swap. `DocumentOrigin` is `scope:<id>`
whenever the document has a scope. Tests: `tests/scopes.rs` (5).

**Shell.** `src-ui/runtime/scope_storage.ts`: `ScopeStorage`, OPFS and memory
implementations, `newScopeId` (48 bits of time, then random, so later ids sort later).
`src-ui/runtime/scopes.ts`: the `Scopes` controller. It boots into the last scope, or
makes one of the demo once the showcase is in. It writes each batch (~500 ms after a
change, and at once on `visibilitychange` to hidden and on `pagehide`) as one segment,
and compacts at 2,000 transactions or 2 MB, on a gap and when switching scopes. A
failed replay sets the segments aside and snapshots what loaded. It holds a Web Lock
per scope: a second tab is read-only, and `takeOver()` steals the lock and reopens.
Wired in `document_client.svelte.ts`; `__iron.scope()`, `__iron.flush()`,
`__iron.undo()`. Tests: `tests/scopes.test.ts` (7, a fake engine), `scopes.txt`
(run-iron), and the full smoke with `&demo`.

**Deviations from the text above:**

1. **The name is in `meta.json`, not the header** (D4 said both). Renaming is not a
   document change, and a header field would have to change without an op. The header
   carries only the id.
2. **The controller runs on the main thread**, beside the document client, not in the
   worker (D7). It reaches the engine through the existing document calls in either
   runtime; OPFS's async API works on both threads. It can move into the worker if
   snapshot strings crossing `postMessage` ever cost too much.
3. **The feed is pulled, not pushed** (§5): `scope_log(since)` on each `stateChanged`,
   batched, instead of a committed-transactions event. `scope_info` is folded into it
   (scope, version, count). `scope_import` is `scope_open(fresh id, text, [])`.
4. **Opening a `.json` makes a new scope** (D4). A plain `document_load`
   (`__iron.load`, as the smoke and tests use) replaces the open scope's contents: the
   controller sees the scope change in the feed, names the new document with the open
   scope's id and writes a snapshot.
5. **A settings restart reopens the scope from storage** after a flush, instead of
   carrying a save into the new engine. The controller pauses meanwhile, because the
   new engine briefly holds the demo.
6. **Read-only does not block editing.** Edits in the second tab apply but are not
   saved, and `scope.notice` says so. Nothing shows the notice or the "open here
   instead" action yet; that is the scope menu (phase 3).
7. **`index.json` holds only `{last}`.** `list()` reads each scope's `meta.json`.
8. **No older schemas.** D4 and §3.3 first kept the migrations for schemas 1–8; the
   user removed backwards compatibility (2026-11-02), so `serial::load` reads schema 9
   only and refuses the rest. The two migration tests in `shapes.rs` and the old-schema checks in `definitions.rs` and `spine.rs` went with it.
9. **Setting broken segments aside copies them** into `log-broken-<time>/` and removes
   the originals; no directory rename.

**Limits.** The flush on `pagehide` is asynchronous and best effort, so killing a tab
loses at most the batch window (~500 ms). `navigator.storage.persist()` is requested
when the first scope is created; the estimate display waits for the menu.

