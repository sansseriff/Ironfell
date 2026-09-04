//! Classic Vello backend, via `bevy_vello`.
//!
//! This module is the only place in the app that names a Vello type. Everything
//! upstream of it speaks [`DisplayList`]. Replacing it with a sparse-strips
//! backend means writing a sibling module, not editing drawing code.
//!
//! What it owns:
//! - the `VelloPlugin` and the full-window compositing camera;
//! - `VelloScene2d` components, attached to [`VectorLayer`] entities;
//! - the screen-pixel → camera-world root transform;
//! - replaying display lists into scenes, only when they changed.

use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::prelude::*;
use bevy::transform::TransformSystems;
use bevy::window::PrimaryWindow;
use bevy_vello::{VelloPlugin, prelude::*};

use super::backend::{BackendKind, LayerSpace, VectorLayer, backend_active};
use super::display_list::{Brush, DisplayList, DrawCmd, Shape};

/// Dispatch on the concrete shape type rather than converting to `BezPath`, so
/// backend fast paths for rectangles and circles stay reachable.
///
/// A macro rather than a function taking `&dyn kurbo::Shape`: that trait is not
/// dyn-compatible, and monomorphising per shape is what keeps the fast paths.
macro_rules! with_shape {
    ($shape:expr, |$s:ident| $body:expr) => {
        match $shape {
            Shape::Rect($s) => $body,
            Shape::RoundedRect($s) => $body,
            Shape::Circle($s) => $body,
            Shape::Line($s) => $body,
            Shape::Path($s) => $body,
        }
    };
}

/// The render layer the Vello canvas composites on. Layers other than this are
/// culled by `bevy_vello`'s extraction, so every scene must carry it.
const VELLO_LAYER: usize = 1;

/// The full-window camera that composites the Vello output.
///
/// Exposed so app-level composition (which camera owns the UI, for instance) can
/// attach to it without this module deciding app policy.
#[derive(Resource, Debug, Clone, Copy)]
pub struct ClassicBackendCamera(pub Entity);

/// Marks a scene whose commands are authored in absolute screen pixels.
#[derive(Component)]
struct ScreenSpaceScene;

pub struct ClassicVelloBackend;

impl Plugin for ClassicVelloBackend {
    fn build(&self, app: &mut App) {
        // `VelloPlugin` cannot be toggled at runtime, so it is always installed.
        // When the backend is inactive its scenes are empty and it composites a
        // cleared texture — cheap, but not free, which is worth remembering when
        // reading A/B numbers with both backends installed.
        app.add_plugins(VelloPlugin {
            canvas_render_layers: RenderLayers::layer(VELLO_LAYER),
            use_cpu: false,
            antialiasing: vello::AaConfig::Area,
        })
        .add_systems(Startup, spawn_backend_camera)
        .add_systems(
            PostUpdate,
            (
                attach_scenes,
                // Must precede propagation: `bevy_vello` extracts via
                // `GlobalTransform`, so a correction written afterwards is a
                // frame late.
                sync_screen_space_transforms.before(TransformSystems::Propagate),
                replay_display_lists,
            )
                .chain()
                .run_if(backend_active(BackendKind::ClassicVello)),
        );
    }
}

fn spawn_backend_camera(mut commands: Commands) {
    let entity = commands
        .spawn((
            Camera2d,
            bevy::render::view::Msaa::Off,
            Camera {
                order: 10,
                clear_color: ClearColorConfig::None,
                ..default()
            },
            RenderLayers::layer(VELLO_LAYER),
            VelloView,
            Name::new("Classic Vello Backend Camera"),
        ))
        .id();
    commands.insert_resource(ClassicBackendCamera(entity));
}

/// Give every new [`VectorLayer`] the components this backend needs.
fn attach_scenes(
    mut commands: Commands,
    layers: Query<(Entity, &VectorLayer), Without<VelloScene2d>>,
) {
    for (entity, layer) in &layers {
        let mut e = commands.entity(entity);
        e.insert((
            VelloScene2d::new(),
            // A Vello scene has no measurable bounds, so its `Aabb` stays
            // zero-sized and it would otherwise be frustum-culled.
            NoFrustumCulling,
            RenderLayers::layer(VELLO_LAYER),
        ));
        match layer.space {
            LayerSpace::Screen => {
                // z carries painter order; x/y are set every frame from the
                // window size by `sync_screen_space_transforms`.
                e.insert((
                    ScreenSpaceScene,
                    Transform::from_xyz(0.0, 0.0, layer.order as f32),
                ));
            }
            LayerSpace::World => {
                // World layers are positioned by whatever `Transform` the app
                // already gave them; only supply one if it is missing.
                e.insert_if_new(Transform::from_xyz(0.0, 0.0, layer.order as f32));
            }
        }
    }
}

/// Keep each screen-space scene's origin pinned to the window's top-left corner.
///
/// The camera sits at the centre of a +Y-up world, so that corner is
/// `(-w/2, +h/2)`. Scale stays at 1: `bevy_vello` already flips Y when building a
/// scene's affine, and flipping again here mirrors the content off-screen. `z` is
/// left alone because it carries painter order.
fn sync_screen_space_transforms(
    windows: Query<&Window, With<PrimaryWindow>>,
    mut scenes: Query<&mut Transform, With<ScreenSpaceScene>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let half_w = window.width() / 2.0;
    let half_h = window.height() / 2.0;

    for mut transform in &mut scenes {
        if transform.translation.x != -half_w || transform.translation.y != half_h {
            transform.translation.x = -half_w;
            transform.translation.y = half_h;
        }
        transform.scale.x = 1.0;
        transform.scale.y = 1.0;
    }
}

/// Re-encode only the layers whose commands actually changed.
fn replay_display_lists(mut layers: Query<(&DisplayList, &mut VelloScene2d), Changed<DisplayList>>) {
    for (list, mut scene) in &mut layers {
        scene.reset();
        for cmd in list.cmds() {
            match cmd {
                DrawCmd::Fill {
                    transform,
                    brush,
                    fill_rule,
                    shape,
                } => {
                    let Brush::Solid(color) = brush;
                    with_shape!(shape, |s| scene.fill(*fill_rule, *transform, *color, None, s));
                }
                DrawCmd::Stroke {
                    transform,
                    style,
                    brush,
                    shape,
                } => {
                    let Brush::Solid(color) = brush;
                    with_shape!(shape, |s| scene.stroke(style, *transform, *color, None, s));
                }
                DrawCmd::PushClip { transform, shape } => {
                    with_shape!(shape, |s| scene.push_layer(
                        peniko::Fill::NonZero,
                        peniko::Mix::Normal,
                        1.0,
                        *transform,
                        s,
                    ));
                }
                DrawCmd::PopLayer => scene.pop_layer(),
            }
        }
    }
}
