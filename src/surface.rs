//! Register a host-owned surface with Bevy, before adding RenderPlugin.
use bevy::{prelude::*, window::{PresentMode, PrimaryWindow, RawHandleWrapper, RawHandleWrapperHolder, WindowCreated}};
use std::sync::{Arc,Mutex};

pub fn spawn_external_window(app: &mut App, handle: RawHandleWrapper, width: u32, height: u32, present_mode: PresentMode) -> Entity {
    let mut window=Window { title: "Ironfell".into(), present_mode, ..default() };
    // The engine and panel compositor consistently use drawable pixels.
    window.resolution.set_scale_factor(1.0);
    window.resolution.set(width as f32,height as f32);
    app.world_mut().spawn((window,PrimaryWindow,handle.clone(),RawHandleWrapperHolder(Arc::new(Mutex::new(Some(handle)))))).id()
}

pub fn announce_window(app: &mut App, window: Entity) {
    app.world_mut().write_message(WindowCreated { window });
}
