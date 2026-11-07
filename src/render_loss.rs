//! What happens when the GPU fails under the renderer: the device is lost
//! (the browser's GPU process dropped it) or wgpu reports an error.
//!
//! Bevy's default handler quits the app, which leaves the canvas blank and
//! says nothing to the host. Here the renderer stops instead, the host is
//! told once (`EngineEvent::RenderLost`), and the main world keeps running,
//! so the host can still save the document before starting a fresh engine
//! on a new canvas (`ControllerManager.recoverRenderer`).
//!
//! Bevy only looks for errors while it renders a frame, and an idle app
//! renders none (`RedrawNeeded`), so a device lost while nothing moves would
//! leave the views blank until the next hover. On WebGPU the device's own
//! `lost` promise is therefore watched too, and tells the host at once.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bevy::prelude::*;
use bevy::render::error_handler::{RenderErrorHandler, RenderErrorPolicy};

/// Set once the loss has been reported, by whichever path saw it first.
/// Bevy calls the handler every frame while rendering is stopped.
#[derive(Resource, Clone, Default)]
struct Reported(Arc<AtomicBool>);

pub fn install(app: &mut App) {
    app.init_resource::<Reported>();
    app.insert_resource(RenderErrorHandler(|error, main_world, _| {
        let first = main_world.get_resource::<Reported>().is_none_or(|r| !r.0.swap(true, Ordering::SeqCst));
        if first {
            let kind = format!("{:?}", error.ty);
            error!("Rendering stopped after a {kind} error; the host restarts the renderer");
            if let Some(events) = main_world.get_resource::<crate::host_events::HostEvents>() {
                events.push(lost(kind, error.description.clone()));
            }
        }
        RenderErrorPolicy::StopRendering
    }));
    #[cfg(target_arch = "wasm32")]
    app.add_systems(Update, watch_device);
}

fn lost(kind: String, error: String) -> crate::host_events::EngineEvent {
    crate::host_events::EngineEvent::RenderLost { kind, error }
}

/// Once the renderer is up: report a lost WebGPU device the moment the
/// browser does. WebGPU's `lost` promise takes any number of handlers, so
/// Bevy's own stays in place (on WebGL2 wgpu keeps only one, Bevy's).
#[cfg(target_arch = "wasm32")]
fn watch_device(
    device: Option<Res<bevy::render::renderer::RenderDevice>>,
    adapter: Option<Res<bevy::render::renderer::RenderAdapterInfo>>,
    reported: Res<Reported>,
    mut watching: Local<bool>,
) {
    if *watching {
        return;
    }
    let (Some(device), Some(adapter)) = (device, adapter) else { return };
    *watching = true;
    if adapter.backend != wgpu::Backend::BrowserWebGpu {
        return;
    }
    let reported = reported.0.clone();
    device.wgpu_device().set_device_lost_callback(move |reason, message| {
        if reported.swap(true, Ordering::SeqCst) {
            return;
        }
        error!("WebGPU device lost ({reason:?}): {message}; the host restarts the renderer");
        let event = lost("DeviceLost".into(), message);
        crate::web_ffi::send_event_from_worker(serde_json::to_string(&event).expect("engine event is serializable"));
    });
}
