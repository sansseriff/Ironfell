#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

cargo build \
  --manifest-path rust/Cargo.toml \
  --target wasm32-unknown-unknown \
  --release

mkdir -p public/wasm

wasm-bindgen \
  --target web \
  --out-dir public/wasm \
  --out-name latency_rust \
  rust/target/wasm32-unknown-unknown/release/latency_rust.wasm

if command -v wasm-opt >/dev/null 2>&1; then
  wasm-opt -Oz \
    --enable-bulk-memory \
    --enable-nontrapping-float-to-int \
    public/wasm/latency_rust_bg.wasm \
    -o public/wasm/latency_rust_bg.wasm
fi
