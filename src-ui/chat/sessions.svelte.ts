/**
 * The chat panels' conversations (history.ts): each panel has a history, one
 * conversation current, and New chat / open / delete. A scope's histories
 * load from its `chats.json` once and save back, debounced, when a turn ends
 * or the list changes. An unsaved page (no scope) keeps them in memory.
 */
import { controllerManager } from '../controller-manager.svelte';
import { mergeView, newThread, parseChats, savedView, type ChatThread, type SavedChats } from './history';

/** Reads and writes a scope's `chats.json`. */
export interface ChatStore {
  read(): Promise<string | null>;
  write(json: string): Promise<void>;
}

export class ChatHistory {
  threads: ChatThread[] = $state([]);
  currentId = $state('');

  constructor(private changed: () => void) {
    const first = newThread();
    this.threads = [first];
    this.currentId = first.id;
  }

  /** The conversation the panel shows. */
  get current(): ChatThread {
    return this.threads.find(thread => thread.id === this.currentId) ?? this.threads[0];
  }

  /** Conversations to list: those with turns, and the current one; most
   *  recent first. */
  get listed(): ChatThread[] {
    return this.threads.filter(thread => thread.id === this.currentId || thread.turns.length)
      .sort((a, b) => b.updated - a.updated);
  }

  /** Start a conversation. The current one, if still empty, is it already. */
  newChat() {
    if (!this.current.turns.length) return;
    const fresh = newThread();
    this.threads = [fresh, ...this.threads];
    this.currentId = fresh.id;
    this.changed();
  }

  open(id: string) {
    if (id === this.currentId || !this.threads.some(thread => thread.id === id)) return;
    this.currentId = id;
    this.changed();
  }

  /** Delete a conversation; deleting the current one shows the most recent
   *  other, or a new one. */
  remove(id: string) {
    const rest = this.threads.filter(thread => thread.id !== id);
    if (rest.length === this.threads.length) return;
    if (id === this.currentId) {
      const next = [...rest].filter(thread => thread.turns.length).sort((a, b) => b.updated - a.updated)[0];
      if (next) this.currentId = next.id;
      else { const fresh = newThread(); rest.unshift(fresh); this.currentId = fresh.id; }
    }
    this.threads = rest;
    this.changed();
  }

  /** Save after a change inside a conversation (a review kept or reverted). */
  save() { this.changed(); }

  /** A turn in `thread` ended: it moves to the top of the list and is saved. */
  touch(thread: ChatThread) {
    thread.updated = Date.now();
    this.changed();
  }

  /** Take the saved history, keeping a conversation already under way. */
  adopt(saved: SavedChats['views'][string] | undefined) {
    const merged = mergeView(this.threads, this.currentId, saved);
    this.threads = merged.threads;
    this.currentId = merged.current;
  }

  saved() { return savedView($state.snapshot(this.threads) as ChatThread[], this.currentId); }
}

class ScopeChats {
  readonly views = new Map<string, ChatHistory>();
  /** What `chats.json` held, once read; until then nothing is written, so a
   *  failed or slow read never overwrites the saved history. */
  private data: SavedChats | null = null;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private pending = false;

  constructor(private store: ChatStore | null) {
    if (!store) return;
    void store.read().then(json => {
      this.data = parseChats(json);
      for (const [view, history] of this.views) history.adopt(this.data.views[view]);
      if (this.pending) this.save();
    }).catch(e => console.warn('[chat] history not loaded', e));
    // Write a pending save when the tab is hidden or closed.
    if (typeof document !== 'undefined') document.addEventListener('visibilitychange', () => { if (document.visibilityState === 'hidden') this.flush(); });
  }

  history(view: string): ChatHistory {
    let history = this.views.get(view);
    if (!history) {
      history = new ChatHistory(() => this.save());
      if (this.data) history.adopt(this.data.views[view]);
      this.views.set(view, history);
    }
    return history;
  }

  private save() {
    if (!this.store) return;
    if (this.timer) clearTimeout(this.timer);
    this.timer = setTimeout(() => this.flush(), 400);
  }

  flush() {
    if (!this.store || !this.timer) return;
    clearTimeout(this.timer);
    this.timer = null;
    if (!this.data) { this.pending = true; return; }
    this.pending = false;
    // Views saved before but not open on this page keep their history.
    const views = { ...(this.data?.views ?? {}) };
    for (const [view, history] of this.views) views[view] = history.saved();
    const json = JSON.stringify({ version: 1, views } satisfies SavedChats);
    this.data = { version: 1, views };
    void this.store.write(json).catch(e => console.warn('[chat] history not saved', e));
  }
}

const scopes = new Map<string, ScopeChats>();

/** The history of chat panel `view` in scope `scope` (`page:<generation>`
 *  for an unsaved page, with no store). */
export function chatHistory(scope: string, view: string, store: ChatStore | null): ChatHistory {
  let chats = scopes.get(scope);
  if (!chats) {
    chats = new ScopeChats(store);
    scopes.set(scope, chats);
    if (scopes.size > 20) scopes.delete(scopes.keys().next().value!);
  }
  return chats.history(view);
}

/** The history of the chat panel `view` (`model:<source>`,
 *  `narrative:<source>`) in the open scope, saved with it; on an unsaved
 *  page, in memory for this engine session. */
export function panelHistory(view: string): ChatHistory {
  const doc = controllerManager.docClient;
  const id = doc.scope.current?.id;
  if (!id) return chatHistory(`page:${doc.generation}`, view, null);
  return chatHistory(id, view, {
    read: async () => {
      const saved = await doc.readChats();
      if (saved?.id !== id) throw new Error('another scope opened while reading its chats');
      return saved.json;
    },
    write: json => doc.saveChats(id, json),
  });
}
