# CLAUDE.md

Do work in the currently checked-out branch. Only do work in a worktree if the user
requests one; don't ask.

If the user requests a worktree, T3 Code creates it: `t3_thread_launch` with a
`worktree` workspaceStrategy for new work, or `t3_worktree_handoff` to move this
thread. Never `git worktree add`, `EnterWorktree`, or `Agent` worktree isolation.

Push every commit. In a worktree thread, open a PR to `master` when done; merge only
when the user asks (AGENTS.md, "Branches, pushing and merging").

Icons: import each Lucide icon from its own path only,
`import Plus from "@lucide/svelte/icons/plus"`, never `{ Plus }` from
`@lucide/svelte`, `@lucide/svelte/icons` or `lucide-svelte` (AGENTS.md says why).

Breaking changes are welcome: never keep backwards compatibility, no migrations or
shims; a format change bumps the schema and old files stop loading (AGENTS.md).

Rust builds and Rust tests (`build.sh`, `cargo build`, `cargo test`), however long,
run without asking unless the user's first message asks for confirmations; check in
before other steps over a minute (AGENTS.md, "Start here").

Council: two members only, the latest Claude Opus and the latest GPT Sol, at high
effort. Don't require a third provider (AGENTS.md, "Council review").

Full shared instructions, including the required Svelte component styling and
Lucide icon conventions, worktree cleanup, and the reasoning:

@AGENTS.md
