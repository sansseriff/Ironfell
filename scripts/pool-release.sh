#!/bin/zsh
# Returns a pool slot to idle (AGENTS.md "Worktree pool"), for a task that
# ends without a merged PR. Run inside the slot. Not needed after a merge: the
# next claim takes the slot over.
#
#   zsh scripts/pool-release.sh            # the branch must be pushed (or empty)
#   zsh scripts/pool-release.sh --discard  # drop its uncommitted and unpushed work
set -euo pipefail

die() { echo "pool-release: $*" >&2; exit 1; }

here="$(git rev-parse --show-toplevel)"
[[ "${here:t}" == pool-<-> ]] || die "$here is not a pool slot (pool-N)"
discard=0
[[ "${1:-}" == "--discard" ]] && discard=1

git fetch -q origin
branch="$(git symbolic-ref -q --short HEAD || true)"
[[ "$branch" != "${here:t}" ]] || branch=""
if (( ! discard )); then
  [[ -z "$(git status --porcelain)" ]] || die "uncommitted changes; commit and push them, or pass --discard"
  if [[ -n "$branch" ]] && [[ -n "$(git rev-list "$branch" --not --remotes=origin)" ]]; then
    die "$branch has unpushed commits; push them, or pass --discard"
  fi
fi
git reset -q --hard
git clean -qfd
git switch -q -C "${here:t}" origin/master
[[ -z "$branch" ]] || git branch -q -D "$branch"
echo "${here:t} idle at $(git rev-parse --short HEAD) (origin/master)"
