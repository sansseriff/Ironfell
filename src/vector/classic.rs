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

use super::backend::{LayerSpace, VectorLayer};
use super::composite::{VECTOR_LAYER, VectorCamera};
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

/// Marks a scene whose commands are authored in absolute screen pixels.
#[derive(Component)]
struct ScreenSpaceScene;

pub struct ClassicVelloBackend;

impl Plugin for ClassicVelloBackend {
    fn build(&self, app: &mut App) {
        // Installing `VelloPlugin` eagerly constructs Vello's renderer and
        // compute pipelines in `Plugin::finish`. The normal Hybrid path therefore
        // does not add this plugin at all; only the explicit classic variant
        // reaches this build method.
        app.add_plugins(VelloPlugin {
            canvas_render_layers: RenderLayers::layer(VECTOR_LAYER),
            use_cpu: false,
            antialiasing: vello::AaConfig::Area,
        })
        // PostStartup: the shared camera is spawned with `Commands` in Startup,
        // so the resource naming it does not exist until those are applied.
        .add_systems(PostStartup, mark_vello_view)
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
                .chain(),
        );
    }
}

/// Tell `bevy_vello` which camera to composite into.
///
/// `VelloView` is a `bevy_vello` type, so attaching it is this backend's job
/// even though the camera itself is shared.
fn mark_vello_view(mut commands: Commands, camera: Res<VectorCamera>) {
    commands.entity(camera.0).insert(VelloView);
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
            RenderLayers::layer(VECTOR_LAYER),
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
        // Every write here goes through `Mut<Transform>`, so an unconditional
        // assignment marks the component changed *every frame* and drags the
        // whole entity through transform propagation for no reason. Compare
        // first: these values are constant in the steady state.
        if transform.translation.x != -half_w || transform.translation.y != half_h {
            transform.translation.x = -half_w;
            transform.translation.y = half_h;
        }
        if transform.scale.x != 1.0 || transform.scale.y != 1.0 {
            transform.scale.x = 1.0;
            transform.scale.y = 1.0;
        }
    }
}

/// Re-encode only the layers whose commands actually changed.
fn replay_display_lists(
    mut layers: Query<(Ref<'_, DisplayList>, &mut VelloScene2d)>,
) {
    for (list, mut scene) in &mut layers {
        if !list.is_changed() {
            continue;
        }
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
