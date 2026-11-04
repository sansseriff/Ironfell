# CLAUDE.md

Do work in the currently checked-out branch. Only do work in a worktree if the user
requests one; don't ask.

If the user requests a worktree, use `.agents/worktrees/<name>` — never `/tmp`, a
session scratchpad, or `.claude/worktrees/`. Create it with `git worktree add`, enter
it with `EnterWorktree`'s `path` parameter, and remove it when the work is done.

Icons: import each Lucide icon from its own path only,
`import Plus from "@lucide/svelte/icons/plus"`, never `{ Plus }` from
`@lucide/svelte`, `@lucide/svelte/icons` or `lucide-svelte` (AGENTS.md says why).

Council: two members only, the latest Claude Opus and the latest GPT Sol, at high
effort. Don't require a third provider (AGENTS.md, "Council review").

Full shared instructions, including the required Svelte component styling and
Lucide icon conventions, worktree cleanup, and the reasoning:

@AGENTS.md
