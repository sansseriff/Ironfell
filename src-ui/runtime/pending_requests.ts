/** Session identity, bounded pending work and guaranteed settlement, independent
 * of Svelte or any particular host. IDs never repeat across attachments. */
export class PendingRequests {
  private nextId = 1;
  private pending = new Map<number, { resolve: (value: any) => void; reject: (error: Error) => void; timer: ReturnType<typeof setTimeout> }>();
  private session: number | null = null;
  attach(session: number) { this.dispose('session replaced'); this.session = session; }
  get size() { return this.pending.size; }
  request<T>(send: (id: number, session: number) => void, timeout = 15000): Promise<T> {
    const session = this.session;
    if (session === null) return Promise.reject(new Error('no session'));
    if (this.pending.size >= 1024) return Promise.reject(new Error('too many pending requests'));
    return new Promise((resolve, reject) => {
      const id = this.nextId++;
      const timer = setTimeout(() => { this.pending.delete(id); reject(new Error('request timed out')); }, timeout);
      this.pending.set(id, { resolve, reject, timer });
      try { send(id, session); } catch (error) { this.settle(id, session, undefined, String(error)); }
    });
  }
  settle(id: number, session: number | undefined, value?: unknown, error?: string) {
    if (session !== this.session) return;
    const pending = this.pending.get(id);
    if (!pending) return;
    this.pending.delete(id); clearTimeout(pending.timer);
    if (error) pending.reject(new Error(error)); else pending.resolve(value);
  }
  dispose(reason = 'session disposed') {
    this.session = null;
    for (const pending of this.pending.values()) { clearTimeout(pending.timer); pending.reject(new Error(reason)); }
    this.pending.clear();
  }
}
