//! A world-space vello scene, spawned purely for contrast with the screen-space
//! overlays in `overlay2d`/`ui_panels`/`timeline`.
//!
//! The difference is entirely in the root `Transform`, not the component type —
//! both this and the overlays are `VelloScene2d`. The overlays carry a
//! [`ScreenSpaceScene`](super::screen_space::ScreenSpaceScene) marker and get a
//! window-sized correction applied every frame; this one does not, so:
//!
//! - its content is authored in world units around its own origin,
//! - `(0, 0)` is the centre of the vello camera's view, not the top-left corner,
//! - +Y points up, matching bevy rather than the DOM,
//! - it pans and zooms with the 2D camera, whereas the overlays stay pinned to the
//!   window no matter where that camera goes.
//!
//! The last point is the interesting one: it is what makes this the right choice
//! for anything that should feel anchored to the scene (annotations on a plot,
//! instrument overlays in a viewport) rather than to the screen.

use bevy::camera::visibility::{NoFrustumCulling, RenderLayers};
use bevy::prelude::*;
use bevy_vello::prelude::*;

#[derive(Component)]
pub struct WorldSpaceDemoScene;

pub fn setup_world_space_demo(mut commands: Commands) {
    commands.spawn((
        VelloScene2d::new(),
        WorldSpaceDemoScene,
        // A `vello::Scene` cannot be measured, so `Aabb` stays default (zero-sized)
        // and the scene would be frustum-culled without this.
        NoFrustumCulling,
        // Must match the vello camera's layer, same as every other vello scene here.
        RenderLayers::layer(1),
        // World origin == centre of the vello camera's view.
        Transform::default(),
    ));
}

/// Orbits a rounded square around the world origin and pulses its corner radius,
/// all in world units. Compare with `render_ui_panels`, which emits raw screen
/// pixels and relies on the screen-space correction transform instead.
pub fn animate_world_space_demo(
    time: Res<Time>,
    mut scenes: Query<(&mut Transform, &mut VelloScene2d), With<WorldSpaceDemoScene>>,
) {
    let Ok((mut transform, mut scene)) = scenes.single_mut() else {
        return;
    };

    scene.reset();

    let t = time.elapsed_secs();
    let pulse = t.sin().mul_add(0.5, 0.5);

    // Authored around the entity's own origin, in world units.
    scene.fill(
        peniko::Fill::NonZero,
        kurbo::Affine::IDENTITY,
        peniko::Color::new([1.0, 0.45, 0.1, 1.0]),
        None,
        &kurbo::RoundedRect::new(-90.0, -90.0, 90.0, 90.0, (pulse as f64) * 45.0),
    );

    // A stroked ring marking the orbit, so the world origin is visible on screen.
    scene.stroke(
        &kurbo::Stroke::new(2.0),
        kurbo::Affine::translate((
            -f64::from(transform.translation.x),
            -f64::from(transform.translation.y),
        )),
        peniko::Color::new([1.0, 0.45, 0.1, 0.35]),
        None,
        &kurbo::Circle::new((0.0, 0.0), 220.0),
    );

    // Orbit the world origin. Because there is no screen-space correction, this
    // motion is in world units and +Y is up.
    let angle = t * 0.6;
    transform.translation.x = 220.0 * angle.cos();
    transform.translation.y = 220.0 * angle.sin();
    transform.rotation = Quat::from_rotation_z(-std::f32::consts::TAU * pulse);
}
