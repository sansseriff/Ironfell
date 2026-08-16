//! Screen-space scaffolding for vello scenes.
//!
//! bevy_vello 0.13 removed the `VelloScreenSpace` marker. Scenes are now either
//! world entities (`VelloScene2d`, placed by a `Transform`) or bevy UI nodes
//! (`UiVelloScene`, placed by the taffy layout engine). This app's overlays are
//! neither laid out by taffy nor authored in world units: their rects arrive from
//! JS as absolute screen pixels, so they stay `VelloScene2d` and get a root
//! `Transform` that maps screen pixels onto the vello camera's world.
//!
//! Only the *origin* needs correcting, not the Y direction. bevy_vello already
//! flips Y when building a scene's affine (`model_matrix.w_axis.y *= -1.0`, "Flip
//! Y-axis to match Vello's y-down coordinate space"), so content inside a scene is
//! authored y-down exactly like screen pixels. Applying a second flip here — e.g. a
//! negative `Transform.scale.y` — cancels that out into a mirror and throws the
//! content off-screen.
//!
//! So the entity's world position simply places the content origin at the window's
//! top-left corner, and y-down content runs correctly downward from there. This
//! keeps every drawing system free to keep emitting raw screen pixels.
//!
//! Window units are physical pixels here: `canvas_view` pins the window's scale
//! factor to 1.0 and sets the resolution from the canvas' physical size, which
//! matches both the JS-supplied panel rects and the `physical_viewport_size` that
//! bevy_vello uses to build its pixel matrix.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

/// Marks a [`VelloScene2d`](bevy_vello::prelude::VelloScene2d) whose contents are
/// authored in absolute screen pixels rather than world units.
#[derive(Component)]
pub struct ScreenSpaceScene;

/// Keeps every screen-space scene's root transform in step with the window size.
///
/// Places the scene's content origin at the window's top-left corner: the camera
/// sits at the centre of a +Y-up world, so that corner is `(-w/2, +h/2)`. Scale is
/// held at 1 — see the module docs for why flipping Y here is wrong. `z` is
/// deliberately left alone so callers can still use it to order overlays.
pub fn sync_screen_space_transforms(
    windows: Query<&Window, With<PrimaryWindow>>,
    mut scenes: Query<&mut Transform, With<ScreenSpaceScene>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let half_w = window.width() / 2.0;
    let half_h = window.height() / 2.0;

    for mut transform in &mut scenes {
        transform.translation.x = -half_w;
        transform.translation.y = half_h;
        transform.scale.x = 1.0;
        transform.scale.y = 1.0;
    }
}
