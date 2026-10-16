# Model interoperability: jobs, live sessions and local acceleration

**Status:** Proposal, pending user approval; planning only. No Model protocol,
transport, runtime output store or native integration has been implemented.
**Date:** 2026-10-16.
**Scope:** The opaque scientific/computational Models of vision 02, not the LLM
authoring client in `src-ui/model/`. Defines a portable contract for external
programs and services, with optional local execution and faster data exchange.
Work order, conditional on approval, lives in
[`../handoff-product.md`](../handoff-product.md) §4.2.

## 1. Boundary and intent

A Model maps declared inputs to computed outputs and may hold private simulation
state. Python, Julia, C++, JavaScript, Rust and hosted services must be able to
participate without adopting Bevy or translating their internals into Iron's
document. Placement is an execution choice: another process, a remote service,
a browser worker, or a host-provided local implementation.

The document stores the Model identity/version, invocation or session declaration,
input bindings, output schema and projection choices. Parameters are ordinary
authored leaves, changed through `iron_document` transactions and gesture APIs.
Model arrays, streamed frames, particle populations and internal state are runtime
data. They do not become authored nodes, transaction logs or undo entries.

```text
authored invocation + resolved inputs -> Model runtime -> provider/transport
                                                    <- outputs/status
immutable runtime outputs -> local projections / Bevy simulation -> rendering
Svelte <-> UI protocol: configuration, commands, status and bounded summaries
```

Bulk Model data terminates at the engine runtime. Native sockets connect directly
to Rust; browser sockets connect in the engine worker, with only the browser API
shim needed to deliver bytes to wasm. Do not route native Model data through
Tauri IPC or Svelte, or browser Model data through the UI main thread by default.
Keep the main-thread web runtime working through the same transport adapter.

## 2. Three support tiers

The first two are portable behavior contracts. The third accelerates either
contract locally; it does not introduce a different scientific API. A provider
advertises supported tiers and capabilities, rather than implementing all three.

| Tier | Contract and uses | Web | Native |
|---|---|---|---|
| **1 — Jobs** | An input snapshot starts an asynchronous evaluation: progress, optional explicitly partial outputs, then a complete result or error. Fits parameter sweeps, finite-element solves, batch trajectories and inverse fits. | Hosted Model over WSS; local worker/Wasm adapter when available. | Same hosted service or a local process over WebSocket; local implementation through the same runtime API. |
| **2 — Live sessions** | Open a stateful session, update parameters or send ordered events, subscribe to timestamped output frames, pause/resume when supported, and close. Fits continuously running simulations, instruments and interactive computed fields. | Bidirectional WSS to a service; worker/Wasm provider for local execution. Rendering consumes the latest accepted local output without awaiting the service. | Same protocol to local or remote processes; native providers may run simulation work in Bevy or background tasks. |
| **3 — Local acceleration** | Negotiate shared CPU buffers or direct local buffer leases to reduce serialization/copying. Retains Tier 1/2 identities, schemas, ordering and lifecycle. | Optional transferable buffers between workers; shared-memory workers only where supported and configured. No shared memory with a remote service. | Control socket plus OS shared memory for another process, or immutable buffer leases for an in-process provider. |

Tier 1 can serve large datasets; size alone does not require a live session.
Tier 2 is about ongoing state and interaction, not a promised latency threshold.
Tier 3 is an optional optimization, not a requirement for normal Model authors.

For broad web/native compatibility, an external Model should offer Tier 1 or 2
over WebSocket. An existing raw-socket or library API can sit behind a small
language-side adapter. Do not require arbitrary scientific code to compile to
Wasm. A Model with native-only dependencies can still serve the browser remotely.
Algorithm identity, schema compatibility and numerical tolerances must be checked
when swapping implementations; the same interface alone does not prove parity.

## 3. Placement and transport

**Baseline external transport: WebSocket, JSON control records and binary data.**
Use WSS for deployed browser services. Native uses the same protocol for remote
services and initially for local processes, so the first Python example works on
both targets. Raw local IPC is an adapter option: Unix-domain sockets on macOS/Linux
and named pipes on Windows, carrying the same application records with explicit
length framing. They are not browser transports and need not ship in the first cut.

Browser endpoints must support browser authentication and origin handling; the
contract cannot require custom WebSocket upgrade headers that only native clients
can set. Connection credentials, executable paths and process-launch settings live
in host/session configuration, resolved from a logical Model binding. Loading a
document does not itself execute a command embedded in that document. Browser use
of an installed localhost service is optional, not the portable deployment baseline.

**Local worker/Wasm is a placement option for both jobs and live sessions.**
Initially use a worker and core WebAssembly with a small generated JS binding where
needed. A Model worker has its own execution budget and can be terminated without
terminating the editor. Browser JS-only providers can use a worker too; this does
not imply embedding a JavaScript engine in native Rust. Their native fallback can
be a service or an explicitly supported host adapter.

Keep an eventual WIT/Component Model binding aligned with the same logical API.
It is not a prerequisite: components can already be transpiled for browser use
through [jco](https://component-model.bytecodealliance.org/language-support/building-a-simple-component/javascript.html),
while [Wasmtime offers a native component host](https://docs.wasmtime.dev/api/wasmtime/component/struct.Component.html).
Matching host imports and runtime capabilities still needs validation; direct
browser component loading is not an assumption. Do not add Wasmtime, a general WASI
environment or dynamic native
plugins in the initial implementation.

**Local acceleration is negotiated independently of behavior.** Browser
`SharedArrayBuffer` is not an arbitrary OS shared-memory mapping usable by a
Python process. It is constrained by the browser's agent-cluster and isolation
rules ([HTML shared-memory rules](https://html.spec.whatwg.org/multipage/structured-data.html#shared-memory)).
Transferable buffers avoid a worker-to-worker clone, but entering Iron's current
wasm linear memory or uploading to the GPU can still copy. Record actual copies;
do not label the whole path zero-copy. No change to browser Bevy threading is
required by this proposal.

## 4. One Model API, with explicit capabilities

The runtime offers typed operations along these lines; names are provisional:

| Operation | Meaning |
|---|---|
| `describe` / `hello` | Agree protocol major version, Model implementation/version, named inputs/outputs, schemas, determinism/state/effect declarations, behavior tiers, delivery policies and resource limits. |
| `evaluate(inputs, request)` | Tier 1 job with a complete immutable input snapshot. |
| `cancel(request)` | Best-effort cancellation; the client can invalidate acceptance immediately even if computation cannot stop. |
| `open(inputs, clock)` | Tier 2 session with explicit initial state/seed and clock policy. |
| `set_inputs(revision, values)` | Atomic parameter update; acknowledge which revision was applied and, for a simulation, its effective step/time. |
| `send_events(events)` | Ordered discrete events, distinct from replaceable parameter values. |
| `subscribe(outputs, delivery, budget)` | Select outputs, receive policy and bounded resource budget; grant data credits. |
| `close` | Release session resources and invalidate further publication. |

`reset`, `pause`, `resume`, `step`, `seek`, `checkpoint`, resumption and output
deltas are optional capabilities. A live instrument may support none of these.
Unsupported operations return a typed error, rather than a false success or an
implicit restart. Bound retained jobs/sessions and retained query/status data too.

Job states are `queued`, `running`, `ready`, `failed`, `cancelled` or `superseded`.
Live states include `opening`, `running`, `paused`, `disconnected`, `failed` and
`closed`. Progress and partial data are explicitly marked; a partial result cannot
masquerade as the completed output. A terminal state is authoritative for its epoch.

## 5. Ordering, time and freshness

- **Identity:** Each message identifies its binding, session epoch, request/run,
  input revision and relevant stream sequence. Use opaque identifiers and encode
  wide counters without depending on JavaScript's integer precision. A reload,
  replacement, restart or cancellation invalidates the old acceptance generation.
- **Reproducibility:** Explicit inputs include seeds, initial state and relevant
  clock/data dependencies. Cache/replay only under declared capabilities, keyed
  by Model implementation/version, schema and complete relevant inputs. Do not
  assume a stochastic or stateful Model is a pure function because it uses Tier 1.
- **Relevant revisions:** Increment the Model's input revision when its resolved
  dependencies change, including relevant preview changes. A caption edit or camera
  move does not reinvoke the Model unless explicitly bound as a Model input.
- **Jobs:** Results for superseded inputs cannot overwrite current results. Default
  expensive evaluations run on commit/explicit request. Debounced preview evaluation
  is an explicit policy, with one latest pending parameter snapshot and bounded work.
- **Live inputs:** Parameters may be latest-wins only when declared replaceable.
  Events such as impulse/reset/trigger remain ordered and are not coalesced. Batches
  preserve parameter/event ordering; an event names the input revision it requires.
  The producer acknowledges input application separately from output publication.
  Frames identify the applied input revision and clock step. Normal consumers accept
  only frames for the current relevant revision; an explicitly retained old frame is
  stale. Historical recording may retain older revisions with their provenance.
- **Three times:** Carry simulation/source time (and step index where relevant),
  local receive time and presentation time separately. Arrival time is not physical
  simulation time. Live wall-clock, explicit stepping and recorded playback are
  distinct clock capabilities; a scientific timestep is not tied to display refresh.
- **Atomic publication:** An output revision names a coherent set of output values
  and buffers. Stage and validate its chunks before one publication at an engine
  boundary. Every composition reads the same accepted revision. Independent Models
  are not implicitly synchronized; coordinated stepping must be explicit.
- **Disconnection:** Keep the last good output marked stale or show unavailable,
  according to the binding's policy. Reconnection creates a new epoch unless the
  provider explicitly negotiates resumption and the required state/base revisions.
  Do not retry a stateful command blindly or claim exactly-once execution. Replay
  requires declared idempotency/deduplication or an explicit reset/resume protocol.
- **Time travel:** Undo restores authored inputs, not a remote process's hidden
  state. It invalidates affected work and requests an appropriate new run/update.
  Seeking a stateful Model requires supported reset/replay/checkpoints or recorded
  output. It does not follow from having a timeline or a seed.

Asynchronous feedback is allowed through explicit session inputs or a delayed
simulation step. It must not create a synchronous dependency cycle in the existing
reactive graph. A drag updates local preview/feedback immediately; a remote answer
is never a prerequisite for the next pointer frame. Fast local simulations may
react continuously, and fast external Models may stream continuously, without
weakening that ownership rule.

## 6. Bulk data, buffering and ownership

Use small JSON control/metadata records and binary arrays. The portable payload
vocabulary starts with scalars/quantities, small records and dense contiguous
row-major arrays, with named dtype, shape, byte length, units and coordinate/domain
metadata. Output schemas describe their meaning (series, spectrum, positions,
field, etc.). No Bevy entity IDs, Rust struct layouts, pointers or wgpu handles
are part of the portable ABI.

For the first wire prototype, use bounded JSON text records and binary records
containing a little-endian header length, a bounded UTF-8 JSON chunk descriptor
and raw little-endian array bytes. The descriptor identifies output revision,
buffer, total length, offset and chunk length. A revision manifest/commit record
defines completeness. Freeze exact schema/framing with cross-language golden
fixtures before connecting product code. Check sizes and shape arithmetic before
allocation; reject unsupported schemas/dtypes rather than guessing.

Negotiate `max_chunk_bytes`, `max_pending_bytes`, `max_inflight_requests` and
per-stream byte/frame credits, under hard client caps. Credits apply to input and
output data as appropriate. They are returned when storage is released, not merely
when a message arrives. Bound producer queues, transport queues, partial assembly,
published snapshots and consumer leases. Oversized data must chunk, decimate or
fail explicitly. Reserve control capacity, interleave bounded chunks and cap
already-enqueued socket bytes so bulk output cannot indefinitely bury input/cancel.
Reserve enough budget for an assembling revision alongside published/in-flight
data, or refuse that subscription. Close on credit/protocol violations; negotiated
queue bounds assume a compliant producer and do not promise control over every
browser/OS-internal buffer.

Each subscription chooses one of two policies:

- **Latest complete state:** Drop/supersede intermediate complete snapshots within
  the negotiated window, release their buffers and report skipped sequences. Never
  combine half of two revisions. This is the normal visualization policy.
- **Lossless sequence:** Preserve every accepted event/sample chunk. Backpressure
  the producer, reduce the requested rate, or stop with an explicit overrun/gap if
  a real-time source cannot pause. A separate visualization subscription can still
  sample latest state. A lossy visualization is not a scientific recording.

Start with complete snapshots. Add deltas only as a negotiated extension carrying
an explicit base revision; on a missing base, request a complete snapshot.
WebSocket supports worker access and binary messages, but its browser API does not
provide this application-level receive-credit contract automatically; implement
it above the transport ([WebSocket standard](https://websockets.spec.whatwg.org/)).

Completed buffers are immutable and runtime-owned. A snapshot handle keeps them
alive through all consumers, including pending GPU uploads. Native shared-memory
slots additionally require a versioned layout, bounds validation, publication
synchronization, generation numbers and explicit leases/acknowledgments; a producer
cannot reuse a slot while a reader holds it. Copy to owned memory when lifetime or
layout cannot be guaranteed. Disconnect and device loss have defined retirement
paths. GPU memory sharing across processes is deferred.

## 7. Integration with Iron

- Extend the ECM runtime signal design rather than adding a second authored value
  system. Bulk outputs live in a runtime buffer/output store; the graph and Bevy
  consumers hold small typed references and revisions. The authored file stores
  invocation/binding data. An explicit recorded run can later be an asset reference;
  project-container and recording implementation remain separate work.
- Model output drives declared derived output ports; existing connections carry
  it downstream to input ports. Providers do not write arbitrary authored leaves,
  use the pointer's gesture overlay, or acquire the document/world from a transport
  callback. Retiring a provider releases only that provider's derived values and
  invalidates dependent projections. Missing/stale output remains observable.
- Introduce a bounded ingress queue/outbox and a named application schedule boundary
  for accepting outputs before dependent graph/simulation consumers. Network I/O,
  worker execution and expensive Model computation never block this boundary.
  Capture input snapshots only after relevant transactions/previews are resolved.
- A small authored declaration may drive a large Bevy simulation. Particle/entity
  populations and simulation internals remain runtime ECS state; publish only
  declared outputs/aggregates back through the Model/signal boundary. A precompiled
  trusted native provider may use Bevy systems/task pools without serializing its
  entire population through the document or a socket. The document does not author
  arbitrary Bevy systems. Browser execution uses a compatible implementation at a
  supported scale or a service, with capability differences visible.
- Continuous outputs are not transactions. A deliberate authored consequence or
  capture is an op in an `iron_document` transaction. This proposal uses the port
  ownership part of the runtime-driver note; it does not adopt general overrides
  of authored leaves or a new collider/physics system.
- Keep protocol/session/buffer logic host-neutral, with browser/native I/O behind
  adapters and a thin Bevy bridge. Module/crate names follow implementation needs;
  start with the smallest extractable module, not a framework or language SDK fleet.
  Keep the existing LLM client and the UI request protocol distinct from this API.

## 8. Native execution requirement and window host

Native Bevy must enable `multi_threaded` in its target feature graph and retain
its task pools and pipelined render thread. One document/world owner at ingress
does not require simulation systems to execute serially. Verify independent ECS
work and rendering/simulation overlap with traces, including the small-document,
large-runtime-population case. Do not require browser Bevy threading for parity.

The proposed Tauri host still owns native windows and the OS event loop. Adapting
`canvas_view` means adapting external-surface registration, not writing an OS
window manager. Tauri/Tao/Wry supply window creation, platform events and webview
text input; the existing DOM path supplies semantic pointer/keyboard input. Iron
must translate the needed window lifecycle/resize/focus/DPI events, retain handles,
map Bevy-issued host requests and implement native wake/presentation scheduling.
Those are real integration duties, and must be inventoried and tested.

Ordinary Bevy applications use winit because `bevy_winit` already connects window
and input events, lifecycle and frame scheduling to Bevy. With a Tauri-owned
window that work is supplied partly by Tauri and partly by the host adapter;
adding a competing `WinitPlugin` is not a shortcut. Plugins requiring `WinitWindows`
are not automatically supported. If the adapter grows into a second windowing
framework, reconsider a winit-owned window with a Wry child webview. Preserve the
shared Svelte/product code under either choice. See
[winit's responsibilities](https://docs.rs/winit/latest/winit/) and
[Wry's native rendering example](https://github.com/tauri-apps/wry/blob/dev/examples/wgpu.rs).

On macOS, initially keep the Bevy app owner and required surface work on the OS
main thread, while ECS tasks and pipelined rendering run on other threads. A
dedicated app-owner thread needs separate proof of main-thread surface/executor
integration. Do not drive native simulation from JS RAF. Restore render-on-demand
only with correct in-flight render/extraction, idle/wake and shutdown behavior.
These are the native feasibility gates in vision 08, not a prerequisite for
prototyping the portable Model protocol in a native headless consumer.

## 9. Acceptance and evidence

No tier is claimed functional until its evidence exists. Record exact producer,
consumer and dependency revisions, fixture/schema/seed, machine, browser/OS,
transport, frame size/rate and queue budgets. There is no universal network-latency
promise. Declare interactive targets and compare against them on the chosen setup.

| Gate | Evidence |
|---|---|
| Portable Model authoring | One Python producer and shared protocol fixtures work with a browser worker consumer and native Rust consumer. Model code has no Bevy/Tauri dependency. Remote web endpoint and native loopback use the same behavior contract. |
| Jobs and freshness | A slow old job cannot replace a new result; cancellation, dependency changes, reload at the same document version, errors and disconnect are tested. Editing an unrelated caption does not rerun the Model. |
| Live semantics | Stateful synthetic simulation accepts parameter revisions and ordered impulses, reports application step/time, and streams coherent outputs. Reconnect/reset/unsupported seek are explicit; all compositions consume the same accepted revision. |
| Data and memory | Scalars plus small, medium and large binary-array fixtures; invalid lengths and partial frames are rejected; a slow consumer stays within negotiated total memory/queue budgets. Latest-state skips and lossless gaps/overruns are observable. |
| End-to-end use | Real Iron bindings expose Model status and read-only derived outputs. At least two compositions react to one accepted output; canonical save and undo contain inputs/bindings, never sample arrays or runtime particles. Local projection changes reuse cached output. |
| Cost and responsiveness | Measure input-revision send to applied acknowledgment and first corresponding output; output ingress to accepted/presented revision; p50/p95, sustained MB/s, allocation/copy bytes, peak resident data and queue age. Do not subtract unsynchronized clocks across hosts. Exercise control changes during bulk transfer. |
| Native simulation capacity | Once the native display gate passes, a small document configures a much larger runtime ECS population. Traces show concurrent ECS work and render/simulation overlap; a stalled webview does not stop simulation. Record native GPU backend/features/limits and compare the same workload with a conventional Bevy/winit host. |
| Optional acceleration | Only after a measured socket/copy bottleneck: compare identical output traces through copied and accelerated paths, including slot reuse, slow readers, cancellation and producer crash. State the fallback and any native-only capability. |
| Regressions | Required native store tests, both wasm renderer builds and run-iron smoke for behavior changes; keep the smoke skill's expected output current. No behavior tests are claimed by this planning revision. |

## 10. Scope of approval and completion record

Approval would adopt the tiers, ownership, baseline WebSocket/binary contract and
the initial implementation sequence in the product handoff. It would authorize
the portable foundation and jobs/live-session vertical slices. Shared memory,
Wasm/Component hosting, language SDK expansion, recording assets and a full native
port remain later work, selected from evidence. This proposal does not require
building every transport before a first useful Model connects.

- **2026-10-16 — proposal drafted:** reviewed vision 02/03/05/06/09, the ECM signal
  design, native plan 08, the runtime-driver/time-machine notes and current engine
  seams. Defined portable jobs/live sessions and optional local acceleration,
  including output ownership, backpressure, revisions, clock semantics and native
  concurrency gates. No implementation or feasibility results; awaiting approval.
