#!/bin/zsh
# Build the WebGPU and WebGL2 dev wasm and bind each where the Vite dev server
# expects it (src-ui/wasm/<backend>/).
#
# Both, because the loader picks one per browser at load time
# (src-ui/runtime/backend_policy.ts): desktop Chromium on macOS/Windows gets
# WebGPU, everything else WebGL2. The headless smoke runs on SwiftShader, which
# has no WebGPU adapter, so it forces `?gfx=webgl2`; BACKENDS=webgl2 skips the
# WebGPU build when only the smoke needs to run.
# dev-opt (opt-level 1, debug info) builds in ~2.5 min cold, ~15 s warm, per backend.
# PROFILE=wasm-bench builds at opt-level 3 for the render benchmark.
set -e
PROFILE=${PROFILE:-dev-opt}
BACKENDS=(${=BACKENDS:-webgpu webgl2})
cd "$(dirname "$0")/../../.."

# The wasm-bindgen CLI must match the crate version pinned in Cargo.lock.
WBG_VERSION=$(awk -F'"' '/^name = "wasm-bindgen"$/{getline; print $2}' Cargo.lock)
CURRENT_WBG=$(wasm-bindgen --version 2>/dev/null | awk '{print $2}')
if [ "$CURRENT_WBG" != "$WBG_VERSION" ]; then
  echo "wasm-bindgen CLI ($CURRENT_WBG) != Cargo.lock ($WBG_VERSION); installing matching CLI…"
  cargo install -f wasm-bindgen-cli --version "$WBG_VERSION"
fi

# The benchmark build needs SIMD like the shipped build (build-wasm.sh): without
# +simd128, vello_common's fearless_simd silently takes a scalar fallback, so CPU
# encode timings would not describe the renderer that ships.
if [ "$PROFILE" = "wasm-bench" ]; then
  export RUSTFLAGS="${RUSTFLAGS:-} -Ctarget-feature=+simd128"
fi

# Each backend is a different feature set, so cargo keeps both sets of
# dependency builds and a warm rebuild of either stays warm. The final
# ironfell.wasm path is shared, so each is bound before the next build.
for backend in $BACKENDS; do
  echo "building $backend ($PROFILE)"
  cargo build --profile "$PROFILE" --no-default-features --features "$backend" --target wasm32-unknown-unknown
  mkdir -p "src-ui/wasm/$backend"
  rm -f "src-ui/wasm/$backend/.stub"   # left by older versions of this script
  wasm-bindgen --out-dir "src-ui/wasm/$backend" --web "target/wasm32-unknown-unknown/$PROFILE/ironfell.wasm"
done

# wasm_loader.ts imports both artifact URLs at build time, so Vite needs a file
# at each path. If a backend was skipped and has never been built, stub it with
# a copy of one that was, marked so a later real build replaces it. A stub
# serves the wrong backend's binary, so this only exists to keep Vite resolving.
built=${BACKENDS[1]}
for backend in webgpu webgl2; do
  if [ ! -e "src-ui/wasm/$backend/ironfell_bg.wasm" ]; then
    mkdir -p "src-ui/wasm/$backend"
    cp src-ui/wasm/$built/ironfell_bg.wasm src-ui/wasm/$built/ironfell.js \
       src-ui/wasm/$built/ironfell.d.ts src-ui/wasm/$built/ironfell_bg.wasm.d.ts "src-ui/wasm/$backend/"
    touch "src-ui/wasm/$backend/.stub"
    echo "warning: src-ui/wasm/$backend is a stub copy of $built" >&2
  fi
done
ls -la src-ui/wasm/*/ironfell_bg.wasm
