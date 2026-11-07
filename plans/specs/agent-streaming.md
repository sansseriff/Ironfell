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

Verified: 65 bun tests (adapters' deltas, NDJSON end to end, idle reset, quiet
timeout, Stop aborting the runtime, `ask` phases, stream splitting, progress
folding, batching); `chat-ui.ts` worker and main with a slow streamed fixture
turn and Stop; `agent-runtime.ts` and `agent-components.ts` worker and main;
live Codex `chat-ui.ts` (10.4 s); live Codex and Claude adapters streaming text
(first text at 6.5 s and 2.7–3.5 s).
