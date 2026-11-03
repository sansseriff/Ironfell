# Agent instructions

## Start here

Read [`plans/README.md`](plans/README.md) first. It lists which plans are current, which
handoff owns the next task for your track (product work or rendering performance), and
which plans are historical. Where plans disagree, that index decides.

- Every document mutation is an op in a transaction through `iron_document`.
- Run the run-iron smoke script before committing behaviour changes, and keep its
  expected output in `.claude/skills/run-iron/SKILL.md` current.
- Record completed work in `plans/roadmap.md`'s Status table and the owning plan.
- Don't push. End commit messages with the `Co-Authored-By` line the session gives you.
- Check in with the user before running a step that takes over a minute: rebuilds,
  the full smoke, cold test runs. Typical times are in the run-iron skill's
  "Typical durations" table (marked ⏱); say which step and its estimate, and add or
  correct a row when a step's time is new or off.

## Svelte frontend: components and icons

These rules apply to every agent (Claude, Codex, or otherwise) adding or changing
UI in `src-ui/`, including new panels such as the Document panel.

- **Use Bits UI or Melt UI primitives, not ad hoc controls.** `bits-ui` is already
  installed and is the default here; prefer it and the existing wrappers in
  `src-ui/lib/ui/`. Use Melt UI where an established project pattern calls for it;
  do not add a second component library just to implement a control.
- **Never ship unstyled, browser-default buttons or form controls.** Bits UI and
  Melt UI supply behavior, not the finished appearance: apply Iron's existing
  styling from `src-ui/ui.css` and theme tokens from `src-ui/app.css`. Match nearby
  controls' sizing, spacing, borders, hover/focus/disabled states, and light/dark
  themes. A plain semantic `<button>` is fine for a simple action when styled to
  match an existing Iron button; an unstyled `<button>` is not.
- **Read a working example before building UI.** Start with
  `src-ui/lib/ui/Select.svelte` (shared select),
  `src-ui/views/html/CompositionSettings.svelte` (menu and icon trigger),
  `src-ui/lib/ConvertDialog.svelte` (dialog and action buttons), and
  `src-ui/views/html/TimelineTransport.svelte` (styled toolbar buttons).
  Reuse the matching component/pattern rather than inventing a new look.
- **Use Lucide icons by default**, via the installed `@lucide/svelte` package.
  Match nearby icon sizes and stroke widths; do not substitute emoji, Unicode
  glyphs, another icon library, or hand-drawn SVG when Lucide has a suitable
  icon. Give icon-only controls an accessible name (`aria-label`) and a
  tooltip/title where appropriate.
- **Import each icon from its own path, and only that way:**

  ```ts
  import Plus from "@lucide/svelte/icons/plus";               // yes
  import ChevronDown from "@lucide/svelte/icons/chevron-down"; // yes: the kebab-case name from lucide.dev
  import { Plus } from "@lucide/svelte";                       // no
  import { Plus } from "@lucide/svelte/icons";                 // no
  import { Plus } from "lucide-svelte";                        // no: the old package
  ```

  `@lucide/svelte` and `@lucide/svelte/icons` both resolve to the package's
  barrel, which re-exports all ~1,900 icons. Tree-shaking trims that from a
  production build, but the Vite dev server loads and transforms the whole
  library for every page that imports from it. `@lucide/svelte/icons/<name>`
  resolves to one file. Check that a name exists with
  `ls node_modules/@lucide/svelte/dist/icons/<name>.svelte`. Never build an
  icon path at runtime (``import(`@lucide/svelte/icons/${name}`)``): to choose
  an icon dynamically, import the candidates and pass the component as a prop,
  as `src-ui/lib/ui/IconButton.svelte` takes `icon`. `vite.config.ts` excludes
  `@lucide/svelte` from `optimizeDeps`, so a newly used icon is served as is
  instead of making Vite re-optimise and reload the page; keep that.

## Worktrees

Do work in the currently checked-out branch. Only do work in a worktree if the user
requests one; don't ask.

Worktrees for this repo live in **`.agents/worktrees/<name>`**, inside the repo and
gitignored. Never create a worktree in `/tmp`, in a session scratchpad, or anywhere
else outside the repo: those directories are invisible from the editor, get orphaned
when the session ends, and quietly hold on to multi-gigabyte Rust/wasm build caches.

### Creating one

The `EnterWorktree` tool's `name` parameter hardcodes `.claude/worktrees/` and cannot
be pointed elsewhere. Do not use it. Create the worktree explicitly, then enter it by
path:

```sh
git worktree add .agents/worktrees/<name> -b <branch>   # add an explicit base ref as the last arg if not branching from HEAD
```

then `EnterWorktree` with `path: .agents/worktrees/<name>`.

Do not use subagent worktree isolation (`Agent` with `isolation: "worktree"`) for work
that should outlive the session — it places the worktree in the session scratchpad
under `/tmp`, which is exactly what this policy exists to prevent.

### Cleaning up

Each worktree in this project carries its own `target/` and `node_modules/`, which
together have measured over 13 GB. Remove worktrees when the work is merged or
abandoned, rather than leaving them to accumulate:

```sh
git worktree list                              # what exists
git worktree remove .agents/worktrees/<name>   # --force if only ignored build artifacts remain
git worktree prune                             # clears bookkeeping if a directory vanished on its own
```

A branch checked out in a worktree cannot be checked out in the main repo — that is the
cause of "already used by worktree" errors. Remove the worktree to free the branch.

Before deleting a branch whose worktree you removed, confirm its commits are reachable
from `master` (`git merge-base --is-ancestor <tip> master`). If they are, the commits
survive deletion and only the name is lost.
