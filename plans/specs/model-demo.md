# Model demo: chaos from a Python Model

**Status:** Proposed 2026-11-24, not approved, nothing built.
**Builds on:**
- the query node, reactivity.md §8 and `query.rs`, built 2026-11-10;
- the web host's `ModelLoader`, `src-ui/runtime/model_loader.ts`;
- Tier 1 of [`model-interop.md`](model-interop.md), which is still a proposal.

**Why:** the query node is the external-callout path that exists. Only a test
fixture exercises it today (`query.txt` with `model-server.ts`, a `sin(kx)`
answer after a delay). This demo shows it doing something worth watching:
physics computed in Python, steered live from Iron.

## 1. What exists

- **The query node.** A `query` node names a Model by URL (`query.model`)
  and sends a `query.input`, usually a `record(…)` bound to sliders. It
  publishes `status` (`idle` / `pending` / `ready` / `error`), `value`,
  `error` and `stale`. Before the first answer, `value` is `query.initial`.
- **Debounce.** The first request goes at once. Later ones wait for the
  input to rest (`query.debounce`, 0.2 s by default), so dragging a slider
  asks once per rest, not once per frame.
- **Late answers.** A newer request overtakes an older one, and the late
  answer is dropped.
- **Only while read.** A query asks only while a binding reads one of its
  outputs. Since `EN-8c`, it asks with its inputs as they are at that moment,
  even on a page that is resting.
- **The transport.** `ModelLoader` POSTs the input as JSON from the engine
  worker. The response's JSON is the value: an array of numbers becomes a
  series, an object a record. Requests time out after 60 s.
- **Expressions over the answer.** `get(record, 'k')`, `at`, `interp`, `sample`
  and `len` read answers. `std:plot` draws `xs`/`ys` series.

Nothing here needs new engine code. The demo is a document, a server and a
run-iron script.

## 2. The demo

One page, "Double pendulum (Python)":

- **Inputs:** sliders for the two initial angles `θ1` and `θ2`, a tiny offset
  `ε` (default 1e-6 rad) and the span `T` (default 20 s).
- **The query:** `query.model="http://localhost:5299/double-pendulum"` with
  `query.input="{= record(theta1: …, theta2: …, eps: …, t_end: …, n: 801)}"`.
- **The Model integrates two copies**, one started from `θ1` and one from
  `θ1 + ε`, and answers with flat series:
  - `t`;
  - `x_a`, `y_a`, `x_b`, `y_b` (the second bob of each copy);
  - `sep` (the distance between the two bobs, log scale).
- **Plots:**
  - the two bobs' paths overlaid (`x_a`/`y_a` and `x_b`/`y_b`);
  - `log10(sep)` against `t`. The roughly straight rise is the Lyapunov
    growth, which is the point of the demo: two runs that agree to six
    digits part within seconds.
- **A drawn pendulum at the playhead.** The rods and bobs are bound to
  `interp(t, x_a, now)` etc., where `now` is the timeline's time. Scrubbing
  moves both pendulums, so the moment they part is visible.
- **Status text:** `status`, `stale` and the Model's compute time. While the
  answer is pending, the old curves stay, marked stale.

## 3. The Model server

- **Where:** `tools/models/double_pendulum.py`.
- **Standard library only:** `http.server`, `json`, `math`. It does fixed-step
  RK4 in pure Python, about 2 × 16,000 steps for 20 s at 1 ms, well under a
  second. NumPy is optional and used only if present, so the demo runs on
  any Python 3.
- **CORS:** `Access-Control-Allow-Origin: *`, and `OPTIONS` answered, as in
  `model-server.ts`.
- **The answer:** sampled down to `n` points, and includes `ms` (the compute
  time).
- **Ending:** an argument `--fail` answers 500, so the error state can be
  shown.
- **Starting:** `python3 tools/models/double_pendulum.py --port 5299`.

## 4. What it demonstrates about the engine

| Behaviour | How the demo shows it |
|---|---|
| Debounce | Drag `θ1`: one request per rest. The server logs each request |
| Pending and stale | A slow `T` (60 s of simulation) shows `pending`, with the old curves marked stale |
| Late answers dropped | Two quick changes: only the newest answer lands |
| Asked only while read | Hide the page: no requests. Show it: one request, with the current inputs (`EN-8c`) |
| Error | Restart the server with `--fail`: `error` shows, and the last good curves stay |

## 5. Verification

- **The test:** `model-demo.txt` (run-iron), with the server running. It
  loads the page, waits for `ready`, moves `θ1`, and checks that one new
  request was made and the curves changed. It also scrubs the timeline,
  checks the drawn bob follows `interp`, and takes screenshots.
- **Its duration:** goes in the skill's "Typical durations" table.
- **Optional engine test:** a test in `iron_document` with a stub answer,
  checking that the page's bindings read the answer's series. That test
  needs no Python.

## 6. Open questions

- **Placing the page:** in the demo document behind a flag (it needs a
  server), or loaded by the script with `__iron.edit`, as `query.txt` does.
- **`interp` arguments:** check its signature against the expression
  language when building (`series.rs`). If it is not `interp(xs, ys, x)`,
  use `sample`.
- **Not covered:** Tier 2 live sessions (streamed frames from a
  running simulation). A follow-up demo, once `model-interop.md` is approved,
  could stream the pendulum live instead of returning a finished trajectory.
