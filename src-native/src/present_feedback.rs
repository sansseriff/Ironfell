//! When each drawable actually reached the display (`MTLDrawable.presentedTime`).
//! `late-fifo` pacing uses it to drain a FIFO backlog; `IRON_NATIVE_PRESENT_TRACE=1`
//! also reports it every 120 rendered frames as `IRON_NATIVE_PRESENT`.
//!
//! With display sync and one present per refresh, one late frame leaves every later
//! frame queued behind it: the queue never drains by itself, and latency ratchets
//! up a refresh at a time. When present→glass stays above its recent floor, the
//! pacer skips one refresh so the queue drains. wgpu keeps drawables private, so
//! `-[CAMetalLayer nextDrawable]` is wrapped to attach a presented handler.
//! Present times are taken after `app.frame()`, so they are exact only with sync
//! rendering; pipelined rendering presents later on its render thread.
use block2::RcBlock;
use objc2::{
    msg_send,
    runtime::{AnyClass, AnyObject, Imp, Sel},
    sel,
};
use objc2_quartz_core::CACurrentMediaTime;
use std::{
    collections::{BTreeMap, VecDeque},
    ptr::NonNull,
    sync::{
        Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, Ordering::Relaxed},
    },
};

static ORIGINAL: OnceLock<Imp> = OnceLock::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static CURRENT: AtomicU64 = AtomicU64::new(0);
/// (drawable id, presentedTime); 0.0 means the drawable never reached the display.
static FEEDBACK: Mutex<Vec<(u64, f64)>> = Mutex::new(Vec::new());
/// Drawables whose presented handler has fired (shown or dropped).
static GLASSED: AtomicU64 = AtomicU64::new(0);

/// Drawables acquired but not yet on the display: frames still queued.
pub fn in_flight() -> u64 {
    (NEXT_ID.load(Relaxed) - 1).saturating_sub(GLASSED.load(Relaxed))
}

/// Set when a backlog is detected; the pacer takes it to skip one refresh.
static SKIP: AtomicBool = AtomicBool::new(false);

/// Whether the pacer should skip this refresh to drain a backlog.
pub fn take_skip() -> bool {
    SKIP.swap(false, Relaxed)
}

type NextDrawable = unsafe extern "C-unwind" fn(*mut AnyObject, Sel) -> *mut AnyObject;

unsafe extern "C-unwind" fn next_drawable(layer: *mut AnyObject, cmd: Sel) -> *mut AnyObject {
    let original: NextDrawable = unsafe { std::mem::transmute(*ORIGINAL.get().unwrap()) };
    let drawable = unsafe { original(layer, cmd) };
    if let Some(drawable) = unsafe { drawable.as_ref() } {
        let id = NEXT_ID.fetch_add(1, Relaxed);
        CURRENT.store(id, Relaxed);
        let handler = RcBlock::new(move |d: NonNull<AnyObject>| {
            let glass: f64 = unsafe { msg_send![d.as_ref(), presentedTime] };
            FEEDBACK.lock().unwrap().push((id, glass));
            GLASSED.fetch_add(1, Relaxed);
        });
        let _: () = unsafe { msg_send![drawable, addPresentedHandler: &*handler] };
    }
    drawable
}

struct Frame {
    id: u64,
    /// Host tick start: where this frame's CPU work began.
    start: f64,
    /// Command buffers created during this frame: `(first, last]` sequence numbers.
    buffers: (u64, u64),
    present: f64,
    /// The vsync the pacer aimed at; 0.0 when it didn't plan one.
    planned: f64,
    /// Bevy `Time` delta of the update that produced this frame.
    step: f64,
    /// `adaptive`: produced while the user was dragging.
    engaged: bool,
}

/// A frame whose display time is now known, for the pacer's budget and margin.
pub struct FrameResult {
    pub start: f64,
    pub present: f64,
    /// When its GPU work finished, if the command-buffer hook is active.
    pub gpu_done: Option<f64>,
    /// The vsync it was planned for; 0.0 when unplanned.
    pub planned: f64,
    /// Reached the display at least half a refresh later than usual for its plan.
    pub missed: bool,
}

#[derive(Default)]
pub struct PresentFeedback {
    print: bool,
    pending: VecDeque<Frame>,
    last_id: u64,
    last_glass: f64,
    last_glass_id: u64,
    last_seq: u64,
    gpu_done: BTreeMap<u64, f64>,
    /// Recent glass − planned vsync. Its median is the usual offset: the vsync clock's
    /// phase against the display wanders, so a minimum goes stale.
    lateness: VecDeque<f64>,
    margin_ms: f64,
    period: f64,
    /// Recent present→glass, whose minimum is the unqueued floor.
    recent: VecDeque<f64>,
    streak: u32,
    cooldown: u32,
    stats: Stats,
}

#[derive(Default)]
struct Stats {
    frames: u32,
    to_glass: Vec<f64>,
    after_planned: Vec<f64>,
    /// On-glass gaps between consecutive drawables, in whole refreshes.
    gaps: [u32; 4],
    dropped: u32,
    skips: u32,
    /// Consecutive frames whose `Time` step differs from their on-glass spacing by
    /// over a quarter refresh: time-based motion visibly speeds up or slows down.
    step_mismatch: u32,
    step_compared: u32,
    /// GPU finish relative to the tick start and to the CPU present.
    gpu_work: Vec<f64>,
    gpu_after_present: Vec<f64>,
    misses: u32,
    /// Present→glass and two-or-more-refresh gaps of frames made while dragging.
    engaged_to_glass: Vec<f64>,
    engaged_repeats: u32,
}

/// Consecutive frames above the floor before skipping, and frames to wait after a
/// skip for its effect to reach the feedback (which lags by a couple of refreshes).
const STREAK: u32 = 3;
const COOLDOWN: u32 = 12;

impl PresentFeedback {
    pub fn install(drain: bool) -> Option<Self> {
        let print = std::env::var_os("IRON_NATIVE_PRESENT_TRACE").is_some();
        if !print && !drain {
            return None;
        }
        let class = AnyClass::get(c"CAMetalLayer").expect("CAMetalLayer");
        let method = class.instance_method(sel!(nextDrawable)).expect("nextDrawable");
        let ours: Imp = unsafe { std::mem::transmute(next_drawable as NextDrawable) };
        ORIGINAL.get_or_init(|| unsafe { method.set_implementation(ours) });
        // Refresh period for the gap histogram and backlog threshold.
        crate::vsync_clock::start();
        if !crate::gpu_timing::install() {
            eprintln!("GPU completion timing unavailable; budgeting CPU work only");
        }
        Some(Self { print, ..Default::default() })
    }

    /// Call after each `app.frame()`, with the tick's start and that update's
    /// `Time` delta. Returns frames whose display time has arrived.
    pub fn after_frame(&mut self, start: f64, planned: f64, step: f64, engaged: bool) -> Vec<FrameResult> {
        let mut results = Vec::new();
        let now = CACurrentMediaTime();
        if let Some((_, period)) = crate::vsync_clock::next_after(now) {
            self.period = period;
        }
        let id = CURRENT.load(Relaxed);
        if id != self.last_id {
            self.last_id = id;
            let seq = crate::gpu_timing::last_seq();
            let buffers = (self.last_seq, seq);
            self.last_seq = seq;
            self.pending.push_back(Frame { id, start, buffers, present: now, planned, step, engaged });
            self.stats.frames += 1;
        }
        self.gpu_done.extend(crate::gpu_timing::take_completed());
        for (id, glass) in std::mem::take(&mut *FEEDBACK.lock().unwrap()) {
            let Some(at) = self.pending.iter().position(|f| f.id == id) else {
                continue;
            };
            let frame = self.pending.remove(at).unwrap();
            if glass <= 0.0 {
                self.stats.dropped += 1;
                continue;
            }
            let to_glass = glass - frame.present;
            self.watch_backlog(to_glass);
            // The frame's last buffer is wgpu's present-only buffer, whose completion
            // follows the drawable's scheduling rather than rendering: leave it out.
            let (first, last) = frame.buffers;
            let work_last = last.saturating_sub(1);
            let gpu_done = (work_last > first && self.gpu_done.contains_key(&work_last)).then(|| {
                self.gpu_done.range(first + 1..=work_last).map(|(_, &end)| end).fold(f64::MIN, f64::max)
            });
            self.gpu_done = self.gpu_done.split_off(&(last + 1));
            if let Some(done) = gpu_done {
                self.stats.gpu_work.push(done - frame.start);
                self.stats.gpu_after_present.push(done - frame.present);
            }
            let missed = frame.planned > 0.0 && self.period > 0.0 && {
                let late = glass - frame.planned;
                self.lateness.push_back(late);
                if self.lateness.len() > 240 {
                    self.lateness.pop_front();
                }
                let mut sorted: Vec<f64> = self.lateness.iter().copied().collect();
                sorted.sort_by(f64::total_cmp);
                late > sorted[sorted.len() / 2] + self.period * 0.5
            };
            self.stats.misses += missed as u32;
            results.push(FrameResult { start: frame.start, present: frame.present, gpu_done, planned: frame.planned, missed });
            self.stats.to_glass.push(to_glass);
            if frame.engaged {
                self.stats.engaged_to_glass.push(to_glass);
            }
            if frame.planned > 0.0 {
                self.stats.after_planned.push(glass - frame.planned);
            }
            if self.last_glass > 0.0 && self.period > 0.0 {
                let refreshes = ((glass - self.last_glass) / self.period).round() as usize;
                self.stats.gaps[refreshes.clamp(1, 4) - 1] += 1;
                if frame.engaged && refreshes >= 2 && frame.id == self.last_glass_id + 1 {
                    self.stats.engaged_repeats += 1;
                }
                if self.print && refreshes >= 2 && frame.id == self.last_glass_id + 1 {
                    // Presented late (CPU overran) or on time but still missed (GPU/compositor)?
                    println!(
                        "IRON_NATIVE_PRESENT_MISS {}",
                        serde_json::json!({
                            "refreshes": refreshes,
                            "presentBeforePlannedVsyncMs": (frame.planned > 0.0).then(|| (frame.planned - frame.present) * 1000.0),
                            "presentToGlassMs": to_glass * 1000.0,
                            "stepMs": frame.step * 1000.0,
                        })
                    );
                }
                // Only consecutive drawables: idle gaps legitimately differ.
                if frame.id == self.last_glass_id + 1 {
                    self.stats.step_compared += 1;
                    if (frame.step - (glass - self.last_glass)).abs() > self.period * 0.25 {
                        self.stats.step_mismatch += 1;
                    }
                }
            }
            self.last_glass = glass;
            self.last_glass_id = frame.id;
        }
        while self.pending.len() > 8 {
            self.pending.pop_front();
        }
        if self.stats.frames >= 120 {
            if self.print {
                self.report();
            }
            self.stats = Stats::default();
        }
        results
    }

    /// The pacer's current margin, for the report.
    pub fn note_margin(&mut self, margin_ms: f64) {
        self.margin_ms = margin_ms;
    }

    fn watch_backlog(&mut self, to_glass: f64) {
        self.recent.push_back(to_glass);
        if self.recent.len() > 240 {
            self.recent.pop_front();
        }
        if self.cooldown > 0 || self.period <= 0.0 {
            self.cooldown = self.cooldown.saturating_sub(1);
            return;
        }
        let floor = self.recent.iter().copied().fold(f64::INFINITY, f64::min);
        if to_glass > floor + self.period * 0.6 {
            self.streak += 1;
            if self.streak >= STREAK {
                SKIP.store(true, Relaxed);
                self.stats.skips += 1;
                self.streak = 0;
                self.cooldown = COOLDOWN;
            }
        } else {
            self.streak = 0;
        }
    }

    fn report(&mut self) {
        let pct = |v: &mut Vec<f64>, q: f64| {
            v.sort_by(f64::total_cmp);
            v.get((v.len() as f64 * q) as usize).map(|x| x * 1000.0)
        };
        let s = &mut self.stats;
        println!(
            "IRON_NATIVE_PRESENT {}",
            serde_json::json!({
                "frames": s.frames,
                "marginMs": self.margin_ms,
                "presentToGlassP50Ms": pct(&mut s.to_glass, 0.5),
                "presentToGlassP95Ms": pct(&mut s.to_glass, 0.95),
                "glassMinusPlannedVsyncP50Ms": pct(&mut s.after_planned, 0.5),
                "glassGapsInRefreshes": {"1": s.gaps[0], "2": s.gaps[1], "3": s.gaps[2], "4+": s.gaps[3]},
                "dropped": s.dropped,
                "backlogSkips": s.skips,
                "misses": s.misses,
                "engagedFrames": s.engaged_to_glass.len(),
                "engagedPresentToGlassP50Ms": pct(&mut s.engaged_to_glass, 0.5),
                "engagedRepeats": s.engaged_repeats,
                "plannedSkips": crate::pacing::PLANNED_SKIPS.swap(0, Relaxed),
                "gpuDoneAfterStartP50Ms": pct(&mut s.gpu_work, 0.5),
                "gpuDoneAfterStartP95Ms": pct(&mut s.gpu_work, 0.95),
                "gpuDoneAfterPresentP95Ms": pct(&mut s.gpu_after_present, 0.95),
                "timeStepMismatch": s.step_mismatch,
                "timeStepCompared": s.step_compared,
            })
        );
    }
}
