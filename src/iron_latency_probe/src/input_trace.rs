//! Optional observational AppKit timestamps. Off by default; never drives input.
use bevy::prelude::*;
use block2::RcBlock;
use objc2::{rc::Retained, runtime::AnyObject};
use objc2_app_kit::{NSEvent, NSEventMask};
use objc2_quartz_core::CACurrentMediaTime;
use std::{cell::Cell, ptr::NonNull};

#[derive(Clone, Copy)]
struct Sample {
    event: f64,
    received: f64,
}
thread_local! {static LATEST:Cell<Option<Sample>>=const{Cell::new(None)};}
struct Monitor(Retained<AnyObject>);
impl Drop for Monitor {
    fn drop(&mut self) {
        unsafe { NSEvent::removeMonitor(&self.0) };
    }
}
pub fn install(app: &mut App) {
    if std::env::var_os("IRON_BEVY_INPUT_TRACE").is_none() {
        return;
    }
    app.add_systems(Startup, setup);
    app.add_systems(PostUpdate, record);
}
fn setup(world: &mut World) {
    let block = RcBlock::new(|event: NonNull<NSEvent>| -> *mut NSEvent {
        let e = unsafe { event.as_ref() };
        if e.windowNumber() != 0 {
            LATEST.with(|s| {
                s.set(Some(Sample {
                    event: e.timestamp(),
                    received: CACurrentMediaTime(),
                }))
            });
        }
        event.as_ptr()
    });
    let mask = NSEventMask::MouseMoved
        | NSEventMask::LeftMouseDragged
        | NSEventMask::LeftMouseDown
        | NSEventMask::LeftMouseUp;
    let monitor = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &block) }
        .expect("AppKit input monitor");
    world.insert_non_send(Monitor(monitor));
}
fn record(
    _monitor: NonSend<Monitor>,
    windows: Query<&Window>,
    buttons: Res<ButtonInput<MouseButton>>,
    targets: Query<&Transform, With<crate::Target>>,
    mut last: Local<f64>,
) {
    let Some(sample) = LATEST.with(Cell::get) else {
        return;
    };
    if *last == sample.event {
        return;
    }
    *last = sample.event;
    let now = CACurrentMediaTime();
    let Ok(window) = windows.single() else {
        return;
    };
    println!(
        "IRON_BEVY_INPUT {}",
        serde_json::json!({"eventTimestamp":sample.event,"monitorReceived":sample.received,"postUpdate":now,"dispatchAgeMs":(sample.received-sample.event)*1000.0,"postUpdateEventAgeMs":(now-sample.event)*1000.0,"cursorLogical":window.cursor_position().map(|p|[p.x,p.y]),"dpr":window.scale_factor(),"leftPressed":buttons.pressed(MouseButton::Left),"leftJustPressed":buttons.just_pressed(MouseButton::Left),"leftJustReleased":buttons.just_released(MouseButton::Left),"targetWorld":targets.single().ok().map(|t|[t.translation.x,t.translation.y])})
    );
}
