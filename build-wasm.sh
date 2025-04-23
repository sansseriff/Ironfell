set -e 



cargo build --no-default-features --profile wasm-release \
--target wasm32-unknown-unknown 

# Generate bindings
for i in target/wasm32-unknown-unknown/wasm-release/*.wasm;
do
    wasm-bindgen --out-dir opt/ --web "$i";
done

echo "starting optimize"
# Optimize wasm package size
wasm-opt -Oz --output src-ui/wasm/ironfell_bg.wasm opt/ironfell_bg.wasm 

# print "starting copy"
echo "starting copy"

cp opt/ironfell.js src-ui/wasm/ironfell.js
cp opt/ironfell.d.ts src-ui/wasm/ironfell.d.ts
cp opt/ironfell_bg.wasm.d.ts src-ui/wasm/ironfell_bg.d.ts
# cp opt/ironfell_bg.wasm src-ui/wasm/ironfell_bg.wasm