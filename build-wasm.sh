set -e

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


RUSTFLAGS="-Zlocation-detail=none -Zfmt-debug=none" cargo build \
  -Z build-std=core,alloc,panic_abort,std \
  -Z build-std-features=optimize_for_size,panic_immediate_abort \
  --no-default-features --profile wasm-release \
  --target wasm32-unknown-unknown

# Generate bindings
for i in target/wasm32-unknown-unknown/wasm-release/*.wasm;
do
    wasm-bindgen --out-dir opt/ --web "$i";
done

echo "starting optimize"
# Optimize wasm package size
wasm-opt --enable-bulk-memory --enable-nontrapping-float-to-int -Oz --output src-ui/wasm/ironfell_bg.wasm opt/ironfell_bg.wasm 

# print "starting copy"
echo "starting copy"

cp opt/ironfell.js src-ui/wasm/ironfell.js
cp opt/ironfell.d.ts src-ui/wasm/ironfell.d.ts
cp opt/ironfell_bg.wasm.d.ts src-ui/wasm/ironfell_bg.d.ts


# to run this for github pages build
# do a regular git commit
# git tag vX.X.X
# git push origin vX.X.X
# git push origin master