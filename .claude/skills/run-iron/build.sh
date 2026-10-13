#!/bin/zsh
# Build the WebGL2 dev-opt wasm and bind it where the Vite dev server expects it.
#
# WebGL2, not WebGPU, because headless Chrome on SwiftShader has no WebGPU
# adapter; `?gfx=webgl2` on the URL forces the loader to match.
# dev-opt (opt-level 1, debug info) builds in ~2.5 min cold, ~15 s warm.
# PROFILE=wasm-bench builds at opt-level 3 for the render benchmark.
set -e
PROFILE=${PROFILE:-dev-opt}
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
cargo build --profile "$PROFILE" --no-default-features --features webgl2 --target wasm32-unknown-unknown
mkdir -p src-ui/wasm/webgl2 src-ui/wasm/webgpu
wasm-bindgen --out-dir src-ui/wasm/webgl2 --web "target/wasm32-unknown-unknown/$PROFILE/ironfell.wasm"

# wasm_loader.ts imports both artifact URLs at build time, so Vite needs a
# file at the WebGPU path even though only WebGL2 is fetched, and glue.ts
# takes its TypeScript types from the WebGPU glue. Stub the directory with a
# copy of the WebGL2 artifacts, and keep the stub current on every build so
# the type declarations match the FFI; a real WebGPU build (no marker file)
# is left alone.
if [ ! -e src-ui/wasm/webgpu/ironfell_bg.wasm ] || [ -e src-ui/wasm/webgpu/.stub ]; then
  cp src-ui/wasm/webgl2/ironfell_bg.wasm src-ui/wasm/webgl2/ironfell.js \
     src-ui/wasm/webgl2/ironfell.d.ts src-ui/wasm/webgl2/ironfell_bg.wasm.d.ts src-ui/wasm/webgpu/
  touch src-ui/wasm/webgpu/.stub
fi
ls -la src-ui/wasm/webgl2/ironfell_bg.wasm
