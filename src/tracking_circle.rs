use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin};

use bevy::{
    color::palettes::basic::{AQUA, LIME, SILVER},
    prelude::*,
};

pub(crate) struct TrackingCircle;

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
    let circle = meshes.add(Circle::new(30.0));
    let color = Color::hsl(55.0, 0.95, 0.7);

    commands.spawn((
        Mesh2d(circle),
        MeshMaterial2d(materials.add(color)),
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));
}

fn update_circle_position(
    mut query: Query<&mut Transform, With<Mesh2d>>,
    mut cursor_moved_events: EventReader<CursorMoved>,
) {
    for mut transform in query.iter_mut() {
        // Update the position of the circle to follow the mouse cursor

        if !cursor_moved_events.is_empty() {
            if let Some(event) = cursor_moved_events.read().last() {
                transform.translation = Vec3::new(event.position.x, event.position.y, 0.0);
            }
        }
    }
}
