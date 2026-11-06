# Ironfell

Technical and scientific communication in age of AI

## Run it

### Iron + Codex and Claude (one terminal)

From the repository root, after dependencies and wasm are built:

```sh
bun run dev:agent:demo --open  # temporary demo, opens the browser
# or:
bun run dev:agent --open       # your last saved scope
```

These start Iron on **127.0.0.1:5175** and the local agent bridge on
**127.0.0.1:4318**. Choose **Codex** or **Claude**, and a model, in the Model or
Lesson composer; the bridge token fills automatically. Keep the terminal open; **Ctrl+C stops both**.
Demo changes are temporary, so export anything you want to keep.

The launcher finds `codex` on PATH or the local Codex app's bundled CLI, and
`claude` (Claude Code) on PATH or `~/.local/bin`. `IRON_CODEX_BIN` and
`IRON_CLAUDE_BIN` take precedence. Each uses its CLI's existing login, so Codex
runs on your ChatGPT plan and Claude on your Claude subscription; if sign-in is
needed, run the login commands printed at startup. To bill Claude to an API key
instead, set `IRON_CLAUDE_AUTH=api-key` and `ANTHROPIC_API_KEY`.
No model call, wasm rebuild or login happens merely by starting the servers.
Occupied ports cause a clear failure, never killing or reusing another server.
Run `bun run dev:agent --help` for options.

`bun run dev` remains frontend-only; `bun run agent:serve` remains bridge-only.

### Tauri + agents (second Claude profile)

```sh
bun run native:agent
```

For adaptive rendering, use this instead:

```sh
bun run native:agent:adaptive
```

This starts the local bridge, builds/runs Tauri, and connects it automatically
with **Claude selected**, using **`~/.claude_2nd`** and its subscription login.
It requests the last saved scope when native persistence is enabled; older
native builds retain their existing unsaved-workspace fallback.
No token to paste. Closing Tauri or pressing Ctrl+C stops the app and bridge.
Codex remains available in the provider menu. No model inference runs at startup.
The token travels through native IPC, not the URL or browser storage; the bridge
does not expose browser pairing in this mode. An occupied bridge port fails
without stopping the existing server.

`IRON_CLAUDE_CONFIG_DIR=/absolute/profile/path` explicitly changes the profile;
`IRON_CLAUDE_BIN` changes the executable. The launcher deliberately defaults to
`.claude_2nd`, even if the parent shell's `CLAUDE_CONFIG_DIR` points elsewhere.
If needed, sign in with `CLAUDE_CONFIG_DIR="$HOME/.claude_2nd" claude auth login`.
Launching an app directly from Finder does not start the bridge; use this command
for the managed app/agent lifecycle.

### Initial build / frontend only

```sh
bun install
bun run wasm:fast   # optimized build (opt 3, SIMD): use this to judge performance
bun run wasm        # dev build (opt 1, debug info): faster to compile, ~3x slower to run
bun run wasm:smallfast  # the shipped build (LTO, wasm-opt -Oz, SIMD), both backends: judge size with this
bun run dev         # serves http://localhost:5173/Ironfell/
```

`bun run dev` serves whichever wasm build ran last. Settings names it, as does the
tooltip on a view's FPS readout (composition settings → **Show FPS**).
After switching builds, hard-reload the page.

**Settings:** the gear beside **+** in any tab bar. Render scale (canvas pixels per
CSS pixel; "Match a 4K panel" for a 4K display in a scaled macOS mode) applies at
once, as does a move to a screen of another density. The 2D renderer and worker
mode restart the app, keeping the document. Settings are kept per browser.
The app follows the system's light/dark appearance unless Settings forces one.

**UI components:** dropdowns, menus and switches come from
[bits-ui](https://bits-ui.com), icons from [@lucide/svelte](https://lucide.dev),
imported per icon (`@lucide/svelte/icons/<name>`). Colours are the tokens in
`src-ui/app.css`; shared component styles are in `src-ui/ui.css`.

**Stress test:** click **+** in any tab bar and pick a size under *Stress test*
(1,000–20,000 shapes). It opens a Stress tab there; the slider at its top rotates
the whole field. Picking another size replaces the scene. `?stress=N` in the URL,
or `await __iron.stress(N)` in the console, does the same.

## Shared controls for a fiber and its shadow

Attach points from separate paths to the same empty controls so a fiber and its
shadow move together. The paths keep their own contours, styling and tangent
handles; each attached point keeps an editable offset from its control.

1. Draw the fiber and shadow as separate paths, select both, and choose **Make
   component**. Enter the component's main and add two **Empty** controls from
   the shape toolbar. Name them endpoint A and endpoint B.
2. Select the main or one of its parts. In the inspector, click **Attach points
   to control**. The task stays pinned to this main while you select geometry.
3. Choose endpoint A. Pick the corresponding start point on each path using
   the numbered point buttons, or edit a point on the canvas and click
   **Collect canvas point**. Click **Attach 2 points** to apply both together.
   For a packed path, first click **Enable point attachments** on that path.
4. Choose endpoint B and repeat for the two end points. Move either empty to
   move both paths' attached points. Rotation and scale also affect the offsets;
   tangent handles retain their authored directions in the path's frame.
5. To make the endpoints connectable, choose **Expose as input port** for A and
   **Expose as output port** for B. Select each empty to set its signal kind,
   connector and signal expression in the existing inspector. Duplicate the main
   to create linked instances, then connect compatible ports with port drags.

Use **Edit offset** to adjust a point relative to its control, or **Detach** to
release it while preserving its current position. Detach points before deleting
their control. Each applied operation is undoable. Pending picks change nothing
until **Attach**; **Done** or **Cancel pending picks** closes the task while
keeping edits already applied. Attachments are authored on the main and shared
by its instances; nested-instance attachment authoring is not supported yet.

See the [authoring workflow specification](plans/specs/authoring-workflows.md)
for implementation details and remaining workflow work.

## Browser checks

With the app running locally, run the fast smoke from the repo root:

```sh
cd .claude/skills/run-iron
GPU=1 SIZE=1680x1000@1 bun run drive.ts < smoke.txt
URL='http://localhost:5173/Ironfell/?gfx=webgl2&demo&showcase=0&layout=default' bun run attachments.ts
```

The default smoke checks startup, dragging, bindings, undo/redo, rejected edits and
save/reload in about five seconds on a warm local GPU. The attachment fixture
checks the shared-control workflow in about ten seconds. The comprehensive suite
remains available as `smoke-full.txt` (about four minutes); use it for broad
regression checks and use focused fixtures for the feature being changed.
