#!/bin/zsh
# Prepares a fresh worktree; T3 Code runs it on worktree creation (t3.json).
# Safe to run again. Builds nothing: wasm and native builds take minutes, so the
# agent runs the one its task needs (run-iron skill, "Typical durations").
set -euo pipefail

root="${T3CODE_PROJECT_ROOT:-}"
here="$(git rev-parse --show-toplevel)"

bun install --frozen-lockfile

# assets/ is gitignored (large .glb models); copy them from the main checkout.
if [[ -n "$root" && "$root" != "$here" && -d "$root/assets" ]]; then
  for f in "$root"/assets/*(N); do
    [[ -e "$here/assets/${f:t}" ]] || cp -R "$f" "$here/assets/"
  done
fi

echo "worktree ready: $here"
