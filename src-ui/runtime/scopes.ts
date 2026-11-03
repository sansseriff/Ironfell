/**
 * The open scope, kept durable (plans/specs/scopes.md §4): boot opens the
 * last scope (or makes one from the demo), every committed transaction goes
 * to the log in batches, and the log is compacted into a fresh snapshot when
 * it grows, when the feed reports a gap, and when switching scopes.
 *
 * The engine provides the snapshot (`save`) and the transactions
 * (`scope_log`); storage is a `ScopeStorage`. One tab writes a scope at a
 * time, under a Web Lock; another tab opens it read-only. Plain TypeScript
 * (no runes) so it runs under `bun test`; the document client mirrors its
 * state into reactive fields through `onChange`.
 */
import { newScopeId, snapshotVersion, type ScopeMeta, type ScopeStorage, type StoredScope } from './scope_storage';

export interface ScopeEngine {
  /** A document call, its JSON reply parsed. */
  call(fn: string, args: unknown[]): Promise<any>;
  save(): Promise<string>;
}

/** Exclusive ownership of a scope across tabs. `acquire` resolves to a
 *  release function, or null when another tab holds it; `onLost` runs if
 *  another tab takes it later ("open here instead"). */
export interface ScopeLocks {
  acquire(name: string, onLost: () => void, steal?: boolean): Promise<(() => void) | null>;
}

export function webLocks(): ScopeLocks | null {
  const locks = typeof navigator !== 'undefined' ? navigator.locks : undefined;
  if (!locks) return null;
  return {
    acquire: (name, onLost, steal = false) => new Promise(resolve => {
      const options: LockOptions = steal ? { steal: true } : { ifAvailable: true };
      locks.request(name, options, lock => {
        if (!lock) { resolve(null); return; }
        return new Promise<void>(release => resolve(release));
      }).catch(() => onLost());
    }),
  };
}

export interface ScopeState {
  current: ScopeMeta | null;
  readOnly: boolean;
  notice: string;
  storage: 'opfs' | 'memory' | 'off';
}

interface LogReply { pending: boolean; scope?: string | null; version?: number; gap?: boolean; ndjson?: string; count?: number }

export class Scopes {
  /** When the log is long enough to fold into a snapshot (§4.2). */
  compactAt = { transactions: 2000, bytes: 2 * 1024 * 1024 };
  /** The longest a committed transaction waits to be written. */
  batchMs = 500;
  onChange: (state: ScopeState) => void = () => {};

  private current: ScopeMeta | null = null;
  private readOnly = false;
  private notice = '';
  private release: (() => void) | null = null;
  /** The newest version the storage holds, in the snapshot or the log. */
  private persisted = 0;
  private logCount = 0;
  private logBytes = 0;
  private timer: ReturnType<typeof setTimeout> | null = null;
  /** Between an engine restart and reopening: the new engine holds the
   *  demo, which must not be written over the scope. */
  private paused = false;
  /** Storage work runs one step at a time, in order. */
  private queue: Promise<unknown> = Promise.resolve();

  constructor(private engine: ScopeEngine, private storage: ScopeStorage, private locks: ScopeLocks | null) {}

  state(): ScopeState {
    return { current: this.current && { ...this.current }, readOnly: this.readOnly, notice: this.notice, storage: this.storage.kind };
  }
  private emit() { this.onChange(this.state()); }
  private serial<T>(work: () => Promise<T>): Promise<T> {
    const next = this.queue.then(work, work);
    this.queue = next.catch(() => {});
    return next;
  }
  private lockName(id: string) { return `iron-scope-${id}`; }

  /**
   * Open what the page's URL names (`route.ts`): a stored scope, a new one
   * (`demo`: let `prepare` finish the demo the engine opened, and make a
   * scope of it; `empty`: the empty template), or the last scope (with
   * none, a new demo). `'opened'` means the document now comes from
   * storage, so the caller adds nothing to it; `'missing'` that the scope
   * named does not exist (nothing was opened).
   */
  boot(prepare: () => Promise<void>, target: { open: string } | { create: 'empty' | 'demo' } | { last: true } = { last: true }): Promise<'opened' | 'created' | 'missing'> {
    return this.serial(async () => {
      this.paused = false;
      if ('create' in target) {
        if (target.create === 'empty') {
          const { snapshot } = await this.engine.call('scope_template', ['empty']);
          await this.adopt(snapshot, 'Untitled');
        } else {
          await prepare();
          await this.createFromOpen('Demo');
        }
        return 'created' as const;
      }
      const id = 'open' in target ? target.open : await this.storage.last();
      const stored = id ? await this.storage.read(id) : null;
      if (stored) {
        await this.openStored(stored);
        return 'opened' as const;
      }
      if ('open' in target) return 'missing' as const;
      await prepare();
      await this.createFromOpen('Demo');
      return 'created' as const;
    });
  }

  /** Leave the open scope (to the scopes page): everything written and
   *  folded into a snapshot, the lock given back. */
  close(): Promise<void> {
    return this.serial(async () => {
      await this.leave();
      this.current = null;
      this.emit();
    });
  }

  /** Whether a scope is open: a restarted engine reopens it (`reopen`). */
  get active(): boolean {
    return this.current !== null;
  }

  /** The engine is being replaced: write nothing until `reopen`. */
  pause() {
    this.paused = true;
    if (this.timer) { clearTimeout(this.timer); this.timer = null; }
  }

  /** Open the current scope again in a new engine (a settings restart),
   *  from storage, which `flush` brought up to date before the restart. */
  reopen(): Promise<void> {
    return this.serial(async () => {
      this.paused = false;
      if (!this.current) return;
      const stored = await this.storage.read(this.current.id);
      if (stored) await this.openStored(stored, true);
    });
  }

  /** A committed change happened (`stateChanged`): write it soon. */
  changed(version: number) {
    if (!this.current || this.readOnly || this.paused || version <= this.persisted || this.timer) return;
    this.timer = setTimeout(() => { this.timer = null; void this.flush(); }, this.batchMs);
  }

  /** Write what is newer than the storage now (page hidden, restart). */
  flush(): Promise<void> {
    if (this.timer) { clearTimeout(this.timer); this.timer = null; }
    return this.serial(() => this.drain());
  }

  /** Open a saved document as a new scope (D4): it gets a fresh id. */
  importText(text: string, name: string): Promise<void> {
    return this.serial(() => this.adopt(text, name));
  }

  // ---- the scope menu (§6) ---------------------------------------------

  /** Every stored scope, most recently modified first, with its size. */
  async list(): Promise<(ScopeMeta & { bytes: number })[]> {
    const metas = await this.storage.list();
    const out = await Promise.all(metas.map(async m => ({ ...m, bytes: await this.storage.size(m.id) })));
    return out.sort((a, b) => b.modified - a.modified);
  }

  estimate() {
    return this.storage.estimate();
  }

  /** A new scope from a template (`scope_template`): `empty` or `demo`. */
  create(kind: 'empty' | 'demo', name = kind === 'demo' ? 'Demo' : 'Untitled'): Promise<void> {
    return this.serial(async () => {
      const { snapshot } = await this.engine.call('scope_template', [kind]);
      await this.adopt(snapshot, name);
    });
  }

  /** Switch to a stored scope: the open one is written and compacted first. */
  open(id: string): Promise<void> {
    return this.serial(async () => {
      if (this.current?.id === id) return;
      const stored = await this.storage.read(id);
      if (!stored) throw new Error(`no scope ${id}`);
      await this.leave();
      await this.openStored(stored);
    });
  }

  rename(name: string): Promise<void> {
    return this.serial(async () => {
      const trimmed = name.trim();
      if (!this.current || this.readOnly || !trimmed || trimmed === this.current.name) return;
      this.current = { ...this.current, name: trimmed, modified: Date.now() };
      await this.storage.writeMeta(this.current);
      this.emit();
    });
  }

  /** A copy of the open scope under a new id, which becomes the open one. */
  duplicate(): Promise<void> {
    return this.serial(async () => {
      if (!this.current) return;
      const name = `${this.current.name} copy`;
      await this.drain();
      const snapshot = await this.engine.save();
      await this.adopt(snapshot, name);
    });
  }

  /** Delete a scope; it cannot be undone. Deleting the open one opens the
   *  most recent other, or a new empty scope when none is left. A scope open
   *  in another tab is refused. */
  remove(id: string): Promise<void> {
    return this.serial(async () => {
      const isCurrent = this.current?.id === id;
      if (isCurrent) {
        if (this.readOnly) throw new Error('open in another tab: delete it there');
        if (this.timer) { clearTimeout(this.timer); this.timer = null; }
        this.release?.();
        this.release = null;
        this.current = null;
      } else if (this.locks) {
        const release = await this.locks.acquire(this.lockName(id), () => {});
        if (!release) throw new Error('open in another tab: delete it there');
        release();
      }
      await this.storage.remove(id);
      if (!isCurrent) { this.emit(); return; }
      const rest = await this.storage.list();
      const next = rest.sort((a, b) => b.modified - a.modified)[0];
      const stored = next ? await this.storage.read(next.id) : null;
      if (stored) {
        await this.openStored(stored);
      } else {
        const { snapshot } = await this.engine.call('scope_template', ['empty']);
        await this.adopt(snapshot, 'Untitled');
      }
    });
  }

  /** The open scope as a saved document, everything written first. */
  exportText(): Promise<{ name: string; text: string }> {
    return this.serial(async () => {
      await this.drain();
      return { name: this.current?.name ?? 'iron-document', text: await this.engine.save() };
    });
  }

  /** "Open here instead": take the scope from the tab holding it, and
   *  reopen it from storage, which has that tab's last writes. */
  takeOver(): Promise<void> {
    return this.serial(async () => {
      if (!this.current || !this.readOnly || !this.locks) return;
      const release = await this.locks.acquire(this.lockName(this.current.id), () => this.lost(), true);
      if (!release) return;
      const stored = await this.storage.read(this.current.id);
      if (!stored) { release(); return; }
      this.release = release;
      await this.openStored(stored, true);
    });
  }

  // ---- steps (each runs inside `serial`) -----------------------------

  private async openStored(stored: StoredScope, locked = false) {
    const id = stored.meta.id;
    if (!locked) this.release = (await this.locks?.acquire(this.lockName(id), () => this.lost())) ?? null;
    this.readOnly = !!this.locks && !this.release;
    const r = await this.engine.call('scope_open', [id, stored.snapshot, stored.log]);
    this.current = stored.meta;
    this.persisted = r.version;
    this.logCount = r.replayed;
    this.logBytes = stored.logBytes;
    this.notice = this.readOnly ? 'Open in another tab: changes here are not saved.' : '';
    if (r.failed) {
      const at = `transaction ${r.failed.index + 1} of ${stored.log.length}`;
      console.warn(`[scopes] replay stopped at ${at}: ${r.failed.message}`);
      this.notice = `Some recent changes could not be restored (${at}); they were set aside.`;
      if (!this.readOnly) {
        await this.storage.setAside(id);
        await this.compact();
      }
    }
    if (!this.readOnly) await this.storage.setLast(id);
    this.emit();
  }

  /** Make `text` a new scope and open it (inside `serial`). */
  private async adopt(text: string, name: string) {
    await this.leave();
    const id = newScopeId();
    const r = await this.engine.call('scope_open', [id, text, []]);
    const now = Date.now();
    const meta: ScopeMeta = { id, name, created: now, modified: now };
    await this.storage.create(meta, text, r.version);
    await this.storage.setLast(id);
    this.release = (await this.locks?.acquire(this.lockName(id), () => this.lost())) ?? null;
    this.current = meta;
    this.readOnly = false;
    this.notice = '';
    this.persisted = r.version;
    this.logCount = this.logBytes = 0;
    this.emit();
  }

  private async createFromOpen(name: string) {
    const id = newScopeId();
    await this.engine.call('scope_assign', [id]);
    const snapshot = await this.engine.save();
    const version = snapshotVersion(snapshot);
    const now = Date.now();
    const meta: ScopeMeta = { id, name, created: now, modified: now };
    await this.storage.create(meta, snapshot, version);
    await this.storage.setLast(id);
    this.release = (await this.locks?.acquire(this.lockName(id), () => this.lost())) ?? null;
    this.current = meta;
    this.readOnly = false;
    this.persisted = version;
    this.logCount = this.logBytes = 0;
    this.emit();
  }

  /** The engine's feed, until it has nothing newer. */
  private async drain(): Promise<void> {
    if (!this.current || this.readOnly || this.paused) return;
    const r: LogReply = await this.engine.call('scope_log', [this.persisted]);
    if (r.pending) {
      await this.installed();
      return this.drain();
    }
    if (r.scope !== this.current.id) {
      // A plain `load` replaced the document: it becomes this scope's
      // contents, as a new snapshot.
      await this.engine.call('scope_assign', [this.current.id]);
      await this.compact();
      return;
    }
    if (r.gap) {
      // The history horizon dropped transactions we never wrote; the
      // snapshot holds the state they produced (§4.2).
      await this.compact();
      return;
    }
    if (!r.count) return;
    const first = r.version! - r.count + 1;
    await this.storage.append(this.current.id, first, r.ndjson!);
    this.persisted = r.version!;
    this.logCount += r.count;
    this.logBytes += r.ndjson!.length;
    if (this.logCount >= this.compactAt.transactions || this.logBytes >= this.compactAt.bytes) await this.compact();
  }

  /** A load is waiting for the engine's next frame; until then `save`
   *  would return the document it replaces. */
  private async installed() {
    for (let i = 0; i < 600; i++) {
      const r: LogReply = await this.engine.call('scope_log', [this.persisted]);
      if (!r.pending) return;
      await new Promise(resolve => setTimeout(resolve, 25));
    }
    throw new Error('the engine did not install the document');
  }

  private async compact() {
    if (!this.current || this.readOnly || this.paused) return;
    await this.installed();
    const snapshot = await this.engine.save();
    const version = snapshotVersion(snapshot);
    await this.storage.compact(this.current.id, snapshot, version);
    this.current = { ...this.current, modified: Date.now() };
    await this.storage.writeMeta(this.current);
    this.persisted = version;
    this.logCount = this.logBytes = 0;
  }

  /** Before switching scopes: write everything, fold it into a snapshot,
   *  and let other tabs have it. */
  private async leave() {
    if (this.current && !this.readOnly) {
      await this.drain();
      await this.compact();
    }
    this.release?.();
    this.release = null;
  }

  private lost() {
    this.release = null;
    if (this.timer) { clearTimeout(this.timer); this.timer = null; }
    this.readOnly = true;
    this.notice = 'Opened in another tab: changes here are not saved.';
    this.emit();
  }
}

/**
 * The scopes page's operations (route.ts, scopes.md §10.2): storage only, no
 * engine, so the page loads without the wasm. A scope opened in another tab
 * holds its Web Lock, and is not deleted from here. Making a scope from a
 * template and exporting one with unreplayed log need the engine: the page
 * opens the app for those (`?new=`, `?export`).
 */
export class ScopeLibrary {
  constructor(private storage: ScopeStorage, private locks: ScopeLocks | null) {}

  /** Every stored scope, most recently modified first, with its size. */
  async list(): Promise<(ScopeMeta & { bytes: number })[]> {
    const metas = await this.storage.list();
    const out = await Promise.all(metas.map(async m => ({ ...m, bytes: await this.storage.size(m.id) })));
    return out.sort((a, b) => b.modified - a.modified);
  }

  estimate() {
    return this.storage.estimate();
  }

  async rename(id: string, name: string): Promise<void> {
    const trimmed = name.trim();
    const meta = (await this.storage.list()).find(m => m.id === id);
    if (!meta || !trimmed || trimmed === meta.name) return;
    await this.storage.writeMeta({ ...meta, name: trimmed, modified: Date.now() });
  }

  /** Whether another tab has the scope open. */
  async busy(id: string): Promise<boolean> {
    if (!this.locks) return false;
    const release = await this.locks.acquire(`iron-scope-${id}`, () => {});
    if (!release) return true;
    release();
    return false;
  }

  async remove(id: string): Promise<void> {
    if (await this.busy(id)) throw new Error('open in another tab: close it there first');
    await this.storage.remove(id);
  }

  /** A saved document (an export, a `.json`) as a new scope; its id is
   *  assigned when the app opens it (`scope_open`). Returns the new id. */
  async importText(text: string, name: string): Promise<string> {
    let version: number;
    try {
      JSON.parse(text);
      version = snapshotVersion(text);
    } catch {
      throw new Error('not an Iron document');
    }
    const id = newScopeId();
    const now = Date.now();
    await this.storage.create({ id, name, created: now, modified: now }, text, version);
    return id;
  }

  /** A copy under a new id: the newest snapshot, then its log as one
   *  segment. Returns the new id. */
  async duplicate(id: string): Promise<string> {
    const stored = await this.storage.read(id);
    if (!stored) throw new Error(`no scope ${id}`);
    const copy = newScopeId();
    const now = Date.now();
    await this.storage.create({ id: copy, name: `${stored.meta.name} copy`, created: now, modified: now }, stored.snapshot, stored.version);
    if (stored.log.length) await this.storage.append(copy, stored.version + 1, stored.log.join('\n') + '\n');
    return copy;
  }

  /** The scope as a saved document, when its snapshot is all of it (no log
   *  since); `null` when the engine must replay the log first. */
  async exportText(id: string): Promise<{ name: string; text: string } | null> {
    const stored = await this.storage.read(id);
    if (!stored) throw new Error(`no scope ${id}`);
    return stored.log.length ? null : { name: stored.meta.name, text: stored.snapshot };
  }
}

