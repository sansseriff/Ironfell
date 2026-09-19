use crate::camera_controller::CameraController;
use bevy::math::bounding::{Aabb3d, Bounded3d};
use bevy::prelude::*;
use bevy::camera::visibility::RenderLayers;
use std::ops::Deref;
use bevy::core_pipeline::tonemapping::Tonemapping;

// Marker for 3D main camera
#[derive(Component)]
pub(crate) struct MainCamera3D;

/// A marker component for our shapes so we can query them separately from the ground plane
#[derive(Component, Clone)]
pub(crate) enum Shape {
    Box(Cuboid),
}

#[derive(Component, Default)]
pub(crate) struct ActiveState {
    pub hover: bool,
    pub selected: bool,
}
impl ActiveState {
    pub(crate) fn is_active(&self) -> bool {
        self.hover || self.selected
    }
}

#[derive(Component, Debug)]
pub(crate) struct Despawnable;

#[derive(Component, Debug)]
pub(crate) struct CurrentVolume(pub Aabb3d);
impl Deref for CurrentVolume {
    type Target = Aabb3d;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Lights, ground and the viewer camera. Content (the torus) is a document
/// node, materialised by `document_bridge`, not spawned here.
pub(crate) fn setup_3d_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Lights
    commands.spawn((
        PointLight {
            shadow_maps_enabled: false,
            intensity: 07_000_000.,
            range: 100.0,
            ..default()
        },
        Transform::from_xyz(8.0, 9.0, 16.0),
    ));
    commands.spawn((
        PointLight {
            shadow_maps_enabled: false,
            intensity: 2_000_000.,
            range: 100.0,
            ..default()
        },
        Transform::from_xyz(-8.0, 9.0, -10.0),
    ));

    // Ground
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(50.0, 50.0).subdivisions(10))),
        MeshMaterial3d(materials.add(Color::srgb(0.75, 0.75, 0.75))),
    ));

    // Camera renders into the "viewer" panel's viewport rect; it stays inactive
    // until JS posts that panel (see apply_viewer_viewport in bevy_app/mod.rs).
    let camera = Camera {
        order: 0,
        clear_color: ClearColorConfig::Default,
        is_active: false,
        ..default()
    };
    commands.spawn((
        Camera3d::default(),
        bevy::render::view::Msaa::Off,
        Tonemapping::BlenderFilmic,
        camera,
        CameraController::default(),
        MainCamera3D,
        RenderLayers::layer(0),
        Projection::Perspective(PerspectiveProjection {
            fov: 60.0_f32.to_radians(),
            near: 0.1,
            far: 1000.0,
            ..default()
        }),
        Transform::from_xyz(0.0, 18., 18.).looking_at(Vec3::new(0., 0., 0.), Vec3::Y),
    ));
}

pub(crate) fn render_active_shapes(
    mut gizmos: Gizmos,
    query: Query<(&Shape, &Transform, &ActiveState)>,
) {
    use bevy::color::palettes::css::BLANCHED_ALMOND;
    use bevy::color::palettes::tailwind::BLUE_400;
    for (shape, transform, active_state) in query.iter() {
        if !active_state.is_active() {
            continue;
        }
        let color = if active_state.selected {
            BLUE_400
        } else {
            BLANCHED_ALMOND
        };
        let translation = transform.translation.xyz();
        match shape {
            Shape::Box(cuboid) => {
                gizmos.primitive_3d(
                    cuboid,
                    Isometry3d::new(translation, transform.rotation),
                    color,
                );
            }
        }
    }
}

pub(crate) fn update_aabbes(
    mut commands: Commands,
    mut config_store: ResMut<bevy::gizmos::config::GizmoConfigStore>,
    query: Query<(Entity, &Shape, &Transform), Or<(Changed<Shape>, Changed<Transform>)>>,
) {
    for (_, config, _) in config_store.iter_mut() {
        config.line.width = 3.;
    }
    for (entity, shape, transform) in query.iter() {
        let translation = transform.translation;
        let rotation = transform.rotation;
        let aabb = match shape {
            Shape::Box(b) => b.aabb_3d(Isometry3d::new(translation, rotation)),
        };
        commands.entity(entity).insert(CurrentVolume(aabb));
    }
}
