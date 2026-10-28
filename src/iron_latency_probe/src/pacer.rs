//! Opt-in `IRON_BEVY_PACING`: one frame per refresh, started as late as the
//! measured work allows.
//!
//! - `late`: no display sync; frames flip onto the display mid-scan (tears).
//! - `late-fifo`: display sync; present a fixed margin before the vsync.
//! - `metal-link`: a `CAMetalDisplayLink` (frame latency 1) supplies each drawable
//!   and its deadline; the wrapped `nextDrawable` hands that drawable to wgpu.
//!
//! Without display sync a frame reaches the glass a few milliseconds after
//! `presentDrawable`, whenever that is, so the vsync grid comes from a
//! `CVDisplayLink` on its own thread (a main-thread `CADisplayLink` jumped while
//! this pacer blocks that thread) and the present→glass time from each
//! drawable's `presentedTime`. wgpu keeps the drawable private, so
//! `-[CAMetalLayer nextDrawable]` is wrapped to attach a presented handler.
//! The pacer runs after `render_system` presents, and sleeps there: winit then
//! delivers the events that arrived meanwhile before the next update reads them.
use bevy::prelude::*;
use bevy::render::{Render, RenderApp, RenderSystems};
use block2::RcBlock;
use objc2::{
    AllocAnyThread, define_class, msg_send,
    rc::Retained,
    runtime::{AnyClass, AnyObject, Imp, NSObject, NSObjectProtocol, ProtocolObject, Sel},
    sel,
};
use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSRunLoop};
use objc2_quartz_core::{
    CAMetalDisplayLink, CAMetalDisplayLinkDelegate, CAMetalDisplayLinkUpdate, CAMetalDrawable,
    CAMetalLayer,
};
use objc2_core_video::{
    CVDisplayLink, CVGetHostClockFrequency, CVOptionFlags, CVReturn, CVTimeStamp, kCVReturnSuccess,
};
use objc2_quartz_core::CACurrentMediaTime;
use std::{
    collections::VecDeque,
    time::Duration,
    ffi::c_void,
    ptr::NonNull,
    sync::{
        Condvar, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering::Relaxed},
    },
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Immediate,
    Fifo,
    MetalLink,
}

static MODE: OnceLock<Mode> = OnceLock::new();
static ORIGINAL: OnceLock<Imp> = OnceLock::new();
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static CURRENT: AtomicU64 = AtomicU64::new(0);
/// Bits of the vsync the next frame is planned for; 0 when unpaced.
static PLANNED_VSYNC: AtomicU64 = AtomicU64::new(0);

/// When the frame being built is meant to reach the display, if paced.
pub fn planned_vsync() -> Option<f64> {
    let v = f64::from_bits(PLANNED_VSYNC.load(Relaxed));
    (v > 0.0).then_some(v)
}
/// (drawable id, presentedTime); 0.0 means the drawable never reached the display.
static FEEDBACK: Mutex<Vec<(u64, f64)>> = Mutex::new(Vec::new());

type NextDrawable = unsafe extern "C-unwind" fn(*mut AnyObject, Sel) -> *mut AnyObject;

unsafe extern "C-unwind" fn next_drawable(layer: *mut AnyObject, cmd: Sel) -> *mut AnyObject {
    let original: NextDrawable = unsafe { std::mem::transmute(*ORIGINAL.get().unwrap()) };
    let drawable = if MODE.get() == Some(&Mode::MetalLink) {
        // Never mix the layer's own nextDrawable with an attached display link
        // (that aborted with an Objective-C exception). Wait for the link's next
        // drawable; null makes wgpu skip the frame as a timeout.
        start_link(layer);
        let guard = LINK.lock().unwrap();
        let (mut s, _) = LINK_READY
            .wait_timeout_while(guard, Duration::from_millis(100), |s| s.drawable.is_none())
            .unwrap();
        match s.drawable.take() {
            // +0 like the original: wgpu retains it inside its autorelease pool.
            Some(d) => Retained::autorelease_ptr(d).cast::<AnyObject>(),
            None => std::ptr::null_mut(),
        }
    } else {
        unsafe { original(layer, cmd) }
    };
    if let Some(drawable) = unsafe { drawable.as_ref() } {
        let id = NEXT_ID.fetch_add(1, Relaxed);
        CURRENT.store(id, Relaxed);
        let handler = RcBlock::new(move |d: NonNull<AnyObject>| {
            let glass: f64 = unsafe { msg_send![d.as_ref(), presentedTime] };
            FEEDBACK.lock().unwrap().push((id, glass));
        });
        let _: () = unsafe { msg_send![drawable, addPresentedHandler: &*handler] };
    }
    drawable
}

struct LinkState {
    drawable: Option<Retained<ProtocolObject<dyn CAMetalDrawable>>>,
    /// `targetTimestamp`: present before this.
    deadline: f64,
    /// `targetPresentationTimestamp`: when the frame is expected on glass.
    presentation: f64,
    seq: u64,
}
// The drawable moves from the link thread to the render thread exactly once.
unsafe impl Send for LinkState {}

static LINK: Mutex<LinkState> =
    Mutex::new(LinkState { drawable: None, deadline: 0.0, presentation: 0.0, seq: 0 });
static LINK_READY: Condvar = Condvar::new();

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = AllocAnyThread]
    struct LinkDelegate;
    unsafe impl NSObjectProtocol for LinkDelegate {}
    unsafe impl CAMetalDisplayLinkDelegate for LinkDelegate {
        #[unsafe(method(metalDisplayLink:needsUpdate:))]
        fn needs_update(&self, _link: &CAMetalDisplayLink, update: &CAMetalDisplayLinkUpdate) {
            let mut s = LINK.lock().unwrap();
            s.drawable = Some(update.drawable());
            s.deadline = update.targetTimestamp();
            s.presentation = update.targetPresentationTimestamp();
            s.seq += 1;
            LINK_READY.notify_all();
        }
    }
);

struct SendLayer(Retained<CAMetalLayer>);
unsafe impl Send for SendLayer {}

/// On first use, attach a `CAMetalDisplayLink` to wgpu's layer, on its own run loop
/// so the pacer's sleep on the main thread can't delay its callbacks.
fn start_link(layer: *mut AnyObject) {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        let layer = SendLayer(unsafe { Retained::retain(layer.cast::<CAMetalLayer>()) }.unwrap());
        std::thread::spawn(move || {
            let layer = layer;
            let link = CAMetalDisplayLink::initWithMetalLayer(CAMetalDisplayLink::alloc(), &layer.0);
            let delegate: Retained<LinkDelegate> = unsafe { msg_send![LinkDelegate::alloc(), init] };
            link.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
            link.setPreferredFrameLatency(1.0);
            let run_loop = NSRunLoop::currentRunLoop();
            let mode = unsafe { NSDefaultRunLoopMode };
            unsafe { link.addToRunLoop_forMode(&run_loop, mode) };
            loop {
                run_loop.runMode_beforeDate(mode, &NSDate::distantFuture());
            }
        });
    });
}

/// (host seconds of the latest vsync, refresh period), from the CVDisplayLink thread.
static VSYNC: Mutex<Option<(f64, f64)>> = Mutex::new(None);

unsafe extern "C-unwind" fn display_tick(
    _link: NonNull<CVDisplayLink>,
    _now: NonNull<CVTimeStamp>,
    output: NonNull<CVTimeStamp>,
    _flags: CVOptionFlags,
    _flags_out: NonNull<CVOptionFlags>,
    _context: *mut c_void,
) -> CVReturn {
    let output = unsafe { output.as_ref() };
    if output.videoTimeScale > 0 {
        let vsync = output.hostTime as f64 / CVGetHostClockFrequency();
        let period = output.videoRefreshPeriod as f64 / output.videoTimeScale as f64;
        *VSYNC.lock().unwrap() = Some((vsync, period));
    }
    kCVReturnSuccess
}

/// Starts the CVDisplayLink clock once; it runs for the life of the process.
fn start_clock() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        let mut link: *mut CVDisplayLink = std::ptr::null_mut();
        #[allow(deprecated)]
        unsafe {
            CVDisplayLink::create_with_active_cg_displays(NonNull::from(&mut link));
            let link = link.as_ref().expect("CVDisplayLink");
            link.set_output_callback(Some(display_tick), std::ptr::null_mut());
            link.start();
        }
    });
}

/// The first vsync strictly after `t`, and the refresh period.
fn next_vsync_after(t: f64) -> Option<(f64, f64)> {
    let (vsync, period) = (*VSYNC.lock().unwrap())?;
    (period > 0.0).then(|| (vsync + ((t - vsync) / period).floor() * period + period, period))
}

pub fn install(app: &mut App, mode: Mode) {
    MODE.set(mode).unwrap();
    let class = AnyClass::get(c"CAMetalLayer").expect("CAMetalLayer");
    let method = class.instance_method(sel!(nextDrawable)).expect("nextDrawable");
    let ours: Imp = unsafe { std::mem::transmute(next_drawable as NextDrawable) };
    ORIGINAL.get_or_init(|| unsafe { method.set_implementation(ours) });
    start_clock();
    app.sub_app_mut(RenderApp)
        .add_systems(Render, pace.in_set(RenderSystems::PostCleanup));
}

struct Frame {
    id: u64,
    present: f64,
    /// The vsync this frame was planned for; 0.0 when unpaced.
    vsync: f64,
}

#[derive(Default)]
struct Pacer {
    pending: VecDeque<Frame>,
    /// Recent present → glass intervals; the median sets the lead.
    to_glass: VecDeque<f64>,
    /// Recent wake → present intervals, which set the work budget.
    work: VecDeque<f64>,
    last_id: u64,
    wake: f64,
    vsync: f64,
    link_seq: u64,
    // Report window.
    frames: u32,
    glass_after_vsync: Vec<f64>,
    dropped: u32,
    unpaced: u32,
    oversleep: f64,
    last_lead: f64,
    last_budget: f64,
}

const MARGIN: f64 = 0.001;

/// `late-fifo`: how long before the vsync to present (`IRON_BEVY_PACER_MARGIN_MS`).
fn fifo_margin() -> f64 {
    static M: OnceLock<f64> = OnceLock::new();
    *M.get_or_init(|| {
        std::env::var("IRON_BEVY_PACER_MARGIN_MS")
            .map_or(5.0, |v| v.parse().expect("IRON_BEVY_PACER_MARGIN_MS: number"))
            / 1000.0
    })
}

fn push(window: &mut VecDeque<f64>, v: f64) {
    window.push_back(v);
    if window.len() > 120 {
        window.pop_front();
    }
}

fn quantile(window: &VecDeque<f64>, q: usize) -> Option<f64> {
    let mut v: Vec<f64> = window.iter().copied().collect();
    v.sort_by(f64::total_cmp);
    v.get(v.len() * q / 100).copied()
}

fn p95(window: &VecDeque<f64>) -> Option<f64> {
    quantile(window, 95)
}

fn median(window: &VecDeque<f64>) -> Option<f64> {
    quantile(window, 50)
}

/// `NonSendMarker`: the sleep must hold up the main (input) thread.
fn pace(mut p: Local<Pacer>, _main: bevy::ecs::system::NonSendMarker) {
    let now = CACurrentMediaTime();
    let id = CURRENT.load(Relaxed);
    if id != p.last_id {
        p.last_id = id;
        let vsync = p.vsync;
        p.pending.push_back(Frame { id, present: now, vsync });
        if p.wake > 0.0 {
            let work = now - p.wake;
            push(&mut p.work, work);
        }
        p.frames += 1;
    }

    for (id, glass) in std::mem::take(&mut *FEEDBACK.lock().unwrap()) {
        let Some(at) = p.pending.iter().position(|f| f.id == id) else {
            continue;
        };
        let frame = p.pending.remove(at).unwrap();
        if glass <= 0.0 {
            p.dropped += 1;
            continue;
        }
        push(&mut p.to_glass, glass - frame.present);
        if frame.vsync > 0.0 {
            p.glass_after_vsync.push(glass - frame.vsync);
        }
        if std::env::var_os("IRON_BEVY_PACER_RAW").is_some() {
            println!("IRON_BEVY_PACER_RAW {} {} {} {} {:?}", frame.id, frame.vsync, frame.present, glass, *VSYNC.lock().unwrap());
        }
    }
    // Drawables whose handler never fired (e.g. while occluded) shouldn't pile up.
    while p.pending.len() > 8 {
        p.pending.pop_front();
    }

    if p.frames >= 120 {
        report(&mut p);
    }

    let mode = *MODE.get().unwrap();
    let Some(work) = p95(&p.work) else {
        unpaced(&mut p);
        return;
    };
    let budget = work + MARGIN;
    let (vsync, lead) = match mode {
        Mode::MetalLink => {
            let guard = LINK.lock().unwrap();
            let seen = p.link_seq;
            let (s, timeout) = LINK_READY
                .wait_timeout_while(guard, Duration::from_millis(50), |s| s.seq == seen)
                .unwrap();
            if timeout.timed_out() {
                drop(s);
                unpaced(&mut p);
                return;
            }
            p.link_seq = s.seq;
            (s.presentation, s.presentation - s.deadline)
        }
        Mode::Immediate | Mode::Fifo => {
            // Immediate: flips land a few ms after present wherever that is, so aim
            // the typical one at vsync. FIFO: present a fixed margin before it.
            let lead = match mode {
                Mode::Immediate => match median(&p.to_glass) {
                    Some(v) => v,
                    None => {
                        unpaced(&mut p);
                        return;
                    }
                },
                _ => fifo_margin(),
            };
            let Some((mut vsync, period)) = next_vsync_after(CACurrentMediaTime() + budget + lead)
            else {
                unpaced(&mut p);
                return;
            };
            // Never plan two frames for one refresh.
            if vsync < p.vsync + period * 0.5 {
                vsync += period;
            }
            (vsync, lead)
        }
    };
    let wake = vsync - lead - budget;
    sleep_until(wake);
    let woke = CACurrentMediaTime();
    p.oversleep += (woke - wake).max(0.0);
    p.wake = woke;
    p.vsync = vsync;
    PLANNED_VSYNC.store(vsync.to_bits(), Relaxed);
    p.last_lead = lead;
    p.last_budget = budget;
}

fn unpaced(p: &mut Pacer) {
    p.unpaced += 1;
    p.vsync = 0.0;
    PLANNED_VSYNC.store(0, Relaxed);
    p.wake = CACurrentMediaTime();
}

fn sleep_until(t: f64) {
    let ahead = t - CACurrentMediaTime();
    if ahead > 0.0015 {
        std::thread::sleep(std::time::Duration::from_secs_f64(ahead - 0.001));
    }
    while CACurrentMediaTime() < t {
        std::hint::spin_loop();
    }
}

fn report(p: &mut Pacer) {
    p.glass_after_vsync.sort_by(f64::total_cmp);
    let pct = |q: f64| {
        p.glass_after_vsync
            .get((p.glass_after_vsync.len() as f64 * q) as usize)
            .map(|v| v * 1000.0)
    };
    let mut to_glass: Vec<f64> = p.to_glass.iter().copied().collect();
    to_glass.sort_by(f64::total_cmp);
    println!(
        "IRON_BEVY_PACER {}",
        serde_json::json!({
            "frames": p.frames,
            "leadMs": p.last_lead * 1000.0,
            "budgetMs": p.last_budget * 1000.0,
            "presentToGlassP50Ms": to_glass.get(to_glass.len() / 2).map(|v| v * 1000.0),
            "glassMinusVsyncP05Ms": pct(0.05),
            "glassMinusVsyncP50Ms": pct(0.5),
            "glassMinusVsyncP95Ms": pct(0.95),
            "dropped": p.dropped,
            "unpaced": p.unpaced,
            "meanOversleepMs": p.oversleep / p.frames as f64 * 1000.0,
        })
    );
    p.frames = 0;
    p.glass_after_vsync.clear();
    p.dropped = 0;
    p.unpaced = 0;
    p.oversleep = 0.0;
}
