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
  `messages` and optional `model`.
- `scripts/agent-bridge/runtimes.ts`: independent provider adapters.
- Codex: `codex app-server`, JSON-lines stdio, initialize/initialized, ephemeral
  thread/start, turn/start, completed agent-message items and turn/completed.
  Uses existing CLI auth. Live-tested with `codex-cli 0.166.0-alpha.5.1`, signed
  in using ChatGPT; no API key was supplied.
- Claude: `claude -p --bare --output-format json`, no built-in tools, strict
  empty MCP configuration, no settings discovery or persisted session.
  Requires `ANTHROPIC_API_KEY` in the bridge environment. Never assumes a
  claude.ai subscription funds third-party integration. Installed CLI
  `2.1.317` supports these flags. Protocol tested with a fixture only.
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

Each turn uses a temporary empty directory. Codex has a read-only sandbox,
never approvals, disabled shell/unified execution, apps, plugins, hooks, skill
search, web search and configured MCP servers. Unexpected server-initiated
requests fail the turn. Claude uses bare mode and no tools. These are runtime
restrictions, not OS-level confidentiality isolation from a hostile local
process. No general agent shell is exposed through Iron's prompt.

The bridge times out at 45 seconds per completion; HTTP at 50 seconds; the
existing outer budget is 120 seconds, covering one repair. Disconnect/shutdown
abort the process. Cleanup escalates to SIGKILL after one second, bounds output
and removes temporary directories. There is no automatic provider fallback.

The agent captures the version before reading context and refuses a reply if
the document changed during generation. Ordinary transactions also commit
against that captured version. Existing API lifetime guards reject disposed
browser/native generations. An engine restart does not yet abort the HTTP
request immediately; it can continue until timeout, but cannot edit that dead
generation. End-to-end cancellation and a UI Stop action remain follow-ups.

Native CSP allows the default loopback port; its adapter uses the same factory.
Native packaging/parity is untested. Other native ports require a CSP change.

## 4. Bootstrap

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
IRON_AGENT_ORIGINS=http://127.0.0.1:5175,http://localhost:5175 bun run agent:serve
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

For Claude, set `ANTHROPIC_API_KEY` in the bridge environment and optionally
`IRON_CLAUDE_BIN`, then select Claude · local runtime. Never enter the provider
key in the bridge-token field. A CLI supporting `--bare` is required.

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

## 6. Follow-up plan

1. Explicit Stop and cancellation through all adapter lifetimes; panel progress
   and usage events; clear queued/running states.
2. Persistent provider sessions, per-scope identity/revisions, bounded history,
   resume/reconnect semantics and normalized provider errors.
3. Shared Iron tools (schema, scoped reads, validated edits) over provider-native
   MCP/dynamic tools, with explicit user approval policy. Preserve the same
   transaction boundary; no arbitrary repository access.
4. Native sidecar supervision/packaging and lifecycle tests; live Claude
   API-backed parity once credentials are supplied.
5. Packaged-native parity before distributing the local bridge integration.

## 7. Primary references

- https://developers.openai.com/codex/app-server (redirects to
  https://learn.chatgpt.com/docs/app-server).
- https://code.claude.com/docs/en/headless.
- https://platform.claude.com/docs/en/agent-sdk/overview (third-party auth).

Reviewed 2026-11-03 with installed CLI help and Codex-generated JSON schemas.
Iron never extracts or copies either runtime's OAuth tokens.
