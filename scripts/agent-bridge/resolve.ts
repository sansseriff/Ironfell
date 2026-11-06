/**
 * Finding the local agent CLIs (plans/specs/agent-runtimes.md): an explicit
 * override, then PATH, then where each installs itself when PATH does not
 * have it. A bad override is an error, never silently replaced.
 */
import { access, stat } from 'node:fs/promises';
import { constants } from 'node:fs';
import { homedir } from 'node:os';
import { delimiter, isAbsolute, join, resolve } from 'node:path';

async function executable(path: string): Promise<boolean> {
  try {
    await access(path, constants.X_OK);
    return (await stat(path)).isFile();
  } catch {
    return false;
  }
}

async function find(name: string, variable: string, label: string, installed: string[], explicit: string | undefined, searchPath: string): Promise<string> {
  const onPath = (file: string) => searchPath.split(delimiter).filter(Boolean).map(directory => resolve(directory, file));
  const candidates = explicit
    ? (isAbsolute(explicit) || explicit.includes('/') ? [resolve(explicit)] : onPath(explicit))
    : [...onPath(name), ...installed];
  for (const candidate of candidates) if (await executable(candidate)) return candidate;
  throw new Error(explicit
    ? `Cannot execute ${variable}=${explicit}. Point it to your ${label} executable.`
    : `${label} not found. Install it or set ${variable} to its executable, then run this command again.`);
}

/** The Codex CLI: `IRON_CODEX_BIN`, PATH, or the Codex app's bundled copy. */
export function resolveCodex(explicit = process.env.IRON_CODEX_BIN, searchPath = process.env.PATH ?? '', home = homedir()): Promise<string> {
  return find('codex', 'IRON_CODEX_BIN', 'Codex CLI', [join(home, '.codex/plugins/.plugin-appserver/codex-cli/bin/codex')], explicit, searchPath);
}

/** Claude Code: `IRON_CLAUDE_BIN`, PATH, or its native installer's places. */
export function resolveClaude(explicit = process.env.IRON_CLAUDE_BIN, searchPath = process.env.PATH ?? '', home = homedir()): Promise<string> {
  return find('claude', 'IRON_CLAUDE_BIN', 'Claude Code', [join(home, '.local/bin/claude'), join(home, '.claude/local/claude')], explicit, searchPath);
}
