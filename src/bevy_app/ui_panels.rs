//! Generic vello-drawn UI panels.
//!
//! Every panel posted from JS with `kind == "ui"` gets a flat gray background drawn
//! into its rect. This is the placeholder substrate for the future in-scene vello UI
//! (widgets as entities, not windows).

use bevy::prelude::*;
use bevy::render::view::RenderLayers;
use bevy_vello::prelude::*;

use crate::panels::Panels;

pub const UI_PANEL_KIND: &str = "ui";

#[derive(Component)]
pub struct UiPanelsScene;

pub fn setup_ui_panels(mut commands: Commands) {
    commands.spawn((
        VelloScene::new(),
        VelloScreenSpace,
        RenderLayers::layer(1),
        UiPanelsScene,
    ));
}

/// Redraw the gray backgrounds whenever panel layout changes (rects are static
/// between layout changes, so the encoded scene is reused frame to frame).
pub fn render_ui_panels(
    mut q_scene: Query<&mut VelloScene, With<UiPanelsScene>>,
    panels: Res<Panels>,
) {
    if !panels.is_changed() {
        return;
    }
    let Ok(mut scene) = q_scene.single_mut() else {
        return;
    };
    scene.reset();

    for (_id, panel) in panels.iter() {
        if panel.kind != UI_PANEL_KIND {
            continue;
        }
        let r = panel.rect;
        scene.fill(
            peniko::Fill::NonZero,
            kurbo::Affine::IDENTITY,
            peniko::Color::new([0.35, 0.36, 0.38, 1.0]),
            None,
            &kurbo::RoundedRect::new(
                r.x as f64,
                r.y as f64,
                (r.x + r.w) as f64,
                (r.y + r.h) as f64,
                6.0,
            ),
        );
    }
}
