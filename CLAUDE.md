# CLAUDE.md

Do work in the currently checked-out branch. Only do work in a worktree if the user
requests one; don't ask.

If the user requests a worktree, use `.agents/worktrees/<name>` — never `/tmp`, a
session scratchpad, or `.claude/worktrees/`. Create it with `git worktree add`, enter
it with `EnterWorktree`'s `path` parameter, and remove it when the work is done.

Full instructions, including cleanup and the reasoning:

@AGENTS.md
