set -e 

# There's an issue with the first few frames needing longer frame intervals when running in worker with debug mode
# https://github.com/bevyengine/bevy/issues/13345
cargo build --no-default-features --profile dev-opt \
--target wasm32-unknown-unknown

# Generate bindings
for i in target/wasm32-unknown-unknown/dev-opt/*.wasm;
do
    wasm-bindgen --out-dir src-ui/wasm --web "$i";
done

# print "starting copy"
echo "starting copy"

bun run dev

# cp wasm/ironfell.js src-ui/ironfell.js
# cp wasm/ironfell_bg.wasm.d.ts src-ui/ironfell_bg.d.ts
# cp wasm/ironfell_
# cp wasm/ironfell_bg.wasm src-ui/ironfell_bg.wasm