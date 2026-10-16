//! Clock acquisition is a host service, never a document/solver dependency.
pub fn unix_millis() -> u64 {
    #[cfg(target_arch = "wasm32")]
    { js_sys::Date::now() as u64 }
    #[cfg(not(target_arch = "wasm32"))]
    { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64 }
}

pub fn graphics_backend() -> &'static str {
    if cfg!(feature="native") { "native" } else if cfg!(feature="webgpu") { "webgpu" } else { "webgl2" }
}

/// Session token acquisition stays at the host boundary.
pub fn origin_token() -> String {
    #[cfg(target_arch = "wasm32")]
    let r = js_sys::Math::random();
    #[cfg(not(target_arch = "wasm32"))]
    let r = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.subsec_nanos() as f64 / 1e9);
    format!("{:016x}", (r * u64::MAX as f64) as u64)
}

pub fn cpu_simd_available() -> bool {
    cfg!(any(target_feature="simd128", target_feature="neon", target_feature="sse2"))
}
