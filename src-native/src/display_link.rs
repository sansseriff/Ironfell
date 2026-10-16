//! macOS presentation pacing. All objects and callbacks stay on the OS main thread.
use objc2::rc::Retained;
use objc2::{MainThreadOnly, Message, define_class, msg_send, sel};
use objc2_app_kit::NSView;
use objc2_foundation::{
    MainThreadMarker, NSObject, NSObjectProtocol, NSRunLoop, NSRunLoopCommonModes,
};
use objc2_quartz_core::{CADisplayLink, CAFrameRateRange};

define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    struct DisplayTarget;
    unsafe impl NSObjectProtocol for DisplayTarget {}
    impl DisplayTarget {
        #[unsafe(method(displayTick:))]
        fn display_tick(&self, link: &CADisplayLink) {
            #[cfg(feature = "composition-probe")]
            { let _ = link; crate::tick(); }
            #[cfg(not(feature = "composition-probe"))]
            crate::tick(crate::pacing::DisplayTiming {
                callback: objc2_quartz_core::CACurrentMediaTime(),
                timestamp: link.timestamp(), target: link.targetTimestamp(),
            });
        }
    }
);

pub struct DisplayDriver {
    link: Retained<CADisplayLink>,
    view: Retained<NSView>,
}

impl DisplayDriver {
    #[cfg(feature = "composition-probe")]
    pub fn new(window: &tauri::WebviewWindow) -> Result<Self, Box<dyn std::error::Error>> {
        // Tauri's window root is suitable for the standalone probe clock.
        let view = unsafe { &*window.ns_view()?.cast::<NSView>() };
        Self::for_view(view)
    }
    pub fn for_view(view: &NSView) -> Result<Self, Box<dyn std::error::Error>> {
        let mtm = MainThreadMarker::new().ok_or("display driver requires OS main thread")?;
        let target: Retained<DisplayTarget> = unsafe { msg_send![DisplayTarget::alloc(mtm), init] };
        let link = unsafe { view.displayLinkWithTarget_selector(&target, sel!(displayTick:)) };
        unsafe { link.addToRunLoop_forMode(&NSRunLoop::mainRunLoop(), NSRunLoopCommonModes) };
        let driver = Self {
            link,
            view: view.retain(),
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
    }
}

impl Drop for DisplayDriver {
    fn drop(&mut self) {
        self.link.invalidate();
    }
}
