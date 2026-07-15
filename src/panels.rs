//! Panel registry: JS-owned layout rectangles mirrored into Bevy.
//!
//! The DOM is the layout engine. JS measures each panel placeholder and posts its
//! rectangle (physical pixels, top-left origin, full-window coordinates) through the
//! `set_panel_viewport` FFI. Rust consumes these rects:
//! - the `viewer` panel drives `MainCamera3D.viewport`
//! - vello panels (timeline, overlay) draw in screen space clipped to their rect

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy_vello::prelude::kurbo;

pub const VIEWER_PANEL: &str = "viewer";
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
    pub kind: String,
    pub rect: PanelRect,
}

#[derive(Resource, Debug, Default)]
pub struct Panels {
    map: HashMap<String, Panel>,
    /// Bumped on every upsert/remove so systems can cheaply react to layout changes.
    pub generation: u32,
}

impl Panels {
    pub fn upsert(&mut self, id: &str, kind: &str, rect: PanelRect) {
        self.map.insert(
            id.to_owned(),
            Panel {
                kind: kind.to_owned(),
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
