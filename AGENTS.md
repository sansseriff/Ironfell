# Agent instructions

## Worktrees

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
