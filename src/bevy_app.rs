use crate::camera_controller::{CameraController, CameraControllerPlugin};
use crate::overlay::OverlayPlugin;
use crate::ray_pick::RayPickPlugin;
use crate::tracking_circle::TrackingCircle;
use crate::{ActiveInfo, WorkerApp};
use bevy::color::palettes::css::BLANCHED_ALMOND;
use bevy::color::palettes::tailwind::BLUE_400;

use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin, LogDiagnosticsPlugin};
use bevy::math::VectorSpace;
use bevy::render::camera::ScalingMode;
use bevy::{
    color::palettes::basic::{AQUA, LIME, SILVER},
    math::bounding::{Aabb3d, Bounded3d},
    prelude::*,
    render::{
        render_asset::RenderAssetUsages,
        render_resource::{Extent3d, TextureDimension, TextureFormat},
    },
};
use rand::Rng;
use std::f32::consts::PI;
use std::ops::Deref;

use bevy::math::{cubic_splines::*, vec2};

use bevy::render::view::RenderLayers;

use crate::asset_reader::WebAssetPlugin;

use bevy::input::mouse::MouseScrollUnit; // Ensure this is imported if used by AccumulatedCustomScroll
use bevy::window::CursorMoved; // Ensure this is imported for accumulate_cursor_delta_system

const MAX_HISTORY_LENGTH: usize = 200;

pub(crate) fn init_app() -> WorkerApp {
    let mut app = App::new();

    let mut default_plugins = DefaultPlugins.set(ImagePlugin::default_nearest());
    default_plugins = default_plugins.set(bevy::window::WindowPlugin {
        primary_window: Some(bevy::window::Window {
            present_mode: bevy::window::PresentMode::AutoNoVsync,
            ..default()
        }),
        ..default()
    });

    app.insert_resource(ClearColor(Color::srgb(0.97, 0.97, 0.97)));

    app.add_plugins((
        WebAssetPlugin::default(),
        default_plugins,
        RayPickPlugin,
        OverlayPlugin,
        TrackingCircle,
        FrameTimeDiagnosticsPlugin {
            max_history_length: MAX_HISTORY_LENGTH,
            smoothing_factor: 2.0 / (MAX_HISTORY_LENGTH as f64 + 1.0),
        },
        CameraControllerPlugin, // LogDiagnosticsPlugin::default(),
    ));

    app.init_resource::<AccumulatedCursorDelta>();
    app.init_resource::<AccumulatedScroll>();

    app.add_systems(Startup, setup)
        .add_systems(Update, (rotate, update_aabbes))
        .add_systems(
            PreUpdate,
            (
                accumulate_cursor_delta_system,
                accumulate_custom_scroll_system,
            ),
        ) // Added accumulate_custom_scroll_system
        .add_systems(PostUpdate, render_active_shapes);

    WorkerApp::new(app)
}

/// A marker component for our shapes so we can query them separately from the ground plane
#[derive(Component, Clone)]
enum Shape {
    Box(Cuboid),
    // Capsule(Capsule3d),
    // Torus(Torus),
    // Cylinder(Cylinder),
    // None,
}

#[derive(Component, Default)]
pub(crate) struct ActiveState {
    pub hover: bool,
    pub selected: bool,
}

#[derive(Component)]
pub(crate) struct MainCamera;

impl ActiveState {
    fn is_active(&self) -> bool {
        self.hover || self.selected
    }
}

#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct AccumulatedCursorDelta {
    // This is already pub
    pub delta: Vec2,
    last_position: Option<Vec2>,
}

#[derive(Resource, Debug, Clone, Copy)]
pub struct AccumulatedScroll {
    // This is already pub
    pub delta: Vec2,
    pub unit: MouseScrollUnit,
}

// Manually implement Default
impl Default for AccumulatedScroll {
    fn default() -> Self {
        Self {
            delta: Vec2::ZERO,
            unit: MouseScrollUnit::Line, // Or MouseScrollUnit::Pixel, choose a sensible default
        }
    }
}

const X_EXTENT: f32 = 13.0;

trait CurveColor: VectorSpace + Into<Color> + Send + Sync + 'static {}
impl<T: VectorSpace + Into<Color> + Send + Sync + 'static> CurveColor for T {}

#[derive(Debug, Component)]
struct Curve<T: CurveColor>(CubicCurve<T>);

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
) {
    let debug_material = materials.add(StandardMaterial {
        base_color_texture: Some(images.add(uv_debug_texture())),
        ..default()
    });

    let meshe_handles = [
        meshes.add(Cuboid::default()),
        meshes.add(Capsule3d::default().mesh().longitudes(16).latitudes(8)),
        meshes.add(
            Torus::default()
                .mesh()
                .major_resolution(8)
                .minor_resolution(6),
        ),
        meshes.add(Cylinder::default().mesh().resolution(3)),
        meshes.add(Capsule3d::default()),
        meshes.add(Cylinder::default()),
        meshes.add(Cuboid::default()),
        meshes.add(Sphere::default().mesh().ico(1).unwrap()),
    ];

    let shapes = [
        Shape::Box(Cuboid::from_size(Vec3::splat(1.1))),
        Shape::Box(Cuboid::from_size(Vec3::new(1., 2., 1.))),
        Shape::Box(Cuboid::from_size(Vec3::new(1.75, 0.52, 1.75))),
        Shape::Box(Cuboid::default()),
        Shape::Box(Cuboid::from_size(Vec3::new(1., 2., 1.))),
        Shape::Box(Cuboid::default()),
        Shape::Box(Cuboid::from_size(Vec3::splat(1.1))),
        Shape::Box(Cuboid::default()),
    ];

    let num_shapes = meshe_handles.len();
    let mut rng = rand::thread_rng();

    let grid_size_x = num_shapes;
    let grid_size_z = 5;
    let spacing = 3.0;
    let height = 1.5; // Height above ground plane

    for x in 0..grid_size_x {
        for z in 0..grid_size_z {
            let index = rng.gen_range(0..num_shapes);
            let mesh = meshe_handles[index].to_owned();
            let shape = shapes[index].to_owned();

            let transform = Transform::from_xyz(
                (x as f32 - (grid_size_x - 1) as f32 / 2.0) * spacing,
                height,
                (z as f32 - (grid_size_z - 1) as f32 / 2.0) * spacing,
            );

            commands.spawn((
                Mesh3d(mesh),
                MeshMaterial3d(debug_material.clone()),
                transform,
                shape,
                ActiveState::default(),
                // RenderLayers::layer(1),
            ));
        }
    }

    // this was my unfinished bezier curve experiment
    // let points = [[
    //     vec2(-1.0, -20.0),
    //     vec2(3.0, 2.0),
    //     vec2(5.0, 3.0),
    //     vec2(9.0, 8.0),
    // ]];

    // commands.spawn((
    //     Sprite::sized(Vec2::new(75., 75.)),
    //     Transform::from_xyz(0., 0.0, 0.),
    //     Curve(CubicBezier::new(points).to_curve().unwrap()),
    // ));

    commands.spawn((
        PointLight {
            shadows_enabled: true,
            intensity: 15_000_000.,
            range: 100.0,
            shadow_depth_bias: 0.2,
            ..default()
        },
        Transform::from_xyz(8.0, 9.0, 16.0),
        // RenderLayers::layer(1),
    ));

    commands.spawn((
        PointLight {
            shadows_enabled: true,
            intensity: 5_000_000.,
            range: 100.0,
            shadow_depth_bias: 0.2,
            ..default()
        },
        Transform::from_xyz(-8.0, 9.0, -10.0),
        // RenderLayers::layer(1),
    ));

    // ground plane
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(50.0, 50.0).subdivisions(10))),
        MeshMaterial3d(materials.add(Color::from(SILVER))),
        // Transform::IDENTITY.with_rotation(Quat::from_rotation_x(PI / 2.)),
        // RenderLayers::layer(1),
    ));

    commands.spawn((
        Camera3d::default(),
        Camera {
            // renders after / on top of the main camera
            order: 1,
            // don't clear the color while rendering this camera
            clear_color: ClearColorConfig::Default,
            ..default()
        },
        CameraController::default(),
        MainCamera,
        Projection::Perspective(PerspectiveProjection {
            fov: 60.0_f32.to_radians(),
            near: 0.1,
            far: 1000.0,
            ..default()
        }),
        // if -z is forward, then z: 18 is behind the origin, looking at the origin
        Transform::from_xyz(0.0, 18., 18.).looking_at(Vec3::new(0., 0., 0.), Vec3::Y),
        // RenderLayers::layer(1),
    ));

    commands.spawn((
        Camera2d,
        Camera {
            order: 2,
            clear_color: ClearColorConfig::None,
            ..default()
        },
        RenderLayers::layer(1),
    ));

    // commands.spawn(Sprite::from_image(
    //     asset_server.load("https://s3.johanhelsing.studio/dump/favicon.png"),
    // ));
}

fn rotate(
    app_info: Res<ActiveInfo>,
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

fn render_active_shapes(mut gizmos: Gizmos, query: Query<(&Shape, &Transform, &ActiveState)>) {
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
            } // Shape::Capsule(c) => {
              //     gizmos.primitive_3d(*c, translation, transform.rotation, color);
              // }
        }
    }
}

/// Creates a colorful test pattern
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

/// entity 的 aabb
#[derive(Component, Debug)]
pub struct CurrentVolume(Aabb3d);

impl Deref for CurrentVolume {
    type Target = Aabb3d;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// 更新 aabb
#[allow(clippy::type_complexity)]
fn update_aabbes(
    mut commands: Commands,
    mut config_store: ResMut<GizmoConfigStore>,
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

// System to populate AccumulatedCursorDelta (from previous suggestion)
fn accumulate_cursor_delta_system(
    mut cursor_moved_events: EventReader<CursorMoved>,
    mut accumulated_delta: ResMut<AccumulatedCursorDelta>,
) {
    accumulated_delta.delta = Vec2::ZERO;
    for event in cursor_moved_events.read() {
        if let Some(last_pos) = accumulated_delta.last_position {
            let current_delta = event.position - last_pos;
            accumulated_delta.delta += current_delta;
        }
        accumulated_delta.last_position = Some(event.position);
    }
}

// If you have a system for AccumulatedScroll, it would look like this:
fn accumulate_custom_scroll_system(
    mut scroll_events: EventReader<bevy::input::mouse::MouseWheel>, // Or your custom event
    mut accumulated_scroll: ResMut<AccumulatedScroll>,
) {
    accumulated_scroll.delta = Vec2::ZERO;
    // Logic to populate accumulated_scroll.delta and accumulated_scroll.unit
    // For example, if using Bevy's MouseWheel event:
    for event in scroll_events.read() {
        accumulated_scroll.delta += Vec2::new(event.x, event.y);
        accumulated_scroll.unit = event.unit; // This might overwrite if units differ in one frame
    }
}
