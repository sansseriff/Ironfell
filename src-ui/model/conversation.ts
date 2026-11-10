import { validImages, type ModelImage } from './images';
import { marked } from 'marked';

export interface ConversationMessage {
  role: 'user' | 'assistant';
  content: string;
  images?: ModelImage[];
}

export interface ConversationOptions {
  history?: ConversationMessage[];
  images?: ModelImage[];
  allowTextReply?: boolean;
  /** The chat turn's id: sent with each model call and the outcome, for the agent log. */
  turnId?: string;
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
  let imageBudget = 4_000_000;
  const result: ConversationMessage[] = [];
  for (const message of messages.slice(-12).reverse()) {
    if (!message || !['user', 'assistant'].includes(message.role) || typeof message.content !== 'string') continue;
    const content = message.content.slice(0, Math.min(8000, remaining));
    if (!content) break;
    const images = message.role === 'user' && validImages(message.images) ? message.images : [];
    const bytes = images.reduce((sum, image) => sum + image.data.length, 0);
    const keepImages = bytes > 0 && bytes <= imageBudget;
    if (keepImages) imageBudget -= bytes;
    result.unshift({ role: message.role, content, ...(keepImages ? { images } : {}) });
    remaining -= content.length;
  }
  return result;
}
