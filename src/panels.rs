//! The canvas side of views (plans/specs/panels-and-session.md).
//!
//! A **view** is a window onto a composition: what it shows (`source`), how
//! (`kind`, its notation: `composition` draws a 2D or 3D composition,
//! `timeline` draws the timeline root's clips over time, `ui` a plain
//! background), and where. The shell lays views out as tabs and splits
//! (dockview) and decides which part of a view is HTML. Whatever part of a
//! visible view the canvas draws arrives here as a **panel**: the view's id,
//! its kind and source, and its rectangle (physical px, top-left origin of the
//! full window canvas), posted through `set_panel_viewport`. A hidden tab
//! posts nothing, so it costs nothing.
//!
//! Rust consumes these rects:
//! - a `composition` panel shows one composition (ECM §7.1): its 2D content
//!   drawn and picked inside the rect, or a 3D camera viewport on its layer
//!   (`document_bridge::panel_views` resolves which)
//! - vello panels (timeline, ui) draw in screen space clipped to their rect

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use kurbo;

/// The main viewer's view. It also draws 2D nodes outside every
/// composition (loose roots from older files).
pub const VIEWER_PANEL: &str = "viewer";
/// The kind of a panel that shows one composition.
pub const COMPOSITION_PANEL: &str = "composition";
pub const TIMELINE_PANEL: &str = "timeline";

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelRect {
    /// Physical px, top-left origin of the full window canvas.
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl PanelRect {
    pub fn center(&self) -> Vec2 {
        Vec2::new(self.x + self.w * 0.5, self.y + self.h * 0.5)
    }

    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.x && p.y >= self.y && p.x <= self.x + self.w && p.y <= self.y + self.h
    }

    pub fn to_kurbo(&self) -> kurbo::Rect {
        kurbo::Rect::new(
            self.x as f64,
            self.y as f64,
            (self.x + self.w) as f64,
            (self.y + self.h) as f64,
        )
    }
}

#[derive(Debug, Clone)]
pub struct Panel {
    /// The view's notation: `composition`, `timeline`, `ui`.
    pub kind: String,
    /// For a composition view, the composition it shows: a node id
    /// (`#4`), a composition name, or empty for the first one with the
    /// right dimensions.
    pub composition: String,
    pub rect: PanelRect,
}

#[derive(Resource, Debug, Default)]
pub struct Panels {
    map: HashMap<String, Panel>,
    /// Bumped on every upsert/remove so systems can cheaply react to layout changes.
    pub generation: u32,
}

impl Panels {
    pub fn upsert(&mut self, id: &str, kind: &str, composition: &str, rect: PanelRect) {
        self.map.insert(
            id.to_owned(),
            Panel {
                kind: kind.to_owned(),
                composition: composition.to_owned(),
                rect,
            },
        );
        self.generation = self.generation.wrapping_add(1);
    }

    pub fn remove(&mut self, id: &str) {
        if self.map.remove(id).is_some() {
            self.generation = self.generation.wrapping_add(1);
        }
    }

    pub fn get(&self, id: &str) -> Option<&Panel> {
        self.map.get(id)
    }

    pub fn rect(&self, id: &str) -> Option<PanelRect> {
        self.map.get(id).map(|p| p.rect)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Panel)> {
        self.map.iter()
    }
}

/// Map a window-space cursor position (physical px, top-left origin) into "overlay world"
/// coordinates: origin at the panel center, y-up. This is the coordinate space the 2D
/// overlay content is authored in (previously provided by a per-window Camera2d).
pub fn overlay_world_from_screen(rect: PanelRect, screen: Vec2) -> Vec2 {
    let c = rect.center();
    Vec2::new(screen.x - c.x, c.y - screen.y)
}

/// Kurbo affine mapping overlay-world coords (panel-center origin, y-up) to
/// screen-space vello coords (window top-left origin, y-down).
pub fn overlay_affine(rect: PanelRect) -> kurbo::Affine {
    let c = rect.center();
    kurbo::Affine::new([1.0, 0.0, 0.0, -1.0, c.x as f64, c.y as f64])
}

/// Document 2D space is y-down with its origin at the viewer panel's top-left
/// corner, one unit per physical pixel (plans/specs/document-spine.md §0). This maps
/// a window-space cursor position into it; `None` outside the panel.
pub fn document_from_screen(rect: PanelRect, screen: Vec2) -> Option<Vec2> {
    rect.contains(screen).then(|| Vec2::new(screen.x - rect.x, screen.y - rect.y))
}

/// Kurbo affine mapping document 2D space to screen-space vello coords.
pub fn document_affine(rect: PanelRect) -> kurbo::Affine {
    kurbo::Affine::translate((rect.x as f64, rect.y as f64))
}


/// A vector view that covers one named panel (plans/specs/view-textures.md): its
/// layers render into a texture the panel's size. While the panel does not
/// exist the view has no rectangle, and its layers draw nowhere.
#[derive(Component, Debug, Clone)]
pub struct PanelVectorView {
    pub panel: String,
}

pub fn sync_panel_vector_views(
    mut commands: Commands,
    panels: Res<Panels>,
    mut views: Query<(Entity, &PanelVectorView, Option<&mut crate::vector::VectorView>)>,
) {
    for (e, pv, view) in &mut views {
        match (panels.rect(&pv.panel), view) {
            (Some(r), view) => {
                let next = crate::vector::VectorView {
                    pos: UVec2::new(r.x.max(0.0) as u32, r.y.max(0.0) as u32),
                    size: UVec2::new(r.w.max(1.0) as u32, r.h.max(1.0) as u32),
                };
                match view {
                    Some(mut v) => {
                        if *v != next {
                            *v = next;
                        }
                    }
                    None => {
                        commands.entity(e).insert(next);
                    }
                }
            }
            (None, Some(_)) => {
                commands.entity(e).remove::<crate::vector::VectorView>();
            }
            (None, None) => {}
        }
    }
}
