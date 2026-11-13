#!/bin/zsh
# Prepares a fresh worktree; T3 Code runs it on worktree creation (t3.json).
# Safe to run again. Builds nothing: wasm and native builds take minutes, so the
# agent runs the one its task needs (run-iron skill, "Typical durations").
set -euo pipefail

root="${T3CODE_PROJECT_ROOT:-}"
here="$(git rev-parse --show-toplevel)"

bun install --frozen-lockfile
# The run-iron skill's driver has its own dependencies (playwright-core).
(cd .claude/skills/run-iron && bun install --frozen-lockfile)

# assets/ is gitignored (large .glb models); copy them from the main checkout.
if [[ -n "$root" && "$root" != "$here" && -d "$root/assets" ]]; then
  for f in "$root"/assets/*(N); do
    [[ -e "$here/assets/${f:t}" ]] || cp -R "$f" "$here/assets/"
  done
fi

# Seed target/ from the main checkout's build cache as APFS copy-on-write
# clones (`cp -c`: seconds, no disk used until a file changes), so the first
# build here recompiles only the workspace's own crates, not the ~300
# dependencies (2m 18s instead of 5m 46s for the webgl2 wasm, 2026-11-13).
# Only the profiles agents build: the wasm and native `dev-opt` outputs, and
# `target/dev-opt`, where build scripts and proc-macros compiled for the host
# go even when `--target` is given (without it every crate rebuilds). The
# cache only helps while the flags match: a different RUSTFLAGS rebuilds all.
if [[ -n "$root" && "$root" != "$here" && ! -e "$here/target" && -d "$root/target" ]]; then
  for d in dev-opt wasm32-unknown-unknown/dev-opt aarch64-apple-darwin/dev-opt; do
    if [[ -d "$root/target/$d" ]]; then
      mkdir -p "$here/target/${d:h}"
      cp -c -R "$root/target/$d" "$here/target/$d" || echo "could not clone target/$d; it builds cold"
    fi
  done
fi

echo "worktree ready: $here"
