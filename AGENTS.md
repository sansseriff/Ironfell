# Agent instructions

## Start here

Read [`plans/README.md`](plans/README.md) first. It lists which plans are current, which
handoff owns the next task for your track (product work or rendering performance), and
which plans are historical. Where plans disagree, that index decides.

- Every document mutation is an op in a transaction through `iron_document`.
- Run the run-iron smoke script before committing behaviour changes, and keep its
  expected output in `.claude/skills/run-iron/SKILL.md` current.
- Record completed work in `plans/roadmap.md`'s Status table and the owning plan.
- End commit messages with the `Co-Authored-By` line the session gives you. Git
  workflow (branches, pushing, PRs) is under "Branches, pushing and merging" below.
- Check in with the user before running a step that takes over a minute: rebuilds,
  the full smoke, cold test runs. Typical times are in the run-iron skill's
  "Typical durations" table (marked ⏱); say which step and its estimate, and add or
  correct a row when a step's time is new or off.

## Council review

The `council` skill (from tstack) runs when the user says "council". This setup has
two providers, Anthropic and OpenAI, so do not require a third provider and do not
report a missing one. Council members: the latest Claude Opus and the latest GPT Sol,
both at high reasoning effort. Resolve the exact models with `orchestrator_capabilities`
and start each one with `delegate_task`. If either member is unavailable, report it
instead of substituting.

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

- **Size Svelte UI in CSS px, and never scale it by `devicePixelRatio`.** A 2D
  canvas view's zoom is CSS px per document px (the engine applies the pixel
  ratio itself, `ViewCamera::scale`), so DOM laid over or beside a canvas view
  (a hybrid view, such as the timeline's column) places itself at
  `document px × zoom` from what the engine serves, in CSS px. If you find
  yourself dividing by `devicePixelRatio` in Svelte, the engine call should
  answer in CSS px instead. Check such UI by screenshot at 2× as well as 1×
  (`SIZE=…@2`, the run-iron skill).

## Branches, pushing and merging

`origin/master` is the base for every new worktree, so keep it current and working.

- **Push whenever you commit,** whatever branch you are on. A pushed branch is the
  backup, and how the user and other agents see the work; a pushed `master` is
  what new worktrees start from. Never force-push `master`, or a branch you did not
  create.
- **In a worktree thread** (its own `t3/<task>` branch), open a PR to `master` when
  the task is done (`gh pr create`), unless the user says otherwise. Before that,
  rebase onto `origin/master` and re-run the checks your change touches (the
  run-iron smoke for behaviour changes).
- **Merge only when the user asks.** Merging the PR is what lets T3 delete the
  worktree ("Delete merged worktrees").
- **In a local thread** on `master`, commit there as before and push each commit.

## Worktrees

Do work in the currently checked-out branch. Only do work in a worktree if the user
requests one; don't ask.

**T3 Code creates and owns worktrees.** A worktree T3 creates is bound to its thread,
so T3's diff view, PR links and cleanup all work. A worktree an agent makes with
`git worktree add` is invisible to T3 and nothing ever cleans it up. So:

- **New work in a worktree:** the user starts a worktree thread in T3, or you start
  one with `t3_thread_launch` and `workspaceStrategy: { type: "worktree" }`.
- **Move this thread's work into a worktree:** `t3_worktree_handoff`.
- **Never** create one with `git worktree add`, Claude Code's `EnterWorktree`, or
  `Agent` with `isolation: "worktree"`. The last two put it in `.claude/worktrees/`
  or a `/tmp` scratchpad.

T3 puts worktrees in the folder set under Settings → Worktrees → Worktree location
(default `~/.t3/worktrees/<project>/<name>`), branching from `origin/master`. On
creation it runs `t3.json`'s setup script, `scripts/setup-worktree.sh`: `bun install`
and the gitignored `assets/` copied from the main checkout. It builds nothing; run
the build your task needs (run-iron skill), and expect a cold Rust/wasm build there
(see "Typical durations").

### Cleaning up

Each worktree carries its own `target/` and `node_modules/`, which together have
measured over 13 GB. T3's cleanup settings remove worktrees whose PR merged, whose
thread was deleted, or that went inactive; it keeps any with local changes. To remove
one by hand:

```sh
git worktree list                     # what exists
git worktree remove <path>            # --force if only ignored build artifacts remain
git worktree prune                    # clears bookkeeping if a directory vanished on its own
```

A branch checked out in a worktree cannot be checked out in the main repo — that is the
cause of "already used by worktree" errors. Remove the worktree to free the branch.

Before deleting a branch whose worktree you removed, confirm its commits are reachable
from `master` (`git merge-base --is-ancestor <tip> master`), or that the branch is
pushed. Then the commits survive deletion and only the name is lost.
