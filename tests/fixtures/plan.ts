/**
 * Stand-ins for the engine's markup reader and planner (iron_document::reply
 * and iron_document::edit) in tests of the agent loop, merging and the chat.
 * Bun cannot run the engine, and these read no markup: a test scripts each
 * model reply with what the engine reads in it (`said`, `scripted`), each
 * edit with the plan the engine makes of it (`planned`), and each revise
 * with the edit it makes (`revised`). The reader and the planner are tested
 * in Rust (src/iron_document/tests/reply.rs, edit.rs).
 */
import type { Reply, StreamView } from '../../src-ui/model/api';
import type { Plan, PlanResult } from '../../src-ui/model/edit';
import { executableReply } from '../../src-ui/model/conversation';

/** A part of a scripted reply: prose, or what its markup asks for. */
export type Part = string | { edit: string } | { revise: string } | { picture: string } | { expand: string[] } | { topics: string[] } | { continue: true };

const replies = new Map<string, Reply>();
const plans = new Map<string, { ok: true; plan: Plan } | { ok: false; errors?: { op: number | null; kind: string; message: string; verb?: string }[]; failure?: string }>();
const revisions = new Map<string, { ok: true; edit: string } | { ok: false; error: string }>();

const prose = (text: string): Reply => ({ edit: null, revise: null, picture: null, expand: [], topics: [], continue: false, note: text.trim(), bare: text.trim() });

function markup(part: Part): string {
  if (typeof part === 'string') return part;
  if ('edit' in part) return part.edit;
  if ('revise' in part) return part.revise;
  if ('picture' in part) return `<look picture="${part.picture}"/>`;
  if ('expand' in part) return `<look expand="${part.expand.join(' ')}"/>`;
  if ('topics' in part) return `<topic name="${part.topics.join(' ')}"/>`;
  return '<continue/>';
}

/** A model reply of `parts`, each on its own line, scripted with what the
 *  engine reads in it. */
export function said(...parts: Part[]): string {
  const text = parts.map(markup).join('\n');
  const reading = prose('');
  for (const part of parts) {
    if (typeof part === 'string') continue;
    if ('edit' in part) reading.edit ??= part.edit;
    else if ('revise' in part) reading.revise ??= part.revise;
    else if ('picture' in part) reading.picture ??= part.picture;
    else if ('expand' in part) reading.expand = reading.expand.length ? reading.expand : part.expand.map((id) => (id.startsWith('#') ? id : `#${id}`));
    else if ('topics' in part) for (const t of part.topics) { if (!reading.topics.includes(t)) reading.topics.push(t); }
    else reading.continue = true;
  }
  const block = parts.find((p) => typeof p === 'object' && 'edit' in p) ?? parts.find((p) => typeof p === 'object' && 'revise' in p);
  const picture = (p: Part) => typeof p === 'object' && 'picture' in p;
  reading.note = parts.filter((p) => p !== block && !picture(p)).map(markup).join('\n').trim();
  reading.bare = parts.filter((p) => !picture(p)).map(markup).join('\n').trim();
  replies.set(text, reading);
  // The agent reads the reply's executable part too: here, all of it.
  replies.set(executableReply(text), reading);
  return text;
}

/** A reply written out, with what the engine reads in it as written, and
 *  in its executable part (`executableReply`: without code examples), the
 *  same unless given. */
export function scripted(text: string, reading: Partial<Reply>, executable: Partial<Reply> = reading): string {
  replies.set(executableReply(text), { ...prose(executableReply(text)), ...executable });
  replies.set(text, { ...prose(text), ...reading });
  return text;
}

/** `edit`, scripted with the plan the engine makes of it (or its errors). */
export function planned(edit: string, result: Plan | { ok: false; errors?: { op: number | null; kind: string; message: string; verb?: string }[]; failure?: string }): string {
  plans.set(edit, 'ops' in result ? { ok: true, plan: result } : result);
  return edit;
}

/** `patch` of `rejected`, scripted with the edit it makes (or its error). */
export function revised(rejected: string, patch: string, result: string | { error: string }): string {
  revisions.set(`${rejected}\u0000${patch}`, typeof result === 'string' ? { ok: true, edit: result } : { ok: false, error: result.error });
  return patch;
}

/** A plan: its verbs in order, each with the ops it writes; `aliases` are
 *  the `$names` the edit gave and the ids they became. */
export function plan(verbs: [string, object[]][], aliases: Record<string, string> = {}, extra: Partial<Plan> = {}): Plan {
  return {
    ops: verbs.flatMap(([, ops]) => ops),
    aliases,
    op_verb: verbs.flatMap(([, ops], i) => ops.map(() => i)),
    verbs: verbs.map(([verb]) => verb),
    summary: Object.entries(aliases).map(([a, id]) => `  <created alias="${a}" id="${id}"/>`),
    expect_version: 0,
    exact: false,
    ...extra,
  };
}

/** The engine's planner, as scripted (`planned`), at `version()`. */
export function fakePlanner(version: () => number = () => 0) {
  return (xml: string): PlanResult => {
    const result = plans.get(xml);
    if (!result) return { ok: false, version: version(), errors: [{ op: null, kind: 'parse', message: `edit not scripted: ${xml.slice(0, 80)}` }] };
    return { ...result, version: version() } as PlanResult;
  };
}

/** The engine's reply reader, as scripted. A reply never scripted reads as
 *  prose, unless it has markup in it, which the test forgot to script. */
export const reader = {
  reply(text: string): Reply {
    const reading = replies.get(text);
    if (reading) return reading;
    if (text.includes('<')) throw new Error(`reply not scripted: ${text.slice(0, 120)}`);
    return prose(text);
  },
  /** The chat's stream view is the engine's (tested in Rust); here the text as prose. */
  stream(text: string): StreamView {
    return { prose: text, edit: null, verbs: [] };
  },
  revise(rejected: string, patch: string): { ok: true; edit: string } | { ok: false; error: string } {
    return revisions.get(`${rejected}\u0000${patch}`) ?? { ok: false, error: `revise not scripted: ${patch.slice(0, 80)}` };
  },
  netlists: () => ({ ok: true, netlists: [] }),
};
