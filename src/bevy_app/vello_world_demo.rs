//! A world-space vector layer, spawned purely for contrast with the screen-space
//! overlays in `overlay2d`/`ui_panels`/`timeline`.
//!
//! The difference is entirely in [`LayerSpace`](crate::vector::LayerSpace), which
//! the backend turns into a root `Transform`. Screen layers get a window-sized
//! correction applied every frame; this one does not, so:
//!
//! - its content is authored in world units around its own origin,
//! - `(0, 0)` is the centre of the backend camera's view, not the top-left corner,
//! - +Y points up, matching bevy rather than the DOM,
//! - it pans and zooms with the 2D camera, whereas the overlays stay pinned to the
//!   window no matter where that camera goes.
//!
//! The last point is the interesting one: it is what makes this the right choice
//! for anything that should feel anchored to the scene (annotations on a plot,
//! instrument overlays in a viewport) rather than to the screen.

use bevy::prelude::*;
use kurbo;
use peniko;

use crate::vector::{DisplayList, DisplayListRebuild, VectorLayer, order};

#[derive(Component)]
pub struct WorldSpaceDemoLayer;

pub fn setup_world_space_demo(mut commands: Commands) {
    commands.spawn((
        DisplayList::default(),
        // World space: positioned by this entity's own Transform, y-up.
        VectorLayer::world(order::WORLD_DEMO),
        WorldSpaceDemoLayer,
        // World origin == centre of the backend camera's view.
        Transform::default(),
    ));
}

/// Orbits a rounded square around the world origin and pulses its corner radius,
/// all in world units. Compare with `render_ui_panels`, which emits raw screen
/// pixels and relies on the screen-space correction transform instead.
pub fn animate_world_space_demo(
    time: Res<Time>,
    mut layers: Query<(&mut Transform, &mut DisplayList), With<WorldSpaceDemoLayer>>,
) {
    let Ok((mut transform, mut list)) = layers.single_mut() else {
        return;
    };

    let t = time.elapsed_secs();
    let pulse = t.sin().mul_add(0.5, 0.5);
    let translation = transform.translation;

    list.rebuild(|b| {
        // Authored around the entity's own origin, in world units.
        b.fill(
            kurbo::Affine::IDENTITY,
            peniko::Color::new([1.0, 0.45, 0.1, 1.0]),
            kurbo::RoundedRect::new(-90.0, -90.0, 90.0, 90.0, (pulse as f64) * 45.0),
        );

        // A stroked ring marking the orbit, so the world origin is visible on screen.
        b.stroke(
            kurbo::Affine::translate((
                -f64::from(translation.x),
                -f64::from(translation.y),
            )),
            kurbo::Stroke::new(2.0),
            peniko::Color::new([1.0, 0.45, 0.1, 0.35]),
            kurbo::Circle::new((0.0, 0.0), 220.0),
        );
    });

    // Orbit the world origin. Because there is no screen-space correction, this
    // motion is in world units and +Y is up.
    let angle = t * 0.6;
    transform.translation.x = 220.0 * angle.cos();
    transform.translation.y = 220.0 * angle.sin();
    transform.rotation = Quat::from_rotation_z(-std::f32::consts::TAU * pulse);
}
