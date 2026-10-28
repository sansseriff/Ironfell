//! Native CPU scheduling and opt-in traces. No GPU completion/scanout claims.
use crate::frame_budget::FrameBudget;
use bevy::{
    prelude::*,
    render::{
        Extract, ExtractSchedule, Render, RenderApp, RenderSystems, view::window::prepare_windows,
    },
};
use objc2_quartz_core::CACurrentMediaTime;
use serde::Serialize;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

#[derive(Clone, Copy, Default)]
pub struct DisplayTiming {
    pub callback: f64,
    pub timestamp: f64,
    pub target: f64,
    pub planned_start: f64,
}

/// Copied during extraction: IDs identify host updates even across idle frames.
#[derive(Clone, Copy, Default, Resource)]
struct FrameStamp {
    frame: u64,
    start: f64,
    timing: DisplayTiming,
    input_received: Option<f64>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scheduling {
    /// Start work at the display callback.
    Display,
    /// Start as late as the CPU budget allows before the callback's target.
    Late,
    /// `late-fifo`: present `margin` before a CVDisplayLink vsync, so the frame
    /// makes the compositor's pickup for it (≈3–4 ms before vsync in the probe).
    LateFifo,
    /// `adaptive`: `late-fifo` while the user is dragging (left button held or a
    /// document gesture active), `Display` otherwise. Display pacing keeps stock
    /// Bevy's queue of finished frames, which absorbs spikes (smooth panning,
    /// flying, animation); dragging gets the low latency. Entering a drag skips
    /// presents until the queued frames have reached the display: at mousedown
    /// nothing has moved yet, so the skipped refreshes don't show.
    Adaptive,
}

/// `adaptive` state shared with the display callback.
#[derive(Default)]
struct Engagement {
    engaged: std::sync::atomic::AtomicBool,
    /// Refreshes still allowed to be skipped while draining the queue on entry.
    drain_left: std::sync::atomic::AtomicU32,
}
/// At most this many refreshes are skipped to drain the queue when a drag starts.
const ENGAGE_DRAIN_MAX: u32 = 3;

#[derive(Clone, Resource)]
pub struct Pacer {
    budget: Arc<Mutex<FrameBudget>>,
    trace: Option<Trace>,
    scheduling: Scheduling,
    margin: f64,
    /// `late-fifo`: advance Bevy `Time` to each frame's planned vsync, not to when
    /// its update happened to start (`IRON_NATIVE_FRAME_CLOCK=wall` opts out).
    vsync_clock: bool,
    last_instant: Arc<Mutex<Option<std::time::Instant>>>,
    /// `late-fifo`: budget to GPU completion (`IRON_NATIVE_GPU_BUDGET=0` budgets
    /// to the CPU present instead).
    gpu_budget: bool,
    engagement: Arc<Engagement>,
}

/// `late-fifo` margin adaptation, seconds: a frame whose GPU work finished before
/// `vsync − margin` but still missed the compositor's pickup widens the margin;
/// on-time frames narrow it slowly.
const MARGIN_UP: f64 = 0.001;
const MARGIN_DOWN: f64 = 0.00001;
const MARGIN_MIN: f64 = 0.002;
const MARGIN_MAX: f64 = 0.008;
/// Most of the reserve a late display callback may use before a refresh is skipped.
const LATE_START_TOLERANCE: f64 = 0.004;

/// Refreshes `late-fifo` gave up because its callback came too late (for traces).
pub static PLANNED_SKIPS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
#[derive(Clone)]
struct Trace(Arc<Mutex<Data>>);
struct Data {
    path: String,
    epoch: f64,
    seconds: Option<f64>,
    rows: VecDeque<Row>,
}
#[derive(Serialize)]
struct Row {
    name: &'static str,
    start: f64,
    end: f64,
    frame: u64,
    callback: f64,
    planned_start: f64,
    target: f64,
    timestamp: f64,
    input_received: Option<f64>,
    input_age_ms: Option<f64>,
    rendered: bool,
}
#[derive(Resource, Default)]
struct Stages {
    acquire: f64,
    acquire_wait: f64,
    render: f64,
}
impl Pacer {
    pub fn mode(&self) -> &'static str {
        match self.scheduling {
            Scheduling::Display => "display",
            Scheduling::Late => "late",
            Scheduling::LateFifo => "late-fifo",
            Scheduling::Adaptive => "adaptive",
        }
    }
    fn is_late(&self) -> bool {
        self.scheduling != Scheduling::Display
    }
    /// Seconds to wait before starting this frame, or `None` to skip this refresh.
    /// `late-fifo` also replaces `timing.target` with the vsync it aims at.
    pub fn delay(&self, timing: &mut DisplayTiming) -> Option<f64> {
        let period = timing.target - timing.timestamp;
        Some(match self.scheduling {
            Scheduling::Display => 0.0,
            Scheduling::Late => {
                self.budget
                    .lock()
                    .unwrap()
                    .delay(CACurrentMediaTime(), timing.target, period)
            }
            Scheduling::LateFifo => return self.late_fifo_delay(timing, period),
            Scheduling::Adaptive => {
                use std::sync::atomic::Ordering::Relaxed;
                if !self.engagement.engaged.load(Relaxed) {
                    // Display pacing: queue normally; backlog drains don't apply.
                    crate::present_feedback::take_skip();
                    return Some(0.0);
                }
                // Just engaged: let the frames queued under display pacing reach
                // the display before the first just-in-time frame.
                // Main thread only: a plain load/store is enough.
                let left = self.engagement.drain_left.load(Relaxed);
                if crate::present_feedback::in_flight() > 0 && left > 0 {
                    self.engagement.drain_left.store(left - 1, Relaxed);
                    return None;
                }
                self.engagement.drain_left.store(0, Relaxed);
                return self.late_fifo_delay(timing, period);
            }
        })
    }
    fn late_fifo_delay(&self, timing: &mut DisplayTiming, period: f64) -> Option<f64> {
        // A FIFO backlog never drains by itself at one present per refresh.
        if crate::present_feedback::take_skip() {
            return None;
        }
        let now = CACurrentMediaTime();
        // The budget's margin is the present-before-vsync margin here.
        let Some(reserve) = self.budget.lock().unwrap().reserve(now, period) else {
            return Some(0.0);
        };
        // Work + margin longer than a refresh can't start "late" without
        // crossing the next display callback, which cancels a pending start:
        // degrade to starting at the callback, i.e. ordinary display pacing.
        if reserve > period * 0.9 {
            return Some(0.0);
        }
        let Some((next, period)) = crate::vsync_clock::next_after(now) else {
            return Some(0.0);
        };
        // The display callback can arrive just after the ideal start for
        // the next vsync. Skipping to the one after repeats a frame; start
        // now instead when the shortfall fits in part of the reserve,
        // which is p95 work plus margin, not a hard minimum.
        let shortfall = now + reserve - next;
        let vsync = if shortfall <= (reserve * 0.5).min(LATE_START_TOLERANCE) {
            next
        } else {
            PLANNED_SKIPS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            next + period
        };
        timing.target = vsync;
        // Less than a period: the next display callback cancels a pending start.
        Some((vsync - reserve - now).clamp(0.0, period * 0.95))
    }
    /// Whether this mode plans frames against vsync and needs display feedback.
    pub fn is_late_fifo(&self) -> bool {
        matches!(self.scheduling, Scheduling::LateFifo | Scheduling::Adaptive)
    }
    /// `adaptive`: whether the user is dragging. Call after each frame.
    pub fn set_engaged(&self, on: bool) {
        use std::sync::atomic::Ordering::Relaxed;
        if self.scheduling != Scheduling::Adaptive {
            return;
        }
        let was = self.engagement.engaged.swap(on, Relaxed);
        if on && !was {
            self.engagement.drain_left.store(ENGAGE_DRAIN_MAX, Relaxed);
        }
    }
    pub fn engaged(&self) -> bool {
        match self.scheduling {
            Scheduling::Adaptive => self.engagement.engaged.load(std::sync::atomic::Ordering::Relaxed),
            Scheduling::LateFifo => true,
            _ => false,
        }
    }
    /// `late-fifo`: budget from tick start to GPU completion (CPU present when the
    /// GPU hook is unavailable) and adapt the margin from display feedback.
    pub fn observe_frames(&self, results: &[crate::present_feedback::FrameResult]) {
        if !self.is_late_fifo() {
            return;
        }
        let mut budget = self.budget.lock().unwrap();
        for r in results {
            let done = if self.gpu_budget { r.gpu_done.unwrap_or(r.present) } else { r.present };
            budget.observe(r.start, done);
            if r.planned <= 0.0 {
                continue;
            }
            let margin = budget.margin();
            if r.missed {
                if done <= r.planned - margin + 0.0005 {
                    budget.set_margin((margin + MARGIN_UP).min(MARGIN_MAX));
                }
            } else {
                budget.set_margin((margin - MARGIN_DOWN).max(MARGIN_MIN));
            }
        }
    }
    pub fn margin_ms(&self) -> f64 {
        self.budget.lock().unwrap().margin() * 1000.0
    }
    /// Time-based motion should move by the interval between the frames' display
    /// times. Update starts wander by the budget and timer lateness while the
    /// display interval is a fixed refresh, so wall-clock steps judder.
    pub fn set_frame_clock(&self, app: &mut ironfell::WorkerApp, timing: DisplayTiming) {
        if !self.vsync_clock {
            return;
        }
        let now = std::time::Instant::now();
        let planned = self.planned_vsync(timing);
        let mut instant = if planned > 0.0 {
            now + std::time::Duration::from_secs_f64((planned - CACurrentMediaTime()).max(0.0))
        } else {
            now
        };
        // Monotonic across planned (future) and unplanned (now) frames.
        let mut last = self.last_instant.lock().unwrap();
        if let Some(previous) = *last {
            instant = instant.max(previous);
        }
        *last = Some(instant);
        app.world_mut()
            .insert_resource(bevy::time::TimeUpdateStrategy::ManualInstant(instant));
    }
    /// The vsync this frame was planned for, when `late-fifo` planned one.
    pub fn planned_vsync(&self, timing: DisplayTiming) -> f64 {
        if self.is_late_fifo() && timing.planned_start > timing.callback {
            timing.target
        } else {
            0.0
        }
    }
    pub fn begin(
        &self,
        app: &mut ironfell::WorkerApp,
        frame: u64,
        timing: DisplayTiming,
        start: f64,
        input_received: Option<f64>,
    ) {
        if self.is_late() {
            let overshoot = if timing.planned_start > timing.callback {
                (start - timing.planned_start).max(0.0)
            } else {
                0.0
            };
            self.budget.lock().unwrap().observe_timer(overshoot);
        }
        if self.is_late() || self.trace.is_some() {
            *app.world_mut().resource_mut::<FrameStamp>() = FrameStamp {
                frame,
                start,
                timing,
                input_received,
            };
        }
    }
    pub fn tick(
        &self,
        timing: DisplayTiming,
        frame: u64,
        start: f64,
        input_received: Option<f64>,
        input_age_ms: Option<f64>,
        rendered: bool,
    ) {
        if let Some(trace) = &self.trace {
            trace.record(Row {
                name: "tick",
                start,
                end: CACurrentMediaTime(),
                frame,
                callback: timing.callback,
                planned_start: timing.planned_start,
                target: timing.target,
                timestamp: timing.timestamp,
                input_received,
                input_age_ms,
                rendered,
            });
        }
    }
    fn stage(&self, name: &'static str, stamp: FrameStamp, start: f64, end: f64) {
        if let Some(trace) = &self.trace {
            trace.record(Row {
                name,
                start,
                end,
                frame: stamp.frame,
                callback: stamp.timing.callback,
                planned_start: stamp.timing.planned_start,
                target: stamp.timing.target,
                timestamp: stamp.timing.timestamp,
                input_received: stamp.input_received,
                input_age_ms: None,
                rendered: true,
            });
        }
    }
    pub fn should_stop(&self) -> bool {
        self.trace.as_ref().is_some_and(|trace| {
            let d = trace.0.lock().unwrap();
            d.seconds
                .is_some_and(|s| CACurrentMediaTime() - d.epoch >= s)
        })
    }
    pub fn save(&self) {
        let Some(trace) = &self.trace else {
            return;
        };
        let d = trace.0.lock().unwrap();
        let value = serde_json::json!({"schema":2,"epoch":d.epoch,"rows":d.rows,
            "requestedSeconds":d.seconds,
            "pacing":self.mode(),"marginMs":self.margin * 1000.0,
            "renderMode":std::env::var("IRON_NATIVE_RENDER_MODE").unwrap_or("pipelined".into()),
            "presentMode":std::env::var("IRON_NATIVE_PRESENT_MODE").unwrap_or("fifo".into()),
            "frameLatency":std::env::var("IRON_NATIVE_FRAME_LATENCY").unwrap_or("2".into())});
        match std::fs::write(&d.path, serde_json::to_vec(&value).unwrap()) {
            Ok(()) => println!("IRON_NATIVE_PACING {} {} rows", d.path, d.rows.len()),
            Err(e) => eprintln!("could not save pacing trace: {e}"),
        }
    }
}
impl Trace {
    fn record(&self, row: Row) {
        let mut data = self.0.lock().unwrap();
        if data.rows.len() == 24000 {
            data.rows.pop_front();
        }
        data.rows.push_back(row);
    }
}
fn extract_stamp(mut stamp: ResMut<FrameStamp>, source: Extract<Res<FrameStamp>>) {
    *stamp = **source;
}
fn acquire_start(mut stages: ResMut<Stages>) {
    stages.acquire = CACurrentMediaTime();
}
fn acquire_end(mut stages: ResMut<Stages>, stamp: Res<FrameStamp>, pacer: Res<Pacer>) {
    let end = CACurrentMediaTime();
    stages.acquire_wait = end - stages.acquire;
    pacer.stage("acquire", *stamp, stages.acquire, end);
}
fn render_start(mut stages: ResMut<Stages>) {
    stages.render = CACurrentMediaTime();
}
fn render_end(stages: Res<Stages>, stamp: Res<FrameStamp>, pacer: Res<Pacer>, queue: Res<bevy::render::renderer::RenderQueue>) {
    let end = CACurrentMediaTime();
    // Queue backpressure is time we want to spend receiving input instead of
    // blocking in nextDrawable. It is not CPU work to reserve before the target.
    // Keep it in traces, but exclude it from the CPU budget prediction.
    // late-fifo budgets to GPU completion from display feedback instead.
    if pacer.is_late() && !pacer.is_late_fifo() {
        pacer
            .budget
            .lock()
            .unwrap()
            .observe(stamp.start + stages.acquire_wait, end);
    }
    pacer.stage("render_submit", *stamp, stages.render, end);
    if pacer.trace.is_some() && std::env::var_os("IRON_NATIVE_GPU_DONE_TRACE").is_some() {
        let pacer=pacer.clone(); let stamp=*stamp;
        // Callback receipt is an upper bound on completion of prior queue work,
        // including polling delay. It is neither a GPU timestamp nor scanout.
        queue.on_submitted_work_done(move || {
            pacer.stage("gpu_done_callback", stamp, end, CACurrentMediaTime());
        });
    }
}

pub fn install(app: &mut ironfell::WorkerApp) -> Pacer {
    let scheduling = match std::env::var("IRON_NATIVE_PACING").as_deref() {
        Ok("late") => Scheduling::Late,
        Ok("late-fifo") => Scheduling::LateFifo,
        Ok("adaptive") => Scheduling::Adaptive,
        Ok("display") | Err(_) => Scheduling::Display,
        Ok(mode) => panic!("unknown IRON_NATIVE_PACING: {mode}"),
    };
    let vsync_paced = matches!(scheduling, Scheduling::LateFifo | Scheduling::Adaptive);
    if vsync_paced {
        crate::vsync_clock::start();
    }
    let margin = std::env::var("IRON_NATIVE_PACING_MARGIN_MS")
        .ok()
        .map(|s| {
            s.parse::<f64>()
                .expect("IRON_NATIVE_PACING_MARGIN_MS must be a number")
        })
        // late-fifo: initial GPU-finish-before-vsync margin; it then adapts. The
        // probe's CPU present at 3 ms before vsync missed, at 4 ms didn't.
        .unwrap_or(if vsync_paced { 4.0 } else { 2.0 });
    assert!(
        margin.is_finite() && (0.25..=8.0).contains(&margin),
        "IRON_NATIVE_PACING_MARGIN_MS must be between 0.25 and 8"
    );
    let trace = std::env::var("IRON_NATIVE_PACING_TRACE").ok().map(|path| {
        let seconds = std::env::var("IRON_NATIVE_PACING_SECONDS").ok().map(|s| {
            let seconds = s
                .parse::<f64>()
                .expect("IRON_NATIVE_PACING_SECONDS must be a number");
            assert!(seconds.is_finite() && seconds > 0.0);
            seconds
        });
        Trace(Arc::new(Mutex::new(Data {
            path,
            epoch: CACurrentMediaTime(),
            seconds,
            rows: VecDeque::new(),
        })))
    });
    let pacer = Pacer {
        budget: Arc::new(Mutex::new(FrameBudget::new(margin / 1000.0))),
        trace,
        scheduling,
        margin: margin / 1000.0,
        vsync_clock: vsync_paced
            && match std::env::var("IRON_NATIVE_FRAME_CLOCK").as_deref() {
                Ok("vsync") | Err(_) => true,
                Ok("wall") => false,
                Ok(other) => panic!("unknown IRON_NATIVE_FRAME_CLOCK: {other}"),
            },
        last_instant: Arc::default(),
        gpu_budget: std::env::var("IRON_NATIVE_GPU_BUDGET").as_deref() != Ok("0"),
        engagement: Arc::default(),
    };
    println!(
        "IRON_NATIVE_SCHEDULING {}",
        serde_json::json!({"pacing":pacer.mode(),"frameClock":if pacer.vsync_clock {"vsync"} else {"wall"},"gpuBudget":pacer.gpu_budget,
        "marginMs":margin,"renderMode":std::env::var("IRON_NATIVE_RENDER_MODE").unwrap_or("pipelined".into())})
    );
    if pacer.is_late() || pacer.trace.is_some() {
        app.init_resource::<FrameStamp>();
        let render = app
            .get_sub_app_mut(RenderApp)
            .expect("install before finalization");
        render.insert_resource(pacer.clone());
        render.init_resource::<FrameStamp>();
        render.init_resource::<Stages>();
        render.add_systems(ExtractSchedule, extract_stamp);
        render.add_systems(
            Render,
            (
                acquire_start
                    .before(prepare_windows)
                    .in_set(RenderSystems::PrepareViews),
                acquire_end
                    .after(prepare_windows)
                    .in_set(RenderSystems::PrepareViews),
                render_start
                    .before(RenderSystems::Render)
                    .after(RenderSystems::Prepare),
                render_end
                    .after(RenderSystems::Render)
                    .before(RenderSystems::Cleanup),
            ),
        );
    }
    pacer
}
