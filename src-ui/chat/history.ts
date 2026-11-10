/**
 * Chat history: each chat panel (Model, and each narrative's Lesson chat)
 * keeps a list of conversations, one of them current. They are workspace
 * state, saved with the scope as `chats.json` (runtime/scope_storage.ts),
 * never in the document or its undo. This module is the plain part: what a
 * conversation is on disk, its title, and what is kept. The reactive store is
 * sessions.svelte.ts.
 */
import { validImages } from '../model/images';
import type { ChatSession, ChatTurn } from './conversation';

/** One conversation in a panel's history. */
export interface ChatThread extends ChatSession {
  id: string;
  /** Milliseconds since the epoch. */
  created: number;
  updated: number;
}

/** A panel's conversations, newest activity first, and the one it shows. */
export interface SavedView { current: string; threads: ChatThread[] }

/** `chats.json`: every chat panel's history in the scope, by view key
 *  (`model:<source>`, `narrative:<source>`). */
export interface SavedChats { version: 1; views: Record<string, SavedView> }

/** Conversations kept per panel; the least recently used go first. */
export const MAX_THREADS = 50;

export function newThread(now = Date.now()): ChatThread {
  return { id: crypto.randomUUID(), created: now, updated: now, draft: '', turns: [], annotations: {} };
}

/** A conversation's name: its first request's first line, shortened. */
export function threadTitle(thread: Pick<ChatThread, 'turns'>): string {
  const first = thread.turns[0]?.instruction.trim().split('\n')[0]?.trim() ?? '';
  if (!first) return 'New chat';
  return first.length > 60 ? `${first.slice(0, 57).trimEnd()}…` : first;
}

/** What is saved of a conversation: its finished turns (a running one is
 *  saved when it ends), their annotations, and the unsent draft. */
export function savedThread(thread: ChatThread): ChatThread {
  const turns = thread.turns.filter((turn: ChatTurn) => turn.status !== 'running');
  const ids = new Set(turns.map(turn => turn.id));
  const annotations = Object.fromEntries(Object.entries(thread.annotations).filter(([id]) => ids.has(id)));
  return { id: thread.id, created: thread.created, updated: thread.updated, draft: thread.draft, draftImages: thread.draftImages, turns, annotations };
}

/** A panel's history as saved: conversations with something in them (and
 *  the current one), most recent first, at most `MAX_THREADS`. */
export function savedView(threads: ChatThread[], current: string): SavedView {
  const kept = threads.filter(thread => thread.id === current || thread.turns.length || (thread.draft.trim() || thread.draftImages?.length))
    .map(savedThread)
    .sort((a, b) => b.updated - a.updated)
    .slice(0, MAX_THREADS);
  return { current: kept.some(thread => thread.id === current) ? current : kept[0]?.id ?? '', threads: kept };
}

/** Read `chats.json`; anything unreadable is an empty history, never an error. */
export function parseChats(json: string | null): SavedChats {
  const empty: SavedChats = { version: 1, views: {} };
  if (!json) return empty;
  try {
    const data = JSON.parse(json);
    if (data?.version !== 1 || typeof data.views !== 'object' || !data.views) return empty;
    const views: Record<string, SavedView> = {};
    for (const [key, view] of Object.entries(data.views as Record<string, any>)) {
      if (!Array.isArray(view?.threads)) continue;
      const threads: ChatThread[] = view.threads.filter((t: any) => typeof t?.id === 'string' && Array.isArray(t.turns))
        .map((t: any) => ({
          id: t.id, created: Number(t.created) || 0, updated: Number(t.updated) || 0,
          draft: typeof t.draft === 'string' ? t.draft : '',
          draftImages: validImages(t.draftImages) ? t.draftImages : [],
          turns: t.turns.filter((turn: any) => typeof turn?.id === 'string' && typeof turn.instruction === 'string' && turn.status !== 'running').map((turn: ChatTurn) => ({ ...turn, images: validImages(turn.images) ? turn.images : [] })),
          annotations: typeof t.annotations === 'object' && t.annotations ? t.annotations : {},
        }));
      views[key] = { current: typeof view.current === 'string' ? view.current : '', threads };
    }
    return { version: 1, views };
  } catch {
    return empty;
  }
}

/** The history a panel shows once its saved one arrives: the saved
 *  conversations, except that one already under way here (turns or a draft)
 *  stays current. */
export function mergeView(live: ChatThread[], current: string, saved: SavedView | undefined): SavedView {
  if (!saved?.threads.length) return { current, threads: live };
  const active = live.find(thread => thread.id === current);
  const started = live.filter(thread => thread.turns.length || (thread.draft.trim() || thread.draftImages?.length));
  if (!started.length) {
    const shown = saved.threads.some(thread => thread.id === saved.current) ? saved.current : saved.threads[0].id;
    return { current: shown, threads: saved.threads };
  }
  const ids = new Set(started.map(thread => thread.id));
  return { current: active && ids.has(active.id) ? active.id : started[0].id, threads: [...started, ...saved.threads.filter(thread => !ids.has(thread.id))] };
}
