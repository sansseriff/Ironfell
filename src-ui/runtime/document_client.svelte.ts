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

  private post: ((data: any) => void) | null = null;
  private pending = new Map<number, Pending>();
  private nextId = 1;

  init(post: (data: any) => void) {
    this.post = post;
    this.error = '';
    for (const p of this.pending.values()) p.reject(new Error('session replaced'));
    this.pending.clear();
    (window as any).__iron = {
      save: () => this.save(),
      load: (text: string) => this.load(text),
      view: (q: ViewQuery) => this.request('documentView', { query: { scope: null, depth: null, ...q } }),
    };
  }

  /** Returns true if the message was one of ours. */
  handleMessage(data: any): boolean {
    switch (data?.ty) {
      case 'documentChanged':
        this.version = data.version;
        void this.refresh();
        return true;
      case 'documentSaved':
      case 'documentLoaded':
      case 'documentView': {
        const p = this.pending.get(data.requestId);
        if (!p) return true;
        this.pending.delete(data.requestId);
        if (data.ok === false) p.reject(new Error(data.error ?? 'request failed'));
        else p.resolve(data.text);
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
