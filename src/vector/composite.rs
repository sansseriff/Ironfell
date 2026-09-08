//! The shared compositing view that every vector backend draws into.
//!
//! This lives outside any backend so app-level composition does not need to know
//! which renderer owns the current output.

use bevy::camera::visibility::RenderLayers;
use bevy::prelude::*;

/// The render layer every vector backend composites on.
///
/// Backends must tag both their output and their camera with this, or Bevy's
/// visibility culling drops the output before it reaches the screen.
pub const VECTOR_LAYER: usize = 1;

/// The full-window camera that vector output is composited into.
///
/// Published as a resource so app-level composition — which camera hosts
/// `bevy_ui`, for instance — can attach to it without this module deciding app
/// policy, and so backends can find the view they render for.
#[derive(Resource, Debug, Clone, Copy)]
pub struct VectorCamera(pub Entity);

pub(super) struct CompositePlugin;

impl Plugin for CompositePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_vector_camera);
    }
}

fn spawn_vector_camera(mut commands: Commands) {
    let entity = commands
        .spawn((
            Camera2d,
            bevy::render::view::Msaa::Off,
            Camera {
                // Above the background camera (-10) and the 3D viewer (0), so
                // vector output lands on top of both.
                order: 10,
                clear_color: ClearColorConfig::None,
                ..default()
            },
            RenderLayers::layer(VECTOR_LAYER),
            Name::new("Vector Composite Camera"),
        ))
        .id();
    commands.insert_resource(VectorCamera(entity));
}
