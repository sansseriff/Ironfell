#!/bin/zsh
# Frees disk space Cargo never reclaims (AGENTS.md "Disk cleanup").
#
# Cargo keeps every hashed artifact it has ever built, so `target/` grows without
# bound. This sweeps files not used in the last CLEAN_DAYS days (default 7) from the
# main checkout and each pool slot, then reports T3 worktrees that look finished.
# Anything swept is rebuilt on demand; nothing under src/ or git is touched.
#
#   scripts/clean-disk.sh            sweep for real
#   scripts/clean-disk.sh --dry-run  show what would go
#
# Run weekly by launchd: scripts/install-clean-schedule.sh.
set -uo pipefail

export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin"
root="${0:A:h:h}"
wt_dir="${POOL_DIR:-$HOME/.t3/worktrees/iron}"
days="${CLEAN_DAYS:-7}"
dry=()
[[ "${1:-}" == --dry-run ]] && dry=(--dry-run)

free() { df -h /System/Volumes/Data | awk 'NR==2 {print $4 " free, " $5 " used"}'; }

echo "== $(date '+%Y-%m-%d %H:%M') clean-disk (older than ${days}d${dry:+, dry run})"

# Sweeping under a running build removes files cargo is about to read.
if pgrep -x cargo >/dev/null || pgrep -x rustc >/dev/null; then
  echo "cargo or rustc is running; skipping this run"
  exit 0
fi

if ! command -v cargo-sweep >/dev/null; then
  cargo install cargo-sweep || { echo "cargo-sweep is missing and would not install"; exit 1; }
fi

echo "before: $(free)"
for checkout in "$root" "$wt_dir"/pool-<->(N/); do
  [[ -d "$checkout/target" ]] || continue
  echo "-- ${checkout:t}"
  (cd "$checkout" && cargo sweep $dry --time "$days" 2>&1 | tail -3)
done
echo "after:  $(free)"

# Report only: T3 owns its worktrees and deletes them with their threads. A worktree
# here is a full source checkout plus a target/ that has diverged from its clone, so
# a forgotten one is the next place the disk goes.
echo "-- T3 worktrees (not pool slots)"
git -C "$root" worktree list --porcelain | awk '/^worktree /{print $2}' | while read -r w; do
  [[ "$w" == "$root" || "${w:t}" == pool-* ]] && continue
  merged=no
  git -C "$w" merge-base --is-ancestor HEAD origin/master 2>/dev/null && merged=yes
  printf '%s  %s uncommitted  merged-into-master=%s\n' "${w:t}" \
    "$(git -C "$w" status --porcelain 2>/dev/null | wc -l | tr -d ' ')" "$merged"
done
echo "(remove one with: git worktree remove --force <path>)"
