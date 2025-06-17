use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin};

use bevy::{
    color::palettes::basic::{AQUA, LIME, SILVER},
    prelude::*,
};

use bevy::render::view::RenderLayers;

pub(crate) struct TrackingCircle;

#[derive(Component)]
struct MyCircle;

impl Plugin for TrackingCircle {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, add_circle)
            .add_systems(Update, update_circle_position);
    }
}

fn add_circle(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ColorMaterial>>,
) {
    let circle = meshes.add(Annulus::new(46.0, 50.0));
    let color = Color::hsla(198.0, 1.0, 0.14, 0.3);

    commands.spawn((
        Mesh2d(circle),
        MyCircle,
        MeshMaterial2d(materials.add(color)),
        Transform::from_xyz(0.0, 0.0, 0.0),
        RenderLayers::layer(1),
    ));
}

fn update_circle_position(
    mut query: Query<&mut Transform, With<MyCircle>>,
    mut cursor_moved_events: EventReader<CursorMoved>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera2d>>,
) {
    if !cursor_moved_events.is_empty() {
        let (camera, camera_transform) = cameras.single().unwrap();
        for (mut circle_transform) in query.iter_mut() {
            if let Some(event) = cursor_moved_events.read().last() {
                let Ok(point) = camera.viewport_to_world_2d(camera_transform, event.position)
                else {
                    return;
                };
                circle_transform.translation = Vec3::new(point.x, point.y, 0.0);
            }
        }
    }
}
