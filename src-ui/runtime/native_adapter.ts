import type { DocumentApi } from '../model/api';
import { applyEdit } from '../model/edit';
import { ask } from '../model/agent';
import { AnthropicTransport } from '../model/transport';

/** Same UI protocol as the web adapters. Native display callbacks drive Bevy;
 * this adapter batches control traffic and never sends frame ticks. */
export class NativeAdapter {
  onmessage: ((event: { data: any }) => void) | null = null;
  private nativePointer = false;
  private generation = 0;
  private sequence = 0;
  private pending: any[] = [];
  private sending = false;
  private disposed = false;
  private requestId = -1;
  private publicRequests = new Map<number, { ty: string; timer: ReturnType<typeof setTimeout> }>();
  private queries = new Map<number, { resolve: (text: string) => void; reject: (e: Error) => void; timer: ReturnType<typeof setTimeout> }>();
  private core = (window as any).__TAURI__.core;
  private channel: any;

  constructor() {
    this.channel = new this.core.Channel();
    const early: any[] = [];
    this.channel.onmessage = (data: any) => this.generation ? this.receive(data) : early.push(data);
    this.core.invoke('open_session', { channel: this.channel }).then((generation: number) => {
      this.generation = generation;
      if (this.disposed) { void this.core.invoke('close_session', { generation }); return; }
      for (const data of early) this.receive(data);
      this.flush();
    }).catch((e: unknown) => this.fail(e));
  }

  postMessage(data: any) {
    if (this.disposed) return;
    if (data.ty === "mousemove" && this.nativePointer) return;
    if (data.requestId >= 0 && !this.publicRequests.has(data.requestId)) {
      const replies: Record<string, string> = { documentSave: 'documentSaved', documentLoad: 'documentLoaded', modelEdit: 'modelEditResult', modelAsk: 'modelAskResult' };
      const ty = replies[data.ty] ?? data.ty;
      const timer = setTimeout(() => { this.publicRequests.delete(data.requestId); this.emit({ty, requestId: data.requestId, ok:false, error:'native request timed out'}); }, data.ty === 'modelAsk' ? 120000 : 15000);
      this.publicRequests.set(data.requestId, { ty, timer });
    }
    if (data.ty === 'modelEdit' || data.ty === 'modelAsk') {
      const operation = data.ty === 'modelEdit'
        ? applyEdit(this.documentApi(), data.xml, data.label)
        : ask(this.documentApi(), new AnthropicTransport(data.apiKey, data.model), data.instruction);
      void operation.then((result) => this.receive({ ty: data.ty + 'Result', requestId: data.requestId, ok: result.ok, [data.ty === 'modelEdit' ? 'outcome' : 'result']: result }))
        .catch((e: unknown) => this.receive({ ty: data.ty + 'Result', requestId: data.requestId, ok: false, error: String(e) }));
      return;
    }
    // The host lifetime belongs to the native window; session disposal only
    // detaches this webview generation, so reload retains the authored document.
    if (['init', 'wasmData', 'releaseApp', 'stopRunning', 'startRunning'].includes(data.ty)) return;
    if (data.ty === 'mousemove' && this.pending.at(-1)?.ty === 'mousemove') this.pending[this.pending.length - 1] = structuredClone(data);
    else this.pending.push(structuredClone(data));
    if (this.pending.length > 1024) { this.fail('native control queue full'); return; }
    queueMicrotask(() => this.flush());
  }

  private async flush() {
    if (!this.generation || this.sending || this.disposed || !this.pending.length) return;
    this.sending = true;
    try {
      while (this.pending.length && !this.disposed) {
        const messages = this.pending.splice(0, 256);
        await this.core.invoke('send_batch', { generation: this.generation, sequence: ++this.sequence, messages });
      }
    } catch (e) { this.fail(e); }
    finally { this.sending = false; }
  }
  private emit(data: any) { if (!this.disposed) this.onmessage?.({ data }); }
  private receive(data: any) {
    if (data.ty === "enginePrepared") this.nativePointer = data.pointerSource === "native";
    const pending = this.publicRequests.get(data.requestId);
    if (pending) { clearTimeout(pending.timer); this.publicRequests.delete(data.requestId); }
    const query = this.queries.get(data.requestId);
    if (query) {
      this.queries.delete(data.requestId); clearTimeout(query.timer);
      if (data.ok) query.resolve(data.text); else query.reject(new Error(data.error));
    } else this.emit(data);
  }
  private query(fn: string, args: any[] = []): Promise<string> {
    return new Promise((resolve, reject) => {
      const requestId = this.requestId--;
      const timer = setTimeout(() => { this.queries.delete(requestId); reject(new Error('native document call timed out')); }, 15000);
      this.queries.set(requestId, { resolve, reject, timer });
      this.postMessage({ ty: 'documentCall', requestId, fn, args });
    });
  }
  private documentApi(): DocumentApi {
    const json = async (fn: string, args: any[] = []) => JSON.parse(await this.query(fn, args));
    return {
      schema: () => json('schema'),
      view: q => this.query('view', [JSON.stringify({ scope: null, depth: null, ...q })]),
      version: async () => Number((await this.query('view', [JSON.stringify({ scope: null, fidelity: 'skeleton', depth: 0 })])).match(/version="(\d+)"/)?.[1] ?? 0),
      nextIds: n => json('next_ids', [n]),
      nextRelationIds: n => json('next_relation_ids', [n]),
      place: (parent, after, count) => json('place', [parent, after, count]),
      apply: (tx, version) => json('apply', [JSON.stringify(tx), version]),
      link: (a, b, keep, actor) => json('link', [a, b, keep, actor]),
      selection: () => json('selection'),
      importSvg: (text, parent, x, y, actor) => json('import_svg_at', [text, parent, x, y, actor]),
    };
  }
  private fail(error: unknown) {
    console.error('Native adapter failed', error);
    this.dispose();
  }
  dispose() {
    if (this.disposed) return;
    for (const [requestId, pending] of this.publicRequests) { clearTimeout(pending.timer); this.emit({ty:pending.ty, requestId, ok:false, error:'native session disposed'}); }
    this.publicRequests.clear();
    this.disposed = true; this.pending = [];
    for (const query of this.queries.values()) { clearTimeout(query.timer); query.reject(new Error('native session disposed')); }
    this.queries.clear();
    if (this.generation) void this.core.invoke('close_session', { generation: this.generation });
    this.channel.onmessage = () => {};
    this.onmessage = null;
  }
}
