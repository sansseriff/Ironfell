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
 * Logs hold document content and prompts: what the model provider already
 * receives. They stay on this machine.
 */
import { appendFile, mkdir, stat } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { join } from 'node:path';

export interface AgentLog {
  readonly dir: string;
  /** Append one record, in order; never throws. */
  write(record: Record<string, unknown>): void;
  /** The hash a call names its system prompt by, writing the prompt the first time. */
  system(text: string): string;
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
    flushed: () => chain,
  };
}
