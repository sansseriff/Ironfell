use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin};

use bevy::{
    color::palettes::basic::{AQUA, LIME, SILVER},
    prelude::*,
};

use crate::bevy_app::MainCamera;

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
    let circle = meshes.add(Circle::new(30.0));
    let color = Color::hsl(55.0, 0.95, 0.7);

    commands.spawn((
        Mesh2d(circle),
        MyCircle,
        MeshMaterial2d(materials.add(color)),
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));
}

fn update_circle_position(
    mut query: Query<&mut Transform, With<MyCircle>>,
    mut cursor_moved_events: EventReader<CursorMoved>,
    cameras: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
) {
    // Update the position of the circle to follow the mouse cursor

    // let mut viewport_pos = cursor_pos_screen;
    // let mut viewport_pos = cursor_pos_screen;

    if !cursor_moved_events.is_empty() {
        let (camera, transform) = cameras.single().unwrap();
        // let mut circle_transform = query.single_mut().unwrap();

        for (mut circle_transform) in query.iter_mut() {
            if let Some(event) = cursor_moved_events.read().last() {
                // if let Some(viewport) = &camera.viewport {
                //     circle_transform -= viewport.physical_position.as_vec2();
                // }

                if let Some(viewport) = &camera.viewport {
                    let viewport_offset_x = viewport.physical_position.as_vec2().x;
                    let viewport_offset_y = viewport.physical_position.as_vec2().y;
                    circle_transform.translation =
                        Vec3::new(event.position.x, -event.position.y, 0.0);
                }

                // circle_transform.translation = Vec3::new(event.position.x - event.delta.unwrap().x, -event.position.y, 0.0);
            }
        }
    }
}
