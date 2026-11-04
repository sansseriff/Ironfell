import type { ChatSession } from './conversation';

const sessions = new Map<string, ChatSession>();

export function chatSession(key: string): ChatSession {
  let session = sessions.get(key);
  if (!session) {
    const created: ChatSession = $state({ draft: '', turns: [], annotations: {} });
    session = created;
    sessions.set(key, session);
    if (sessions.size > 40) sessions.delete(sessions.keys().next().value!);
  }
  return session;
}
