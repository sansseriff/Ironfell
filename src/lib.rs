use bevy::{
    ecs::system::SystemState, platform_support::collections::HashMap, prelude::*,
    window::WindowCloseRequested,
};
use std::ops::{Deref, DerefMut};

mod web_ffi;
pub use web_ffi::*;

mod canvas_view;
use canvas_view::*;

mod ray_pick;

mod bevy_app;

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

#[derive(Debug, Resource)]
pub(crate) struct ActiveInfo {
    pub hover: HashMap<Entity, u64>,
    pub selection: HashMap<Entity, u64>,
    /// Object responding to drag
    pub drag: Entity,
    /// Last frame's drag position
    pub last_drag_pos: Vec2,
    /// Whether running in a worker
    pub is_in_worker: bool,
    /// Whether to automatically execute rotation animations for scene objects
    pub auto_animate: bool,
    /// Remaining frames
    ///
    /// When automatic frame animation is disabled, the scene will only be updated by mouse events.
    /// Since frame rendering needs to be driven by requestAnimationFrame to maintain synchronization
    /// with browser display refresh, mouse events will not directly call app.update(), but instead
    /// reset this counter of pending update frames
    pub remaining_frames: u32,
}

impl ActiveInfo {
    pub fn new() -> Self {
        ActiveInfo {
            hover: HashMap::default(),
            selection: HashMap::default(),
            drag: Entity::PLACEHOLDER,
            last_drag_pos: Vec2::ZERO,
            is_in_worker: false,
            auto_animate: true,
            remaining_frames: 0,
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
