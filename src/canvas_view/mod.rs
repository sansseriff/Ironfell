//! Single-window canvas bootstrap.
//!
//! Wraps the one full-window canvas (HTML canvas on the main thread, or an
//! OffscreenCanvas in a worker) into a Bevy `Window` + `RawHandleWrapper`.
//! There is exactly one window; panel subdivision happens via camera viewports
//! (see `crate::panels`).

use app_surface::{CanvasWrapper, OffscreenCanvasWrapper};
use bevy::ecs::system::SystemState;
use bevy::prelude::*;
use bevy::window::{
    PresentMode, PrimaryWindow, RawHandleWrapper, RawHandleWrapperHolder, Window, WindowCreated,
    WindowResized, WindowWrapper,
};
use std::sync::{Arc, Mutex};

pub(crate) use app_surface::{Canvas, OffscreenCanvas};

/// Encapsulate ViewObj to simultaneously support Canvas and Offscreen
#[derive(Debug)]
pub enum ViewObj {
    Canvas(WindowWrapper<CanvasWrapper>),
    Offscreen(WindowWrapper<OffscreenCanvasWrapper>),
}

impl ViewObj {
    #[allow(dead_code)]
    pub fn from_canvas(canvas: Canvas) -> Self {
        ViewObj::Canvas(WindowWrapper::new(CanvasWrapper::new(canvas)))
    }

    pub fn from_offscreen_canvas(canvas: OffscreenCanvas) -> Self {
        ViewObj::Offscreen(WindowWrapper::new(OffscreenCanvasWrapper::new(canvas)))
    }

    pub fn physical_resolution(&self) -> (u32, u32) {
        match self {
            ViewObj::Canvas(canvas) => canvas.physical_resolution(),
            ViewObj::Offscreen(offscreen) => offscreen.physical_resolution(),
        }
    }
}

/// NonSend resource keeping the canvas wrapper (and thus the raw handles) alive
/// for the lifetime of the app.
pub struct ActiveCanvas {
    pub view: ViewObj,
    pub window: Entity,
}

/// Spawn the single primary window for the provided canvas and wire up its raw handle.
///
/// **Must run before `RenderPlugin` is added.** `RenderPlugin::build` — not
/// `finish` — is what starts renderer initialization, and it looks for the
/// primary window right then to build a surface for adapter selection. A window
/// spawned afterwards is too late: see the `RawHandleWrapperHolder` note below.
pub fn spawn_canvas_window(app: &mut App, view: ViewObj) -> Entity {
    let (width, height) = view.physical_resolution();

    let mut window = Window {
        title: "Ironfell".to_owned(),
        present_mode: PresentMode::AutoNoVsync,
        ..default()
    };
    // Scale is handled JS-side; everything Rust-side is physical pixels.
    window.resolution.set_scale_factor(1.0);
    window.resolution.set(width as f32, height as f32);

    let raw_handle = match &view {
        ViewObj::Canvas(wrapper) => RawHandleWrapper::new(wrapper),
        ViewObj::Offscreen(wrapper) => RawHandleWrapper::new(wrapper),
    }
    .expect("failed to wrap canvas window handle");

    // `RawHandleWrapperHolder` is not optional here, even though `RawHandleWrapper`
    // alone is what the rest of this app reads.
    //
    // `RenderPlugin` initializes the renderer from an async task, so it cannot
    // borrow the handle out of the ECS; it looks specifically for
    // `RawHandleWrapperHolder` on the primary window and, finding one, creates a
    // surface to pass as `compatible_surface` when requesting the adapter.
    //
    // On WebGPU a missing holder is invisible: an adapter can be resolved with no
    // surface at all. On WebGL2 it is fatal — the adapter *is* the canvas's GL
    // context, so wgpu's GL backend returns no adapters unless a surface is
    // supplied, and Bevy then panics with "Unable to find a GPU!". `Window` only
    // requires `CursorOptions`, so nothing inserts this component for us.
    let entity = app
        .world_mut()
        .spawn((
            window,
            PrimaryWindow,
            raw_handle.clone(),
            RawHandleWrapperHolder(Arc::new(Mutex::new(Some(raw_handle)))),
        ))
        .id();

    app.insert_non_send_resource(ActiveCanvas {
        view,
        window: entity,
    });

    info!("Created canvas window {entity:?} ({width}x{height})");
    entity
}

/// Emit `WindowCreated` for the window spawned by [`spawn_canvas_window`].
///
/// Separate from the spawn because `Messages<WindowCreated>` is registered by
/// `WindowPlugin`, which does not exist yet at spawn time.
pub fn announce_canvas_window(app: &mut App) {
    let Some(entity) = app
        .world()
        .get_non_send_resource::<ActiveCanvas>()
        .map(|c| c.window)
    else {
        return;
    };
    app.world_mut()
        .write_message(WindowCreated { window: entity });
}

/// Sync the Bevy window resolution to the canvas' current physical size and emit
/// `WindowResized`. Called from the `resize` FFI after JS updates the canvas backing size.
pub fn update_canvas_window(app: &mut App) {
    let Some((width, height)) = app
        .world()
        .get_non_send_resource::<ActiveCanvas>()
        .map(|c| c.view.physical_resolution())
    else {
        return;
    };

    let mut system_state: SystemState<(Query<(Entity, &mut Window)>, MessageWriter<WindowResized>)> =
        SystemState::new(app.world_mut());
    // bevy 0.19: SystemState::get_mut returns Result.
    let Ok((mut windows, mut resize_events)) = system_state.get_mut(app.world_mut()) else {
        return;
    };

    for (entity, mut window) in windows.iter_mut() {
        window.resolution.set_scale_factor(1.0);
        window.resolution.set(width as f32, height as f32);
        resize_events.write(WindowResized {
            window: entity,
            width: width as f32,
            height: height as f32,
        });
    }

    system_state.apply(app.world_mut());
}
