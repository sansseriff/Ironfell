/**
 * The write surface: an `<edit>` in, one transaction out (doc 04 §9.2;
 * plans/specs/markup.md).
 *
 * The engine reads the edit (iron_document::edit, the `edit_plan` call):
 * the same markup as the standard components and the demo's gallery, nodes
 * as elements and verbs on what exists, all one transaction. What comes
 * back is a plan: its ops, the verb that wrote each, the edit's `$names`
 * and what to tell the model once it applies. This file applies the plan
 * as the model, through `api.apply` (so a merge with the person's changes
 * sees every op, merge.ts), and answers in the edit's own terms.
 *
 *   <edit expect-version="7">
 *     <rect in="#1" id="$b" after="#M" x="10" y="20" w="30" h="30" fill.color="@series-1"/>
 *     <set id="#P" x="12" size.h="{= #O.slider.value * 200}"/>
 *     <link from="$clip" to="#Q" rel="animates" offset="0.5"/>
 *     <place def="std:plot" in="#C" id="$plot" x="40" y="40" ys="{= $self.xs * 2}"/>
 *   </edit>
 */
import type { ApplyResult, DocumentApi, SolverSummary } from './api';

/** What the engine planned: one transaction, and how to speak of it. */
export interface Plan {
  ops: object[];
  /** `$name` -> the node it named. */
  aliases: Record<string, string>;
  /** For each op, the verb (the edit's top-level element, from 0) that wrote it. */
  op_verb: number[];
  /** Each verb, briefly, in the model's terms: `create $sinv`, `set #P`. */
  verbs: string[];
  /** Lines for the `<applied>` answer: what was created, placed, built. */
  summary: string[];
  expect_version: number;
  exact: boolean;
}

export type PlanResult =
  | { ok: true; version: number; plan: Plan }
  | { ok: false; version?: number; errors?: EditErrorLine[]; failure?: string };

/** Pull the `<edit>…</edit>` block out of a model reply that may say more. */
export function extractEdit(text: string): string | null {
  const m = text.match(/<edit[\s>][\s\S]*?<\/edit>|<edit\s*\/>/);
  return m ? m[0] : null;
}

export interface EditOutcome {
  ok: boolean;
  version?: number;
  aliases?: Record<string, string>;
  /** Markup the model can act on (doc 04 §9.5). Present when `ok` is false. */
  failure?: string;
  /** What the solver did, when constraints were involved. */
  solver?: SolverSummary;
  /** Markup for the model about an applied edit: what it created, placed and
   *  built, and the solver's outcome (clusters, moved nodes, unmet requests). */
  summary?: string;
}

/** Plan and apply one edit as the model, answering in the model's terms. */
export async function applyEdit(api: DocumentApi, xml: string, label = 'model edit'): Promise<EditOutcome> {
  const planned = await api.plan(xml);
  if (!planned.ok) {
    if (planned.failure) return { ok: false, version: planned.version, failure: planned.failure };
    return { ok: false, version: planned.version, failure: failed(planned.version ?? await api.version(), planned.errors ?? []) };
  }
  const { plan } = planned;
  const tx = { label, actor: 'model', ts: Date.now(), ops: plan.ops, ...(plan.exact ? { exact: true } : {}) };
  const result: ApplyResult = await api.apply(tx, plan.expect_version);
  if (!result.ok) {
    return { ok: false, version: result.version, failure: failed(result.version ?? await api.version(), inModelTerms(plan, result.errors ?? [])) };
  }
  const lines = [...plan.summary, ...(result.solver ? solverLines(result.solver) : [])];
  return {
    ok: true,
    version: result.version,
    aliases: plan.aliases,
    ...(result.solver ? { solver: result.solver } : {}),
    ...(lines.length ? { summary: `<applied version="${result.version}">\n${lines.join('\n')}\n</applied>` } : {}),
  };
}

type EditErrorLine = { op: number | null; kind: string; message: string; verb?: string };

/** The store's errors as the model wrote the edit: each op as the verb that
 *  wrote it (a verb may write several ops), and ids this edit minted as the
 *  names it gave them. */
function inModelTerms(plan: Plan, errors: { op: number | null; kind: string; message: string }[]): EditErrorLine[] {
  const aliasOf = new Map(Object.entries(plan.aliases).map(([alias, id]) => [id, alias]));
  const renamed = (message: string) => message.replace(/#[\w/~]+/g, (id) => aliasOf.get(id) ?? id);
  return errors.map((e) => {
    const verb = e.op === null ? null : plan.op_verb[e.op] ?? null;
    return { op: verb, kind: e.kind, message: renamed(e.message), ...(verb === null ? {} : { verb: plan.verbs[verb] }) };
  });
}

function solverLines(s: SolverSummary): string[] {
  return [
    `  <solver clusters="${s.clusters.join(' ')}" moved="${s.moved.join(' ')}"/>`,
    ...(s.unmet ?? []).map((u) => `  <unmet leaf="${u.leaf}" requested="${u.requested}" solved="${u.solved}">${escapeXml(u.reason)}</unmet>`),
  ];
}

/** `<edit-failed>` markup. Errors that say the same thing are one line
 *  listing their ops, so forty of one mistake read as one. `op` counts the
 *  edit's verbs from 0. */
function failed(version: number, errors: EditErrorLine[]): string {
  const groups = new Map<string, EditErrorLine[]>();
  for (const e of errors) {
    const key = `${e.kind}\u0000${e.message}`;
    groups.set(key, [...(groups.get(key) ?? []), e]);
  }
  const lines = [...groups.values()].map((group) => {
    const [e] = group;
    const ops = [...new Set(group.map((g) => g.op).filter((op): op is number => op !== null))].sort((a, b) => a - b);
    const where = ops.length > 1 ? ` ops="${ops.join(' ')}"` : ops.length === 1 ? ` op="${ops[0]}"${e.verb ? ` verb="${escapeAttr(e.verb)}"` : ''}` : '';
    const times = group.length > 1 ? ` (${group.length} times)` : '';
    return `  <error${where} kind="${e.kind}">${escapeXml(e.message)}${times}</error>`;
  });
  return `<edit-failed applied="none" version="${version}">\n${lines.join('\n')}\n</edit-failed>`;
}

function escapeAttr(s: string): string {
  return escapeXml(s).replace(/"/g, '&quot;');
}

function escapeXml(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}
