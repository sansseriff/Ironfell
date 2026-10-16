# CLAUDE.md

Before starting substantial work, ask the user whether it should be done in a worktree.
Small-scale work may be done in the main checkout without asking.

If the user chooses a worktree, use `.agents/worktrees/<name>` — never `/tmp`, a
session scratchpad, or `.claude/worktrees/`. Create it with `git worktree add`, enter
it with `EnterWorktree`'s `path` parameter, and remove it when the work is done.

Full instructions, including cleanup and the reasoning:

@AGENTS.md
