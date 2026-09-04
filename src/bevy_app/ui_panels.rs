//! Generic UI panel backgrounds.
//!
//! Every panel posted from JS with `kind == "ui"` gets a flat gray background drawn
//! into its rect. This is the placeholder substrate for the future in-scene vector UI
//! (widgets as entities, not windows).

use bevy::prelude::*;
use kurbo;
use peniko;

use crate::panels::Panels;
use crate::vector::{DisplayList, DisplayListRebuild, VectorLayer, order};

pub const UI_PANEL_KIND: &str = "ui";

#[derive(Component)]
pub struct UiPanelsLayer;

pub fn setup_ui_panels(mut commands: Commands) {
    commands.spawn((
        DisplayList::default(),
        VectorLayer::screen(order::UI_PANELS),
        UiPanelsLayer,
    ));
}

/// Rebuild the gray backgrounds. `rebuild` compares content, so running this
/// every frame costs a rebuild of a handful of rects and re-encodes nothing
/// unless the layout actually moved.
pub fn render_ui_panels(
    mut layers: Query<&mut DisplayList, With<UiPanelsLayer>>,
    panels: Res<Panels>,
) {
    let Ok(mut list) = layers.single_mut() else {
        return;
    };

    list.rebuild(|b| {
        for (_id, panel) in panels.iter() {
            if panel.kind != UI_PANEL_KIND {
                continue;
            }
            let r = panel.rect;
            b.fill(
                kurbo::Affine::IDENTITY,
                peniko::Color::new([0.35, 0.36, 0.38, 1.0]),
                kurbo::RoundedRect::new(
                    r.x as f64,
                    r.y as f64,
                    (r.x + r.w) as f64,
                    (r.y + r.h) as f64,
                    6.0,
                ),
            );
        }
    });
}
