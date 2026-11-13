#!/bin/zsh
# Starts a task in a pool slot (pool-1 … pool-5, AGENTS.md "Worktree pool"):
# run inside the slot as a task's first step.
#
#   zsh scripts/pool-start.sh <task>     # → branch t3/<task> from the latest origin/master
#
# Refuses while the slot is still in use: uncommitted changes, unpushed
# commits, or an open PR on its current branch (pass --force to ignore the
# PR). Keeps the slot's own target/, so only what changed since its last build
# recompiles; then bun install and assets/ as on creation.
set -euo pipefail

die() { echo "pool-start: $*" >&2; exit 1; }

force=0
if [[ "${1:-}" == "--force" ]]; then force=1; shift; fi
task="${1:-}"
[[ -n "$task" ]] || die "usage: pool-start.sh [--force] <task>"
branch="t3/${task#t3/}"

here="$(git rev-parse --show-toplevel)"
[[ "${here:t}" == pool-<-> ]] || die "$here is not a pool slot (pool-N)"
main="$(git worktree list --porcelain | awk '/^worktree /{print $2; exit}')"

[[ -z "$(git status --porcelain)" ]] || die "uncommitted changes in ${here:t}; commit and push them, or discard them, first"

git fetch -q --prune origin
prev="$(git symbolic-ref -q --short HEAD || true)"
# The slot's parking branch (pool-N) is local only and stays.
[[ "$prev" != "${here:t}" ]] || prev=""
if [[ -n "$prev" ]]; then
  if ! git rev-parse -q --verify "origin/$prev" >/dev/null; then
    [[ "$(git rev-list --count "origin/master..$prev")" == 0 ]] || die "$prev is not pushed"
  elif [[ -n "$(git rev-list "origin/$prev..$prev")" ]]; then
    die "$prev has unpushed commits"
  fi
  if (( ! force )) && command -v gh >/dev/null; then
    state="$(gh pr view "$prev" --json state -q .state 2>/dev/null || true)"
    [[ "$state" != "OPEN" ]] || die "$prev has an open PR; the slot is still in use (--force to take it anyway)"
  fi
fi

git switch -q --no-track -c "$branch" origin/master
# The previous branch is pushed (checked above), so only its local name goes.
[[ -z "$prev" ]] || git branch -q -D "$prev"

T3CODE_PROJECT_ROOT="$main" zsh "$here/scripts/setup-worktree.sh" >/dev/null
echo "${here:t} on $branch at $(git rev-parse --short HEAD) (origin/master)"
