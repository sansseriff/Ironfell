# Ironfell

Technical and scientific communication in age of AI

## Run it

```sh
bun install
bun run wasm:fast   # optimized build (opt 3, SIMD): use this to judge performance
bun run wasm        # dev build (opt 1, debug info): faster to compile, ~3x slower to run
bun run dev         # serves http://localhost:5173/Ironfell/
```

`bun run dev` serves whichever wasm build ran last. The bottom-right overlay says
which: `Build: wasm-bench (opt 3, SIMD)` in green, or a red `dev-opt … SLOW` line.
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
