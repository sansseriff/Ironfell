#!/bin/zsh
# Creates the standing worktree pool: pool-1 … pool-N (default 5) beside T3's
# worktrees, each a checkout of origin/master on a local parking branch named
# after it (T3 lists no detached worktrees), with its target/ seeded
# from the main checkout's build cache (setup-worktree.sh). Slots are reused
# task after task (pool-start.sh), so their build caches stay warm. Safe to run
# again: existing slots are left alone. See AGENTS.md, "Worktree pool".
set -euo pipefail

scripts="${0:A:h}"
main="$(git -C "$scripts" worktree list --porcelain | awk '/^worktree /{print $2; exit}')"
dir="${POOL_DIR:-$HOME/.t3/worktrees/iron}"
count="${1:-5}"

git -C "$main" fetch -q origin
for n in $(seq 1 "$count"); do
  slot="$dir/pool-$n"
  if [[ -e "$slot" ]]; then
    echo "pool-$n exists: $slot"
    continue
  fi
  git -C "$main" worktree add -q --no-track -B "pool-$n" "$slot" origin/master
  (cd "$slot" && T3CODE_PROJECT_ROOT="$main" zsh "$scripts/setup-worktree.sh")
  echo "pool-$n created: $slot"
done
