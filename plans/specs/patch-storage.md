# Patch storage: chunks, loading by point, the index

**Status:** Draft for the user's approval (PT-4b, 2026-11-21). **Not approved; do not
build from it.** It is the storage half of [`patches.md`](patches.md) (PT-4a), and is
built in PT-11 (plans/handoff-patches.md §2). Once approved it revises
[`scopes.md`](scopes.md) §4 and §8, [`reactive-engine.md`](reactive-engine.md) §13, and
iron-platform's `README.md` and `server/migrations`; until then those stand as they
are, and iron-platform is not edited.

**Read first:** patches.md §3 to §6 and §10; scopes.md §2, §4 and §8; reactive-engine.md
§13; iron-platform's `README.md` ("How a scope is stored", "How sync works") and
`server/migrations/0001_init.sql`; the note's §4.3.

## 1. What this decides

| # | Question | Decision |
|---|---|---|
| S1 | The loading unit | a **chunk**: an owner's authored subtree, or a set of patches. Content-addressed |
| S2 | What a snapshot is | a **manifest**, a tree of chunk hashes, plus the chunks |
| S3 | What a view loads | the chunks its point needs: the base of the owners it shows and what they read, and the patch chunks whose bounds hold its point |
| S4 | The index | three derived tables, never authoritative; the server keeps them for a whole scope, a client for what it holds |
| S5 | The browser cache | **OPFS files stay**: chunks and the log as files, the index in memory. SQLite on OPFS is not adopted now, with a trigger for revisiting it (§6) |
| S6 | Region edits under sync | ops add or remove values per axis, or move one endpoint, never set a whole region, so two concurrent widenings both land |
| S7 | Ops on parts not loaded | the server still holds the whole scope and validates; a client applies remote ops to the chunks it holds and drops the rest, refetching on need (§8) |

## 2. Chunks

### 2.1 What a chunk holds

- **A base chunk** is one owner's authored subtree (a composition, a definition, a
  placed instance's own nodes), cut where reactive-engine.md §13 cuts a `Summary`:
  nodes, facets, relations inside it.
- **A patch chunk** is a set of patches (patches.md §2) with their entries, grouped by
  the owner whose nodes they set and, within that, by their first value on the owner's
  leading sequence axis (a deck's slide, a narrative joint). A chunk is cut at about
  64 KB of JSON.
- **The axis chunk** of an owner holds its axes and their values, tombstones included
  (patches.md §3.1). It is small, and always loaded with the owner's base.

### 2.2 Addressing

A chunk's address is the SHA-256 of its canonical JSON (the spine's canonical form).
Chunks are immutable: an edit writes a new chunk, and unchanged chunks are shared
between versions, between snapshots, and between scopes that copy them.

### 2.3 The manifest

A manifest is the snapshot: the header (schema, scope id, version), then a tree whose
nodes are `Summary`s (reactive-engine.md §13) and whose leaves are chunk hashes. Each
`Summary` gains three fields for patches:

| Field | Holds |
|---|---|
| `axes` | the axes the owner declares |
| `bounds` | per axis, the hull of its patch chunks' regions: an interval on an ordered axis, a value set on an unordered one, the shortest suffix pattern on the instance axis. A point outside a chunk's bounds never needs it |
| `keys` | the (node, field) pairs its patches set, as a Bloom filter when large |

A summary's hash is the hash of its fields and its children's hashes, so a manifest is a
Merkle tree: two versions share every subtree that did not change, and a point's needs
can be found by walking the tree without loading the chunks.

## 3. Loading by point

A view has a point (patches.md §2): one value on every axis in play. To show it, a
client loads:
1. the manifest (or the parts of it the view's owners hang under);
2. the base chunks of the owners the view shows, and of what their summaries say they
   read (`summary.reads`, reactive-engine.md §13), transitively;
3. the axis chunks of those owners;
4. the patch chunks under those owners whose `bounds` hold the point.

**Moving along an axis loads the arrow** (patches.md §8): the chunks whose bounds hold
the new point and not the old. A deck's next slide, a lesson's next joint, the next take
is a prefetch of those.

**What is not loaded is ordinary** (scopes.md §8's constraint 2): a read of a chunk not
yet loaded is `Pending` (reactive-engine.md §6.4) and draws as a placeholder until it
arrives. A `deref` may ask for a chunk no summary predicted, and waits the same way.

**Local scopes load whole**, as today, until a scope is large enough to need otherwise
(PT-11b's exit sets the size); the format is the same either way.

## 4. The index

Derived from the patches, never written by hand, rebuilt from chunks at will:

| Table | Key → | Answers (patches.md §10) |
|---|---|---|
| **by field** | (node, field) → patches setting it, with their regions and ranks | why a value is what it is; an edit's blast radius; winning regions |
| **by value** | a field's value (and theme token) → (node, field, patch) | find: "everything blue" |
| **by axis value** | an axis value → patches whose region holds it | what applies on slide 7, or in hover |

- **The server** keeps the index for a whole scope in Postgres (§5), maintained by the
  room from each accepted transaction. The agent's queries go there, so a query never
  loads a scope it does not need.
- **A client** keeps the same index in memory for the chunks it holds, built as they
  load. Offline, a query answers over what is cached and says so.

## 5. The server

### 5.1 Tables

The catalogue gains the index. Contents stay iron_document's: the tables are derived,
and only the room writes them.

```sql
create type keyrange as range (subtype = text, collation = "C");

create table chunks (                       -- every chunk a manifest names
    scope_id  text not null references scopes(id) on delete cascade,
    hash      text not null references blobs(hash),
    owner     text not null,                -- the owner's node id
    kind      text not null check (kind in ('base', 'patches', 'axes')),
    primary key (scope_id, hash)
);

create table axis_values (
    scope_id  text not null,
    axis      text not null,                -- the axis node's id
    value     text not null,                -- the value's id
    position  text not null collate "C",    -- an order key; ordered axes only
    live      boolean not null,
    primary key (scope_id, axis, value)
);

create table patches (
    scope_id  text not null,
    patch     text not null,
    owner     text not null,
    rank      text not null collate "C",
    chunk     text not null,
    primary key (scope_id, patch)
);

create table patch_regions (                -- one row per axis the region names
    scope_id  text not null,
    patch     text not null,
    axis      text not null,
    interval  keyrange,                     -- ordered: between endpoint positions
    values    text[],                       -- unordered: the set
    excluded  boolean not null default false, -- `not {…}`
    suffix    text[],                       -- instance: the pattern's steps
    primary key (scope_id, patch, axis)
);
create index on patch_regions using gist (scope_id, axis, interval);
create index on patch_regions using gin (values);

create table patch_entries (
    scope_id  text not null,
    patch     text not null,
    node      text not null,
    field     text not null,
    value     jsonb not null,
    primary key (scope_id, patch, node, field)
);
create index on patch_entries (scope_id, node, field);
create index on patch_entries using gin (value jsonb_path_ops);
```

- **Intervals by order key.** An ordered axis's positions are order keys (as child
  order's), so inserting a slide never rewrites an interval. Moving a value changes its
  key, and the room rewrites the intervals that name it as an endpoint, which is rare.
- **Ranks are order keys** too (patches.md P1), so they compare as text.
- **A point query** ("what applies at this point") is one indexed lookup per axis,
  intersected; the winner per field is the highest rank among the rows that remain
  (patches.md §6.1).

### 5.2 Rooms

A room still holds the whole scope's `Store` (iron-platform README): it validates every
push, as today. After each accepted transaction it updates the index rows the
transaction's patch and axis ops touched, in the same database transaction as the log
row. Compaction writes the chunks that changed and a new manifest, and moves
`scopes.snapshot_hash` to the manifest's hash.

## 6. The browser cache (S5)

**Decision: OPFS files, as scopes.md §4 lays them out, with chunks as files and the
index in memory.**

```
scopes/<scope id>/
  meta.json
  manifest-<version>.json      replaces snapshot-<version>.json
  chunks/<sha256>.json         base, patch and axis chunks; shared by every manifest
  log/<first version>.ndjson   as today: the outbox when a server exists
  workspace.json
  assets/<sha256>
```

**Why not SQLite on OPFS now** (Notion's choice): it would hold the same index as the
server, and queries over a large cached scope would not need it in memory. Against it:
- the client's index covers only what it holds, which a view's point bounds; the
  server answers whole-scope queries;
- content-addressed chunks are files already, and OPFS writes them atomically;
- SQLite's OPFS VFS needs a dedicated worker with synchronous access handles, beside the
  engine's worker, and adds about a megabyte of wasm;
- two storage engines in one client is a second thing to keep consistent.

**Revisit when** building a cached scope's index on open takes over 100 ms, or its
index does not fit comfortably in memory (measured in PT-11b), or offline search over
whole scopes becomes a requirement.

## 7. Sync of patches (S6)

- **Entries** are per-field writes: the server's order decides, last write wins, as
  every op today (iron-platform README).
- **Regions** are edited by ops per axis: add values to a set, remove values, move one
  endpoint of an interval. Two people widening one patch on different values both land,
  since the server applies both adds; two moving the same endpoint, the later wins. No
  op writes a whole region.
- **Ranks** are a field of the patch: last write wins.
- **Axis values** are nodes: created, deleted (tombstoned) and moved as nodes are.

## 8. Ops on parts not loaded (S7)

What patches make easier: an entry is a row, and can be applied to a cached chunk
without its node's subtree loaded.

What stays as it is:
- **The server holds the whole scope.** A room's `Store` validates every op against the
  whole document; partial loading is the client's, not the server's.
- **A client receiving a remote op** applies it to the chunks it holds. An op touching a
  chunk it does not hold marks that chunk's cached copy (if any) stale; the next load of
  it fetches the current one.
- **A client authoring an op** does so on what it has loaded. An op that needs a chunk it
  has not loaded (a structural edit whose validation reads elsewhere) loads that chunk
  first, `Pending` until it arrives.

**Still unsolved,** and not this spec's: a room over a partly loaded scope (a server
that does not hold the whole `Store`), and validating structural ops against chunks
nobody has loaded. Both wait for a scope too large for a room's memory.

## 9. Phases (PT-11)

| Phase | Builds | Exit |
|---|---|---|
| **PT-11a. Chunked snapshots** | manifests and chunks, locally; loads whole, as today; a schema bump | save, reload and export round-trip on the suite's documents; an edit rewrites only the chunks it changed (counted) |
| **PT-11b. Loading by point** | the client loads a view's point (§3) and fetches the arrow when it moves; placeholders for what is pending | a 50-slide deck opens on slide 1 loading only slide 1's chunks; the next slide fetches only its arrow; the in-memory index's build time measured (§6's trigger) |
| **PT-11c. The server's index** | §5's tables, maintained by rooms; queries answered by the server | the agent's find and why are answered for a scope the client has not loaded; the index agrees with a rebuild from chunks on every test transaction |
| **PT-11d. Sync of regions** | §7's ops in iron-platform's rooms | two clients widening one patch at once both land; the existing sync tests pass |

## 10. What this revises, once approved

- **scopes.md §4:** the layout gains `manifest-<version>.json` and `chunks/`; §4.3's
  load reads the manifest and the chunks it names. **§8:** the loading unit is the
  chunk, and "what a narrative position needs" is §3's point.
- **reactive-engine.md §13:** the loading unit is the owner's chunk and its patch chunks;
  `Summary` gains `axes`, `bounds` and `keys` (§2.3).
- **iron-platform:** `README.md`'s "How a scope is stored" (manifests and chunks) and
  "How sync works" (region ops); a migration adding §5.1's tables.
