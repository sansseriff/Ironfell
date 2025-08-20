use crate::ActivityControl;
use crate::camera_controller::CameraController;
use bevy::math::bounding::{Aabb3d, Bounded3d};
use bevy::prelude::*;
use bevy::render::view::RenderLayers;
use bevy::render::{
    render_asset::RenderAssetUsages,
    render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use std::ops::Deref;

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

pub(crate) fn setup_3d_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    _asset_server: Res<AssetServer>,
) {
    let debug_material = materials.add(StandardMaterial {
        base_color_texture: Some(images.add(uv_debug_texture())),
        ..default()
    });

    let meshe_handles = [meshes.add(
        Torus::default()
            .mesh()
            .major_resolution(8)
            .minor_resolution(6),
    )];
    let shape = Shape::Box(Cuboid::from_size(Vec3::new(1.75, 0.52, 1.75)));

    commands.spawn((
        Mesh3d(meshe_handles[0].to_owned()),
        MeshMaterial3d(debug_material.clone()),
        Transform::from_xyz(0.0, 1.5, 0.0),
        shape,
        ActiveState::default(),
        RenderLayers::layer(0),
    ));

    // Lights
    commands.spawn((
        PointLight {
            shadows_enabled: false,
            intensity: 15_000_000.,
            range: 100.0,
            // shadow_depth_bias: 0.2,
            ..default()
        },
        Transform::from_xyz(8.0, 9.0, 16.0),
    ));
    commands.spawn((
        PointLight {
            shadows_enabled: false,
            intensity: 5_000_000.,
            range: 100.0,
            // shadow_depth_bias: 0.2,
            ..default()
        },
        Transform::from_xyz(-8.0, 9.0, -10.0),
    ));

    // Ground
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(50.0, 50.0).subdivisions(10))),
        MeshMaterial3d(materials.add(Color::srgb(0.75, 0.75, 0.75))),
    ));

    // Camera
    commands.spawn((
        Camera3d::default(),
        Camera {
            order: 0,
            clear_color: ClearColorConfig::Default,
            ..default()
        },
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

pub(crate) fn rotate_3d_shapes(
    app_info: Res<ActivityControl>,
    mut query: Query<&mut Transform, With<Shape>>,
    time: Res<Time>,
) {
    if !app_info.auto_animate {
        return;
    }
    for mut transform in &mut query {
        transform.rotate_y(time.delta_secs() / 2.);
    }
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

fn uv_debug_texture() -> Image {
    const TEXTURE_SIZE: usize = 8;
    let mut palette: [u8; 32] = [
        255, 102, 159, 255, 255, 159, 102, 255, 236, 255, 102, 255, 121, 255, 102, 255, 102, 255,
        198, 255, 102, 198, 255, 255, 121, 102, 255, 255, 236, 102, 255, 255,
    ];
    let mut texture_data = [0; TEXTURE_SIZE * TEXTURE_SIZE * 4];
    for y in 0..TEXTURE_SIZE {
        let offset = TEXTURE_SIZE * y * 4;
        texture_data[offset..(offset + TEXTURE_SIZE * 4)].copy_from_slice(&palette);
        palette.rotate_right(4);
    }
    Image::new_fill(
        Extent3d {
            width: TEXTURE_SIZE as u32,
            height: TEXTURE_SIZE as u32,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        &texture_data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}
