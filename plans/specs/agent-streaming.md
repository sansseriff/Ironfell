# Agent streaming: live progress, streamed prose, partial-edit previews

**Status:** Phases 1–2 built 2026-11-06 in the `t3/agent-streaming` worktree
(§6); phases 3–4 proposed, not started. Builds the follow-ups deferred by
[`narrative-panel.md` §9.3](narrative-panel.md) (real token streaming, Stop) and
[`agent-runtimes.md` §6](agent-runtimes.md) item 1 (progress, cancellation).

## 1. Today

`ChatPanel → DocumentClient.ask → request('modelAsk') → worker or native_adapter →
ask() (model/agent.ts) → ModelTransport.complete → whole reply text`. Nothing
crosses back until the turn ends, so `ChatMessage.svelte` can only show a static
"Working…". Every provider already produces incremental output that Iron throws
away:

| Runtime | What it emits (verified 2026-11-06) | What Iron reads now |
|---|---|---|
| Codex app-server (`codex-cli 0.166.0-alpha.5.1`, `generate-json-schema`) | `turn/started`, `item/started`, `item/agentMessage/delta {itemId, delta}`, `item/reasoning/summaryTextDelta {itemId, summaryIndex, delta}`, `item/reasoning/summaryPartAdded`, `item/reasoning/textDelta`, `item/completed`, `turn/completed`; `turn/start` takes `summary` and `effort` | `item/completed` (agentMessage) and `turn/completed` |
| Claude Code (`claude -p --output-format stream-json --verbose --include-partial-messages`) | `stream_event` lines carrying Messages API events: `content_block_start`, `content_block_delta` (`text_delta`; `thinking_delta` when the model thinks), `message_delta`, then a final `result` line | `--output-format json`, one object at exit |
| Claude browser API (`AnthropicTransport`) | `messages.stream()` events, `thinking` and `text` deltas | `finalMessage()` only |

Probe: a Sonnet run of the stream-json command above streamed three
`text_delta`s and a `result success`.

**Reasoning text is not shared by either login (2026-11-06).** Codex on the
ChatGPT login (`gpt-6.1-sol`) sends no reasoning item at all with `summary:
"auto"`; with `-c model_supports_reasoning_summaries=true` it sends one whose
`summary` is empty, even at `detailed` and high effort. Claude Code's
stream-json sends `thinking_delta`s with empty text. So the panel shows the
"Thinking" step and its timer until the reply's text begins, and reasoning
appears only where a runtime shares it (the browser API path's summarized
thinking; Codex summaries if an account or model provides them). Iron keeps
`summary: "auto"` on `turn/start` for that case and drops empty deltas.

Timeouts matter as much as the UI: the bridge allows 45 s per completion, HTTP
50 s. The failing request in the screenshot that prompted this spec took 38.7 s
over two attempts. Streaming lets the limit become an **idle** timeout (no event
for N s) plus a much larger total cap, so long component builds stop dying at 45 s.

## 2. One event stream, end to end

```ts
type AgentEvent =
  | { kind: 'phase'; phase: 'connecting' | 'reading' | 'thinking' | 'writing' | 'looking' | 'applying' | 'repairing'; attempt: number }
  | { kind: 'reasoning'; delta: string; part: number }   // summaries, or thinking text
  | { kind: 'text'; delta: string }                       // the reply as it forms
  | { kind: 'applied'; summary?: string } | { kind: 'rejected'; failure: string }
  | { kind: 'usage'; usage: unknown };
```

1. **Transport.** `complete(system, messages, { onEvent, signal })`. The old
   one-shot signature stays valid (no `onEvent` means no streaming).
2. **Bridge.** `POST /complete` with `Accept: application/x-ndjson` streams one
   JSON line per event, ending in `{type:'done', text, usage}` or
   `{type:'error'}`. Without that header it answers as today, so old clients
   and `__iron.ask` scripts keep working. A client disconnect aborts the child
   (already wired through `AbortSignal`). That gives Stop for free.
3. **Adapters.** Codex maps the notifications above, passing
   `summary: "concise"` (or `"auto"`) so reasoning summaries arrive. Claude
   switches to stream-json and parses `stream_event` lines. Both keep their
   output-size bounds. `AnthropicTransport` forwards `stream.on('text')` and
   thinking deltas.
4. **ask().** Emits a `phase` event at each step it already has: reading the
   view, the model call, look, apply, repair N. It forwards transport deltas.
5. **Worker/native boundary.** `modelAsk` gets a progress message keyed by
   request id, posted alongside the existing reply. The native adapter calls
   the same callback in-process. Coalesce deltas to one post per animation
   frame so a fast model cannot flood `postMessage` or Svelte.
6. **DocumentClient → ChatTurn.** `turn.progress: { phase, attempt, reasoning,
   text, startedAt per phase }`. `ChatPanel` passes an `onEvent` into `execute`.
   Stop calls `AbortController.abort()` and the turn ends `failed: stopped`.

## 3. The UI (T3-style work log)

The reference is T3 Code's chat timeline, studied in narrative-panel.md §9.1:
assistant rows, work entries and a follow-bottom scroll. Reimplement it in
Svelte, Bits UI and Iron tokens; copy nothing. Inside the running assistant
message:

- **Header:** a live phase label with elapsed seconds ("Thinking · 12s",
  "Writing edit · 7 verbs", "Repairing · attempt 2"), plus a Stop
  `IconButton` (Lucide `square`).
- **Work log:** a `Collapsible`, open while running and collapsed when done,
  like T3. Each phase is one row, with reasoning-summary text streamed under
  "Thinking". When the turn finishes it reads "Worked for 38s".
- **Prose:** text before `<edit` renders through `ChatMarkdown` as it arrives.
  Hold back a trailing `<…` that might begin `<edit`, `<look` or `::view` so
  markup never flashes as prose.
- **Edit forming:** once `<edit` opens, show a live list of verbs as each one
  closes ("create rect "pump body"", "link figures", "place std:laser in
  bench"), plus the raw XML in the existing details block. That is the
  "formulating a command" view, and it costs nothing beyond §2.
- No decorative motion that is not backed by an event (narrative-panel.md §9.3).

## 4. Partial-edit previews (the ambitious part)

Build the edit in the compositions while it streams, then commit the real one
atomically.

### 4.1 What makes it tractable

- **Apply only complete verbs.** Primitive verbs are self-closing elements
  (`<create …/>`), so a tolerant scanner simply ignores a trailing incomplete
  one. Nothing has to be guessed or auto-closed. Verbs only append, so each
  tick adds a suffix of ops to the previous tick's.
- **Aliases** are minted lazily and kept stable across ticks: one map per
  attempt, and the final apply reuses it. Selection and camera then do not
  jump at commit. Stop at the first verb that names an alias not yet seen (a
  forward reference): the preview is the prefix that resolves.
- **Errors mid-stream are silent.** A verb that fails validation pauses the
  preview there. Only the final edit reports `<edit-failed>` to the model.
- **Bodied verbs need more work.** `<import-svg>` needs a tolerant SVG closer:
  keep complete child elements, auto-close open `<g>`/`<svg>`, and drop a
  partial path `d` or redraw it at its last complete command. `<table>` grows
  a row at a time. Component verbs (`<place>`, `<make-component>`) go through
  `compose`, one verb at a time.

### 4.2 What the store lacks

`Store::preview` (iron_document lib.rs:866) only overlays **leaf values** during
a gesture. It cannot create, delete or reparent. Previews therefore need a
store-level **provisional transaction**:

- `apply_provisional(ops)` and `revert_provisional()`. Not in undo history, the
  op log, OPFS persistence or the version counter. Applied incrementally: the
  new suffix only, with a full revert if an earlier op ever changes.
- One owner at a time, like a gesture. A user edit, undo or a scope switch
  reverts it. The final turn then meets the existing version guard and says
  "document changed while the agent was thinking".
- Commit: `revert_provisional()`, then the ordinary `apply` at the captured
  version. Its ids equal the preview's, so there is no flicker.
- Preview nodes draw through chrome as provisional: an accent dashed outline
  and reduced opacity, via a context field per chrome.md. Optionally follow
  them with the camera.

### 4.3 Costs and risks

| Risk | Mitigation |
|---|---|
| Re-applying on every token | Throttle to 5–10 Hz and apply suffixes only; render-cache damage is per chunk |
| Large `import-svg` (Prussia-scale) | Preview only top-level groups, or at a lower rate; the final import is unchanged |
| A provisional op reaching OPFS or history | One flag checked in the persistence and history paths, with Rust tests that a revert leaves the log byte-identical |
| The user edits while the agent streams | Treat it as gesture ownership: the user wins and the preview reverts |
| Repair attempts | Revert, then preview the new attempt from scratch |
| Codex message items | Codex can split a reply into several agentMessage items; concatenate them by `itemId` order, as `turn/completed` already does |

## 5. Phases

| Phase | Builds | Size | Exit |
|---|---|---|---|
| 1 | §2 plumbing, phase events, live header, Stop, idle timeout | M (TS only; bridge, adapters, worker message) | A Codex and a Claude turn show phases live; Stop kills the child; a 60 s turn completes |
| 2 | Streamed prose, reasoning summaries, the forming-verbs list (§3) | M | Prose appears token by token; a long edit lists its verbs as they close |
| 3 | Provisional transactions in `iron_document` and the primitive-verb preview (§4.1–4.2) | L (Rust store + overlay chrome) | Rects and text appear while streaming; the commit is one undo step; Rust tests prove a revert leaves no trace |
| 4 | Bodied verbs: tolerant SVG, tables, component verbs | M–L | An `import-svg` reply grows group by group |

Phases 1–2 are independent of the engine and safe to ship alone. Phase 3 is the
first that changes `iron_document`.

## 6. Built (phases 1–2, 2026-11-06)

- **Events:** `src-ui/model/progress.ts` (`AgentEvent`, `AskLive`, a 50 ms
  batcher that merges deltas and sends a phase change at once).
  `ask()` emits `reading`, `thinking` (attempt N; a repair is attempt 2),
  `looking`, `applying`; a reply that arrives after Stop is not applied.
- **Bridge:** `Accept: application/x-ndjson` streams `text`/`reasoning`/`ping`
  lines, then `done` or `error`; without it, the old single JSON. Idle 90 s
  (reset by each delta), total 10 min. Codex forwards `item/agentMessage/delta`
  and `item/reasoning/summaryTextDelta`; Claude parses stream-json (and waits
  for both process exit and the last line, which can arrive after close).
- **Transports:** `LocalRuntimeTransport` reads the NDJSON (30 s bridge-silence
  limit); `AnthropicTransport` forwards `text` and `thinking` stream events and
  honours the signal.
- **Boundary:** `modelAskProgress` (worker/native → page, keyed by request id)
  and `modelAskCancel` (page → worker/native, aborts that turn's controller).
  Client turn limit 15 min.
- **UI:** `ChatProgress.svelte` (work log: current step, elapsed, Stop; steps
  with reasoning; "Worked for …" folded once done), streamed prose through
  `ChatMarkdown`, and "Writing the edit · N commands" with the last six verbs
  and the markup so far (`streamView`, `editVerbs` in `chat/conversation.ts`).
  A stopped turn reads "Stopped. Nothing was changed." and restores the prompt.

- **Tokens (added the same day):** two more events, `thinking-tokens` (Claude
  Code's `system`/`thinking_tokens` line, its running estimate of hidden
  thinking, every few seconds) and `usage` (exact output tokens with thinking,
  at the end of each model call: Codex `thread/tokenUsage/updated`, Claude's
  `result`, the browser API's final message). The work log shows "~N tokens ·
  R tok/s" while running (estimate: the thinking estimate or shared reasoning,
  plus reply characters ÷ a characters-per-token ratio that starts at 3 and is
  recalibrated from each exact count; measured ~3.4 for Codex, ~2 for Claude),
  and "N tokens (T thinking) · R tok/s" once done, per second of model time.
  Codex reports no count while it reasons, so its counter starts with the reply.

Verified: 65 bun tests (adapters' deltas, NDJSON end to end, idle reset, quiet
timeout, Stop aborting the runtime, `ask` phases, stream splitting, progress
folding, batching); `chat-ui.ts` worker and main with a slow streamed fixture
turn and Stop; `agent-runtime.ts` and `agent-components.ts` worker and main;
live Codex `chat-ui.ts` (10.4 s); live Codex and Claude adapters streaming text
(first text at 6.5 s and 2.7–3.5 s).

## 7. Agent log (2026-11-06, a stopgap)

Until a backend keeps conversations, the bridge writes a local log for
debugging the agent: `scripts/agent-bridge/log.ts`, JSON lines, one file a day
in `.agents/agent-logs/` (gitignored), capped at 200 MB a file. On by default
for `dev:agent`, the standalone bridge and the native launcher;
`IRON_AGENT_LOG=<folder>` moves it, `IRON_AGENT_LOG=0` turns it off. The
browser API path (no bridge) is not logged.

- `system`: each distinct system prompt once per file, by hash (it is ~29 KB).
- `call` (bridge): turn id, provider, model, ms, first-token ms, usage,
  the system prompt's hash, the messages sent (document view, selection,
  history, request, look and repair rounds), the raw reply, shared reasoning,
  and the error, timeout or Stop if any. About 90 KB a call on the demo.
- `turn` (page, `POST /log`, best effort): instruction, ok, attempts, error,
  note, final edit and outcome, history length, and `trace`: each reply's fate
  in order (`look`, `prose`, `edit` with ok/failure/summary, `no-edit`,
  `stale`, `stopped`, `error`). `AskResult.trace` carries the same.

Calls and outcome share the chat turn's id (`ConversationOptions.turnId`,
sent as `turn`). Each chat message has **Copy turn log**: the turn as the chat
saw it (steps, tokens, trace, edit, failure, what had streamed) as JSON, with
the log file to look in. Logs hold document content and prompts, which the
provider already receives; they stay on this machine.

Verified: 69 bun tests (bridge records, `/log` validation and auth, `ask`'s
trace and single outcome record, the native launcher's log, `turnLog`);
`chat-ui.ts` worker and main check every finished turn has its calls and its
outcome in the log (7 turns); live Codex `chat-ui.ts` logged 2 turns (209 KB).


## 8. Repairing a rejected edit (2026-11-06)

The first logged turn (an EDFA component, Claude Opus, 286 s) failed twice
on harness bugs, not on the model's reading of the prompt. Attempt 1 used
`$aliases` inside bindings (`port.signal="{= $spump.port.signal / 10 …}"`),
which the manifest implied were allowed but `edit.ts` passed to the
expression parser unresolved; the store's error had no op and a JSON column
("unexpected '$' at 1 at line 1 column 869"). Attempt 2 dropped the bindings
and left `id` off nodes nothing named (points, ions); every such create was
given the first alias's id, so 41 ops failed as "#U already exists", an id
the model never wrote. Each repair resent the whole 102-op edit.

Fixed:

- Every create without an id, or with an `$alias`, gets its own minted id; an
  alias created twice is refused.
- `$aliases` resolve inside `{= …}` bindings as in id and ref attributes.
- `<edit-failed>` reads as the model wrote the edit: `op` counts the edit's
  verbs from 0 (a verb writing several store ops is one op), with
  `verb="create $sinv"`; ids the edit minted read as their aliases; a bad
  expression the store fails to read in is placed at its verb by its byte
  column; identical errors are one line, `ops="…"` and "(N times)".
- The manifest says `id` is optional on `<create>` and aliases work in
  bindings.
- **`<revise>`** (`src-ui/model/revise.ts`): a repair names only what changes
  in the rejected edit, kept by `ask` as written: `<change op>` (set or add
  attributes), `<replace op>`, `<drop op>`, `<insert after|before>`, and
  `<text old new all>` (exact text, once unless `all`; runs last). `op` is an
  `$alias`/`as=` or the op number, always of the edit as sent, so a revise's
  own order never shifts its handles; root attributes (`expect-version`)
  carry over. The revised edit is applied whole as one transaction and is
  what the chat shows, the turn returns and the log records (`trace` step
  `revised: true`; a revise that cannot apply is `kind: 'revise'` with a
  `<revise-failed>` reason). After a rejected revised edit, the full revised
  edit is shown back so a second revise's numbers match. A turn now has three
  replies (the edit and two repairs), since a repair is usually short. The
  chat streams a revise like an edit ("Revising the rejected edit", its steps
  listed).

Verified: 79 bun tests (10 new in `tests/revise.test.ts`); both logged edits
replayed through `__iron.edit` on the engine apply, attempt 1 with its live
bindings (inversion 0.838, gain 20.27 dB, out 10.64 mW); `chat-ui.ts`,
`agent-runtime.ts`, `agent-components.ts` worker and main; fast smoke.

## 9. Merging an agent's edit with changes made while it worked (2026-11-06)

The second logged build (a Mach–Zehnder modulator, 298 s, 31.7k output
tokens, 169 ops) was thrown away unapplied: the person drew a rectangle and
moved the fiber while it worked (version 14 to 18), and `ask` refused any
edit once the version moved. Replayed on a fresh document, the edit applies.

`src-ui/model/merge.ts` replaces that refusal. At apply time, if the version
moved since the agent read it, `changesSince` reads the transactions after
it from the store's log (`scope_log`, now `DocumentApi.logSince`): leaves
set (solver `derived_ops` included), facets added or removed, nodes moved or
reordered, deleted (a re-create is a restore). `conflicts` checks the
agent's parsed ops against them: a leaf both set, a node or a create's
parent the person deleted, a node both moved, a facet both changed, or a
delete of a node the person changed. None: the edit is applied at the
current version, the turn's `merged` counts the transactions it sits on
(the chat's footer says "merged with N changes made while it worked"), and a
change landing between check and apply is checked again (twice). Conflicts:
`kind="conflict"` errors at the agent's ops, saying what the person set, read
in the agent's terms like any rejection; the agent revises (`<drop op>` keeps
the person's change; an empty `<revise/>` resends its own), and the resend is
judged against the version it was told of. Store-built edits (place,
make-component, circuit, table, context, import, narrative) are refused only
if a node they name was deleted. If the log no longer reaches back
(compaction), the turn ends with an error rather than a retry. Everything
else the store still checks, as ordinary rejections.

Verified: 84 bun tests (5 in `tests/merge.test.ts`: merged apply at the
current version, a conflict dropped or kept by revise, conflict rules, the
log reader, the check-apply race and a log gap); the engine's `scope_log`
shape checked live; `agent-runtime.ts`, `agent-components.ts`, `chat-ui.ts`
worker and main; fast smoke.
