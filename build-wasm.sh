set -e

# --- target selection ----------------------------------------------------------
# Usage: ./build-wasm.sh [webgpu|webgl2] [classic]   (default: webgpu)
#
# `classic` adds Vello Classic as a second selectable renderer (`?bevy=classic`).
# WebGPU only — it flattens paths in compute shaders, which WebGL2 lacks.
#
# One source tree, two artifacts. The backend is an `ironfell` cargo feature; see
# [features] in Cargo.toml. Exactly one may be enabled — `src/lib.rs` enforces it.
TARGET="${1:-webgpu}"
case "$TARGET" in
  webgpu|webgl2) ;;
  *) echo "unknown target '$TARGET' (expected webgpu or webgl2)" >&2; exit 1 ;;
esac

FEATURES="$TARGET"
if [ "${2:-}" = "classic" ]; then
  if [ "$TARGET" != "webgpu" ]; then
    echo "classic requires the webgpu target (compute shaders)" >&2; exit 1
  fi
  FEATURES="$TARGET,classic"
fi

# `-Zlocation-detail=none -Zfmt-debug=none` are applied to the WebGPU build only.
# They break the WebGL2 build: with them the app acquires an adapter, initializes
# and then fails every frame with wgpu errors that bevy's default
# `RenderErrorPolicy` treats as fatal (`error_handler.rs:79`), so nothing renders.
#
# Established by a controlled A/B — same commit, same machine, same profile, same
# `wasm-opt -Oz`, built back to back with only these two flags differing. The
# mechanism was not identified. `-Zfmt-debug=none` empties every derived `Debug`
# across all crates (std, wgpu and naga included, via `build-std`), so anything
# using `{:?}` to build a functional string silently gets "". naga's GLSL
# identifier generation was the obvious suspect and does not use `Debug`.
#
# The WebGPU build is unaffected because it never executes wgpu's GL backend.
# Cost of omitting them for WebGL2 is ~326 KB (~1.8%).
if [ "$TARGET" = "webgpu" ]; then
  TARGET_RUSTFLAGS="-Zlocation-detail=none -Zfmt-debug=none"
else
  TARGET_RUSTFLAGS=""
fi

# Per-target output. Both artifacts coexist so the loader can choose one at
# runtime; the wasm-bindgen glue is target-specific too (the WebGL2 glue carries
# a shim for every WebGL2 call the GL backend makes, and is ~60 KB larger), so
# the whole directory is per-target rather than just the .wasm.
OUT_DIR="src-ui/wasm/$TARGET"
mkdir -p "$OUT_DIR" opt
echo "building features: $FEATURES -> $OUT_DIR"
# -------------------------------------------------------------------------------

# --- wasm-bindgen version sync -------------------------------------------------
# Cargo.lock is the single source of truth for the wasm-bindgen version. The CLI
# used to generate bindings MUST exactly match the crate, or wasm-bindgen aborts
# with a "schema version" mismatch. Read the locked version and install the
# matching CLI locally if it differs. The CI workflow derives the same version
# from Cargo.lock, so a single `cargo update` + committed lockfile keeps this
# machine and the runner in lockstep — no manual version bumps in two places.
WBG_VERSION=$(awk -F'"' '/^name = "wasm-bindgen"$/{getline; print $2}' Cargo.lock)
CURRENT_WBG=$(wasm-bindgen --version 2>/dev/null | awk '{print $2}')
if [ "$CURRENT_WBG" != "$WBG_VERSION" ]; then
  echo "wasm-bindgen CLI ($CURRENT_WBG) != Cargo.lock ($WBG_VERSION); installing matching CLI…"
  cargo install -f wasm-bindgen-cli --version "$WBG_VERSION"
fi
# -------------------------------------------------------------------------------


# `+simd128` is load-bearing for the sparse-strips work: `fearless_simd` (used by
# vello_common) silently selects a scalar fallback without it, so any hybrid CPU
# measurement taken without this flag is not measuring the renderer that ships.
# It also has to be here rather than in .cargo/config.toml, because this script
# replaces RUSTFLAGS wholesale and cargo does not merge the two.
# Baseline SIMD has been available in every current browser for years; verify with
# `rustc --print cfg --target wasm32-unknown-unknown -Ctarget-feature=+simd128`.
RUSTFLAGS="$TARGET_RUSTFLAGS -Ctarget-feature=+simd128" cargo build \
  -Z build-std=core,alloc,panic_abort,std \
  -Z build-std-features=optimize_for_size \
  --no-default-features --features "$FEATURES" --profile wasm-release \
  --target wasm32-unknown-unknown

# Generate bindings
for i in target/wasm32-unknown-unknown/wasm-release/*.wasm;
do
    wasm-bindgen --out-dir opt/ --web "$i";
done

echo "starting optimize"
# Optimize wasm package size
wasm-opt --enable-bulk-memory --enable-nontrapping-float-to-int --enable-simd -Oz --output "$OUT_DIR/ironfell_bg.wasm" opt/ironfell_bg.wasm

# print "starting copy"
echo "starting copy"

cp opt/ironfell.js "$OUT_DIR/ironfell.js"
cp opt/ironfell.d.ts "$OUT_DIR/ironfell.d.ts"
cp opt/ironfell_bg.wasm.d.ts "$OUT_DIR/ironfell_bg.d.ts"


# to run this for github pages build
# do a regular git commit
# git tag vX.X.X
# git push origin vX.X.X
# git push origin master