//! Records the build's optimization level, so the app can say on screen
//! whether it is the slow development build or an optimized one
//! (`fps_overlay`). Cargo sets `OPT_LEVEL` from the profile.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let opt = std::env::var("OPT_LEVEL").unwrap_or_else(|_| "?".into());
    println!("cargo:rustc-env=IRON_OPT_LEVEL={opt}");
}
