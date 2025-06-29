set -e 



# RUSTFLAGS="-Zlocation-detail=none" cargo +nightly build --no-default-features --profile wasm-release \
# --target wasm32-unknown-unknown 



RUSTFLAGS="-Zlocation-detail=none -Zfmt-debug=none" cargo +nightly build \
    -Z build-std=std,panic_abort \
    -Z build-std-features="optimize_for_size" \
    -Z build-std-features=panic_immediate_abort \
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
# git tag vX.X.X
# git push origin vX.X.X