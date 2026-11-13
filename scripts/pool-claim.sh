#!/bin/zsh
# Claims a free pool slot for a task (AGENTS.md "Worktree pool"): what "start
# work in an available worktree" runs. Run from any checkout of the project.
#
#   zsh scripts/pool-claim.sh <task>    # prints the claimed slot's path
#
# Free means: nothing uncommitted, and either idle (on its parking branch,
# pool-N) or its branch's PR merged or closed. The first free slot is put on
# t3/<task> from the latest origin/master (pool-start.sh), under a lock so two
# claims never take the same slot. Exits 1, claiming nothing, when none is free.
set -euo pipefail

die() { echo "pool-claim: $*" >&2; exit 1; }

task="${1:-}"
[[ -n "$task" ]] || die "usage: pool-claim.sh <task>"
scripts="${0:A:h}"
dir="${POOL_DIR:-$HOME/.t3/worktrees/iron}"

lock="$dir/.pool-lock"
for _ in {1..60}; do
  mkdir "$lock" 2>/dev/null && break
  sleep 1
done
[[ -d "$lock" ]] || die "another claim holds $lock; if none is running, remove it"
trap 'rmdir "$lock" 2>/dev/null' EXIT

free() {
  local slot="$1" branch state
  [[ -z "$(git -C "$slot" status --porcelain)" ]] || return 1
  branch="$(git -C "$slot" symbolic-ref -q --short HEAD)" || return 1
  [[ "$branch" == "${slot:t}" ]] && return 0
  state="$(cd "$slot" && gh pr view "$branch" --json state -q .state 2>/dev/null)" || return 1
  [[ "$state" == MERGED || "$state" == CLOSED ]]
}

for slot in "$dir"/pool-<->(N/n); do
  if free "$slot"; then
    (cd "$slot" && zsh "$scripts/pool-start.sh" --force "$task") >&2
    echo "$slot"
    exit 0
  fi
done
zsh "$scripts/pool-status.sh" >&2
die "no pool slot is free"
