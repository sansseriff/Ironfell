#!/bin/zsh
# Build the WebGL2 dev-opt wasm and bind it where the Vite dev server expects it.
#
# WebGL2, not WebGPU, because headless Chrome on SwiftShader has no WebGPU
# adapter; `?gfx=webgl2` on the URL forces the loader to match.
# dev-opt (opt-level 1, debug info) builds in ~2.5 min cold, ~15 s warm.
set -e
cd "$(dirname "$0")/../../.."

# The wasm-bindgen CLI must match the crate version pinned in Cargo.lock.
WBG_VERSION=$(awk -F'"' '/^name = "wasm-bindgen"$/{getline; print $2}' Cargo.lock)
CURRENT_WBG=$(wasm-bindgen --version 2>/dev/null | awk '{print $2}')
if [ "$CURRENT_WBG" != "$WBG_VERSION" ]; then
  echo "wasm-bindgen CLI ($CURRENT_WBG) != Cargo.lock ($WBG_VERSION); installing matching CLI…"
  cargo install -f wasm-bindgen-cli --version "$WBG_VERSION"
fi

cargo build --profile dev-opt --no-default-features --features webgl2 --target wasm32-unknown-unknown
mkdir -p src-ui/wasm/webgl2 src-ui/wasm/webgpu
wasm-bindgen --out-dir src-ui/wasm/webgl2 --web target/wasm32-unknown-unknown/dev-opt/ironfell.wasm

# wasm_loader.ts imports both artifact URLs at build time, so Vite needs a
# file at the WebGPU path even though only WebGL2 is fetched. Stub it.
[ -e src-ui/wasm/webgpu/ironfell_bg.wasm ] || cp src-ui/wasm/webgl2/ironfell_bg.wasm src-ui/wasm/webgpu/ironfell_bg.wasm
[ -e src-ui/wasm/webgpu/ironfell.js ] || cp src-ui/wasm/webgl2/ironfell.js src-ui/wasm/webgpu/ironfell.js
ls -la src-ui/wasm/webgl2/ironfell_bg.wasm
