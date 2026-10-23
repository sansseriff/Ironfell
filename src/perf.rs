//! Per-phase CPU timings for the render benchmark (roadmap stage H: measure
//! before culling or caching).
//!
//! A phase is timed with a guard, `let _t = perf::span(Phase::Encode);`, and
//! each completed span is one sample. Samples go to one process-wide table so
//! main-world and render-world systems report to the same place (on wasm both
//! run on the worker thread). `document_timings` reads the table as JSON, and
//! `__iron.timings(reset)` from the shell.
//!
//! The cost is two clock reads and a push per span, a handful of spans per
//! frame, so it is always on.

use bevy::platform::time::Instant;
use std::sync::Mutex;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    /// `sync_document`: apply queued transactions and reconcile entities.
    Sync,
    /// `sync_document` after the tools, with only previews to drain: rewrite
    /// entities a drag moved, in-frame.
    GestureReconcile,
    /// `render_document_layer`: walk every 2D view's nodes into display lists.
    DisplayList,
    /// `extract_layers`: copy changed display lists into the render world.
    Extract,
    /// `prepare_scene`: encode changed views' display lists into Vello scenes
    /// (flatten, tile and strip on the CPU).
    Encode,
    /// `prepare_scene` for editor-feedback views ([`crate::vector::Overlay`]):
    /// hover, selection and grips, encoded apart from content.
    OverlayEncode,
    /// `render_scene`: submit each changed view's scene to the GPU.
    Render,
    /// `pick_document_2d_system`: hit-test the pointer against a 2D view,
    /// every frame the pointer is over one.
    Pick,
    /// Rebuild a cached selection/hover box and grip eligibility. Idle frames
    /// reuse it; excludes drawing the small overlay command list.
    OverlayGeometry,
    /// Independent resolved bounds/index update, including hidden compositions.
    Bounds,
    /// Bevy's transform propagation (`TransformSystems::Propagate`), timed
    /// from outside the set; every frame, idle ones included.
    Propagate,
    /// A Bevy-side producer's step and publish (`document_bridge::gas`),
    /// only while it runs.
    Produce,
    /// One whole frame on the worker (`WorkerApp::frame`), main world and,
    /// when it renders, extraction and rendering. CPU only: the GPU's share
    /// shows in frames per second.
    Frame,
    /// Whole frames in which some view, content or overlay, re-encoded: the
    /// cost of a change.
    ChangedFrame,
}

impl Phase {
    const ALL: [Phase; 14] = [
        Phase::Sync,
        Phase::GestureReconcile,
        Phase::DisplayList,
        Phase::Extract,
        Phase::Encode,
        Phase::OverlayEncode,
        Phase::Render,
        Phase::Pick,
        Phase::OverlayGeometry,
        Phase::Bounds,
        Phase::Propagate,
        Phase::Produce,
        Phase::Frame,
        Phase::ChangedFrame,
    ];

    fn name(self) -> &'static str {
        match self {
            Phase::Sync => "sync",
            Phase::GestureReconcile => "gesture_reconcile",
            Phase::DisplayList => "display_list",
            Phase::Extract => "extract",
            Phase::Encode => "encode",
            Phase::OverlayEncode => "overlay_encode",
            Phase::Render => "render",
            Phase::Pick => "pick",
            Phase::OverlayGeometry => "overlay_geometry",
            Phase::Bounds => "bounds",
            Phase::Propagate => "propagate",
            Phase::Produce => "produce",
            Phase::Frame => "frame",
            Phase::ChangedFrame => "changed_frame",
        }
    }
}

/// Samples kept per phase: two minutes at 60 fps, enough for any scenario.
const CAP: usize = 7200;

struct Table {
    samples: [Vec<f32>; Phase::ALL.len()],
    bounds_updated: u32,
    pick_candidates: u32,
    /// Whether a view encoded during the current frame.
    encoded: bool,
    /// The largest encode since the last reset: commands emitted, and the
    /// layers that encode culled.
    commands: u32,
    culled: u32,
    /// Nodes drawn by the last display-list walk.
    drawn: u32,
}

static TABLE: Mutex<Table> = Mutex::new(Table { samples: [const { Vec::new() }; Phase::ALL.len()], encoded: false, commands: 0, culled: 0, drawn: 0, bounds_updated: 0, pick_candidates: 0 });

pub fn set_bounds_updates(n: u32) { if let Ok(mut t) = TABLE.lock() { t.bounds_updated = t.bounds_updated.max(n); } }
pub fn set_pick_candidates(n: u32) { if let Ok(mut t) = TABLE.lock() { t.pick_candidates = t.pick_candidates.max(n); } }

pub struct Span {
    phase: Phase,
    start: Instant,
}

pub fn span(phase: Phase) -> Span {
    Span { phase, start: Instant::now() }
}

impl Span {
    /// Drop without recording: the phase turned out to have nothing to do,
    /// and an idle frame's near-zero sample would hide the real cost.
    pub fn discard(self) {
        std::mem::forget(self);
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        let ms = self.start.elapsed().as_secs_f32() * 1000.0;
        if let Ok(mut t) = TABLE.lock() {
            push(&mut t, self.phase, ms);
        }
    }
}

/// Record one sample measured by hand, for a phase whose work is split
/// between callers a guard cannot separate. An encode of either kind makes
/// the frame a changed frame.
pub fn record(phase: Phase, ms: f32) {
    if let Ok(mut t) = TABLE.lock() {
        push(&mut t, phase, ms);
        if matches!(phase, Phase::Encode | Phase::OverlayEncode) {
            t.encoded = true;
        }
    }
}

fn push(t: &mut Table, phase: Phase, ms: f32) {
    let v = &mut t.samples[phase as usize];
    if v.len() < CAP {
        v.push(ms);
    }
}

/// Close a frame begun at `start`: record it as a frame, and as a changed
/// frame if a view encoded during it.
pub fn end_frame(start: Instant) {
    let ms = start.elapsed().as_secs_f32() * 1000.0;
    if let Ok(mut t) = TABLE.lock() {
        push(&mut t, Phase::Frame, ms);
        if std::mem::take(&mut t.encoded) {
            push(&mut t, Phase::ChangedFrame, ms);
        }
    }
}

pub fn set_encode_counts(commands: u32, culled: u32) {
    if let Ok(mut t) = TABLE.lock() {
        t.encoded = true;
        if commands >= t.commands {
            t.commands = commands;
            t.culled = culled;
        }
    }
}

pub fn set_drawn(drawn: u32) {
    if let Ok(mut t) = TABLE.lock() {
        t.drawn = drawn;
    }
}

/// Every phase's sample count, total, p50, p95 and max in milliseconds, plus
/// the last encode's command and cull counts. `reset` clears the samples
/// after reading, so a scenario reads only its own frames.
pub fn report(reset: bool) -> serde_json::Value {
    let Ok(mut t) = TABLE.lock() else { return serde_json::json!({"error": "timings unavailable"}) };
    let mut phases = serde_json::Map::new();
    for p in Phase::ALL {
        let v = &mut t.samples[p as usize];
        let mut sorted = v.clone();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let pct = |q: f32| sorted.get(((sorted.len() as f32 * q) as usize).min(sorted.len().saturating_sub(1))).copied().unwrap_or(0.0);
        let round = |x: f32| (x as f64 * 100.0).round() / 100.0;
        phases.insert(
            p.name().into(),
            serde_json::json!({
                "n": sorted.len(),
                "total": round(sorted.iter().sum()),
                "p50": round(pct(0.5)),
                "p95": round(pct(0.95)),
                "max": round(sorted.last().copied().unwrap_or(0.0)),
            }),
        );
        if reset {
            v.clear();
        }
    }
    let out = serde_json::json!({ "phases": phases, "commands": t.commands, "culled": t.culled, "drawn": t.drawn, "bounds_updated": t.bounds_updated, "pick_candidates": t.pick_candidates });
    if reset {
        t.commands = 0;
        t.culled = 0;
        t.bounds_updated = 0;
        t.pick_candidates = 0;
    }
    out
}
