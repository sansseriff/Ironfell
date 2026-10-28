//! macOS presentation pacing. All objects and callbacks stay on the OS main thread.
use objc2::rc::Retained;
use objc2::{MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::NSView;
use objc2_foundation::{
    MainThreadMarker, NSObject, NSObjectProtocol, NSRunLoop, NSRunLoopCommonModes,
};
use objc2_quartz_core::{CADisplayLink, CAFrameRateRange};
#[cfg(not(feature = "composition-probe"))]
use {
    crate::pacing::{DisplayTiming, Pacer},
    objc2::DefinedClass,
    objc2_foundation::NSTimer,
    std::cell::RefCell,
};

#[derive(Default)]
struct TargetState {
    #[cfg(not(feature = "composition-probe"))]
    pacer: Option<Pacer>,
    #[cfg(not(feature = "composition-probe"))]
    pending: RefCell<Option<(Retained<NSTimer>, DisplayTiming)>>,
}

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = TargetState]
    struct DisplayTarget;
    unsafe impl NSObjectProtocol for DisplayTarget {}
    impl DisplayTarget {
        #[unsafe(method(displayTick:))]
        fn display_tick(&self, link: &CADisplayLink) {
            #[cfg(feature = "composition-probe")]
            { let _ = link; crate::tick(); }
            #[cfg(not(feature = "composition-probe"))]
            {
                self.cancel_pending();
                let callback = objc2_quartz_core::CACurrentMediaTime();
                let mut timing = DisplayTiming {
                    callback, planned_start: callback,
                    timestamp: link.timestamp(), target: link.targetTimestamp(),
                };
                let Some(delay) = self.ivars().pacer.as_ref().unwrap().delay(&mut timing) else {
                    // Skip this refresh (late-fifo draining a backlog).
                    return;
                };
                if delay < 0.00025 {
                    crate::tick(timing);
                } else {
                    // Return to AppKit while waiting: input remains live. Never
                    // sleep/spin on the main thread or accumulate queued ticks.
                    timing.planned_start = objc2_quartz_core::CACurrentMediaTime() + delay;
                    let timer = unsafe {
                        NSTimer::timerWithTimeInterval_target_selector_userInfo_repeats(
                            delay, self, sel!(firePending:), None, false)
                    };
                    timer.setTolerance(0.0);
                    unsafe { NSRunLoop::mainRunLoop().addTimer_forMode(&timer, NSRunLoopCommonModes) };
                    *self.ivars().pending.borrow_mut() = Some((timer, timing));
                }
            }
        }
        #[cfg(not(feature = "composition-probe"))]
        #[unsafe(method(firePending:))]
        fn fire_pending(&self, _timer: &NSTimer) {
            let pending = self.ivars().pending.borrow_mut().take();
            if let Some((_timer, timing)) = pending { crate::tick(timing); }
        }
    }
);

#[cfg(not(feature = "composition-probe"))]
impl DisplayTarget {
    fn cancel_pending(&self) {
        if let Some((timer, _)) = self.ivars().pending.borrow_mut().take() {
            timer.invalidate();
        }
    }
}

pub struct DisplayDriver {
    link: Retained<CADisplayLink>,
    view: Retained<NSView>,
    #[cfg(not(feature = "composition-probe"))]
    target: Retained<DisplayTarget>,
}

impl DisplayDriver {
    #[cfg(feature = "composition-probe")]
    pub fn new(window: &tauri::WebviewWindow) -> Result<Self, Box<dyn std::error::Error>> {
        // Tauri's window root is suitable for the standalone probe clock.
        let view = unsafe { &*window.ns_view()?.cast::<NSView>() };
        Self::for_view(view)
    }
    pub fn for_view(
        view: &NSView,
        #[cfg(not(feature = "composition-probe"))] pacer: Pacer,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mtm = MainThreadMarker::new().ok_or("display driver requires OS main thread")?;
        let target = DisplayTarget::alloc(mtm).set_ivars(TargetState {
            #[cfg(not(feature = "composition-probe"))]
            pacer: Some(pacer),
            ..Default::default()
        });
        let target: Retained<DisplayTarget> = unsafe { msg_send![super(target), init] };
        let link = unsafe { view.displayLinkWithTarget_selector(&target, sel!(displayTick:)) };
        unsafe { link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes) };
        let driver = Self {
            link,
            view: view.retain(),
            #[cfg(not(feature = "composition-probe"))]
            target,
        };
        driver.update_rate();
        Ok(driver)
    }
    fn update_rate(&self) {
        if let Some(screen) = self.view.window().and_then(|w| w.screen()) {
            let fps = screen.maximumFramesPerSecond().max(1) as f32;
            let range = self.link.preferredFrameRateRange();
            if range.preferred != fps || range.minimum != fps || range.maximum != fps {
                self.link.setPreferredFrameRateRange(CAFrameRateRange {
                    minimum: fps,
                    maximum: fps,
                    preferred: fps,
                });
            }
        }
    }
    pub fn wake(&self) {
        self.update_rate();
        self.link.setPaused(false);
    }
    pub fn pause(&self) {
        self.link.setPaused(true);
        #[cfg(not(feature = "composition-probe"))]
        self.target.cancel_pending();
    }
}

impl Drop for DisplayDriver {
    fn drop(&mut self) {
        self.pause();
        self.link.invalidate();
    }
}
