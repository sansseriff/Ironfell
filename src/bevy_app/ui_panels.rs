//! Generic UI panel backgrounds.
//!
//! Every panel posted from JS with `kind == "ui"` gets a flat gray background drawn
//! into its rect. This is the placeholder substrate for the future in-scene vector UI
//! (widgets as entities, not windows). Each panel is its own vector view
//! (plans/view-textures.md), so it renders into a texture its own size.

use bevy::prelude::*;
use kurbo;
use peniko;

use crate::panels::{PanelVectorView, Panels};
use crate::vector::{DisplayList, DisplayListRebuild, VectorLayer, order};

pub const UI_PANEL_KIND: &str = "ui";

/// The background layer of one UI panel, which is also its view.
#[derive(Component)]
pub struct UiPanelLayer {
    panel: String,
}

/// Keep one background layer per UI panel and rebuild each. `rebuild`
/// compares content, so running this every frame re-encodes nothing unless
/// the layout actually moved.
pub fn render_ui_panels(
    mut commands: Commands,
    panels: Res<Panels>,
    mut layers: Query<(Entity, &UiPanelLayer, &mut DisplayList)>,
) {
    for (e, layer, _) in &layers {
        if panels.get(&layer.panel).is_none_or(|p| p.kind != UI_PANEL_KIND) {
            commands.entity(e).despawn();
        }
    }
    for (id, panel) in panels.iter() {
        if panel.kind != UI_PANEL_KIND {
            continue;
        }
        let r = panel.rect;
        match layers.iter_mut().find(|(_, l, _)| &l.panel == id) {
            Some((_, _, mut list)) => {
                list.rebuild(|b| {
                    b.fill(
                        kurbo::Affine::IDENTITY,
                        peniko::Color::new([0.35, 0.36, 0.38, 1.0]),
                        kurbo::RoundedRect::new(r.x as f64, r.y as f64, (r.x + r.w) as f64, (r.y + r.h) as f64, 6.0),
                    );
                });
            }
            None => {
                let e = commands.spawn((DisplayList::default(), PanelVectorView { panel: id.clone() }, UiPanelLayer { panel: id.clone() })).id();
                commands.entity(e).insert(VectorLayer::screen(order::UI_PANELS).in_view(e));
            }
        }
    }
}
