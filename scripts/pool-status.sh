#!/bin/zsh
# Lists the pool slots (AGENTS.md "Worktree pool"): each one's branch,
# uncommitted files and PR state. A slot is free when it is idle (on its
# parking branch, pool-N) or
# its branch's PR is merged or closed, with nothing uncommitted.
set -uo pipefail

dir="${POOL_DIR:-$HOME/.t3/worktrees/iron}"
for slot in "$dir"/pool-<->(N/); do
  branch="$(git -C "$slot" symbolic-ref -q --short HEAD || echo detached)"
  [[ "$branch" != "${slot:t}" ]] || branch=idle
  dirty="$(git -C "$slot" status --porcelain | wc -l | tr -d ' ')"
  pr="-"
  if [[ "$branch" != idle ]] && command -v gh >/dev/null; then
    pr="$(cd "$slot" && gh pr view "$branch" --json state,number -q '"#\(.number) \(.state)"' 2>/dev/null || echo "no PR")"
  fi
  printf '%-8s %-40s %3s uncommitted  %s\n' "${slot:t}" "$branch" "$dirty" "$pr"
done
