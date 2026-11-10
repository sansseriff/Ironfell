import { validImages, wireImages, type ModelImage } from './images';
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
    // Newest first, each image on its own: the most recent references are the
    // ones kept, and the model is told when any were left out.
    const attached = message.role === 'user' && validImages(message.images) ? message.images : [];
    const images = wireImages(attached).filter(image => {
      if (image.data.length > imageBudget) return false;
      imageBudget -= image.data.length;
      return true;
    });
    const omitted = attached.length - images.length;
    const note = omitted ? `\n[${omitted} image${omitted === 1 ? '' : 's'} attached here ${omitted === 1 ? 'is' : 'are'} not resent]` : '';
    result.unshift({ role: message.role, content: content + note, ...(images.length ? { images } : {}) });
    remaining -= content.length;
  }
  return result;
}
