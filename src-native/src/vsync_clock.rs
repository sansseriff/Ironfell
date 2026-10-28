//! Vsync grid from a `CVDisplayLink`, which runs on its own thread. The standalone
//! probe (`src/iron_latency_probe`) found a main-thread `CADisplayLink`'s timestamps
//! jumping by part of a period whenever that thread was busy.
use objc2_core_video::{
    CVDisplayLink, CVGetHostClockFrequency, CVOptionFlags, CVReturn, CVTimeStamp, kCVReturnSuccess,
};
use std::{
    ffi::c_void,
    ptr::NonNull,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering::Relaxed},
    },
};

/// (host seconds of the latest vsync, refresh period); host seconds = CACurrentMediaTime.
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

static STARTED: AtomicBool = AtomicBool::new(false);
static WARNED: AtomicBool = AtomicBool::new(false);

/// Starts the clock; once running it runs for the life of the process. With no
/// active display (asleep, locked at launch) creation fails, and `next_after`
/// retries, so pacing recovers when a display comes back.
pub fn start() {
    if STARTED.load(Relaxed) {
        return;
    }
    let mut link: *mut CVDisplayLink = std::ptr::null_mut();
    // CVDisplayLink is deprecated in favour of view/screen display links, which
    // deliver on the main run loop: exactly what this clock avoids.
    #[allow(deprecated)]
    unsafe {
        CVDisplayLink::create_with_active_cg_displays(NonNull::from(&mut link));
        let Some(link) = link.as_ref() else {
            if !WARNED.swap(true, Relaxed) {
                eprintln!("CVDisplayLink unavailable (no active display); vsync pacing starts immediately until one appears");
            }
            return;
        };
        link.set_output_callback(Some(display_tick), std::ptr::null_mut());
        link.start();
    }
    STARTED.store(true, Relaxed);
}

/// The first vsync strictly after `t`, and the refresh period.
pub fn next_after(t: f64) -> Option<(f64, f64)> {
    start();
    let (vsync, period) = (*VSYNC.lock().unwrap())?;
    (period > 0.0).then(|| (vsync + ((t - vsync) / period).floor() * period + period, period))
}
