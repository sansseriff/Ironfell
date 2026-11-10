/**
 * The agent log: what each model call was sent and replied, and how each turn
 * ended, as JSON lines in a local, gitignored folder (one file a day). A stopgap
 * until a backend keeps conversations (plans/specs/agent-streaming.md §7).
 *
 * Records:
 *   { kind: 'system', hash, text }   a system prompt, once per file (calls name its hash)
 *   { kind: 'call', turn, … }        one model call, from the bridge
 *   { kind: 'turn', turn, … }        a turn's outcome, from the page (POST /log)
 *
 * Image attachments never go into the JSONL: a call's message keeps
 * `{ name, mediaType, bytes, sha256, file }`, and the image itself is written
 * once to `images/<sha256>.<ext>` beside the day's files, so an agent reading
 * the log sees plain text and can open the picture by its `file` path.
 *
 * Logs hold document content and prompts: what the model provider already
 * receives. They stay on this machine.
 */
import { appendFile, mkdir, stat, writeFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { join } from 'node:path';

interface LoggedMessage { role: string; content: string; images?: readonly { name: string; mediaType: string; data: string }[] }

export interface AgentLog {
  readonly dir: string;
  /** Append one record, in order; never throws. */
  write(record: Record<string, unknown>): void;
  /** The hash a call names its system prompt by, writing the prompt the first time. */
  system(text: string): string;
  /** A call's messages as logged: each image replaced by a reference to its file. */
  messages(messages: readonly LoggedMessage[]): unknown[];
  /** Resolves once every record so far is on disk. */
  flushed(): Promise<void>;
}

/** Where logs go: `IRON_AGENT_LOG` (a folder, or `0` for none), else
 *  `<root>/.agents/agent-logs`. */
export function agentLogDir(root: string, env: NodeJS.ProcessEnv = process.env): string | null {
  const setting = env.IRON_AGENT_LOG;
  if (setting === '0' || setting === 'off') return null;
  return setting || join(root, '.agents', 'agent-logs');
}

/** A day's file stops growing at `maxBytes`; a note on stderr says so once. */
export function agentLog(dir: string, maxBytes = 200_000_000): AgentLog {
  let chain: Promise<void> = Promise.resolve();
  let made = false;
  const seen = new Set<string>();
  const full = new Set<string>();
  const day = () => {
    const d = new Date();
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
  };
  const write = (record: Record<string, unknown>) => {
    const file = join(dir, `${day()}.jsonl`);
    const line = `${JSON.stringify({ at: new Date().toISOString(), ...record })}\n`;
    chain = chain.then(async () => {
      if (full.has(file)) return;
      const size = await stat(file).then(s => s.size, () => 0);
      if (size + line.length > maxBytes) {
        full.add(file);
        console.error(`Agent log ${file} reached ${maxBytes} bytes; not logging more today.`);
        return;
      }
      if (!made) { await mkdir(dir, { recursive: true }); made = true; }
      await appendFile(file, line);
    }).catch(error => console.error('Agent log write failed:', error));
  };
  return {
    dir,
    write,
    system(text) {
      const hash = createHash('sha256').update(text).digest('hex').slice(0, 16);
      // Once per day's file, so each file stands alone.
      const key = `${day()}:${hash}`;
      if (!seen.has(key)) { seen.add(key); write({ kind: 'system', hash, text }); }
      return hash;
    },
    messages(messages) {
      return messages.map(message => {
        if (!message.images?.length) return message;
        const images = message.images.map(image => {
          const bytes = Buffer.from(image.data, 'base64');
          const sha256 = createHash('sha256').update(bytes).digest('hex');
          const file = `images/${sha256.slice(0, 32)}.${image.mediaType.split('/')[1] ?? 'bin'}`;
          if (!seen.has(file)) {
            seen.add(file);
            chain = chain.then(async () => {
              await mkdir(join(dir, 'images'), { recursive: true });
              await writeFile(join(dir, file), bytes, { flag: 'wx' }).catch(error => { if (error?.code !== 'EEXIST') throw error; });
            }).catch(error => console.error('Agent log image write failed:', error));
          }
          return { name: image.name, mediaType: image.mediaType, bytes: bytes.length, sha256, file };
        });
        return { ...message, images };
      });
    },
    flushed: () => chain,
  };
}
