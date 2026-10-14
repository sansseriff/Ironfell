#!/bin/sh
# Builds only the Bevy-free control; leaves the served Iron app untouched.
set -eu
cd "$(dirname "$0")/../../.."
RUSTFLAGS="${RUSTFLAGS:-} -Ctarget-feature=+simd128" cargo build -p iron_renderer_probe --profile wasm-bench --target wasm32-unknown-unknown
mkdir -p .agents/render-perf/vello-probe
wasm-bindgen --out-dir .agents/render-perf/vello-probe --web target/wasm32-unknown-unknown/wasm-bench/iron_renderer_probe.wasm
