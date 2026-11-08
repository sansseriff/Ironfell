# Local agent runtimes: Codex and Claude

**Status:** Bootstrap implemented 2026-11-03/04 in the `agent-runtimes` worktree,
initially based on `0d8e416`; current main (`fcba711`) integrated before merge
validation. The main checkout's unrelated work was preserved. This is LLM agent integration, not the computational Model
proposal in `model-interop.md`.

## 1. Existing surface and decisions

Iron already had a bounded edit loop: `src-ui/model/api.ts` exposes the shared
browser/native document API; `manifest.ts` generates instructions from its
schema; `agent.ts` supplies tree/model/selection/netlist context and one repair
opportunity; `edit.ts` turns edit markup into operations applied atomically by
`iron_document`, with model provenance, solver reporting and undo. Panels and
`__iron.ask` already existed, but required the browser Anthropic transport.

Keep those boundaries. A runtime is a `ModelTransport`, not another document
store or an alternative mutation path. Providers share Iron's schema and edit
language, not their authentication or vendor protocols.

## 2. Implemented architecture

`Iron panel → DocumentClient → browser worker/main or native adapter → ask →
LocalRuntimeTransport → authenticated loopback HTTP bridge → provider process`.

The reply returns to `applyEdit` and the existing transaction machinery. The
bridge receives instructions and document context, not a browser API key. It
neither owns nor mutates the document.

- `scripts/agent-bridge/server.ts`: `POST /complete` with `provider`, `system`,
  `messages` and optional `model` and `effort`; `POST /models` with `provider` lists what each
  runtime offers (token-checked, cached five minutes, outside the one-at-a-time
  completion lock).
- `scripts/agent-bridge/resolve.ts`: finds each CLI (`IRON_CODEX_BIN` /
  `IRON_CLAUDE_BIN`, PATH, then its installer's place: the Codex app's bundled CLI,
  `~/.local/bin/claude`). `dev:agent` and the bridge alone (`bun scripts/agent-bridge/server.ts`) both use it; Claude is
  optional (a missing one fails only when asked).
- `scripts/agent-bridge/runtimes.ts`: independent provider adapters.
- Codex: `codex app-server`, JSON-lines stdio, initialize/initialized, ephemeral
  thread/start, turn/start, completed agent-message items and turn/completed.
  Uses existing CLI auth. Live-tested with `codex-cli 0.166.0-alpha.5.1`, signed
  in using ChatGPT; no API key was supplied.
- Claude: `claude -p --output-format stream-json --verbose --include-partial-messages`
  (since 2026-11-06, [`agent-streaming.md`](agent-streaming.md)), no built-in tools, strict empty MCP
  configuration, no settings sources or persisted session. **By default it runs on
  the signed-in Claude Code login (the user's Claude subscription)**, with
  `--safe-mode` (every customization off: CLAUDE.md, skills, plugins, hooks,
  agents, output styles; auth and model selection kept), and without
  `ANTHROPIC_API_KEY`/`ANTHROPIC_AUTH_TOKEN` or a surrounding Claude Code session's
  variables (`CLAUDECODE`, `CLAUDE_CODE_*`, …) in its environment (decided by the
  user 2026-11-04: development uses subscriptions). `IRON_CLAUDE_AUTH=api-key`
  selects the old path, `--bare` and `ANTHROPIC_API_KEY`. Live-tested 2026-11-04
  with Claude Code `2.1.318` signed in via claude.ai: Haiku answered through the
  bridge in 2.6 s with no key. Claude Code adds two short account-level notes of
  its own (the account email, a downloaded-files rule) to the context even in safe
  mode; they go only to Anthropic, which has the prompt anyway. This is personal
  use of one's own Claude Code; distributing Iron with claude.ai login for other
  people would need Anthropic's approval (see the Agent SDK reference below).
- Models: the composer has a model picker beside the runtime picker, filled from
  `/models`: Codex's own list for the login (`model/list` on its app-server, its
  default marked), Claude Code's aliases (opus, sonnet, haiku, fable: each the
  latest of its family), the Claude API's ids for the API-key path. "Default" is
  the runtime's own; a model typed in Agent settings that is not listed shows as
  custom. The runtime and each runtime's model are remembered in browser storage
  (`iron.agent.v1`).
- Reasoning effort (2026-11-07): one menu with the model (as T3 Code's), the
  chosen model's levels beside it. Codex's come from `model/list`
  (`supportedReasoningEfforts`: low … max, and `ultra` on some) and go to
  `turn/start` as `effort`; Claude Code's are low … max, passed as `--effort`;
  the browser API path sends `output_config.effort`. "Default" sends nothing,
  so the runtime's own applies: Codex's `model_reasoning_effort` in
  `~/.codex/config.toml`, Claude Code's built-in default (the bridge strips
  `CLAUDE_EFFORT` and reads no settings), the API model's default. Each
  runtime's effort is remembered with its model; a level the chosen model does
  not list falls back to Default. The bridge validates it and logs it per call.
- Fast (Codex, 2026-11-07): `model/list` gives each model's `serviceTiers`
  (every current model offers `priority`, named "Fast": 1.5–2x speed, some at
  increased usage). A Fast checkbox under the effort levels, shown when the
  model offers it, sends `serviceTier: "priority"` on `turn/start`; off sends
  nothing, so `service_tier` in `~/.codex/config.toml` applies ("default").
  Remembered per runtime; the trigger shows a bolt. The bridge accepts a tier
  only for Codex.
- Existing `AnthropicTransport` remains available as “Claude · browser API”.
- Shared `AgentSettings.svelte` in conversation and narrative panels supplies
  provider, endpoint, in-memory bridge token and optional runtime model. Blank
  runtime model means the runtime default, not the legacy browser API model.
- `__iron.configureAgent({provider, endpoint?, token?, model?})` exposes the same
  configuration to the existing driver; it does not return secrets.

Provider processes are stateless between asks. The shared conversation UI now
passes bounded recent history; repair receives the original context and failed
exchange. See [`narrative-panel.md` §9](narrative-panel.md#9-shared-conversation-ui-t3-investigation-2026-11-03)
for the T3-inspired Model/Lesson UI and its current validation. It is not yet
durable chat across reloads, persistent provider sessions, MCP or an autonomous tool loop.
“UI” here means authored 2D document content, not rewriting Svelte/repo files.

## 3. Security and lifecycle

The bridge binds only `127.0.0.1`, checks Host and an explicit Origin allowlist,
requires a random bearer token, accepts JSON, bounds bodies to 1 MB, and permits
one completion at a time. HTTP cannot choose executable paths, arguments,
working directories or environment variables. Provider credentials stay local;
document context is sent to the selected model provider.

Local development pairing: the CLI enables `POST /pair`, gated by the exact
Origin allowlist, an HTTP loopback Origin, JSON and a custom preflight header.
Embedded bridge instances must opt in with `allowPairing: true`. Vite dev pages
discover the token on initialization, settings opening and before each local
ask (including after token rotation). Tokens stay in page memory, never URLs or
browser storage. Failed discovery preserves manual credentials. Native/static
builds retain manual token entry. This is a trusted local-development convenience,
not isolation from local programs that can forge HTTP headers. Do not allow
untrusted apps on the permitted origins. Provider credentials are never returned.

Since 2026-11-06 the bridge also keeps a local agent log of prompts, replies and
turn outcomes for debugging ([`agent-streaming.md`](agent-streaming.md) §7;
`IRON_AGENT_LOG=0` turns it off). It holds document content and stays on this machine.

Each turn uses a temporary empty directory. Codex has a read-only sandbox,
never approvals, disabled shell/unified execution, apps, plugins, hooks, skill
search, web search and configured MCP servers. Unexpected server-initiated
requests fail the turn. Claude uses bare mode and no tools. These are runtime
restrictions, not OS-level confidentiality isolation from a hostile local
process. No general agent shell is exposed through Iron's prompt.

Since 2026-11-06 ([`agent-streaming.md`](agent-streaming.md)) the bridge times out
a runtime that produces nothing for 90 seconds (each streamed delta resets it),
and any completion after 10 minutes; a streamed response pings every 10 seconds
and the transport gives up after 30 seconds without a byte. The whole turn
(repair and looks included) is capped at 15 minutes. Before that: 45 s per
completion, 50 s HTTP, 120 s per turn. On the subscription path the Claude
environment also drops `ANTHROPIC_BASE_URL` with the token: from inside an agent
session (T3, Claude Code) it pointed at the host's proxy and every call got 401. Disconnect/shutdown
abort the process. Cleanup escalates to SIGKILL after one second, bounds output
and removes temporary directories. There is no automatic provider fallback.

The agent captures the version before reading context and refuses a reply if
the document changed during generation. Ordinary transactions also commit
against that captured version. Existing API lifetime guards reject disposed
browser/native generations. An engine restart does not yet abort the HTTP
request immediately; it can continue until timeout, but cannot edit that dead
generation. End-to-end cancellation and a UI Stop action remain follow-ups.

Native CSP allows the default loopback port; its adapter uses the same factory.
Native launcher/pairing and packaged smoke are verified below; full native
provider-edit parity and a distributable bundled sidecar remain follow-ups.
Other native ports require a CSP change.

## 4. Bootstrap

### Native launcher (2026-11-04)

`bun run native:agent` runs the same managed agents and second Claude
profile with the adaptive frame mode (the native default since 2026-11-06): synchronous rendering,
adaptive pacing, and frame latency 2. The alias is regression-tested against
that preset, and a subprocess fixture verifies all three settings reach the
native child without changing the profile. No separate bridge setup is needed.

`bun run native:agent` starts an authenticated loopback bridge on 4318 and
builds/runs Tauri via `native:run`, requesting the last saved scope when native
persistence is enabled (otherwise the existing unsaved-workspace fallback).
It requires the second Claude profile at
`~/.claude_2nd`, uses subscription auth regardless of parent API-key defaults,
and passes that profile only to its own runtime processes. An explicit
`IRON_CLAUDE_CONFIG_DIR` overrides the profile; `IRON_CLAUDE_BIN` overrides the
executable. Codex is optional and remains selectable. No default-account fallback
or paid inference happens at startup. Both services stop on Ctrl+C, native exit
or startup failure, without killing an existing port owner.

The launcher passes a fresh token, fixed loopback endpoint and default provider
to the native process through its environment. The `agent_connection` Tauri
command returns only those connection fields to the local main webview, not
provider credentials. It rejects non-local origins and unexpected endpoints.
The UI uses this IPC path in production/native mode, selects Claude on first
pairing, and reacquires the same connection after webview reload. Token snapshots
still protect manual edits and stale sessions. Native bridge browser pairing is
disabled. Finder-launched builds without the launcher retain manual fallback;
this is a development supervisor, not a bundled Bun/CLI sidecar installer.

Validation: 51 JS tests pass, including five native launcher/IPC/profile fixtures;
frontend checking has zero errors (10 existing warnings), launcher TypeScript
passes, and the Rust native-connection validation test passes. Fresh native
package builds succeed (warm ~30–33 s; shared-engine rebuild ~2 min). The packaged
app smoke and exact `bun run native:agent` entrypoint pass: native workspace,
private connection, Model tab, document reads with Model active, automatic
client pairing, Claude default, authenticated model listing, no token storage,
private connection after reload, editor readiness, client auto-pairing after
reload and native document reads after reload. All 12 checks pass without errors.
The second profile's subscription login was confirmed without logging account
details or invoking a model; profile routing is also verified in a subprocess
fixture. No paid inference from the second account was performed.

The initial test incorrectly waited for an editor on the new scope-picker route;
normal agent launches now open the last scope and tests explicitly open an
ephemeral demo. Native readiness is published when the control session wakes,
independent of a display callback, preventing loading-overlay/display-clock
startup and reload stalls. Initial document-change notification still follows
the first frame. Native IPC uses asynchronous dispatch. `__iron.agentState()`
reports connection readiness without returning the token; tests use it rather
than relying on popup visibility/timing. Test windows are kept drawable only in
the opt-in self-test mode.

Logs: `.agents/native-agent-{tests,check,build,selftest,command,rust-tests}.log`.
Local-only changes; unrelated main-checkout edits are preserved. Nothing pushed.

Commit validation 2026-11-05 used an isolated snapshot containing only agent and
native-launcher changes plus required native boot/reload support: 52 JS tests,
frontend checking (0 errors / 9 existing warnings), and the fast run-iron smoke
(six assertions, no page errors) pass. Smoke expected output is unchanged.
Cached wasm was reused; native build and 12-check evidence above are retained.
Unrelated scope/persistence, FPS, timeline and other UI edits remain uncommitted.
Logs: `.agents/native-agent-commit-{tests,check,smoke}.log` in the main checkout.

### Browser launcher

Recommended: one terminal in the main repository after integration:

```sh
cd /Users/andrew/Documents/PROGRAM_LOCAL/iron
bun run dev:agent:demo --open
```

`bun run dev:agent --open` instead opens the last saved scope. Both commands
start Vite on 127.0.0.1:5175 and the bridge on 127.0.0.1:4318, derive the exact
allowed app origins, generate an in-memory pairing token, and discover Codex
from `IRON_CODEX_BIN`, PATH, or the local Codex app's bundled CLI. No token or
provider key needs pasting. The terminal prints the app URL, executable, login
help and Ctrl+C instruction. `--open` is optional. Ctrl+C stops both services and
active bridge requests; a startup failure closes services started by this
launcher without killing anything already running. These commands reuse built
wasm and installed dependencies; they never start a rebuild or paid inference.
`bun run dev:agent --help` lists the options. The combined launcher deliberately
uses bridge port 4318 to match the app's default endpoint.

For advanced/custom-port use, the separate commands remain available:

```sh
cd /Users/andrew/Documents/PROGRAM_LOCAL/iron
bun run dev -- --host 127.0.0.1 --port 5175 --strictPort
```

```sh
cd /Users/andrew/Documents/PROGRAM_LOCAL/iron
export IRON_CODEX_BIN="$HOME/.codex/plugins/.plugin-appserver/codex-cli/bin/codex"
IRON_AGENT_ORIGINS=http://127.0.0.1:5175,http://localhost:5175 bun scripts/agent-bridge/server.ts
```

The executable above exists on this machine; other installations can use
`codex` on PATH or an explicit trusted path. Use that CLI's supported login
command if not signed in. Open `http://127.0.0.1:5175/Ironfell/?demo`, select
Model or Lesson and choose Codex in the composer's provider menu. The local
development page fills the bridge session token automatically, including after
refreshes. The **Agent settings gear beside Send** shows connection status and a
**Connect automatically** retry; manual token entry remains available.
Endpoint defaults to `http://127.0.0.1:4318`. Omit `showcase=0` to load the demo
Lesson; otherwise that tab offers to create a narrative.
Ask for a shape in a named 2D composition. Demo pages are ephemeral; export
valuable persisted scopes before experimenting.

For Claude, sign in once with `claude auth login` (Claude Code), then pick Claude
and a model in the composer; `IRON_CLAUDE_BIN` overrides the executable. To bill an
API key instead, start the bridge with `IRON_CLAUDE_AUTH=api-key` and
`ANTHROPIC_API_KEY`. Never enter a provider key in the bridge-token field.

Default allowed origins are Vite on port 5173 (localhost and 127.0.0.1),
`tauri://localhost` and `http://tauri.localhost`. Override the exact
comma-separated list with `IRON_AGENT_ORIGINS`, never a wildcard.
`IRON_AGENT_PORT` changes the port. `IRON_AGENT_TOKEN` supplies a token of at
least 32 characters; otherwise one is generated. Bridge tokens stay in browser
memory, not localStorage. The legacy browser API key behavior is unchanged.

## 5. Tests and evidence

`bun test tests/`: 45 pass, including launcher discovery, startup/pairing,
cleanup and occupied-port safety, plus local pairing restrictions/discovery. Covers HTTP auth,
Origin/Host restrictions, body limits, concurrency, timeout, disconnect,
Codex handshake/events/failure/approval/exit/cancellation, Claude auth/tool
flags, transaction routing, repair, stale replies and no-edit/transport errors.
Deterministic fixtures are separate from live inference.

`bun .claude/skills/run-iron/agent-pairing.ts` passes automatic browser pairing,
reload, bridge token rotation, authenticated send, and absence of token storage
or URL leakage, using a temporary fixture bridge and no paid inference. The
shared chat worker/main regression also passes after this change. Frontend check:
0 errors / 9 existing warnings; frontend build and strict bridge TypeScript pass.

With the worktree Vite server on 5175:

```sh
bun .claude/skills/run-iron/agent-runtime.ts
RUNTIME=main bun .claude/skills/run-iron/agent-runtime.ts
LIVE_AGENT=1 IRON_CODEX_BIN="$HOME/.codex/plugins/.plugin-appserver/codex-cli/bin/codex" \
  bun .claude/skills/run-iron/agent-runtime.ts
```

Each test requires an ephemeral `?demo` URL, waits for the initial demo to
finish loading, creates an isolated 2D composition, requests an 80×60 rectangle at
(100,120), then requests x=240 and green fill. It checks engine readback,
unchanged dimensions/y, undo and no page errors, then restores the original
ephemeral document. Total deadline is 55 seconds, with 20 seconds per
completion. Live calls consume the signed-in account's usage. Evidence without
tokens is in gitignored `.agents/agent-runtime/{fixture,live}-{worker,main}.json`.

Fixture and **live Codex** runs passed in worker and main-thread browser modes
on 2026-11-03 using cached wasm; live edits each succeeded first attempt.
Integration validation 2026-11-04, after incorporating main `fcba711`: both wasm
backends rebuilt (85.53 s total), 45 JS tests, frontend check (0 errors / 9
existing warnings), strict bridge/launcher types, web and native-mode frontend
bundles, fast `smoke.txt`, broad `smoke-full.txt`, `narrative.txt`, pairing/rotation,
fixture Codex create/edit/undo and shared chat in worker/main modes all pass.
Browser runs report no page errors; smoke expected output is unchanged. No new
paid inference was needed. Native packaging/runtime and live Claude remain
unverified; frontend native-mode builds do not imply packaged-native validation.
Validation logs and fixture evidence are retained in the main checkout's ignored
`.agents/agent-runtimes-validation/`. Nothing is pushed.

Post-merge test isolation: Vite treats port 0 as its default port, not an ephemeral
listener. Launcher tests now reserve an explicit ephemeral app port, so an unrelated
dev server on 5173 cannot satisfy a post-shutdown request and produce a false failure.

## 6. Follow-up plan

1. ~~Explicit Stop and cancellation through all adapter lifetimes; panel progress
   and usage events; clear queued/running states.~~ Stop, progress and streaming
   done 2026-11-06 ([`agent-streaming.md`](agent-streaming.md)); usage events open.
2. Persistent provider sessions, per-scope identity/revisions, bounded history,
   resume/reconnect semantics and normalized provider errors.
3. Shared Iron tools (schema, scoped reads, validated edits) over provider-native
   MCP/dynamic tools, with explicit user approval policy. Preserve the same
   transaction boundary; no arbitrary repository access.
4. Distributable native sidecar packaging and broader provider-edit parity;
   managed native startup, lifecycle and IPC pairing are implemented above.
5. Reasoning effort per turn (both CLIs take one: Codex per model, Claude Code
   `--effort`).
6. Packaged-native parity before distributing the local bridge integration.

## 7. Primary references

- https://developers.openai.com/codex/app-server (redirects to
  https://learn.chatgpt.com/docs/app-server).
- https://code.claude.com/docs/en/headless.
- https://platform.claude.com/docs/en/agent-sdk/overview (third-party auth).

Reviewed 2026-11-03 with installed CLI help and Codex-generated JSON schemas.
Iron never extracts or copies either runtime's OAuth tokens.
