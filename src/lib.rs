// Exactly one graphics backend, enforced at compile time.
//
// Neither mistake is visible at runtime until the app fails to start, and the
// "both" case is the dangerous one: bevy's WebGL2 support is gated on
// `all(feature = "webgl", ..., not(feature = "webgpu"))`, so a build with both
// features compiles the WebGL2 paths out, asks for `Backends::BROWSER_WEBGPU`,
// and dies looking for an adapter. Cargo features unify across the dependency
// graph, so anything that pulls `bevy/webgpu` transitively could do this to the
// WebGL2 build without touching this crate.
#[cfg(all(feature = "webgpu", feature = "webgl2"))]
compile_error!(
    "features `webgpu` and `webgl2` are mutually exclusive: bevy compiles its \
     WebGL2 paths out when `webgpu` is present, producing a build that cannot \
     acquire an adapter. Build each target separately."
);

// Classic Vello flattens paths in compute shaders. WebGL2 has none, so this is a
// build that could never run rather than one that merely runs slowly.
#[cfg(all(feature = "classic", feature = "webgl2"))]
compile_error!(
    "feature `classic` requires `webgpu`: vello classic flattens paths in \
     compute shaders, which WebGL2 does not provide."
);

#[cfg(not(any(feature = "webgpu", feature = "webgl2")))]
compile_error!(
    "no graphics backend selected: build with `--features webgpu` or \
     `--features webgl2` (see [features] in Cargo.toml)."
);

use bevy::{
    ecs::system::SystemState, platform::collections::HashMap, prelude::*,
    window::WindowCloseRequested,
};
use std::ops::{Deref, DerefMut};

// original web ffi module
mod web_ffi;
pub use web_ffi::*;

mod canvas_view;

pub mod panels;

// ray_pick legacy module removed (superseded by new picking systems)

pub mod bevy_app; // expose init_app and related types
pub use bevy_app::*; // re-export init_app symbols

mod fps_overlay;

mod tracking_circle;

mod asset_reader; // kept private


// mod asset_loader;

// The 2D vector rendering seam: display lists in, backend-rendered pixels out.
mod vector;

// The authored store's runtime projection: reconciler, provenance, writeback.
pub mod document_bridge;

mod camera_controller;

pub struct WorkerApp {
    pub app: App,
    /// 手动包装事件需要
    pub window: Entity,
    pub scale_factor: f32,
}

impl Deref for WorkerApp {
    type Target = App;

    fn deref(&self) -> &Self::Target {
        &self.app
    }
}

impl DerefMut for WorkerApp {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.app
    }
}

impl WorkerApp {
    pub fn new(app: App, window: Entity) -> Self {
        Self {
            app,
            window,
            scale_factor: 1.0,
        }
    }

    pub fn to_physical_size(&self, x: f32, y: f32) -> Vec2 {
        Vec2::new(x * self.scale_factor, y * self.scale_factor)
    }
}

/// Frame / animation driving data retained from the original ActiveInfo.
/// Interaction (selection / hover / drag) has been moved to dedicated resources in the new picking pipeline.
#[derive(Debug, Resource)]
pub(crate) struct ActivityControl {
    pub is_in_worker: bool,
    pub auto_animate: bool,
    pub remaining_frames: u32,
}

impl ActivityControl {
    pub fn new() -> Self {
        ActivityControl {
            is_in_worker: false,
            auto_animate: true,
            remaining_frames: 0,
        }
    }
}

// -------------------------------------------------------------------------------------------------
// New interaction / picking scaffolding (to be wired in subsequent patches)
// -------------------------------------------------------------------------------------------------

#[derive(Default, Debug, Clone, Copy)]
pub struct ButtonSnapshot {
    pub left: bool,
    pub right: bool,
    pub middle: bool,
}

#[derive(Default, Debug, Clone, Copy)]
pub struct ModifierSnapshot {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

#[derive(Resource, Debug, Default)]
pub struct PointerState {
    pub screen: Vec2,
    pub delta: Vec2,
    pub overlay_world: Option<Vec2>,
    pub world_ray: Option<Ray3d>,
    pub buttons: ButtonSnapshot,
    pub modifiers: ModifierSnapshot,
    pub just_pressed_left: bool,
    pub just_released_left: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct Hit2D {
    pub entity: Entity,
    pub z: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct Hit3D {
    pub entity: Entity,
    pub distance: f32,
}

#[derive(Resource, Debug, Default)]
pub struct PointerHits {
    pub overlay: Vec<Hit2D>,
    pub world3d: Vec<Hit3D>,
    pub primary: Option<Entity>,
}

#[derive(Resource, Debug, Default)]
pub struct SelectionState {
    pub selected: HashMap<Entity, ()>,
    pub hovered: HashMap<Entity, ()>,
    pub last_primary: Option<Entity>,
}

#[derive(Debug, Clone, Copy)]
pub enum DragKind {
    Overlay2D,
    World3D,
    Group,
    /// Scrubbing a slider's value: previewed through the store each frame,
    /// committed as one `Set` on release.
    SliderValue,
}

#[derive(Resource, Debug)]
pub struct DragState {
    pub target: Option<Entity>,
    pub kind: Option<DragKind>,
    pub grab_offset_2d: Vec2,
    pub plane_origin: Vec3,
    pub plane_normal: Vec3,
    pub grab_offset_world: Vec3,
}

impl Default for DragState {
    fn default() -> Self {
        Self {
            target: None,
            kind: None,
            grab_offset_2d: Vec2::ZERO,
            plane_origin: Vec3::ZERO,
            plane_normal: Vec3::Y,
            grab_offset_world: Vec3::ZERO,
        }
    }
}

// Marker for a composite vector group (single VelloScene acting as many shapes)
#[derive(Component, Debug)]
pub struct GroupAggregate {
    pub version: u32,
    pub shape_count: u32,
}

impl Default for GroupAggregate {
    fn default() -> Self {
        Self {
            version: 0,
            shape_count: 0,
        }
    }
}

pub(crate) fn close_bevy_window(mut app: Box<WorkerApp>) {
    let mut windows_state: SystemState<Query<(Entity, &mut Window)>> =
        SystemState::from_world(app.world_mut());
    // bevy 0.19: get_mut returns Result. Without the `?`-style unwrap this silently
    // compiles as `Result::iter`, which yields the Query rather than its rows.
    let Ok(windows) = windows_state.get_mut(app.world_mut()) else {
        return;
    };
    let entity = windows.iter().last().map(|(entity, _)| entity);
    if let Some(entity) = entity {
        app.world_mut()
            .write_message(WindowCloseRequested { window: entity });
        windows_state.apply(app.world_mut());
        app.update();
    }
    // Dropping the WorkerApp (and its App) releases the wgpu device/surfaces.
}
