//! Observe local AppKit motion without consuming it. WKWebView continues to
//! own HTML hit testing, buttons, text, scrolling and accessibility.
use block2::RcBlock;
use objc2::{MainThreadMarker, rc::Retained, runtime::AnyObject};
use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSView};
use objc2_foundation::NSPoint;
use std::{cell::Cell, ptr::NonNull, time::Instant};
#[derive(Clone, Copy)]
pub struct Sample {
    pub point: NSPoint,
    pub modifiers: u8,
    pub received: Instant,
}
thread_local! {static LATEST:Cell<Option<Sample>>=const{Cell::new(None)};}
pub fn latest() -> Option<Sample> {
    LATEST.with(Cell::get)
}
pub struct NativeInput {
    monitor: Retained<AnyObject>,
}
impl NativeInput {
    pub fn new(view: &NSView) -> Result<Self, Box<dyn std::error::Error>> {
        MainThreadMarker::new().ok_or("native input requires OS main thread")?;
        let window = view.window().ok_or("native input view has no window")?;
        window.setAcceptsMouseMovedEvents(true);
        let number = window.windowNumber();
        let trace = std::env::var_os("IRON_NATIVE_INPUT_TRACE").is_some();
        let block = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            let e = unsafe { event.as_ref() };
            if e.windowNumber() == number {
                if trace {
                    println!(
                        "IRON_NATIVE_INPUT {:?} {:?}",
                        e.r#type(),
                        e.locationInWindow()
                    );
                }
                let flags = e.modifierFlags();
                let modifiers = (flags.contains(NSEventModifierFlags::Shift) as u8)
                    | ((flags.contains(NSEventModifierFlags::Control) as u8) << 1)
                    | ((flags.contains(NSEventModifierFlags::Option) as u8) << 2)
                    | ((flags.contains(NSEventModifierFlags::Command) as u8) << 3);
                LATEST.with(|s| {
                    s.set(Some(Sample {
                        point: e.locationInWindow(),
                        modifiers,
                        received: Instant::now(),
                    }))
                });
            }
            // Observational only. Never steal an event from WKWebView or menus.
            event.as_ptr()
        });
        let mask = NSEventMask::MouseMoved
            | NSEventMask::LeftMouseDown
            | NSEventMask::LeftMouseUp
            | NSEventMask::OtherMouseDown
            | NSEventMask::OtherMouseUp
            | NSEventMask::LeftMouseDragged
            | NSEventMask::RightMouseDragged
            | NSEventMask::OtherMouseDragged;
        let monitor =
            unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &block) }
                .ok_or("could not install local native motion monitor")?;
        Ok(Self { monitor })
    }
}
impl Drop for NativeInput {
    fn drop(&mut self) {
        unsafe { NSEvent::removeMonitor(&self.monitor) };
        LATEST.with(|s| s.set(None));
    }
}
