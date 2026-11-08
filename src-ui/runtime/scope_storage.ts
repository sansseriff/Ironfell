/**
 * Where scopes live (plans/specs/scopes.md §4): per scope, versioned
 * snapshots, the log as one small segment file per batch of committed
 * transactions, and metadata. The engine provides the snapshot and the
 * transactions; this only stores them. The web implementation is OPFS on
 * Chromium; the memory one serves tests, `?demo` and the native build. When a
 * server arrives, a server-backed implementation plugs in here (§8).
 *
 *   <OPFS root>/iron/
 *     index.json                      { last }: the scope to open at boot
 *     scopes/<id>/meta.json           ScopeMeta
 *     scopes/<id>/snapshot-<v>.json   the canonical document; normally one
 *     scopes/<id>/log/<first>.ndjson  one segment per batch
 *     scopes/<id>/log-broken-<time>/  segments a replay could not apply
 *     scopes/<id>/workspace.json      the dock layout (D10): which views are
 *                                     open, where; workspace state, never
 *                                     in the document or its undo
 *     scopes/<id>/chats.json          the chat panels' conversations
 *                                     (src-ui/chat/sessions.svelte.ts):
 *                                     workspace state too
 */

export interface ScopeMeta {
  id: string;
  name: string;
  /** Milliseconds since the epoch. */
  created: number;
  modified: number;
}

export interface StoredScope {
  meta: ScopeMeta;
  /** The newest snapshot that parses, and its version. */
  snapshot: string;
  version: number;
  /** Every logged transaction newer than the snapshot's oldest segment, one
   *  JSON line each, in version order (the engine skips ones the snapshot
   *  already holds). */
  log: string[];
  /** Bytes of log, for the compaction threshold. */
  logBytes: number;
  /** The dock layout last saved with it (`workspace.json`), if any. */
  workspace: string | null;
}

export interface ScopeStorage {
  readonly kind: 'opfs' | 'memory';
  list(): Promise<ScopeMeta[]>;
  /** The scope boot opens, if it still exists. */
  last(): Promise<string | null>;
  setLast(id: string): Promise<void>;
  create(meta: ScopeMeta, snapshot: string, version: number): Promise<void>;
  read(id: string): Promise<StoredScope | null>;
  /** One new segment holding `ndjson`, whose first transaction is `first`. */
  append(id: string, first: number, ndjson: string): Promise<void>;
  /** Write `snapshot` at `version`, then drop older snapshots and the
   *  segments it covers. A crash at any point leaves one whole snapshot. */
  compact(id: string, snapshot: string, version: number): Promise<void>;
  /** Move every segment to `log-broken-<time>/`, kept for inspection. */
  setAside(id: string): Promise<void>;
  writeMeta(meta: ScopeMeta): Promise<void>;
  /** Replace the scope's dock layout (`workspace.json`). */
  writeWorkspace(id: string, json: string): Promise<void>;
  /** The scope's chat conversations (`chats.json`), if any were saved. */
  readChats(id: string): Promise<string | null>;
  /** Replace the scope's chat conversations (`chats.json`). */
  writeChats(id: string, json: string): Promise<void>;
  remove(id: string): Promise<void>;
  /** Bytes the scope's files hold (snapshots, log, set-aside segments). */
  size(id: string): Promise<number>;
  /** Storage used and available to the origin, when the browser says. */
  estimate(): Promise<{ usage: number; quota: number } | null>;
}

/** 128 random bits as 26 Crockford base32 characters, the top digit holding
 *  three (`iron_document::ScopeId`). The first 48 bits are the time, as a
 *  ULID's are, so ids created later sort later (D1). */
export function newScopeId(now = Date.now()): string {
  const bytes = new Uint8Array(16);
  crypto.getRandomValues(bytes);
  let t = BigInt(Math.floor(now));
  for (let i = 5; i >= 0; i--) { bytes[i] = Number(t & 0xffn); t >>= 8n; }
  let n = 0n;
  for (const b of bytes) n = (n << 8n) | BigInt(b);
  const alphabet = '0123456789ABCDEFGHJKMNPQRSTVWXYZ';
  let out = '';
  for (let i = 25; i >= 0; i--) out += alphabet[Number((n >> BigInt(5 * i)) & 31n)];
  return out;
}

/** The version a canonical snapshot records, read from its first line. */
export function snapshotVersion(snapshot: string): number {
  const m = /"version":\s*(\d+)/.exec(snapshot.slice(0, 300));
  if (!m) throw new Error('snapshot has no version');
  return Number(m[1]);
}

function parses(text: string): boolean {
  try { JSON.parse(text); return true; } catch { return false; }
}

const pad = (n: number) => String(n).padStart(12, '0');
const lines = (ndjson: string) => ndjson.split('\n').filter(l => l.trim() !== '');

// ---- memory ---------------------------------------------------------------

interface MemoryScope { meta: ScopeMeta; snapshots: Map<number, string>; segments: Map<number, string>; broken: Map<string, Map<number, string>>; workspace: string | null; chats?: string | null }

export class MemoryScopeStorage implements ScopeStorage {
  readonly kind = 'memory' as const;
  private scopes = new Map<string, MemoryScope>();
  private lastId: string | null = null;

  async list() { return [...this.scopes.values()].map(s => ({ ...s.meta })); }
  async last() { return this.lastId && this.scopes.has(this.lastId) ? this.lastId : null; }
  async setLast(id: string) { this.lastId = id; }
  async create(meta: ScopeMeta, snapshot: string, version: number) {
    this.scopes.set(meta.id, { meta: { ...meta }, snapshots: new Map([[version, snapshot]]), segments: new Map(), broken: new Map(), workspace: null });
  }
  async read(id: string): Promise<StoredScope | null> {
    const s = this.scopes.get(id);
    if (!s) return null;
    const snap = [...s.snapshots].sort((a, b) => b[0] - a[0]).find(([, t]) => parses(t));
    if (!snap) return null;
    const segs = [...s.segments].sort((a, b) => a[0] - b[0]).map(([, t]) => t);
    return { meta: { ...s.meta }, snapshot: snap[1], version: snap[0], log: segs.flatMap(lines), logBytes: segs.reduce((n, t) => n + t.length, 0), workspace: s.workspace };
  }
  async append(id: string, first: number, ndjson: string) {
    this.scopes.get(id)?.segments.set(first, ndjson);
  }
  async compact(id: string, snapshot: string, version: number) {
    const s = this.scopes.get(id);
    if (!s) return;
    s.snapshots.set(version, snapshot);
    for (const v of [...s.snapshots.keys()]) if (v < version) s.snapshots.delete(v);
    for (const first of [...s.segments.keys()]) if (first <= version) s.segments.delete(first);
  }
  async setAside(id: string) {
    const s = this.scopes.get(id);
    if (!s || s.segments.size === 0) return;
    s.broken.set(`log-broken-${Date.now()}`, s.segments);
    s.segments = new Map();
  }
  async writeMeta(meta: ScopeMeta) {
    const s = this.scopes.get(meta.id);
    if (s) s.meta = { ...meta };
  }
  async writeWorkspace(id: string, json: string) {
    const s = this.scopes.get(id);
    if (s) s.workspace = json;
  }
  async readChats(id: string) { return this.scopes.get(id)?.chats ?? null; }
  async writeChats(id: string, json: string) {
    const s = this.scopes.get(id);
    if (s) s.chats = json;
  }
  async remove(id: string) { this.scopes.delete(id); }
  async size(id: string) {
    const s = this.scopes.get(id);
    if (!s) return 0;
    const sum = (m: Map<number, string>) => [...m.values()].reduce((n, t) => n + t.length, 0);
    return sum(s.snapshots) + sum(s.segments) + [...s.broken.values()].reduce((n, m) => n + sum(m), 0) + JSON.stringify(s.meta).length + (s.workspace?.length ?? 0) + (s.chats?.length ?? 0);
  }
  async estimate() { return null; }

  /** For tests: the segment files, by first version. */
  segments(id: string): number[] { return [...(this.scopes.get(id)?.segments.keys() ?? [])].sort((a, b) => a - b); }
  snapshots(id: string): number[] { return [...(this.scopes.get(id)?.snapshots.keys() ?? [])].sort((a, b) => a - b); }
  broken(id: string): number { return this.scopes.get(id)?.broken.size ?? 0; }
}

// ---- OPFS -----------------------------------------------------------------

/** OPFS with writable files: Chromium, and WebKit (Safari, the native
 *  build's WKWebView) where `createWritable` exists; older WebKit has the
 *  directory but not the writer, and gets an unsaved page instead. */
export function opfsAvailable(): boolean {
  return typeof navigator !== 'undefined' && typeof navigator.storage?.getDirectory === 'function'
    && typeof FileSystemFileHandle !== 'undefined' && typeof FileSystemFileHandle.prototype.createWritable === 'function';
}

export class OpfsScopeStorage implements ScopeStorage {
  readonly kind = 'opfs' as const;
  private rootDir: Promise<FileSystemDirectoryHandle> | null = null;
  private persistAsked = false;

  private root(): Promise<FileSystemDirectoryHandle> {
    this.rootDir ??= navigator.storage.getDirectory().then(d => d.getDirectoryHandle('iron', { create: true }));
    return this.rootDir;
  }
  private async scopesDir() { return (await this.root()).getDirectoryHandle('scopes', { create: true }); }
  private async scopeDir(id: string, create = false): Promise<FileSystemDirectoryHandle | null> {
    try { return await (await this.scopesDir()).getDirectoryHandle(id, { create }); } catch { return null; }
  }
  private async logDir(dir: FileSystemDirectoryHandle) { return dir.getDirectoryHandle('log', { create: true }); }

  private async write(dir: FileSystemDirectoryHandle, name: string, text: string) {
    const file = await dir.getFileHandle(name, { create: true });
    // A writable commits atomically on close (a swap file until then).
    const w = await file.createWritable();
    await w.write(text);
    await w.close();
  }
  private async readText(dir: FileSystemDirectoryHandle, name: string): Promise<string | null> {
    try { return await (await (await dir.getFileHandle(name)).getFile()).text(); } catch { return null; }
  }
  private async names(dir: FileSystemDirectoryHandle): Promise<{ name: string; kind: string }[]> {
    const out: { name: string; kind: string }[] = [];
    for await (const [name, handle] of (dir as any).entries() as AsyncIterable<[string, FileSystemHandle]>) out.push({ name, kind: handle.kind });
    return out;
  }
  private static versionOf(name: string, prefix: string, suffix: string): number | null {
    if (!name.startsWith(prefix) || !name.endsWith(suffix)) return null;
    const n = Number(name.slice(prefix.length, name.length - suffix.length));
    return Number.isInteger(n) ? n : null;
  }

  async list(): Promise<ScopeMeta[]> {
    const out: ScopeMeta[] = [];
    const dir = await this.scopesDir();
    for (const { name, kind } of await this.names(dir)) {
      if (kind !== 'directory') continue;
      const text = await this.readText(await dir.getDirectoryHandle(name), 'meta.json');
      if (text && parses(text)) out.push(JSON.parse(text));
    }
    return out.sort((a, b) => b.modified - a.modified);
  }
  async last(): Promise<string | null> {
    const text = await this.readText(await this.root(), 'index.json');
    const id = text && parses(text) ? JSON.parse(text).last : null;
    return typeof id === 'string' && (await this.scopeDir(id)) ? id : null;
  }
  async setLast(id: string) { await this.write(await this.root(), 'index.json', JSON.stringify({ last: id })); }

  async create(meta: ScopeMeta, snapshot: string, version: number) {
    const dir = (await this.scopeDir(meta.id, true))!;
    await this.write(dir, `snapshot-${pad(version)}.json`, snapshot);
    await this.write(dir, 'meta.json', JSON.stringify(meta));
    await this.logDir(dir);
    // Ask once that the browser not evict scopes under storage pressure.
    if (!this.persistAsked) { this.persistAsked = true; void navigator.storage.persist?.().catch(() => {}); }
  }

  async read(id: string): Promise<StoredScope | null> {
    const dir = await this.scopeDir(id);
    if (!dir) return null;
    const metaText = await this.readText(dir, 'meta.json');
    if (!metaText || !parses(metaText)) return null;
    const snaps = (await this.names(dir))
      .map(e => ({ name: e.name, v: OpfsScopeStorage.versionOf(e.name, 'snapshot-', '.json') }))
      .filter((e): e is { name: string; v: number } => e.v !== null)
      .sort((a, b) => b.v - a.v);
    let snapshot: string | null = null, version = 0;
    for (const s of snaps) {
      const text = await this.readText(dir, s.name);
      if (text && parses(text)) { snapshot = text; version = s.v; break; }
    }
    if (snapshot === null) return null;
    const log = await this.logDir(dir);
    const segs = (await this.names(log))
      .map(e => ({ name: e.name, v: OpfsScopeStorage.versionOf(e.name, '', '.ndjson') }))
      .filter((e): e is { name: string; v: number } => e.v !== null)
      .sort((a, b) => a.v - b.v);
    const out: string[] = [];
    let logBytes = 0;
    for (const s of segs) {
      const text = (await this.readText(log, s.name)) ?? '';
      logBytes += text.length;
      out.push(...lines(text));
    }
    return { meta: JSON.parse(metaText), snapshot, version, log: out, logBytes, workspace: await this.readText(dir, 'workspace.json') };
  }

  async append(id: string, first: number, ndjson: string) {
    const dir = await this.scopeDir(id);
    if (dir) await this.write(await this.logDir(dir), `${pad(first)}.ndjson`, ndjson);
  }

  async compact(id: string, snapshot: string, version: number) {
    const dir = await this.scopeDir(id);
    if (!dir) return;
    await this.write(dir, `snapshot-${pad(version)}.json`, snapshot);
    for (const { name } of await this.names(dir)) {
      const v = OpfsScopeStorage.versionOf(name, 'snapshot-', '.json');
      if (v !== null && v < version) await dir.removeEntry(name).catch(() => {});
    }
    const log = await this.logDir(dir);
    for (const { name } of await this.names(log)) {
      const v = OpfsScopeStorage.versionOf(name, '', '.ndjson');
      if (v !== null && v <= version) await log.removeEntry(name).catch(() => {});
    }
  }

  async setAside(id: string) {
    const dir = await this.scopeDir(id);
    if (!dir) return;
    const log = await this.logDir(dir);
    const entries = await this.names(log);
    if (entries.length === 0) return;
    const broken = await dir.getDirectoryHandle(`log-broken-${Date.now()}`, { create: true });
    for (const { name } of entries) {
      const text = await this.readText(log, name);
      if (text !== null) await this.write(broken, name, text);
      await log.removeEntry(name).catch(() => {});
    }
  }

  async writeMeta(meta: ScopeMeta) {
    const dir = await this.scopeDir(meta.id);
    if (dir) await this.write(dir, 'meta.json', JSON.stringify(meta));
  }

  async writeWorkspace(id: string, json: string) {
    const dir = await this.scopeDir(id);
    if (dir) await this.write(dir, 'workspace.json', json);
  }

  async readChats(id: string) {
    const dir = await this.scopeDir(id);
    return dir ? this.readText(dir, 'chats.json') : null;
  }

  async writeChats(id: string, json: string) {
    const dir = await this.scopeDir(id);
    if (dir) await this.write(dir, 'chats.json', json);
  }

  async remove(id: string) {
    await (await this.scopesDir()).removeEntry(id, { recursive: true }).catch(() => {});
  }

  async size(id: string): Promise<number> {
    const dir = await this.scopeDir(id);
    if (!dir) return 0;
    const walk = async (d: FileSystemDirectoryHandle): Promise<number> => {
      let n = 0;
      for await (const [, handle] of (d as any).entries() as AsyncIterable<[string, FileSystemHandle]>) {
        if (handle.kind === 'file') n += (await (handle as FileSystemFileHandle).getFile()).size;
        else n += await walk(handle as FileSystemDirectoryHandle);
      }
      return n;
    };
    return walk(dir);
  }

  async estimate() {
    try {
      const e = await navigator.storage.estimate();
      return { usage: e.usage ?? 0, quota: e.quota ?? 0 };
    } catch {
      return null;
    }
  }
}
