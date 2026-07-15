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
    PresentMode, PrimaryWindow, RawHandleWrapper, Window, WindowCreated, WindowResized,
    WindowWrapper,
};

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
pub fn create_canvas_window(app: &mut App, view: ViewObj) -> Entity {
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

    let entity = app
        .world_mut()
        .spawn((window, PrimaryWindow, raw_handle))
        .id();

    app.world_mut()
        .send_event(WindowCreated { window: entity });
    app.insert_non_send_resource(ActiveCanvas {
        view,
        window: entity,
    });

    info!("Created canvas window {entity:?} ({width}x{height})");
    entity
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

    let mut system_state: SystemState<(Query<(Entity, &mut Window)>, EventWriter<WindowResized>)> =
        SystemState::new(app.world_mut());
    let (mut windows, mut resize_events) = system_state.get_mut(app.world_mut());

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
