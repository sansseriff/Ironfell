import { marked } from 'marked';

export interface ConversationMessage {
  role: 'user' | 'assistant';
  content: string;
}

export interface ConversationOptions {
  history?: ConversationMessage[];
  allowTextReply?: boolean;
}

export function executableReply(text: string): string {
  return marked.lexer(text).map(token => {
    if (token.type === 'html') return token.raw;
    if ((token.type === 'paragraph' || token.type === 'text') && 'tokens' in token && token.tokens) {
      return token.tokens.filter(part => part.type === 'html' || part.type === 'text').map(part => part.raw).join('');
    }
    return '';
  }).join('\n');
}

export function boundedHistory(messages: ConversationMessage[] = []): ConversationMessage[] {
  let remaining = 48000;
  const result: ConversationMessage[] = [];
  for (const message of messages.slice(-12).reverse()) {
    if (!message || !['user', 'assistant'].includes(message.role) || typeof message.content !== 'string') continue;
    const content = message.content.slice(0, Math.min(8000, remaining));
    if (!content) break;
    result.unshift({ role: message.role, content });
    remaining -= content.length;
  }
  return result;
}
