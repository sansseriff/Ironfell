#!/bin/sh
# The standalone Bevy/winit latency control (README.md), built with the
# worktree's target cache. Pass IRON_BEVY_* settings in the environment,
# e.g. IRON_BEVY_PACING=late-fifo or IRON_BEVY_MOTION=1.
cd "$(dirname "$0")/../.." || exit 1
MACOSX_DEPLOYMENT_TARGET=14.0 exec cargo run --manifest-path src/iron_latency_probe/Cargo.toml --target-dir target --profile dev-opt --target aarch64-apple-darwin "$@"
