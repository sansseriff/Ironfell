use bevy::{
    ecs::system::SystemState, platform::collections::HashMap, prelude::*,
    window::WindowCloseRequested,
};
use std::ops::{Deref, DerefMut};

// original web ffi module
mod web_ffi;
pub use web_ffi::*;

// ffi module for specific to reflection, inspector features
mod ffi_inspector_bridge;
pub use ffi_inspector_bridge::*;

// mod type_registry; // Disabled for now - used for streaming updates

mod canvas_view;
use canvas_view::*;

// ray_pick legacy module removed (superseded by new picking systems)

pub mod bevy_app; // expose init_app and related types
pub use bevy_app::*; // re-export init_app symbols

mod fps_overlay;

mod tracking_circle;

mod asset_reader; // kept private

// mod bevy_vello;
// use bevy_vello::*;

// use bevy_vello::{VelloPlugin, prelude::*, render::VelloRenderer};

// mod asset_loader;

// mod type_registry;

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
    pub fn new(app: App) -> Self {
        Self {
            app,
            window: Entity::PLACEHOLDER,
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
}

#[derive(Resource, Debug, Default)]
pub struct DragState {
    pub target: Option<Entity>,
    pub kind: Option<DragKind>,
    pub grab_offset_2d: Vec2,
    pub plane_origin: Vec3,
    pub plane_normal: Vec3,
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

pub(crate) fn close_bevy_window(mut app: Box<App>) {
    let mut windows_state: SystemState<Query<(Entity, &mut Window)>> =
        SystemState::from_world(app.world_mut());
    let windows = windows_state.get_mut(app.world_mut());
    let (entity, _window) = windows.iter().last().unwrap();
    app.world_mut()
        .send_event(WindowCloseRequested { window: entity });
    windows_state.apply(app.world_mut());

    app.update();
}
