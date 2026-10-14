/**
 * The shell's handle on the authored document: save, load, and views.
 *
 * Everything crosses as messages keyed by `requestId`, so this works the same
 * over the worker's postMessage and the main-thread adapter. The store pushes
 * `documentChanged` whenever its version advances, and the client re-reads
 * the view it is showing. Reactive fields are Svelte 5 runes, so components
 * bind to them directly.
 *
 * Also exposed as `window.__iron` for driving and debugging:
 *   await __iron.save()            -> canonical JSON string
 *   await __iron.load(text)        -> replaces the document
 *   await __iron.view({fidelity:'summary', depth: 2})
 *   await __iron.edit('<edit>…</edit>')   -> apply markup as the model would
 *   await __iron.ask('make the circle red') -> a real model turn (needs a key)
 *   await __iron.bench(20, 200)    -> the solver benchmark in this build (JSON)
 *   __iron.selection()             -> picked node ids, in pick order
 *   await __iron.link('#a', '#b', keep?) -> link two pictures (ECM §7.2)
 *   await __iron.counterparts('#a')     -> every picture of #a's entity
 *   await __iron.focus('#a')       -> "Show in": frame #a in its panels
 *   await __iron.session()         -> the session's selection [{node, panel, chain, …}]
 *   await __iron.stress(5000)      -> add the render benchmark's `stress` composition
 *   await __iron.timings(reset?)   -> per-phase CPU render timings since the last reset
 */
export type Fidelity = 'skeleton' | 'summary' | 'full';

export interface ViewQuery {
  scope?: string | null;
  fidelity: Fidelity;
  depth?: number | null;
}

type Pending = { resolve: (v: any) => void; reject: (e: Error) => void };

export class DocumentClient {
  view = $state('');
  version = $state(0);
  fidelity: Fidelity = $state('full');
  depth: number | null = $state(null);
  error = $state('');

  // The model surface's state, for the prompt panel.
  apiKey = $state('');
  modelName = $state('claude-opus-5');
  asking = $state(false);
  lastAsk: any = $state(null);

  // What the pointer picked and hovers, as node ids (pushed by the engine).
  selection: string[] = $state([]);
  hovered: string[] = $state([]);
  /** Every picture of the first selected node's entity, for "Show in". */
  counterparts: { entity: string | null; figures: { node: string; composition: string | null; name: string | null }[] } =
    $state({ entity: null, figures: [] });
  /** The tool the pointer drives (`select`, `pen`), as the engine reports it. */
  tool = $state('select');
  /** A pending merge question from Link (ECM I5): the person picks the survivor. */
  question: any = $state(null);

  private post: ((data: any) => void) | null = null;
  private pending = new Map<number, Pending>();
  private nextId = 1;
  /** Whether `?stress=N` has been acted on (once per page). */
  private stressed = false;

  init(post: (data: any) => void) {
    this.post = post;
    this.error = '';
    for (const p of this.pending.values()) p.reject(new Error('session replaced'));
    this.pending.clear();
    try {
      this.apiKey = localStorage.getItem('iron.anthropicKey') ?? '';
    } catch {
      // storage unavailable; the key is entered per session
    }
    (window as any).__iron = {
      save: () => this.save(),
      load: (text: string) => this.load(text),
      view: (q: ViewQuery) => this.request('documentView', { query: { scope: null, depth: null, ...q } }),
      edit: (xml: string, label?: string) => this.edit(xml, label),
      ask: (instruction: string) => this.ask(instruction),
      bench: async (iterations = 20, repeats = 200) =>
        JSON.parse(await this.request('solveBench', { iterations, repeats })),
      selection: () => [...this.selection],
      hovered: () => [...this.hovered],
      link: (a: string, b: string, keep = '') => this.link(a, b, keep),
      counterparts: (node: string) => this.call('counterparts', [node]),
      focus: (node: string) => this.call('focus', [node]),
      // The selection as the store's session records it, with how each
      // node was reached (panel, chain).
      session: () => this.call('selection', []),
      // The render benchmark (roadmap stage H): a composition of `shapes`
      // shapes, and per-phase CPU timings (`src/perf.rs`).
      stress: (shapes = 5000) => this.call('stress', [shapes]),
      spatial: (enabled = true) => this.call('spatial', [enabled]),
      timings: (reset = false) => this.call('timings', [reset]),
    };
  }

  setApiKey(key: string) {
    this.apiKey = key;
    try {
      localStorage.setItem('iron.anthropicKey', key);
    } catch {
      // storage unavailable
    }
  }

  /** Pick a tool, as its key would (V select, P pen). */
  setTool(tool: 'select' | 'pen') {
    this.post?.({ ty: 'toolKey', key: tool === 'pen' ? 'p' : 'v', m: 0 });
  }

  /** Apply `<edit>` markup as the model would. Resolves with the outcome. */
  edit(xml: string, label?: string): Promise<any> {
    return this.request('modelEdit', { xml, label });
  }

  /** One model turn: view, prompt, apply. */
  async ask(instruction: string): Promise<any> {
    if (!this.apiKey) throw new Error('no API key set');
    this.asking = true;
    try {
      const result = await this.request('modelAsk', { instruction, apiKey: this.apiKey, model: this.modelName });
      this.lastAsk = result;
      return result;
    } finally {
      this.asking = false;
    }
  }

  /** One JSON-answering document FFI call (`document_<fn>`). */
  async call(fn: string, args: unknown[]): Promise<any> {
    return JSON.parse(await this.request('documentCall', { fn, args }));
  }

  /** Link two pictures (ECM §7.2). A merge question is kept in `question`
   *  for the dialog and returned; answer it by calling again with `keep`. */
  async link(a: string, b: string, keep = ''): Promise<any> {
    const r = await this.call('link', [a, b, keep, 'human']);
    this.question = r.question ? { ...r.question, aNode: a, bNode: b } : null;
    return r;
  }

  private async refreshCounterparts(): Promise<void> {
    const first = this.selection[0];
    this.counterparts = first ? await this.call('counterparts', [first]) : { entity: null, figures: [] };
  }

  /** Returns true if the message was one of ours. */
  handleMessage(data: any): boolean {
    switch (data?.ty) {
      case 'selection':
        this.selection = (data.list ?? []).map(String);
        void this.refreshCounterparts();
        return true;
      case 'hover':
        this.hovered = (data.list ?? []).map(String);
        return true;
      case 'documentChanged':
        this.version = data.version;
        // `?stress=N`: add the benchmark composition once the demo scene
        // exists (its tab is in the default layout, src-ui/views/workspace.ts).
        if (!this.stressed) {
          this.stressed = true;
          const shapes = Number(new URLSearchParams(location.search).get('stress'));
          if (shapes > 0) void this.call('stress', [shapes]);
        }
        void this.refresh();
        void this.refreshCounterparts();
        return true;
      case 'documentSaved':
      case 'documentLoaded':
      case 'documentView':
      case 'documentCall':
      case 'solveBench': {
        const p = this.pending.get(data.requestId);
        if (!p) return true;
        this.pending.delete(data.requestId);
        if (data.ok === false) p.reject(new Error(data.error ?? 'request failed'));
        else p.resolve(data.text);
        return true;
      }
      case 'modelEditResult':
      case 'modelAskResult': {
        // Resolve with the structured outcome even on a rejected edit: the
        // failure markup is the useful part.
        const p = this.pending.get(data.requestId);
        if (!p) return true;
        this.pending.delete(data.requestId);
        if (data.error) p.reject(new Error(data.error));
        else p.resolve(data.outcome ?? data.result);
        return true;
      }
      default:
        return false;
    }
  }

  private request(ty: string, payload: object): Promise<any> {
    if (!this.post) return Promise.reject(new Error('no session'));
    const requestId = this.nextId++;
    return new Promise((resolve, reject) => {
      this.pending.set(requestId, { resolve, reject });
      this.post!({ ty, requestId, ...payload });
    });
  }

  async refresh(): Promise<void> {
    try {
      this.view = await this.request('documentView', {
        query: { scope: null, fidelity: this.fidelity, depth: this.depth },
      });
      this.error = '';
    } catch (e) {
      this.error = String(e);
    }
  }

  save(): Promise<string> {
    return this.request('documentSave', {});
  }

  async load(text: string): Promise<void> {
    try {
      await this.request('documentLoad', { text });
      this.error = '';
    } catch (e) {
      this.error = String(e);
      throw e;
    }
  }

  /** Save to a download named iron-document.json. */
  async download(): Promise<void> {
    const text = await this.save();
    const url = URL.createObjectURL(new Blob([text], { type: 'application/json' }));
    const a = document.createElement('a');
    a.href = url;
    a.download = 'iron-document.json';
    a.click();
    URL.revokeObjectURL(url);
  }

  async openFile(file: File): Promise<void> {
    await this.load(await file.text());
  }
}
