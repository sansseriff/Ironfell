//! The build's profile, for the shell's Settings view and the FPS readout
//! (`engine_api::document_build_info`, `document_frame_stats`). The Stats
//! composition that used to show it, with the frame rate, is gone: each
//! composition view has its own FPS readout (`view_rates`).

/// The build's profile name, optimization level, wasm SIMD, and whether it
/// is fast enough to judge performance by: optimized with SIMD, as what
/// ships and what benchmarks measure. `.claude/skills/run-iron/build.sh`
/// builds `dev-opt` (opt 1) by default and `wasm-bench` (opt 3, SIMD) with
/// `PROFILE=wasm-bench`. Also reported to the shell's Settings view
/// (`web_ffi::document_build_info`).
pub(crate) fn build_profile() -> (&'static str, &'static str, bool, bool) {
    let opt = env!("IRON_OPT_LEVEL");
    let simd = crate::host_services::cpu_simd_available();
    let fast = matches!(opt, "2" | "3" | "s" | "z") && simd;
    let name = match opt {
        "1" => "dev-opt",
        "3" => "wasm-bench",
        "z" => "release",
        _ => "custom",
    };
    (name, opt, simd, fast)
}
