//! The Stats composition's producer (plans/specs/free-running.md §4): frame
//! rate, renderer, build and canvas size, published as runtime signals onto
//! the text lines of the session's `stats` composition (`standard::
//! stats_ops`). It replaced a `bevy_ui` overlay, so the app links no UI
//! layout or Bevy text.
//!
//! The composition is a live source: a visible Stats view keeps the window
//! free running, which is what makes its frame rate a measurement of
//! rendering. Set to On demand, it updates on input only.

use super::liveness::{LiveSources, Liveness};
use super::DocumentStore;
use crate::vector::BackendKind;
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use iron_document::facet::Name as DocName;
use iron_document::standard::{STATS, STATS_LINES};
use iron_document::{Leaf, NodeId, Value, Version};
use std::collections::BTreeMap;

/// The producer's registered name.
pub const PRODUCER: &str = "stats";
/// Seconds between publishes: the text reshapes only when its string does.
const PERIOD: f64 = 0.25;

#[derive(Default)]
pub(super) struct Found {
    version: Option<Version>,
    composition: Option<NodeId>,
    lines: Vec<NodeId>,
}

/// The session's `stats` composition and its lines, in `STATS_LINES` order.
fn find(store: &iron_document::Store) -> (Option<NodeId>, Vec<NodeId>) {
    let doc = store.document();
    let name_of = |id: NodeId| doc.get::<DocName>(id).and_then(|n| n.text.constant()).and_then(|v| v.as_str()).map(str::to_owned);
    let Some(comp) = iron_document::composition::compositions(doc).into_iter().find(|&c| c.is_session() && name_of(c).as_deref() == Some(STATS)) else {
        return (None, Vec::new());
    };
    let lines = STATS_LINES.iter().filter_map(|want| doc.children(comp).iter().copied().find(|&k| name_of(k).as_deref() == Some(want))).collect();
    (Some(comp), lines)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn step_and_publish(
    mut store: ResMut<DocumentStore>,
    liveness: Res<Liveness>,
    mut sources: ResMut<LiveSources>,
    diagnostics: Res<DiagnosticsStore>,
    backend: Option<Res<BackendKind>>,
    window: Option<Single<&Window, With<bevy::window::PrimaryWindow>>>,
    time: Res<Time<Real>>,
    mut found: Local<Found>,
    mut last: Local<f64>,
) {
    let version = store.0.document().version();
    if found.version != Some(version) {
        if let Some(old) = found.composition {
            sources.0.remove(&old);
        }
        let (composition, lines) = find(&store.0);
        *found = Found { version: Some(version), composition, lines };
    }
    let Some(comp) = found.composition else { return };
    if found.lines.len() != STATS_LINES.len() {
        return;
    }
    sources.0.insert(comp);
    if !liveness.running.contains(&comp) {
        return;
    }
    let now = time.elapsed_secs_f64();
    if now - *last < PERIOD {
        return;
    }
    *last = now;

    let fps = diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS);
    let num = |v: Option<f64>| v.map_or_else(|| "–".to_owned(), |v| format!("{v:.1}"));
    let fps_line = format!("FPS {}  avg {}  ema {}", num(fps.and_then(|d| d.value())), num(fps.and_then(|d| d.average())), num(fps.and_then(|d| d.smoothed())));
    let renderer = match backend.as_deref() {
        Some(BackendKind::Hybrid) => "vello_gpu (sparse strips)",
        None => "none",
    };
    let (name, opt, simd, fast) = build_profile();
    let build = format!("Build {name} (opt {opt}, {}){}", if simd { "SIMD" } else { "no SIMD" }, if fast { "" } else { " - SLOW" });
    let canvas = window.map_or_else(|| "Canvas –".to_owned(), |w| {
        let s = w.physical_size();
        format!("Canvas {}x{} px", s.x, s.y)
    });
    let texts = [fps_line, format!("2D {renderer}"), build, canvas];
    let producer = store.0.producer(PRODUCER).unwrap_or_else(|| store.0.register_producer(PRODUCER));
    let values: BTreeMap<Leaf, Value> = found.lines.iter().zip(texts).map(|(&node, text)| (Leaf { node, path: "text.content".parse().expect("declared leaf") }, Value::Str(text))).collect();
    store.0.publish(producer, values);
}

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
