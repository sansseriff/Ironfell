// plans/specs/scopes.md phase 2: the durable log's segments, compaction, a gap
// in the feed, a replay that fails, and a second tab. The engine is a fake
// with the real calls' shapes (src/engine_api.rs `scope_open`, `scope_log`,
// `scope_assign`); the Rust side is tests/scopes.rs in iron_document.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { ScopeLibrary, Scopes, type ScopeEngine, type ScopeLocks } from '../src-ui/runtime/scopes';
import { MemoryScopeStorage } from '../src-ui/runtime/scope_storage';

/** A store: a version, the scope it names, and a horizon-bounded log. */
class FakeEngine implements ScopeEngine {
  version = 0;
  scope: string | null = null;
  horizon = 200;
  log: { version: number; label: string }[] = [];
  opened: { id: string; snapshot: string; log: string[] }[] = [];
  /** Replay fails at this version. */
  failAt = 0;
  pendingFor = 0;

  apply(n = 1) {
    for (let i = 0; i < n; i++) {
      this.version++;
      this.log.push({ version: this.version, label: `edit ${this.version}` });
      if (this.log.length > this.horizon) this.log.shift();
    }
  }
  async save() {
    return `{"schema": 11,${this.scope ? ` "scope": "${this.scope}",` : ''} "next_id": 1, "next_rel": 1, "version": ${this.version},\n "nodes": {}}`;
  }
  async call(fn: string, args: any[]): Promise<any> {
    switch (fn) {
      case 'scope_open': {
        const [id, snapshot, log] = args as [string, string, string[]];
        this.opened.push({ id, snapshot, log });
        this.version = Number(/"version": (\d+)/.exec(snapshot)![1]);
        let replayed = 0, skipped = 0, failed: any = null;
        for (const [index, line] of log.entries()) {
          const t = JSON.parse(line);
          if (t.version <= this.version) { skipped++; continue; }
          if (t.version === this.failAt) { failed = { index, message: 'bad op' }; break; }
          this.version = t.version; replayed++;
        }
        this.scope = id;
        this.log = [];
        this.pendingFor = 1;
        return { version: this.version, replayed, skipped, failed };
      }
      case 'scope_assign': this.scope = args[0] || null; return { scope: this.scope };
      case 'scope_template': return { snapshot: `{"schema": 11, "next_id": 1, "next_rel": 1, "version": ${args[0] === 'demo' ? 30 : 1},\n "nodes": {}}` };
      case 'scope_log': {
        if (this.pendingFor > 0) { this.pendingFor--; return { pending: true }; }
        const since = args[0] as number;
        const base = { pending: false, scope: this.scope, version: this.version };
        if (this.version <= since) return { ...base, gap: false, ndjson: '', count: 0 };
        if (!this.log.length || this.log[0].version > since + 1) return { ...base, gap: true, ndjson: '', count: 0 };
        const txs = this.log.filter(t => t.version > since);
        return { ...base, gap: false, ndjson: txs.map(t => JSON.stringify(t) + '\n').join(''), count: txs.length };
      }
    }
    throw new Error(`unexpected call ${fn}`);
  }
}

/** Web Locks across "tabs" sharing one storage. */
function sharedLocks() {
  const held = new Map<string, () => void>();
  const locks = (): ScopeLocks => ({
    async acquire(name, onLost, steal) {
      if (held.has(name) && !steal) return null;
      held.get(name)?.();
      held.set(name, onLost);
      return () => held.delete(name);
    },
  });
  return locks;
}

const id = (s: Scopes) => s.state().current!.id;

test('a new page makes a scope of the demo, and edits go to the log one segment per batch', async () => {
  const storage = new MemoryScopeStorage();
  const engine = new FakeEngine();
  engine.apply(3); // the demo
  const scopes = new Scopes(engine, storage, null);
  let prepared = false;
  assert.equal(await scopes.boot(async () => { prepared = true; engine.apply(1); }), 'created');
  assert.ok(prepared, 'the showcase runs before the scope is made');
  assert.match(id(scopes), /^[0-9A-HJKMNP-TV-Z]{26}$/);
  assert.equal(engine.scope, id(scopes));
  assert.deepEqual(storage.snapshots(id(scopes)), [4]);

  engine.apply(2); await scopes.flush();
  engine.apply(1); await scopes.flush();
  await scopes.flush(); // nothing new: no empty segment
  assert.deepEqual(storage.segments(id(scopes)), [5, 7]);

  // A reload opens it: the snapshot, then every logged transaction.
  const next = new FakeEngine();
  const reopened = new Scopes(next, storage, null);
  assert.equal(await reopened.boot(async () => assert.fail('an opened scope adds no showcase')), 'opened');
  assert.equal(next.opened[0].id, id(scopes));
  assert.deepEqual(next.opened[0].log.map(l => JSON.parse(l).version), [5, 6, 7]);
  assert.equal(next.version, 7);
});

test('the log is folded into a snapshot past a threshold and when the feed has a gap', async () => {
  const storage = new MemoryScopeStorage();
  const engine = new FakeEngine();
  const scopes = new Scopes(engine, storage, null);
  scopes.compactAt = { transactions: 5, bytes: 1e9 };
  await scopes.boot(async () => {});
  const sid = id(scopes);

  engine.apply(3); await scopes.flush();
  assert.deepEqual(storage.segments(sid), [1]);
  engine.apply(3); await scopes.flush();
  assert.deepEqual(storage.segments(sid), [], 'six logged: compacted');
  assert.deepEqual(storage.snapshots(sid), [6]);

  // More than the horizon between two flushes: the feed cannot supply the
  // oldest, so the snapshot takes their place.
  engine.apply(250); await scopes.flush();
  assert.deepEqual(storage.snapshots(sid), [256]);
  assert.deepEqual(storage.segments(sid), []);
  engine.apply(1); await scopes.flush();
  assert.deepEqual(storage.segments(sid), [257], 'and the feed resumes after it');
});

test('a plain load replaces the open scope\'s contents with a new snapshot', async () => {
  const storage = new MemoryScopeStorage();
  const engine = new FakeEngine();
  const scopes = new Scopes(engine, storage, null);
  await scopes.boot(async () => {});
  const sid = id(scopes);
  engine.apply(2); await scopes.flush();
  // `__iron.load(text)`: another document, with no scope.
  engine.scope = null; engine.version = 40; engine.log = [];
  await scopes.flush();
  assert.equal(engine.scope, sid, 'it is given this scope');
  assert.deepEqual(storage.snapshots(sid), [40]);
  assert.deepEqual(storage.segments(sid), []);
});

test('a transaction that fails to replay keeps what loaded and sets the rest aside', async () => {
  const storage = new MemoryScopeStorage();
  const first = new FakeEngine();
  const scopes = new Scopes(first, storage, null);
  await scopes.boot(async () => {});
  const sid = id(scopes);
  first.apply(2); await scopes.flush();
  first.apply(2); await scopes.flush();

  const next = new FakeEngine();
  next.failAt = 3;
  const reopened = new Scopes(next, storage, null);
  await reopened.boot(async () => {});
  assert.equal(next.version, 2);
  assert.match(reopened.state().notice, /could not be restored/);
  assert.equal(storage.broken(sid), 1);
  assert.deepEqual(storage.segments(sid), []);
  assert.deepEqual(storage.snapshots(sid), [2], 'what loaded is the new snapshot');
});

test('a second tab opens the scope read-only, writes nothing, and can take it over', async () => {
  const storage = new MemoryScopeStorage();
  const locks = sharedLocks();
  const a = new FakeEngine();
  const tabA = new Scopes(a, storage, locks());
  await tabA.boot(async () => {});
  const sid = id(tabA);

  const b = new FakeEngine();
  const tabB = new Scopes(b, storage, locks());
  assert.equal(await tabB.boot(async () => {}), 'opened');
  assert.ok(tabB.state().readOnly);
  b.apply(3); await tabB.flush();
  assert.deepEqual(storage.segments(sid), [], 'the read-only tab wrote nothing');

  a.apply(2); await tabA.flush();
  assert.deepEqual(storage.segments(sid), [1]);

  // "Open here instead": B takes the lock, reopens from storage (A's writes
  // included), and A becomes read-only.
  await tabB.takeOver();
  assert.ok(!tabB.state().readOnly);
  assert.ok(tabA.state().readOnly);
  assert.equal(b.version, 2);
  a.apply(1); await tabA.flush();
  b.apply(1); await tabB.flush();
  assert.deepEqual(storage.segments(sid), [1, 3]);
});

test('an engine restart writes nothing until the scope is reopened', async () => {
  const storage = new MemoryScopeStorage();
  const engine = new FakeEngine();
  const scopes = new Scopes(engine, storage, null);
  await scopes.boot(async () => {});
  const sid = id(scopes);
  engine.apply(2); await scopes.flush();

  // The new engine holds the demo, unnamed.
  scopes.pause();
  engine.scope = null; engine.version = 9; engine.log = [];
  await scopes.flush();
  assert.deepEqual(storage.snapshots(sid), [0], 'the demo did not overwrite the scope');
  await scopes.reopen();
  assert.equal(engine.version, 2);
  assert.equal(engine.scope, sid);
});

test('opening a saved file makes a new scope and leaves the old one compacted', async () => {
  const storage = new MemoryScopeStorage();
  const engine = new FakeEngine();
  const scopes = new Scopes(engine, storage, null);
  await scopes.boot(async () => {});
  const old = id(scopes);
  engine.apply(2); scopes.changed(engine.version);
  await scopes.importText('{"schema": 8, "next_id": 1, "next_rel": 1, "version": 12,\n "nodes": {}}', 'imported');
  assert.notEqual(id(scopes), old);
  assert.equal(scopes.state().current!.name, 'imported');
  assert.deepEqual(storage.snapshots(old), [2], 'the edits before the switch were kept');
  assert.deepEqual(storage.snapshots(id(scopes)), [12]);
  assert.equal(await storage.last(), id(scopes));
});

test('the scope menu: new, rename, open, duplicate, export, delete', async () => {
  const storage = new MemoryScopeStorage();
  const engine = new FakeEngine();
  const scopes = new Scopes(engine, storage, null);
  await scopes.boot(async () => {});
  const first = id(scopes);
  engine.apply(2); scopes.changed(engine.version);

  await scopes.create('empty');
  const blank = id(scopes);
  assert.notEqual(blank, first);
  assert.equal(scopes.state().current!.name, 'Untitled');
  assert.equal(engine.version, 1, 'the empty template is open');
  assert.deepEqual(storage.snapshots(first), [2], 'switching wrote the first scope');

  await scopes.rename('  Notes  ');
  assert.equal((await storage.list()).find(m => m.id === blank)!.name, 'Notes');

  await scopes.open(first);
  assert.equal(id(scopes), first);
  assert.equal(engine.version, 2);
  assert.equal(await storage.last(), first, 'boot opens it next time');

  engine.apply(1);
  await scopes.duplicate();
  const copy = id(scopes);
  assert.equal(scopes.state().current!.name, 'Demo copy');
  assert.deepEqual(storage.snapshots(copy), [3], 'the copy has the unsaved edit');
  assert.deepEqual(storage.snapshots(first), [3], 'and so does the original');

  const out = await scopes.exportText();
  assert.equal(out.name, 'Demo copy');
  assert.match(out.text, /"version": 3/);

  const listed = await scopes.list();
  assert.equal(listed.length, 3);
  assert.ok(listed.every(m => m.bytes > 0));

  await scopes.remove(blank);
  assert.equal((await storage.list()).length, 2, 'another scope deleted');
  assert.equal(id(scopes), copy, 'the open one stays open');

  await scopes.remove(copy);
  assert.equal(id(scopes), first, 'deleting the open one opens the most recent other');
  await scopes.remove(first);
  assert.equal(scopes.state().current!.name, 'Untitled', 'none left: a new empty one');
  assert.equal((await storage.list()).length, 1);
});

test('a scope open in another tab cannot be deleted from here', async () => {
  const storage = new MemoryScopeStorage();
  const locks = sharedLocks();
  const tabA = new Scopes(new FakeEngine(), storage, locks());
  await tabA.boot(async () => {});
  const a = id(tabA);
  const tabB = new Scopes(new FakeEngine(), storage, locks());
  await tabB.boot(async () => {});
  await tabB.create('demo');
  await assert.rejects(tabB.remove(a), /another tab/);
  assert.ok(await storage.read(a), 'still there');
});

test('the app opens the scope its URL names, a new one, or says the scope is missing', async () => {
  const storage = new MemoryScopeStorage();
  const engine = new FakeEngine();
  const first = new Scopes(engine, storage, null);
  assert.equal(await first.boot(async () => {}, { create: 'empty' }), 'created');
  const a = id(first);
  await first.close();
  assert.equal(first.state().current, null, 'closed: back to the scopes page');
  const second = new Scopes(engine, storage, null);
  assert.equal(await second.boot(async () => {}, { create: 'demo' }), 'created');
  const b = id(second);
  assert.notEqual(a, b);
  const third = new Scopes(new FakeEngine(), storage, null);
  assert.equal(await third.boot(async () => {}, { open: a }), 'opened');
  assert.equal(id(third), a, 'the named scope, not the last one');
  const fourth = new Scopes(new FakeEngine(), storage, null);
  assert.equal(await fourth.boot(async () => {}, { open: 'NOSUCHSCOPE' }), 'missing');
  assert.equal(fourth.state().current, null);
});

test('the scopes page lists, renames, duplicates, imports, exports and deletes without an engine', async () => {
  const storage = new MemoryScopeStorage();
  const locks = sharedLocks();
  const engine = new FakeEngine();
  engine.apply(3);
  const app = new Scopes(engine, storage, locks());
  await app.boot(async () => {});
  const a = id(app);
  engine.apply(2);
  await app.flush(); // two transactions in the log, past the snapshot
  const library = new ScopeLibrary(storage, locks());
  assert.deepEqual((await library.list()).map(m => m.name), ['Demo']);
  assert.equal(await library.busy(a), true, 'the app holds it');
  await assert.rejects(library.remove(a), /another tab/);
  assert.equal(await library.exportText(a), null, 'its log needs the engine to replay');

  const copy = await library.duplicate(a);
  const stored = await storage.read(copy);
  assert.equal(stored!.meta.name, 'Demo copy');
  assert.equal(stored!.log.length, 2, 'the log came along');
  const next = new FakeEngine();
  const reopened = new Scopes(next, storage, null);
  await reopened.boot(async () => {}, { open: copy });
  assert.equal(next.version, 5, 'the copy replays to where the original was');

  const imported = await library.importText(await new FakeEngine().save(), 'Imported');
  await library.rename(imported, '  Notes ');
  assert.equal((await library.list()).find(m => m.id === imported)!.name, 'Notes');
  assert.ok((await library.exportText(imported))!.text.includes('"version": 0'), 'a scope with no log exports its snapshot');
  await assert.rejects(library.importText('not json', 'x'), /not an Iron document/);

  await app.close();
  assert.equal(await library.busy(a), false, 'closing gives the lock back');
  await library.remove(a);
  assert.deepEqual((await library.list()).map(m => m.name).sort(), ['Demo copy', 'Notes']);
});
