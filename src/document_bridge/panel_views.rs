//! Panels per composition (entity-composition-model.md §7.1).
//!
//! The shell posts panel rectangles with a composition spec (a node id, a
//! composition name, or nothing). Each frame [`resolve_panel_views`] turns
//! the composition panels into [`PanelViews`]: which composition each one
//! shows, whether it is 2D or 3D, and the render layer its 3D content and
//! camera use. Rendering, picking, dragging and the 3D cameras all read
//! this one resolution, so a panel never disagrees with itself about what
//! it shows.
//!
//! Render layer 0 holds the scenery every 3D panel shares (ground, lights)
//! and 3D nodes outside every composition. Each composition gets its own
//! layer the first time anything asks, and keeps it.

use super::DocumentStore;
use crate::panels::{COMPOSITION_PANEL, PanelRect, Panels, VIEWER_PANEL};
use bevy::prelude::*;
use iron_document::composition::{self, Dims};
use iron_document::facet::Name as DocName;
use iron_document::{Document, NodeId};
use std::collections::BTreeMap;

/// The composition a materialised node is drawn in (`None`: outside every
/// composition). Written by the reconciler with the node's other values.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct InComposition(pub Option<NodeId>);

/// What one composition panel shows.
#[derive(Debug, Clone, PartialEq)]
pub struct PanelView {
    pub panel: String,
    pub rect: PanelRect,
    /// `None` when the spec names nothing that exists (yet).
    pub composition: Option<NodeId>,
    pub dims: Dims,
    /// The render layer of the composition's 3D content (0 when none).
    pub layer: usize,
    /// The viewer panel also draws 2D nodes outside every composition.
    pub loose: bool,
    /// A 2D view's pan: document space's origin sits at the panel's
    /// top-left plus this offset, in window px (`ViewCameras`).
    pub pan: Vec2,
    /// A 2D view's zoom: window px per document px.
    pub zoom: f32,
}

impl PanelView {
    /// Document 2D space to window space, for this panel.
    pub fn affine(&self) -> kurbo::Affine {
        kurbo::Affine::translate(((self.rect.x + self.pan.x) as f64, (self.rect.y + self.pan.y) as f64)) * kurbo::Affine::scale(self.zoom as f64)
    }

    /// A window-space point in this panel's document space; `None` outside
    /// the panel.
    pub fn to_doc(&self, screen: Vec2) -> Option<Vec2> {
        self.rect.contains(screen).then(|| (Vec2::new(screen.x - self.rect.x - self.pan.x, screen.y - self.rect.y - self.pan.y)) / self.zoom)
    }

    /// Whether a node drawn in `composition` belongs in this panel.
    pub fn shows(&self, composition: Option<NodeId>) -> bool {
        match composition {
            Some(c) => self.composition == Some(c),
            None => self.loose,
        }
    }
}

/// Every composition panel, in panel-id order.
#[derive(Resource, Default, Debug, PartialEq)]
pub struct PanelViews(pub Vec<PanelView>);

impl PanelViews {
    pub fn get(&self, panel: &str) -> Option<&PanelView> {
        self.0.iter().find(|v| v.panel == panel)
    }

    /// The composition panel under a window-space point.
    pub fn at(&self, screen: Vec2) -> Option<&PanelView> {
        self.0.iter().find(|v| v.rect.contains(screen))
    }

    /// Panels showing `composition`.
    pub fn showing(&self, composition: NodeId) -> impl Iterator<Item = &PanelView> {
        self.0.iter().filter(move |v| v.composition == Some(composition))
    }
}

/// How each 2D view looks at its composition: a pan and a zoom, set by
/// scrolling, pinching, Space-drag, the zoom keys and "Show in"
/// (`bevy_app::select::navigate_views`). Presentation only, workspace
/// state (plans/specs/panels-and-session.md): not in the document, not undoable.
#[derive(Resource, Default, Debug)]
pub struct ViewCameras(pub bevy::platform::collections::HashMap<String, ViewCamera>);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewCamera {
    pub pan: Vec2,
    pub zoom: f32,
}

impl Default for ViewCamera {
    fn default() -> Self {
        ViewCamera { pan: Vec2::ZERO, zoom: 1.0 }
    }
}

/// Nodes to frame in every panel showing their composition, queued by the
/// `document_focus` FFI and served by `bevy_app::scene3d::apply_focus`.
#[derive(Resource, Default, Debug)]
pub struct FocusRequests(pub Vec<NodeId>);

/// Composition → render layer, assigned on first use and never reused.
#[derive(Resource, Debug)]
pub struct CompositionLayers {
    layers: BTreeMap<NodeId, usize>,
    next: usize,
}

impl Default for CompositionLayers {
    fn default() -> Self {
        CompositionLayers { layers: BTreeMap::new(), next: 1 }
    }
}

impl CompositionLayers {
    /// The layer for nodes drawn in `composition`; 0 outside every one.
    pub fn of(&mut self, composition: Option<NodeId>) -> usize {
        let Some(c) = composition else { return 0 };
        *self.layers.entry(c).or_insert_with(|| {
            let l = self.next;
            self.next += 1;
            l
        })
    }
}

/// Which composition a spec names: a node id, a composition's name, or
/// (empty) the first composition with matching dimensions — 2D unless the
/// document has only 3D ones.
fn resolve(doc: &Document, spec: &str) -> Option<NodeId> {
    let all = composition::compositions(doc);
    if spec.is_empty() {
        return all.iter().copied().find(|&c| composition::dims(doc, c) == Some(Dims::Two)).or_else(|| all.first().copied());
    }
    // Ids need their `#`: bare base 62 would read most names as ids.
    if spec.starts_with('#') {
        return spec.parse::<NodeId>().ok().filter(|id| all.contains(id));
    }
    all.into_iter().find(|&c| {
        doc.get::<DocName>(c).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(spec)
    })
}

/// Resolve every composition panel against the current document. Writes the
/// resource only when something differs, so its change detection means
/// "what a panel shows changed".
pub(super) fn resolve_panel_views(
    panels: Res<Panels>,
    store: Res<DocumentStore>,
    cameras: Res<ViewCameras>,
    mut layers: ResMut<CompositionLayers>,
    mut views: ResMut<PanelViews>,
) {
    let doc = store.0.document();
    let mut next: Vec<PanelView> = panels
        .iter()
        .filter(|(_, p)| p.kind == COMPOSITION_PANEL)
        .map(|(id, p)| {
            let composition = resolve(doc, &p.composition);
            let dims = composition.and_then(|c| composition::dims(doc, c)).unwrap_or(Dims::Two);
            PanelView {
                panel: id.clone(),
                rect: p.rect,
                composition,
                dims,
                layer: if dims == Dims::Three { layers.of(composition) } else { 0 },
                loose: id == VIEWER_PANEL,
                pan: cameras.0.get(id).map_or(Vec2::ZERO, |c| c.pan),
                zoom: cameras.0.get(id).map_or(1.0, |c| c.zoom),
            }
        })
        .collect();
    next.sort_by(|a, b| a.panel.cmp(&b.panel));
    if views.0 != next {
        views.0 = next;
    }
}
