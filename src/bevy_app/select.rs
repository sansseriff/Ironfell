//! The select tool (roadmap stage E): Figma's selection, drag and transform
//! semantics over the document's compositions (plans/specs/select-tool.md).
//!
//! Every hit carries its *chain*: the nodes from the panel composition's top
//! level down to the hit node, through groups and embeds. A click selects
//! one node along it: the outermost one whose container is not *open*. A
//! container is open while something inside it is selected (so a sibling of
//! the selection is one click away), and a top-level embed is always open,
//! like a top-level frame in Figma: its contents select directly and its
//! background selects the embed. Cmd/Ctrl-click selects the hit node itself;
//! a double-click enters one level; Shift adds or removes.
//!
//! Pressing a selected node, or inside the selection's box, drags the whole
//! selection through `Store::begin_drag`. The box's corners and edges resize
//! (Shift keeps the aspect, Alt resizes about the centre), just outside a
//! corner rotates (Shift snaps to 15°). Every gesture previews through the
//! store and closes as one transaction; none of the box is document state.
//!
//! Gestures this tool does not draw a box for keep their own paths in
//! `interaction`: dragging meshes in 3D, content on planes in 3D, sliders.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use iron_document::composition::{self, Dims};
use iron_document::copy::{self, Fragment};
use iron_document::{Actor, DragHandle, DragMode, Handle, LeafPath, NodeId, Op, Store, TransactionInput, Value};
use bevy::ecs::system::SystemParam;
use kurbo::{Affine, ParamCurve as _, Point, Rect, Shape as _};

use crate::bevy_app::interaction::{DragCameras, PendingCommit, drag_point, intersect_ray_plane};
use crate::document_bridge::{Doc2d, DocumentPanelLayer, DocumentStore, GestureRequest, GestureRequests, NodeMap, PanelView, PanelViews, PendingTransactions, Provenance, Shape2d, nest};
use crate::vector::{DisplayList, DisplayListRebuild, Overlay, VectorLayer, VectorView, order};

/// A node as reached in one panel: the chain from the panel composition's
/// top level down to it, through every group and embed on the way.
#[derive(Clone, Debug, PartialEq)]
pub struct Reach {
    pub panel: String,
    pub chain: Vec<NodeId>,
}

impl Reach {
    pub fn node(&self) -> NodeId {
        *self.chain.last().expect("a chain is never empty")
    }
}

/// Modifier keys as the shell last reported them (bits below): the browser
/// knows Alt and Cmd, which never arrive as keys.
#[derive(Resource, Default)]
pub struct ShellModifiers(pub u8);

pub const MOD_SHIFT: u8 = 1;
pub const MOD_CTRL: u8 = 2;
pub const MOD_ALT: u8 = 4;
pub const MOD_META: u8 = 8;

/// Tool shortcuts from the shell, as keys with their modifier bits:
/// Escape, Enter, Tab, arrows, Delete/Backspace, and `a` (with Cmd/Ctrl).
#[derive(Resource, Default)]
pub struct ToolKeys(pub Vec<(String, u8)>);

/// Clipboard traffic (plans/specs/clipboard-and-assets.md). The system
/// clipboard belongs to the shell: Cmd+C answers it with the fragment's
/// text (`send_clipboard_from_worker`), and a paste event hands text back
/// here. Nothing is kept on this side but the last cut.
#[derive(Resource, Default)]
pub struct Clipboard {
    /// Text from paste events, placed on the next frame.
    pub pastes: Vec<String>,
    /// The last cut's text, until it is pasted once: that first paste moves
    /// what was cut (figures relink to their entities); later ones copy.
    cut: Option<String>,
}

/// Which tool the pointer drives (Figma's keys: V select, P pen, R
/// rectangle, O ellipse).
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ActiveTool {
    #[default]
    Select,
    Pen,
    Rect,
    Ellipse,
}

impl ActiveTool {
    pub fn name(self) -> &'static str {
        match self {
            ActiveTool::Select => "select",
            ActiveTool::Pen => "pen",
            ActiveTool::Rect => "rect",
            ActiveTool::Ellipse => "ellipse",
        }
    }
}

/// Window px the pointer may travel and still click.
const SLOP: f32 = 3.0;
/// Seconds and window px between two clicks that make a double-click.
const DOUBLE_CLICK: (f64, f32) = (0.4, 5.0);
/// Grip sizes, window px: a corner's reach, an edge's, how far outside a
/// corner rotation reaches, and the drawn corner square.
const CORNER_HIT: f64 = 8.0;
const EDGE_HIT: f64 = 5.0;
const ROTATE_HIT: f64 = 24.0;
const HANDLE: f64 = 10.0;
/// A path point's drawn square and a Bézier handle's end dot (radius).
const POINT: f64 = 8.0;
const HANDLE_END: f64 = 4.0;
/// How near (window px) a press must land to a path's point or handle end.
const POINT_HIT: f64 = 7.0;
/// Rotation snap with Shift.
const SNAP: f64 = std::f64::consts::PI / 12.0;

const BLUE: [f32; 4] = [0.05, 0.6, 1.0, 1.0];

/// Composition-space boxes and grip eligibility shared by drawing and cursor
/// testing. View pan/zoom is applied at use time, never baked into these boxes.
/// This conservative bridge-generation cache is a stopgap until bounds have
/// their own per-subtree revisions (roadmap H).
#[derive(Resource, Default)]
pub struct SelectionGeometryCache {
    selection: HashMap<String, CachedSelection>,
    hover: Option<(GeometryStamp, Reach, Option<Affine>)>,
    lit: HashMap<String, CachedLit>,
}

/// What lights up in one panel for the current highlights.
struct CachedLit {
    stamp: GeometryStamp,
    highlights: (std::collections::BTreeSet<NodeId>, std::collections::BTreeSet<NodeId>),
    boxed: Vec<Vec<NodeId>>,
    marks: Vec<LitMark>,
}

/// One occurrence of a lit node, in panel document px: a leaf's outline,
/// or a group's or embed's frame (Figma outlines a group by its box, never
/// by its descendants).
struct LitMark {
    chain: Vec<NodeId>,
    selected: bool,
    shape: LitShape,
}

enum LitShape {
    /// Panel document px ← the leaf's space, and its outline there.
    Outline(Affine, kurbo::BezPath),
    /// The unit square onto the box.
    Frame(Affine),
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct GeometryStamp {
    version: iron_document::Version,
    resolved: u64,
    time: u64,
}

impl GeometryStamp {
    fn new(store: &Store, revision: u64) -> Self {
        Self { version: store.document().version(), resolved: revision, time: store.time().to_bits() }
    }
}

struct CachedSelection {
    stamp: GeometryStamp,
    after_propagation: bool,
    reaches: Vec<Reach>,
    geometry: SelectionGeometry,
}

#[derive(Default)]
struct SelectionGeometry {
    frame: Option<Affine>,
    frames: Vec<Affine>,
    resizable: bool,
    rotatable: bool,
}

impl SelectionGeometryCache {
    fn selection<'a>(&'a mut self, geo: &Geo, panel: &str, items: &[&Reach], stamp: GeometryStamp, after_propagation: bool) -> &'a SelectionGeometry {
        let cached = self.selection.get(panel);
        // Embed bounds can read Bevy globals. A cache rebuilt by cursor testing
        // before propagation must be finalized by drawing afterwards, but the
        // next idle cursor test can reuse that finalized entry.
        if cached.is_none_or(|c| c.stamp != stamp || (after_propagation && !c.after_propagation) || c.reaches.len() != items.len()
            || c.reaches.iter().zip(items).any(|(a, b)| a != *b)) || geo.store.has_changes()
        {
            let timing = crate::perf::span(crate::perf::Phase::OverlayGeometry);
            if items.is_empty() { timing.discard(); }
            let frames: Vec<Affine> = items.iter().filter_map(|r| geo.frame(&r.chain)).collect();
            let frame = match items {
                [] => None,
                [_] => frames.first().copied(),
                _ => frames.iter().map(|f| f.transform_rect_bbox(Rect::new(0.0, 0.0, 1.0, 1.0)))
                    .reduce(|a, b| a.union(b)).map(unit),
            };
            let mut capabilities = Vec::new();
            if frame.is_some() {
                for r in items {
                    geo.resize_capabilities(&r.chain, &mut capabilities);
                }
            }
            let resizable = !capabilities.is_empty() && capabilities.iter().all(|b| *b);
            // Rotation writes the selected roots, unlike group resizing.
            let rotatable = frame.is_some() && !items.is_empty() && items.iter()
                .filter(|r| geo.space(&r.chain).is_some() && geo.local_box(r.node()).is_some())
                .all(|r| geo.free(r.node(), "transform2d.rot"));
            self.selection.insert(panel.to_owned(), CachedSelection {
                stamp, after_propagation, reaches: items.iter().map(|r| (*r).clone()).collect(),
                geometry: SelectionGeometry { frame, frames, resizable, rotatable },
            });
        }
        &self.selection.get(panel).expect("cached selection").geometry
    }

    /// Every occurrence in `view` of every lit node, less the containers in
    /// `boxed` (already drawn as the selection's or the hover's box; a
    /// group's frame costs a walk of its descendants). Rebuilt when geometry
    /// or the highlights change; the scan is over the lit nodes and the
    /// document's embeds, never over a panel's content.
    fn lit<'a>(&'a mut self, geo: &Geo, view: &PanelView, embeds: &[NodeId], highlights: &crate::document_bridge::Highlights, boxed: Vec<Vec<NodeId>>, stamp: GeometryStamp) -> &'a [LitMark] {
        let key = (&highlights.selected, &highlights.hovered);
        if self.lit.get(&view.panel).is_none_or(|c| c.stamp != stamp || (&c.highlights.0, &c.highlights.1) != key || c.boxed != boxed) || geo.store.has_changes() {
            let timing = crate::perf::span(crate::perf::Phase::OverlayGeometry);
            if highlights.selected.is_empty() && highlights.hovered.is_empty() { timing.discard(); }
            let doc = geo.doc();
            let mut marks = Vec::new();
            let selected = highlights.selected.iter().map(|&n| (n, true));
            let hovered = highlights.hovered.iter().filter(|n| !highlights.selected.contains(n)).map(|&n| (n, false));
            for (id, selected) in selected.chain(hovered) {
                if !doc.is_live(id) {
                    continue;
                }
                for chain in occurrences(doc, view, embeds, id) {
                    let shape = match geo.ty(id) {
                        "group" | "embed" if boxed.contains(&chain) => None,
                        "group" | "embed" => geo.frame(&chain).map(LitShape::Frame),
                        _ => geo.map.entity(id).and_then(|e| geo.nodes.get(e).ok()).and_then(|(d, g)| {
                            Some(LitShape::Outline(geo.space(&chain)? * nest::affine_of(g), nest::shape_path(&d.shape)))
                        }),
                    };
                    if let Some(shape) = shape {
                        marks.push(LitMark { chain, selected, shape });
                    }
                }
            }
            self.lit.insert(view.panel.clone(), CachedLit { stamp, highlights: (highlights.selected.clone(), highlights.hovered.clone()), boxed, marks });
        }
        &self.lit.get(&view.panel).expect("cached lit").marks
    }

    fn hover(&mut self, geo: &Geo, reach: &Reach, stamp: GeometryStamp) -> Option<Affine> {
        if self.hover.as_ref().is_none_or(|(s, r, _)| *s != stamp || r != reach) || geo.store.has_changes() {
            let _timing = crate::perf::span(crate::perf::Phase::OverlayGeometry);
            self.hover = Some((stamp, reach.clone(), geo.frame(&reach.chain)));
        }
        self.hover.as_ref().and_then(|(_, _, f)| *f)
    }
}

#[derive(Resource, Default)]
pub struct SelectTool {
    /// How each selected node was reached. A selection made elsewhere (the
    /// shell, Show in) is resolved to a reach when first needed.
    pub reach: HashMap<NodeId, Reach>,
    pub hover: Option<Reach>,
    /// The marquee being drawn: its panel and its rectangle in document px.
    pub marquee: Option<(String, Rect)>,
    gesture: Gesture,
    /// A middle-button drag: `Some(None)` while held off any 2D view,
    /// `Some(Some(pan))` while it pans one. Apart from `gesture`, since it
    /// belongs to no tool.
    middle: Option<Option<MiddlePan>>,
    last_click: Option<(f64, Vec2, NodeId)>,
    cursor: String,
    /// Nodes a paste or duplicate is creating, to select once their
    /// transaction has applied.
    select_when_live: Vec<NodeId>,
}

/// Panning a view with the middle button: the panel, where the press was,
/// and the pan then.
struct MiddlePan {
    panel: String,
    start: Vec2,
    pan: Vec2,
}

#[derive(Default)]
enum Gesture {
    #[default]
    Idle,
    /// Pressed, not yet dragged: a click if released here.
    Press { panel: String, start: Vec2, then: AfterPress },
    Move { label: String, start: Point, view: PanelView, drags: Vec<(DragHandle, Affine)> },
    Transform { label: String, view: PanelView, kind: TransformKind, frame: Affine, start: Point, parts: Vec<Part>, rot0: Option<f64> },
    Marquee { view: PanelView, start: Point, prefix: Vec<NodeId>, roots: Vec<NodeId>, base: Vec<Reach> },
    /// A drag `interaction::drag_apply_system` runs (3D, planes, sliders).
    Legacy,
    /// Space-drag: the view's pan follows the pointer.
    Pan { panel: String, start: Vec2, pan: Vec2 },
    /// A path point's handle: `out` or in, mirrored to the other unless Alt;
    /// `to_local` maps window px into the path's space.
    Handle { point: NodeId, out: bool, to_local: Affine, at: Point },
    /// A lone shape's own handle (`ShapeGrip`); `to_local` maps window px
    /// into the shape's space, `arc` is (start, sweep, inner) at the press.
    Shape { id: NodeId, grip: ShapeGrip, to_local: Affine, arc: [f64; 3] },
}

/// Figma's handles on a lone selected shape: a dot inside each of a rect's
/// corners sets the corner radius, and an ellipse's arc has a start, a
/// sweep (its end) and an inner ratio.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum ShapeGrip {
    Corner(usize),
    Start,
    Sweep,
    Inner,
}

/// Window px a shape must span on screen, both ways, to show its handles,
/// and how far inside its outline they sit.
const SHAPE_GRIPS_MIN: f64 = 48.0;
const SHAPE_GRIP_INSET: f64 = 12.0;
/// A rect's corners, clockwise from the top-left, as unit-square points.
const CORNERS: [(f64, f64); 4] = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];

enum AfterPress {
    /// A drag moves the selection.
    Move { click: Click },
    /// A drag draws a marquee over `roots` (reached through `prefix`).
    Marquee { prefix: Vec<NodeId>, roots: Vec<NodeId>, click: Click },
}

/// What a press that did not become a drag does on release.
enum Click {
    Keep,
    Only(Reach),
    Remove(NodeId),
    Clear,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum TransformKind {
    /// The grip in the box's unit square: corners and edge midpoints.
    Resize { grip: (f64, f64) },
    Rotate,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Grip {
    Resize(f64, f64),
    /// Outside this corner.
    Rotate(f64, f64),
    Body,
}

/// What resizing a node writes.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    /// `size.w`, `size.h` (rects, sliders); `aspect` when a camera fixes h.
    Sized { aspect: bool },
    /// `radius.rx`, `radius.ry`; the origin is the centre.
    Ellipse,
    /// A 2D embed's window, `frame.*`, the content staying put.
    Window { frame: bool },
    /// Resized through its children; rotates as itself.
    Group,
    /// Nothing to write (a slider with no `size`).
    Fixed,
    /// A path: its points and their handles move, so its stroke keeps its
    /// width.
    Points,
}

/// One node a resize or rotation writes, as it was when the gesture began.
#[derive(Clone, Debug)]
struct Part {
    id: NodeId,
    kind: Kind,
    /// Document px of the panel ← the node's parent space.
    parent: Affine,
    trs: [f64; 5],
    bounds: Rect,
    /// A path's points: id, position, in and out handles (path space).
    points: Vec<(NodeId, Point, kurbo::Vec2, kurbo::Vec2)>,
}

/// Read access to everything geometry needs.
pub(crate) struct Geo<'a, 'w, 's> {
    pub store: &'a Store,
    pub map: &'a NodeMap,
    pub globals: &'a Query<'w, 's, &'static GlobalTransform>,
    pub nodes: &'a Query<'w, 's, (&'static Doc2d, &'static GlobalTransform)>,
}

fn kaffine(a: iron_document::Affine2) -> Affine {
    Affine::new(a.0)
}

pub(crate) fn leaf(s: &str) -> LeafPath {
    s.parse().expect("declared leaf")
}

/// The unit square onto `r`.
fn unit(r: Rect) -> Affine {
    Affine::translate((r.x0, r.y0)) * Affine::scale_non_uniform(r.width().max(1e-6), r.height().max(1e-6))
}

fn wrap(a: f64) -> f64 {
    let t = std::f64::consts::TAU;
    a - t * ((a + std::f64::consts::PI) / t).floor()
}

impl Geo<'_, '_, '_> {
    fn doc(&self) -> &iron_document::Document {
        self.store.document()
    }

    fn num(&self, id: NodeId, path: &str) -> Option<f64> {
        self.store.resolved(id, leaf(path)).and_then(|v| v.as_f64())
    }

    pub(crate) fn ty(&self, id: NodeId) -> &'static str {
        self.doc().live(id).map(|n| n.ty.name()).unwrap_or("")
    }

    pub(crate) fn world(&self, id: NodeId) -> Affine {
        kaffine(self.store.world_affine(id))
    }

    fn has(&self, id: NodeId, kind: iron_document::ComponentKind) -> bool {
        self.doc().component(id, kind).is_some()
    }

    /// Panel document px ← the space the chain's node is placed in (its
    /// composition's). `None` through anything 3D.
    pub(crate) fn space(&self, chain: &[NodeId]) -> Option<Affine> {
        let doc = self.doc();
        let mut a = Affine::IDENTITY;
        let (last, outer) = chain.split_last()?;
        for &e in outer {
            if !nest::is_embed(doc, e) {
                continue;
            }
            let source = composition::embedded(doc, e)?;
            let placed_in = composition::composition_of(doc, e).and_then(|c| composition::dims(doc, c));
            if composition::dims(doc, source) != Some(Dims::Two) || placed_in == Some(Dims::Three) {
                return None;
            }
            a = a * self.world(e);
        }
        let in_3d = composition::composition_of(doc, *last).and_then(|c| composition::dims(doc, c)) == Some(Dims::Three);
        (!in_3d).then_some(a)
    }

    /// The node's box in its own space and what resizing it writes.
    fn local_box(&self, id: NodeId) -> Option<(Rect, Kind)> {
        use iron_document::ComponentKind as K;
        let doc = self.doc();
        match self.ty(id) {
            "rect" => Some((Rect::new(0.0, 0.0, self.num(id, "size.w")?, self.num(id, "size.h")?), Kind::Sized { aspect: false })),
            "slider" => {
                if self.has(id, K::Size) {
                    return Some((Rect::new(0.0, 0.0, self.num(id, "size.w")?, self.num(id, "size.h")?), Kind::Sized { aspect: false }));
                }
                let (d, _) = self.map.entity(id).and_then(|e| self.nodes.get(e).ok())?;
                let Shape2d::Slider { w, h, .. } = d.shape.clone() else { return None };
                Some((Rect::new(0.0, 0.0, w as f64, h as f64), Kind::Fixed))
            }
            "ellipse" => {
                let (rx, ry) = (self.num(id, "radius.rx")?, self.num(id, "radius.ry")?);
                Some((Rect::new(-rx, -ry, rx, ry), Kind::Ellipse))
            }
            "embed" => {
                if let Some((_, camera, w, h)) = nest::embed_3d(self.store, id) {
                    return Some((Rect::new(0.0, 0.0, w, h), Kind::Sized { aspect: camera.is_some() }));
                }
                let e = nest::embed_2d(doc, self.map, self.globals, self.nodes, id)?;
                Some((e.region, Kind::Window { frame: self.has(id, K::Frame) }))
            }
            "path" => {
                let (outline, _) = crate::document_bridge::path_outline(self.store, id);
                let r = outline.control_box();
                (r.width() > 0.0 || r.height() > 0.0 || outline.elements().len() > 0).then_some((r, Kind::Points))
            }
            "group" => {
                let inv = self.world(id).inverse();
                let r = doc
                    .children(id)
                    .iter()
                    .filter_map(|&c| self.local_box(c).map(|(b, _)| (inv * self.world(c)).transform_rect_bbox(b)))
                    .reduce(|a, b| a.union(b))?;
                Some((r, Kind::Group))
            }
            _ => None,
        }
    }

    /// The unit square onto the node's box, in panel document px.
    fn frame(&self, chain: &[NodeId]) -> Option<Affine> {
        let id = *chain.last()?;
        let (b, _) = self.local_box(id)?;
        Some(self.space(chain)? * self.world(id) * unit(b))
    }

    /// The selection's box in `panel`: the node's own (oriented) box for
    /// one node, the bounding box of every node's for several.
    fn selection_frame(&self, items: &[&Reach]) -> Option<Affine> {
        match items {
            [] => None,
            [one] => self.frame(&one.chain),
            many => {
                let r = many
                    .iter()
                    .filter_map(|r| self.frame(&r.chain))
                    .map(|f| f.transform_rect_bbox(Rect::new(0.0, 0.0, 1.0, 1.0)))
                    .reduce(|a, b| a.union(b))?;
                Some(unit(r))
            }
        }
    }

    fn trs(&self, id: NodeId) -> [f64; 5] {
        let n = |f: &str, d: f64| self.num(id, &format!("transform2d.{f}")).unwrap_or(d);
        [n("x", 0.0), n("y", 0.0), n("rot", 0.0), n("sx", 1.0), n("sy", 1.0)]
    }

    /// The nodes a gesture on `chain`'s node writes: the node itself, or for
    /// a resize of a group, its leaves.
    fn parts(&self, chain: &[NodeId], resize: bool, out: &mut Vec<Part>) {
        let Some(&id) = chain.last() else { return };
        let Some((bounds, kind)) = self.local_box(id) else { return };
        let Some(space) = self.space(chain) else { return };
        if resize && kind == Kind::Group {
            for &c in self.doc().children(id) {
                let mut sub = chain.to_vec();
                sub.push(c);
                self.parts(&sub, true, out);
            }
            return;
        }
        let parent = self.doc().live(id).and_then(|n| n.parent).map_or(Affine::IDENTITY, |p| self.world(p));
        let points = if kind == Kind::Points { self.points(id) } else { Vec::new() };
        out.push(Part { id, kind, parent: space * parent, trs: self.trs(id), bounds, points });
    }

    /// A path's points, in order: id, position, in and out handles.
    pub(crate) fn points(&self, path: NodeId) -> Vec<(NodeId, Point, kurbo::Vec2, kurbo::Vec2)> {
        let n = |c: NodeId, l: &str| self.num(c, l).unwrap_or(0.0);
        self.doc()
            .children(path)
            .iter()
            .filter(|&&c| self.ty(c) == "point")
            .map(|&c| (c, Point::new(n(c, "transform2d.x"), n(c, "transform2d.y")), kurbo::Vec2::new(n(c, "handles.ix"), n(c, "handles.iy")), kurbo::Vec2::new(n(c, "handles.ox"), n(c, "handles.oy"))))
            .collect()
    }

    /// Whether a leaf can follow a gesture right now.
    fn free(&self, id: NodeId, path: &str) -> bool {
        self.doc().leaf(id, leaf(path)).is_some() && !matches!(self.store.handle(id, leaf(path)), Handle::Locked(_))
    }

    fn resizable(&self, parts: &[Part]) -> bool {
        !parts.is_empty()
            && parts.iter().all(|p| match p.kind {
                Kind::Sized { .. } => self.free(p.id, "size.w") || self.free(p.id, "size.h"),
                Kind::Ellipse => self.free(p.id, "radius.rx") || self.free(p.id, "radius.ry"),
                Kind::Window { frame } => frame,
                Kind::Points => true,
                Kind::Group | Kind::Fixed => false,
            })
    }

    /// Same eligibility as resize Parts, without allocating gesture poses or
    /// collecting path points. Detailed Parts are built only on a press.
    fn resize_capabilities(&self, chain: &[NodeId], out: &mut Vec<bool>) {
        let Some(&id) = chain.last() else { return };
        if self.ty(id) == "group" {
            for &c in self.doc().children(id) {
                let mut sub = chain.to_vec();
                sub.push(c);
                self.resize_capabilities(&sub, out);
            }
            return;
        }
        let Some((_, kind)) = self.local_box(id) else { return };
        if self.space(chain).is_none() { return; }
        out.push(match kind {
            Kind::Sized { .. } => self.free(id, "size.w") || self.free(id, "size.h"),
            Kind::Ellipse => self.free(id, "radius.rx") || self.free(id, "radius.ry"),
            Kind::Window { frame } => frame,
            Kind::Points => true,
            Kind::Group | Kind::Fixed => false,
        });
    }

    /// A rect's corner radii or an ellipse's arc (start, sweep, inner), as
    /// resolved; an absent component reads as its defaults.
    fn radii(&self, id: NodeId) -> [f64; 4] {
        ["tl", "tr", "br", "bl"].map(|c| self.num(id, &format!("corners.{c}")).unwrap_or(0.0))
    }

    fn arc(&self, id: NodeId) -> [f64; 3] {
        [self.num(id, "arc.start").unwrap_or(0.0), self.num(id, "arc.sweep").unwrap_or(std::f64::consts::TAU), self.num(id, "arc.inner").unwrap_or(0.0)]
    }

    /// A lone shape's handles (`ShapeGrip`) and where they are on screen,
    /// given window px ← the shape's space; none while it is small there.
    pub(crate) fn shape_grips(&self, id: NodeId, to_screen: Affine) -> Vec<(ShapeGrip, Point)> {
        let Some((b, _)) = self.local_box(id) else { return Vec::new() };
        let [a, bb, c, d, _, _] = to_screen.as_coeffs();
        // Window px per local unit along each axis.
        let (kx, ky) = (a.hypot(bb).max(1e-9), c.hypot(d).max(1e-9));
        if b.width() * kx < SHAPE_GRIPS_MIN || b.height() * ky < SHAPE_GRIPS_MIN {
            return Vec::new();
        }
        match self.ty(id) {
            "rect" => {
                let radii = iron_document::shape::clamp_radii(b.width(), b.height(), self.radii(id));
                CORNERS
                    .iter()
                    .zip(radii)
                    .enumerate()
                    .map(|(i, (&(u, v), r))| {
                        let inward = kurbo::Vec2::new(1.0 - 2.0 * u, 1.0 - 2.0 * v);
                        let dx = r.max(SHAPE_GRIP_INSET / kx).min(b.width() * 0.5);
                        let dy = r.max(SHAPE_GRIP_INSET / ky).min(b.height() * 0.5);
                        let corner = Point::new(b.x0 + u * b.width(), b.y0 + v * b.height());
                        (ShapeGrip::Corner(i), to_screen * (corner + kurbo::Vec2::new(inward.x * dx, inward.y * dy)))
                    })
                    .collect()
            }
            "ellipse" => {
                let (rx, ry) = (b.width() * 0.5, b.height() * 0.5);
                let [start, sweep, inner] = self.arc(id);
                // On the rim, pulled in so it stays clear of the box's edges.
                let rim = |t: f64| {
                    let p = Point::new(rx * t.cos(), ry * t.sin());
                    let screen_r = (to_screen * p).distance(to_screen * Point::ORIGIN).max(1.0);
                    let k = (1.0 - SHAPE_GRIP_INSET / screen_r).max(0.0);
                    to_screen * Point::new(p.x * k, p.y * k)
                };
                let full = sweep.abs() >= std::f64::consts::TAU - 1e-9;
                let mid = start + sweep * 0.5;
                let mut out = vec![(ShapeGrip::Sweep, rim(start + sweep))];
                if !full {
                    out.push((ShapeGrip::Start, rim(start)));
                }
                out.push((ShapeGrip::Inner, to_screen * Point::new(rx * inner * mid.cos(), ry * inner * mid.sin())));
                out
            }
            _ => Vec::new(),
        }
    }

    fn rotatable(&self, parts: &[Part]) -> bool {
        !parts.is_empty() && parts.iter().all(|p| self.free(p.id, "transform2d.rot"))
    }
}

/// Position, rotation, box size and whether `sy` flips, from the map of
/// the unit square (or of the node's local axes, for size 1) into the
/// parent's space; shear is dropped.
fn decompose(f: Affine, trs: [f64; 5]) -> (f64, f64, f64, f64, f64, bool) {
    let [a, b, c, d, e, g] = f.as_coeffs();
    let [_, _, rot0, sx, sy] = trs;
    let phi = b.atan2(a);
    let rot = rot0 + wrap(phi - if sx < 0.0 { std::f64::consts::PI } else { 0.0 } - rot0);
    let w = a.hypot(b) / sx.abs().max(1e-9);
    let (s, co) = phi.sin_cos();
    let y2 = -s * c + co * d;
    let h = y2 / sy * if sx < 0.0 { -1.0 } else { 1.0 };
    (e, g, rot, w, h.abs(), h < 0.0)
}

impl Part {
    /// The leaf values that apply `a` (panel document px) to this node, or
    /// to its points.
    fn apply(&self, a: Affine, resize: bool) -> Vec<(NodeId, &'static str, f64)> {
        let [x, y, rot, sx, sy] = self.trs;
        let q = self.parent.inverse() * a * self.parent;
        let l = Affine::translate((x, y)) * Affine::rotate(rot) * Affine::scale_non_uniform(sx, sy);
        let mut out = Vec::new();
        let id = self.id;
        // Every leaf the gesture may drive, not only the ones that differ
        // from the start: one that returns there must be previewed back.
        let mut put = |path: &'static str, _start: f64, new: f64| {
            if new.is_finite() {
                out.push((id, path, new));
            }
        };
        let (bw, bh) = (self.bounds.width(), self.bounds.height());
        match (resize, self.kind) {
            (true, Kind::Sized { aspect }) => {
                let (nx, ny, nrot, w, h, flip) = decompose(q * l * Affine::scale_non_uniform(bw.max(1e-6), bh.max(1e-6)), self.trs);
                put("transform2d.x", x, nx);
                put("transform2d.y", y, ny);
                put("transform2d.rot", rot, nrot);
                put("size.w", bw, w);
                if !aspect {
                    put("size.h", bh, h);
                }
                put("transform2d.sy", sy, if flip { -sy } else { sy });
            }
            (true, Kind::Ellipse) => {
                // As a rect, about the centre: either axis stretches alone,
                // so a circle becomes an ellipse.
                let f = q * l * unit(self.bounds);
                let centre = f * Point::new(0.5, 0.5);
                let (_, _, nrot, w, h, flip) = decompose(f, self.trs);
                put("transform2d.x", x, centre.x);
                put("transform2d.y", y, centre.y);
                put("transform2d.rot", rot, nrot);
                put("radius.rx", bw * 0.5, w * 0.5);
                put("radius.ry", bh * 0.5, h * 0.5);
                put("transform2d.sy", sy, if flip { -sy } else { sy });
            }
            (true, Kind::Window { frame: true }) => {
                let r = (l.inverse() * q * l).transform_rect_bbox(self.bounds);
                put("frame.x0", self.bounds.x0, r.x0);
                put("frame.y0", self.bounds.y0, r.y0);
                put("frame.x1", self.bounds.x1, r.x1);
                put("frame.y1", self.bounds.y1, r.y1);
            }
            (true, Kind::Points) => {
                // Path space to path space; handles take the linear part.
                let m = l.inverse() * q * l;
                let [a0, b0, c0, d0, _, _] = m.as_coeffs();
                let lin = |v: kurbo::Vec2| kurbo::Vec2::new(a0 * v.x + c0 * v.y, b0 * v.x + d0 * v.y);
                for &(pid, p, hi, ho) in &self.points {
                    let np = m * p;
                    out.push((pid, "transform2d.x", np.x));
                    out.push((pid, "transform2d.y", np.y));
                    if hi != kurbo::Vec2::ZERO || ho != kurbo::Vec2::ZERO {
                        let (ni, no) = (lin(hi), lin(ho));
                        out.extend([(pid, "handles.ix", ni.x), (pid, "handles.iy", ni.y), (pid, "handles.ox", no.x), (pid, "handles.oy", no.y)]);
                    }
                }
            }
            (true, _) => {}
            (false, _) => {
                let (nx, ny, nrot, ..) = decompose(q * l, self.trs);
                put("transform2d.x", x, nx);
                put("transform2d.y", y, ny);
                put("transform2d.rot", rot, nrot);
            }
        }
        out
    }
}

/// Which node along `chain` a press selects: the outermost one whose
/// container is not open (or the hit node itself, `deep`).
fn target_index(doc: &iron_document::Document, chain: &[NodeId], selected: &[&Reach], deep: bool) -> usize {
    if deep {
        return chain.len() - 1;
    }
    for i in 0..chain.len() {
        let open = (i == 0 && nest::is_embed(doc, chain[0])) || selected.iter().any(|r| r.chain.len() > i + 1 && r.chain[..=i] == chain[..=i]);
        if !open {
            return i;
        }
    }
    chain.len() - 1
}

/// Where `id` shows in a 2D panel, for a selection made elsewhere: in a
/// panel showing its composition, or through embeds of it.
fn find_reach(doc: &iron_document::Document, views: &PanelViews, id: NodeId) -> Option<Reach> {
    let comp = composition::composition_of(doc, id);
    let lineage = nest::lineage(doc, id);
    let views2d = || views.0.iter().filter(|v| v.dims == Dims::Two);
    if let Some(v) = views2d().find(|v| v.shows(comp)) {
        return Some(Reach { panel: v.panel.clone(), chain: lineage });
    }
    let comp = comp?;
    fn search(doc: &iron_document::Document, roots: &[NodeId], prefix: &[NodeId], comp: NodeId, depth: u32) -> Option<Vec<NodeId>> {
        for &n in roots {
            let mut chain = prefix.to_vec();
            chain.push(n);
            if nest::is_embed(doc, n) {
                let Some(source) = composition::embedded(doc, n) else { continue };
                if source == comp {
                    return Some(chain);
                }
                if depth < nest::MAX_DEPTH
                    && let Some(found) = search(doc, doc.children(source), &chain, comp, depth + 1)
                {
                    return Some(found);
                }
            } else if let Some(found) = search(doc, doc.children(n), &chain, comp, depth) {
                return Some(found);
            }
        }
        None
    }
    for v in views2d() {
        if let Some(mut chain) = search(doc, &nest::panel_roots(doc, v), &[], comp, 0) {
            chain.extend(lineage);
            return Some(Reach { panel: v.panel.clone(), chain });
        }
    }
    None
}

/// Every chain by which `id` shows in `view`: directly, when the panel
/// shows its composition, and through each 2D embed of that composition
/// the panel shows, recursively. `embeds` is every embed in the document.
fn occurrences(doc: &iron_document::Document, view: &PanelView, embeds: &[NodeId], id: NodeId) -> Vec<Vec<NodeId>> {
    fn prefixes(doc: &iron_document::Document, view: &PanelView, embeds: &[NodeId], comp: Option<NodeId>, depth: u32) -> Vec<Vec<NodeId>> {
        let mut out = Vec::new();
        if view.shows(comp) {
            out.push(Vec::new());
        }
        let Some(comp) = comp.filter(|_| depth < nest::MAX_DEPTH) else { return out };
        for &e in embeds.iter().filter(|&&e| composition::embedded(doc, e) == Some(comp)) {
            for mut prefix in prefixes(doc, view, embeds, composition::composition_of(doc, e), depth + 1) {
                prefix.extend(nest::lineage(doc, e));
                out.push(prefix);
            }
        }
        out
    }
    let lineage = nest::lineage(doc, id);
    prefixes(doc, view, embeds, composition::composition_of(doc, id), 0)
        .into_iter()
        .map(|mut chain| {
            chain.extend_from_slice(&lineage);
            chain
        })
        .collect()
}

/// The grip under a window point for a box `frame` (window px).
fn grip_at(frame: Affine, p: Point, resizable: bool, rotatable: bool) -> Option<Grip> {
    let corners = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
    let at = |u: f64, v: f64| frame * Point::new(u, v);
    if resizable {
        for &(u, v) in &corners {
            if at(u, v).distance(p) <= CORNER_HIT {
                return Some(Grip::Resize(u, v));
            }
        }
        for &(u, v) in &[(0.5, 0.0), (1.0, 0.5), (0.5, 1.0), (0.0, 0.5)] {
            let (a, b) = if u == 0.5 { (at(0.0, v), at(1.0, v)) } else { (at(u, 0.0), at(u, 1.0)) };
            let seg = kurbo::Line::new(a, b);
            let t = ((p - a).dot(b - a) / (b - a).hypot2().max(1e-9)).clamp(0.0, 1.0);
            if seg.eval(t).distance(p) <= EDGE_HIT {
                return Some(Grip::Resize(u, v));
            }
        }
    }
    let q = frame.inverse() * p;
    let inside = (0.0..=1.0).contains(&q.x) && (0.0..=1.0).contains(&q.y);
    if inside {
        return Some(Grip::Body);
    }
    if rotatable {
        for &(u, v) in &corners {
            if at(u, v).distance(p) <= ROTATE_HIT {
                return Some(Grip::Rotate(u, v));
            }
        }
    }
    None
}

/// The CSS cursor for a grip: a resize arrow along the grip's direction
/// from the box centre, or a rotation arrow (drawn by the shell) at it.
fn cursor_for(frame: Affine, grip: Option<Grip>) -> String {
    let dir = |u: f64, v: f64| {
        let c = frame * Point::new(0.5, 0.5);
        let p = frame * Point::new(u, v);
        (p.y - c.y).atan2(p.x - c.x).to_degrees()
    };
    match grip {
        Some(Grip::Resize(u, v)) => {
            let a = dir(u, v).rem_euclid(180.0);
            let k = ((a + 22.5) / 45.0).floor() as i32 % 4;
            ["ew-resize", "nwse-resize", "ns-resize", "nesw-resize"][k as usize].into()
        }
        Some(Grip::Rotate(u, v)) => format!("rotate:{:.0}", dir(u, v)),
        _ => "default".into(),
    }
}

/// Separating-axis test: does the parallelogram `f` (of the unit square)
/// touch the rectangle?
fn touches(f: Affine, r: Rect) -> bool {
    let poly = [f * Point::new(0.0, 0.0), f * Point::new(1.0, 0.0), f * Point::new(1.0, 1.0), f * Point::new(0.0, 1.0)];
    let rect = [Point::new(r.x0, r.y0), Point::new(r.x1, r.y0), Point::new(r.x1, r.y1), Point::new(r.x0, r.y1)];
    let axes = [kurbo::Vec2::new(1.0, 0.0), kurbo::Vec2::new(0.0, 1.0), (poly[1] - poly[0]).turn_90(), (poly[3] - poly[0]).turn_90()];
    axes.iter().all(|ax| {
        let span = |pts: &[Point; 4]| pts.iter().map(|p| p.to_vec2().dot(*ax)).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), d| (lo.min(d), hi.max(d)));
        let (a0, a1) = span(&poly);
        let (b0, b1) = span(&rect);
        a1 >= b0 && b1 >= a0
    })
}

fn set_selection(sel: &mut crate::SelectionState, tool: &mut SelectTool, list: Vec<Reach>) {
    let ids: Vec<NodeId> = list.iter().map(Reach::node).collect();
    if sel.selected != ids {
        sel.selected = ids;
    }
    tool.reach = list.into_iter().map(|r| (r.node(), r)).collect();
}

fn selected_reaches(sel: &crate::SelectionState, tool: &SelectTool) -> Vec<Reach> {
    sel.selected.iter().filter_map(|id| tool.reach.get(id).cloned()).collect()
}

/// Selected nodes in `panel` that no other selected node contains: what a
/// gesture moves (a group and its child both selected move once).
fn movers<'a>(reaches: &'a [Reach], panel: &str) -> Vec<&'a Reach> {
    reaches
        .iter()
        .filter(|r| r.panel == panel)
        .filter(|r| !reaches.iter().any(|o| o.panel == r.panel && o.chain.len() < r.chain.len() && r.chain[..o.chain.len()] == o.chain[..]))
        .collect()
}

/// Selected nodes no other selected node contains.
fn outermost(doc: &iron_document::Document, selected: &[NodeId]) -> Vec<NodeId> {
    selected.iter().copied().filter(|&id| !selected.iter().any(|&o| o != id && doc.is_ancestor(o, id))).collect()
}

/// Where a paste goes, as in Figma: the view under the pointer (else the
/// selection's, else the first 2D view); beside the selection there, else
/// into the artboard under the pointer or the view's composition; at the
/// copied position when that is on screen, else centred in the view.
fn paste_placement(geo: &Geo, hits: &crate::PointerHits, views: &PanelViews, reaches: &[Reach], frag: &Fragment, share: bool) -> Option<copy::Placement> {
    let doc = geo.doc();
    let two = |v: &&PanelView| v.dims == Dims::Two;
    let under = hits.panel.as_deref().and_then(|p| views.get(p)).filter(two);
    let view = under.or_else(|| reaches.first().and_then(|r| views.get(&r.panel)).filter(two)).or_else(|| views.0.iter().find(two))?;
    // The container, and view document px → its composition's space.
    let mine = movers(reaches, &view.panel);
    let (parent, to_comp) = match mine.last() {
        Some(r) => (doc.live(r.node()).and_then(|n| n.parent), geo.space(&r.chain)?.inverse()),
        None if under.is_some() => {
            let (c, a) = crate::bevy_app::pen::drop_target(geo, hits, view)?;
            (Some(c), a)
        }
        None => (view.composition, Affine::IDENTITY),
    };
    // An embed never goes inside the composition it shows (a cycle the
    // store refuses): from an artboard of that composition, paste beside it.
    let shows_target = |root: &NodeId| {
        let source = frag.doc.get::<iron_document::component::Embed>(*root).and_then(|e| e.source.constant()).and_then(|v| v.as_ref());
        source.is_some() && source == parent.and_then(|p| composition::composition_of(doc, p))
    };
    let (parent, to_comp) = if frag.roots.iter().any(shows_target) { (view.composition, Affine::IDENTITY) } else { (parent, to_comp) };
    // What the view shows, in that space.
    let r = view.rect;
    let shown = (to_comp * view.affine().inverse()).transform_rect_bbox(Rect::new(r.x as f64, r.y as f64, (r.x + r.w) as f64, (r.y + r.h) as f64));
    // Where the copied roots were, by their origins.
    let origins: Vec<Point> = frag
        .roots
        .iter()
        .filter_map(|&root| {
            let t = frag.doc.component(root, iron_document::ComponentKind::Transform2d)?;
            let n = |f: &str| t.get(f).and_then(|s| s.constant()).and_then(|v| v.as_f64());
            let p = frag.parent_world.get(&root).copied().unwrap_or(iron_document::Affine2::IDENTITY).apply([n("x")?, n("y")?]);
            Some(Point::new(p[0], p[1]))
        })
        .collect();
    let offset = match origins.iter().map(|&p| Rect::from_points(p, p)).reduce(|a, b| a.union(b)) {
        Some(b) if !origins.iter().any(|p| shown.contains(*p)) => {
            let d = shown.center() - b.center();
            [d.x, d.y]
        }
        _ => [0.0, 0.0],
    };
    Some(copy::Placement { parent, beside: false, offset, share_entities: share })
}

fn type_label(doc: &iron_document::Document, nodes: &[NodeId]) -> String {
    match nodes {
        [one] => doc.live(*one).map(|n| n.ty.name().to_owned()).unwrap_or_else(|| "node".into()),
        many => format!("{} nodes", many.len()),
    }
}

/// The container a chain's node sits in and its siblings there, with the
/// chain prefix that reaches them.
fn siblings(doc: &iron_document::Document, views: &PanelViews, r: &Reach) -> (Vec<NodeId>, Vec<NodeId>) {
    let prefix = r.chain[..r.chain.len() - 1].to_vec();
    let roots = match prefix.last() {
        Some(&c) if nest::is_embed(doc, c) => composition::embedded(doc, c).map(|s| doc.children(s).to_vec()).unwrap_or_default(),
        Some(&c) => doc.children(c).to_vec(),
        None => views.get(&r.panel).map(|v| nest::panel_roots(doc, v)).unwrap_or_default(),
    };
    (prefix, roots)
}

/// Children of a node as chains: a group's children, an embed's content.
fn children(doc: &iron_document::Document, r: &Reach) -> Vec<Reach> {
    let id = r.node();
    let kids = if nest::is_embed(doc, id) { composition::embedded(doc, id).map(|s| doc.children(s).to_vec()).unwrap_or_default() } else { doc.children(id).to_vec() };
    kids.into_iter()
        .filter(|&k| doc.live(k).is_some_and(|n| matches!(n.ty.name(), "rect" | "ellipse" | "path" | "slider" | "group" | "embed" | "mesh")))
        .map(|k| {
            let mut chain = r.chain.clone();
            chain.push(k);
            Reach { panel: r.panel.clone(), chain }
        })
        .collect()
}

/// The topmost hit under the pointer: its chain and how it was hit.
pub(crate) enum Source {
    Overlay(usize),
    World(usize),
    Embed3d,
}

pub(crate) fn top_hit(hits: &crate::PointerHits) -> Option<(Vec<NodeId>, Source)> {
    if let Some(h) = hits.overlay.first() {
        return Some((h.chain.clone(), Source::Overlay(0)));
    }
    if let Some(h) = hits.world3d.first() {
        return Some((h.chain.clone(), Source::World(0)));
    }
    (!hits.embed_chain.is_empty()).then(|| (hits.embed_chain.clone(), Source::Embed3d))
}

#[allow(clippy::too_many_arguments)]
/// The document side of the tool: the store, what it writes, and the
/// entities geometry reads.
#[derive(SystemParam)]
pub struct ToolDoc<'w, 's> {
    store: ResMut<'w, DocumentStore>,
    gestures: ResMut<'w, GestureRequests>,
    pending: ResMut<'w, PendingTransactions>,
    map: Res<'w, NodeMap>,
    globals: Query<'w, 's, &'static GlobalTransform>,
    nodes: Query<'w, 's, (&'static Doc2d, &'static GlobalTransform)>,
    view_cameras: ResMut<'w, crate::document_bridge::ViewCameras>,
    keyboard: Res<'w, ButtonInput<KeyCode>>,
    geometry_revision: Res<'w, crate::document_bridge::GeometryRevision>,
    geometry_cache: ResMut<'w, SelectionGeometryCache>,
}

/// What the drags the box does not run need (`interaction`).
#[derive(SystemParam)]
pub struct LegacyDrag<'w, 's> {
    drag: ResMut<'w, crate::DragState>,
    commit: ResMut<'w, PendingCommit>,
    provenance: Query<'w, 's, &'static Provenance>,
    cameras: DragCameras<'w, 's>,
    planes: Query<'w, 's, (&'static GlobalTransform, &'static crate::document_bridge::planes::EmbedPlane)>,
}

pub fn select_tool_system(
    pointer: Res<crate::PointerState>,
    hits: Res<crate::PointerHits>,
    time: Res<Time<Real>>,
    mut tool: ResMut<SelectTool>,
    mut keys: ResMut<ToolKeys>,
    mut sel: ResMut<crate::SelectionState>,
    views: Res<PanelViews>,
    active: Res<ActiveTool>,
    docp: ToolDoc,
    legacy: LegacyDrag,
    mut clipboard: ResMut<Clipboard>,
    origin: Res<crate::document_bridge::DocumentOrigin>,
) {
    let tool = &mut *tool;
    let ToolDoc { mut store, mut gestures, mut pending, map, globals, nodes, mut view_cameras, keyboard, geometry_revision, mut geometry_cache } = docp;
    // What the last paste or duplicate made becomes the selection once it
    // exists (its reach is found below, like any selection made elsewhere).
    if !tool.select_when_live.is_empty() && tool.select_when_live.iter().all(|&id| store.0.document().is_live(id)) {
        let ids = std::mem::take(&mut tool.select_when_live);
        tool.reach.retain(|id, _| ids.contains(id));
        sel.selected = ids;
    }
    let space = keyboard.pressed(KeyCode::Space);
    let LegacyDrag { mut drag, mut commit, provenance, cameras, planes } = legacy;
    // Keep a reach for every selected node, dropping the rest.
    {
        let doc = store.0.document();
        tool.reach.retain(|id, r| sel.selected.contains(id) && r.chain.iter().all(|&n| doc.live(n).is_some()));
        for &id in &sel.selected {
            if !tool.reach.contains_key(&id)
                && let Some(r) = find_reach(doc, &views, id)
            {
                tool.reach.insert(id, r);
            }
        }
        if tool.hover.as_ref().is_some_and(|h| h.chain.iter().any(|&n| doc.live(n).is_none())) {
            tool.hover = None;
        }
    }

    let mods = |bit: u8| match bit {
        MOD_SHIFT => pointer.modifiers.shift,
        MOD_ALT => pointer.modifiers.alt,
        _ => pointer.modifiers.ctrl || pointer.modifiers.meta,
    };

    // Shortcuts.
    for (key, m) in std::mem::take(&mut keys.0) {
        let shift = m & MOD_SHIFT != 0;
        let command = m & (MOD_CTRL | MOD_META) != 0;
        let reaches = selected_reaches(&sel, tool);
        let doc = store.0.document();
        match key.as_str() {
            "escape" if !matches!(tool.gesture, Gesture::Idle) => {
                // Esc mid-gesture puts everything back.
                if let Gesture::Move { drags, .. } = &tool.gesture {
                    for (h, _) in drags {
                        store.0.end_drag(h);
                    }
                }
                if let Some(h) = drag.handle.take() {
                    store.0.end_drag(&h);
                }
                drag.target = None;
                drag.kind = None;
                gestures.0.push(GestureRequest::Cancel);
                tool.gesture = Gesture::Idle;
                tool.marquee = None;
            }
            "escape" | "enter" if key == "escape" || shift => {
                // Up a level: each node's container, or nothing at the top.
                let mut up: Vec<Reach> = Vec::new();
                for r in &reaches {
                    if r.chain.len() > 1 {
                        let p = Reach { panel: r.panel.clone(), chain: r.chain[..r.chain.len() - 1].to_vec() };
                        if !up.contains(&p) {
                            up.push(p);
                        }
                    }
                }
                set_selection(&mut sel, tool, up);
            }
            "enter" if reaches.len() == 1 && iron_document::shape::convertible(&store.0, reaches[0].node()) => {
                edit_as_path(&store.0, &mut pending, reaches[0].node());
            }
            "enter" => {
                let down: Vec<Reach> = reaches.iter().flat_map(|r| children(doc, r)).collect();
                if !down.is_empty() {
                    set_selection(&mut sel, tool, down);
                }
            }
            "tab" => {
                let Some(r) = reaches.first() else { continue };
                let (prefix, roots) = siblings(doc, &views, r);
                let Some(i) = roots.iter().position(|&n| n == r.node()) else { continue };
                let n = roots.len();
                let next = roots[if shift { (i + n - 1) % n } else { (i + 1) % n }];
                let mut chain = prefix;
                chain.push(next);
                set_selection(&mut sel, tool, vec![Reach { panel: r.panel.clone(), chain }]);
            }
            "a" if command => {
                let (panel, prefix, roots) = match reaches.first() {
                    Some(r) => {
                        let (p, roots) = siblings(doc, &views, r);
                        (r.panel.clone(), p, roots)
                    }
                    None => {
                        let Some(v) = hits.panel.as_deref().and_then(|p| views.get(p)).filter(|v| v.dims == Dims::Two) else { continue };
                        (v.panel.clone(), Vec::new(), nest::panel_roots(doc, v))
                    }
                };
                let all = roots
                    .into_iter()
                    .filter(|&n| doc.live(n).is_some_and(|n| matches!(n.ty.name(), "rect" | "ellipse" | "path" | "slider" | "group" | "embed")))
                    .map(|n| {
                        let mut chain = prefix.clone();
                        chain.push(n);
                        Reach { panel: panel.clone(), chain }
                    })
                    .collect();
                set_selection(&mut sel, tool, all);
            }
            "d" if command => match copy::duplicate(&store.0, &sel.selected) {
                Ok(pasted) => {
                    for note in &pasted.notes {
                        info!("duplicate: {note}");
                    }
                    let label = format!("duplicate {}", type_label(doc, &sel.selected));
                    pending.0.push(TransactionInput { label, actor: Actor::Human, ts: 0, idempotency_key: None, ops: pasted.ops, exact: false });
                    tool.select_when_live = pasted.roots;
                }
                Err(e) => info!("duplicate: {e}"),
            },
            "c" | "x" if command => {
                let text = match copy::fragment(&store.0, &sel.selected, &origin.0) {
                    Ok(frag) => {
                        for note in &frag.dropped {
                            info!("copy: {note}");
                        }
                        frag.to_text()
                    }
                    Err(e) => {
                        info!("copy: {e}");
                        String::new()
                    }
                };
                crate::web_ffi::send_clipboard_from_worker(text.clone());
                if key == "x" && !text.is_empty() {
                    let ops = outermost(doc, &sel.selected).into_iter().map(|id| Op::Delete { id }).collect();
                    let label = format!("cut {}", type_label(doc, &sel.selected));
                    pending.0.push(TransactionInput { label, actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false });
                    set_selection(&mut sel, tool, Vec::new());
                    clipboard.cut = Some(text);
                } else if key == "c" {
                    clipboard.cut = None;
                }
            }
            "delete" | "backspace" => {
                let ops: Vec<Op> = sel
                    .selected
                    .iter()
                    .filter(|&&id| !sel.selected.iter().any(|&o| o != id && doc.descendants(o).contains(&id)))
                    .map(|&id| Op::Delete { id })
                    .collect();
                if ops.is_empty() {
                    continue;
                }
                let label = format!("delete {}", type_label(doc, &sel.selected));
                pending.0.push(TransactionInput { label, actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false });
                set_selection(&mut sel, tool, Vec::new());
            }
            "arrowleft" | "arrowright" | "arrowup" | "arrowdown" if matches!(tool.gesture, Gesture::Idle) => {
                let step = if shift { 10.0 } else { 1.0 };
                let d = match key.as_str() {
                    "arrowleft" => (-step, 0.0),
                    "arrowright" => (step, 0.0),
                    "arrowup" => (0.0, -step),
                    _ => (0.0, step),
                };
                let geo = Geo { store: &store.0, map: &map, globals: &globals, nodes: &nodes };
                let mut parts = Vec::new();
                let mut moved = Vec::new();
                let panels: Vec<String> = reaches.iter().map(|r| r.panel.clone()).collect();
                for panel in panels.iter().take(1) {
                    for r in movers(&reaches, panel) {
                        geo.parts(&r.chain, false, &mut parts);
                        moved.push(r.node());
                    }
                }
                let writes: Vec<(NodeId, &'static str, f64)> = parts.iter().flat_map(|p| p.apply(Affine::translate(d), false)).collect();
                let label = format!("nudge {}", type_label(doc, &moved));
                preview_writes(&mut store.0, writes);
                gestures.0.push(GestureRequest::Commit(label));
            }
            _ => {}
        }
    }

    for text in std::mem::take(&mut clipboard.pastes) {
        if !Fragment::is_fragment_text(&text) {
            continue;
        }
        let frag = match Fragment::from_text(&text) {
            Ok(f) => f,
            Err(e) => {
                warn!("paste: {e}");
                continue;
            }
        };
        let share = clipboard.cut.as_deref() == Some(text.as_str());
        if share {
            clipboard.cut = None;
        }
        let geo = Geo { store: &store.0, map: &map, globals: &globals, nodes: &nodes };
        let reaches = selected_reaches(&sel, tool);
        let Some(place) = paste_placement(&geo, &hits, &views, &reaches, &frag, share) else {
            info!("paste: no 2D view to paste into");
            continue;
        };
        match copy::paste(&store.0, &frag, &place, &origin.0) {
            Ok(pasted) => {
                for note in pasted.notes.iter().chain(&frag.dropped) {
                    info!("paste: {note}");
                }
                let label = format!("paste {}", type_label(&frag.doc, &frag.roots));
                pending.0.push(TransactionInput { label, actor: Actor::Human, ts: 0, idempotency_key: None, ops: pasted.ops, exact: false });
                tool.select_when_live = pasted.roots;
            }
            Err(e) => warn!("paste: {e}"),
        }
    }

    // Middle-drag pans the 2D view it began over, as in Figma, whatever the
    // tool. While it does, nothing hovers.
    match (pointer.buttons.middle, &tool.middle) {
        (true, None) => {
            let view = hits.panel.as_deref().and_then(|p| views.get(p)).filter(|v| v.dims == Dims::Two);
            tool.middle = Some(view.map(|v| MiddlePan {
                panel: v.panel.clone(),
                start: pointer.screen,
                pan: view_cameras.0.get(&v.panel).map_or(Vec2::ZERO, |c| c.pan),
            }));
        }
        (true, Some(Some(m))) => {
            view_cameras.0.entry(m.panel.clone()).or_default().pan = m.pan + (pointer.screen - m.start);
        }
        (false, Some(_)) => tool.middle = None,
        _ => {}
    }
    if matches!(tool.middle, Some(Some(_))) && matches!(tool.gesture, Gesture::Idle) {
        tool.hover = None;
        if sel.hovered.is_some() {
            sel.hovered = None;
        }
        if tool.cursor != "grabbing" {
            crate::web_ffi::send_cursor_from_worker("grabbing".into());
            tool.cursor = "grabbing".into();
        }
        return;
    }

    // Another tool has the pointer (`pen`, `shape_tool`); a gesture already
    // under way finishes.
    if *active != ActiveTool::Select && matches!(tool.gesture, Gesture::Idle) {
        if tool.hover.take().is_some() || sel.hovered.is_some() {
            sel.hovered = None;
        }
        if tool.cursor != "crosshair" {
            crate::web_ffi::send_cursor_from_worker("crosshair".into());
            tool.cursor = "crosshair".into();
        }
        return;
    }

    let now = time.elapsed_secs_f64();
    let screen = Point::new(pointer.screen.x as f64, pointer.screen.y as f64);
    let doc_point = |v: &PanelView| v.affine().inverse() * screen;

    // ---- Press ----
    if pointer.just_pressed_left && matches!(tool.gesture, Gesture::Idle) {
        tool.hover = None;
        let view = hits.panel.as_deref().and_then(|p| views.get(p)).cloned();
        let reaches = selected_reaches(&sel, tool);
        let geo = Geo { store: &store.0, map: &map, globals: &globals, nodes: &nodes };
        'press: {
            let Some(view) = view else { break 'press };
            // Space held: the press pans the view instead.
            if space && view.dims == Dims::Two {
                let pan = view_cameras.0.get(&view.panel).map_or(Vec2::ZERO, |c| c.pan);
                tool.gesture = Gesture::Pan { panel: view.panel.clone(), start: pointer.screen, pan };
                break 'press;
            }
            // A lone selected path shows its points and handles, which come
            // before the box's grips: a point drags through the store like
            // any node, a handle writes the point's `handles` leaves.
            if view.dims == Dims::Two
                && let [r] = movers(&reaches, &view.panel)[..]
                && geo.ty(r.node()) == "path"
                && let Some(space) = geo.space(&r.chain)
            {
                let path = r.node();
                let to_screen = view.affine() * space * geo.world(path);
                let mut hit: Option<(NodeId, Point, Option<bool>)> = None;
                for (pid, p, hi, ho) in geo.points(path) {
                    for (out, h) in [(true, ho), (false, hi)] {
                        if hit.is_none() && h != kurbo::Vec2::ZERO && (to_screen * (p + h)).distance(screen) <= POINT_HIT {
                            hit = Some((pid, p, Some(out)));
                        }
                    }
                    if hit.is_none() && (to_screen * p).distance(screen) <= POINT_HIT {
                        hit = Some((pid, p, None));
                    }
                }
                match hit {
                    Some((point, at, Some(out))) => {
                        tool.gesture = Gesture::Handle { point, out, to_local: to_screen.inverse(), at };
                        break 'press;
                    }
                    Some((point, _, None)) => {
                        let start = doc_point(&view);
                        let inv = space.inverse();
                        let anchor = geo.world(point).inverse() * (inv * start);
                        let h = store.0.begin_drag(point, [anchor.x, anchor.y]);
                        tool.gesture = if h.is_locked() { Gesture::Idle } else { Gesture::Move { label: "move point".into(), start, view: view.clone(), drags: vec![(h, inv)] } };
                        break 'press;
                    }
                    None => {}
                }
            }
            // A lone rect's corner dots and a lone ellipse's arc handles
            // come before the box's grips too.
            if view.dims == Dims::Two
                && let [r] = movers(&reaches, &view.panel)[..]
                && matches!(geo.ty(r.node()), "rect" | "ellipse")
                && let Some(space) = geo.space(&r.chain)
            {
                let id = r.node();
                let to_screen = view.affine() * space * geo.world(id);
                let hit = geo.shape_grips(id, to_screen).into_iter().find(|(_, p)| p.distance(screen) <= POINT_HIT);
                if let Some((grip, _)) = hit {
                    tool.gesture = Gesture::Shape { id, grip, to_local: to_screen.inverse(), arc: geo.arc(id) };
                    break 'press;
                }
            }
            // The selection's grips come first.
            if view.dims == Dims::Two {
                let mine = movers(&reaches, &view.panel);
                if let Some(frame) = geo.selection_frame(&mine) {
                    let resize_parts = {
                        let mut v = Vec::new();
                        for r in &mine {
                            geo.parts(&r.chain, true, &mut v);
                        }
                        v
                    };
                    let rotate_parts = {
                        let mut v = Vec::new();
                        for r in &mine {
                            geo.parts(&r.chain, false, &mut v);
                        }
                        v
                    };
                    let grip = grip_at(view.affine() * frame, screen, geo.resizable(&resize_parts), geo.rotatable(&rotate_parts));
                    let ids: Vec<NodeId> = mine.iter().map(|r| r.node()).collect();
                    let start = doc_point(&view);
                    match grip {
                        Some(Grip::Resize(u, v)) => {
                            tool.gesture = Gesture::Transform {
                                label: format!("resize {}", type_label(geo.doc(), &ids)),
                                view: view.clone(),
                                kind: TransformKind::Resize { grip: (u, v) },
                                frame,
                                start,
                                parts: resize_parts,
                                rot0: None,
                            };
                            break 'press;
                        }
                        Some(Grip::Rotate(..)) => {
                            let rot0 = (rotate_parts.len() == 1).then(|| rotate_parts[0].trs[2]);
                            tool.gesture = Gesture::Transform { label: format!("rotate {}", type_label(geo.doc(), &ids)), view: view.clone(), kind: TransformKind::Rotate, frame, start, parts: rotate_parts, rot0 };
                            break 'press;
                        }
                        _ => {}
                    }
                }
            }

            let top = top_hit(&hits);
            let shift = mods(MOD_SHIFT);
            let deep = mods(MOD_META);
            let here: Vec<&Reach> = reaches.iter().filter(|r| r.panel == view.panel).collect();
            let inside_box = view.dims == Dims::Two
                && geo.selection_frame(&movers(&reaches, &view.panel)).is_some_and(|f| matches!(grip_at(view.affine() * f, screen, false, false), Some(Grip::Body)));

            let Some((chain, source)) = top else {
                // Empty canvas: drag the selection from inside its box, or
                // draw a marquee over the panel's top level.
                if inside_box {
                    tool.gesture = Gesture::Press { panel: view.panel.clone(), start: pointer.screen, then: AfterPress::Move { click: Click::Clear } };
                } else if view.dims == Dims::Two {
                    let roots = nest::panel_roots(geo.doc(), &view);
                    tool.gesture = Gesture::Press { panel: view.panel.clone(), start: pointer.screen, then: AfterPress::Marquee { prefix: Vec::new(), roots, click: if shift { Click::Keep } else { Click::Clear } } };
                } else if !shift {
                    set_selection(&mut sel, tool, Vec::new());
                }
                break 'press;
            };

            let mut k = target_index(geo.doc(), &chain, &here, deep);
            // A double-click on a selected node enters it.
            let dbl = tool.last_click.is_some_and(|(t, p, id)| now - t <= DOUBLE_CLICK.0 && p.distance(pointer.screen) <= DOUBLE_CLICK.1 && id == chain[k]);
            if dbl && k + 1 < chain.len() && sel.selected.contains(&chain[k]) {
                k += 1;
                tool.last_click = None;
            } else if dbl && sel.selected == [chain[k]] && iron_document::shape::convertible(&store.0, chain[k]) {
                // A double-click on a lone rect or ellipse edits its points.
                tool.last_click = None;
                edit_as_path(&store.0, &mut pending, chain[k]);
                break 'press;
            }
            let target = Reach { panel: view.panel.clone(), chain: chain[..=k].to_vec() };
            let id = target.node();
            let is_selected = sel.selected.contains(&id);

            // Gestures the box does not cover: meshes, planes in 3D,
            // sliders, and anything in a 3D panel.
            let hit_entity = match source {
                Source::Overlay(i) => Some(hits.overlay[i].entity),
                Source::World(i) => Some(hits.world3d[i].entity),
                Source::Embed3d => None,
            };
            let through = match source {
                Source::Overlay(i) => hits.overlay[i].through,
                _ => None,
            };
            let entity = map.entity(id);
            let is_slider = entity.and_then(|e| nodes.get(e).ok()).is_some_and(|(d, _)| matches!(d.shape, Shape2d::Slider { .. }));
            let is_3d_target = matches!(source, Source::World(_)) && k + 1 == chain.len();
            let legacy = view.dims == Dims::Three || through.is_some() || is_3d_target || (is_slider && k + 1 == chain.len());
            if legacy {
                if !(shift && is_selected) {
                    let mut list = if shift { selected_reaches(&sel, tool) } else { Vec::new() };
                    if !list.contains(&target) {
                        list.push(target.clone());
                    }
                    set_selection(&mut sel, tool, list);
                }
                let Some(entity) = entity.or(hit_entity.filter(|_| k + 1 == chain.len())) else { break 'press };
                // A 3D node reached through a plane (the plane itself, not
                // its content) drags in 3D through the camera that saw it.
                let flat = nodes.contains(entity);
                let (camera, through) = match source {
                    Source::World(i) => (Some(hits.world3d[i].camera), None),
                    _ if !flat && through.is_some() => (through.map(|t| t.0), None),
                    _ => (None, through),
                };
                let space = match source {
                    Source::Overlay(i) => hits.overlay[i].space,
                    _ => Affine::IDENTITY,
                };
                begin_legacy(entity, id, view.panel.clone(), space, through, camera, is_slider, &pointer, &mut drag, &mut store, &provenance, &globals, &cameras, &views, &planes);
                tool.gesture = Gesture::Legacy;
                break 'press;
            }

            // A top-level embed's background: a drag draws a marquee over
            // its content, a click selects it (Figma's top-level frame).
            let artboard = chain.len() == 1 && nest::is_embed(geo.doc(), id) && !matches!(source, Source::Embed3d);
            if artboard && !is_selected && !inside_box {
                let roots = composition::embedded(geo.doc(), id).map(|s| geo.doc().children(s).to_vec()).unwrap_or_default();
                let click = if shift {
                    let mut list = selected_reaches(&sel, tool);
                    list.push(target.clone());
                    set_selection(&mut sel, tool, list);
                    Click::Keep
                } else {
                    Click::Only(target.clone())
                };
                tool.gesture = Gesture::Press { panel: view.panel.clone(), start: pointer.screen, then: AfterPress::Marquee { prefix: chain.clone(), roots, click } };
                tool.last_click = Some((now, pointer.screen, id));
                break 'press;
            }

            let click = if is_selected {
                if shift { Click::Remove(id) } else if here.len() > 1 { Click::Only(target.clone()) } else { Click::Keep }
            } else {
                let mut list = if shift { selected_reaches(&sel, tool) } else { Vec::new() };
                list.push(target.clone());
                set_selection(&mut sel, tool, list);
                Click::Keep
            };
            tool.last_click = Some((now, pointer.screen, id));
            tool.gesture = Gesture::Press { panel: view.panel.clone(), start: pointer.screen, then: AfterPress::Move { click } };
        }
    }

    // ---- Drag ----
    // While held; the release frame only closes the gesture, as the pose
    // the pointer last showed is what commits.
    if pointer.buttons.left {
        let reaches = selected_reaches(&sel, tool);
        let geo = Geo { store: &store.0, map: &map, globals: &globals, nodes: &nodes };
        // A press that has travelled becomes its drag.
        if let Gesture::Press { panel, start, then } = &tool.gesture
            && pointer.screen.distance(*start) > SLOP
            && let Some(view) = views.get(panel).cloned()
        {
            let p0 = view.affine().inverse() * Point::new(start.x as f64, start.y as f64);
            match then {
                AfterPress::Move { .. } => {
                    let mine = movers(&reaches, &view.panel);
                    let mut drags = Vec::new();
                    let ids: Vec<NodeId> = mine.iter().map(|r| r.node()).collect();
                    let label = format!("move {}", type_label(geo.doc(), &ids));
                    let grabs: Vec<(NodeId, Affine, Point)> = mine
                        .iter()
                        .filter_map(|r| {
                            let inv = geo.space(&r.chain)?.inverse();
                            Some((r.node(), inv, geo.world(r.node()).inverse() * (inv * p0)))
                        })
                        .collect();
                    for (id, inv, anchor) in grabs {
                        let h = store.0.begin_drag(id, [anchor.x, anchor.y]);
                        match &h.mode {
                            DragMode::Locked(reasons) | DragMode::Axes { locked: reasons, .. } => {
                                for why in reasons {
                                    info!("{id} locked for this drag: {why}");
                                }
                            }
                            DragMode::Cluster(c) => info!("{id} dragged through cluster {c}"),
                        }
                        if !h.is_locked() {
                            drags.push((h, inv));
                        }
                    }
                    tool.gesture = Gesture::Move { label, start: p0, view, drags };
                }
                AfterPress::Marquee { prefix, roots, click } => {
                    let base = if matches!(click, Click::Keep) { reaches.clone() } else { Vec::new() };
                    tool.gesture = Gesture::Marquee { view, start: p0, prefix: prefix.clone(), roots: roots.clone(), base };
                }
            }
        }
        match &tool.gesture {
            Gesture::Pan { panel, start, pan } => {
                view_cameras.0.entry(panel.clone()).or_default().pan = *pan + (pointer.screen - *start);
            }
            Gesture::Shape { id, grip, to_local, arc } => {
                let geo = Geo { store: &store.0, map: &map, globals: &globals, nodes: &nodes };
                let writes = shape_grip_writes(&geo, *id, *grip, *to_local * screen, *arc, mods(MOD_ALT), mods(MOD_SHIFT));
                preview_writes(&mut store.0, writes);
            }
            Gesture::Handle { point, out, to_local, at } => {
                let v = *to_local * screen - *at;
                let (mine, other) = if *out { (("handles.ox", "handles.oy"), ("handles.ix", "handles.iy")) } else { (("handles.ix", "handles.iy"), ("handles.ox", "handles.oy")) };
                let mut writes = vec![(*point, mine.0, v.x), (*point, mine.1, v.y)];
                // Smooth unless Alt breaks the handles apart.
                if !mods(MOD_ALT) {
                    writes.extend([(*point, other.0, -v.x), (*point, other.1, -v.y)]);
                }
                preview_writes(&mut store.0, writes);
            }
            Gesture::Move { start, view, drags, .. } => {
                let p = doc_point(view);
                let mut d = p - *start;
                if mods(MOD_SHIFT) {
                    if d.x.abs() > d.y.abs() { d.y = 0.0 } else { d.x = 0.0 }
                }
                let drags = drags.clone();
                for (h, inv) in &drags {
                    let q = *inv * (*start + d);
                    let t0 = bevy::platform::time::Instant::now();
                    let report = store.0.drag_to(h, [q.x, q.y]);
                    if let DragMode::Cluster(cluster) = &h.mode {
                        // CS phase 3 exit 9: the budget is 2 ms per frame.
                        let ms = t0.elapsed().as_secs_f64() * 1e3;
                        info!(
                            "solve: cluster {cluster}, {} vars, {} iterations, {} evaluations, {ms:.2} ms{}",
                            report.vars,
                            report.iterations,
                            report.evaluations,
                            if report.converged { "" } else { "; not converged, pose kept" }
                        );
                    }
                }
            }
            Gesture::Transform { view, kind, frame, start, parts, rot0, .. } => {
                let p = doc_point(view);
                let a = match *kind {
                    TransformKind::Resize { grip } => {
                        let u = frame.inverse() * p;
                        let anchor = if mods(MOD_ALT) { (0.5, 0.5) } else { (1.0 - grip.0, 1.0 - grip.1) };
                        // Relative to where the press landed, not the grip's
                        // exact corner, so pressing alone changes nothing.
                        let u0 = frame.inverse() * *start;
                        let scale = |g: f64, a: f64, u: f64, u0: f64| if g == 0.5 || (u0 - a).abs() < 1e-9 { 1.0 } else { (u - a) / (u0 - a) };
                        let (mut fx, mut fy) = (scale(grip.0, anchor.0, u.x, u0.x), scale(grip.1, anchor.1, u.y, u0.y));
                        if mods(MOD_SHIFT) {
                            let m = if grip.0 == 0.5 { fy.abs() } else if grip.1 == 0.5 { fx.abs() } else { fx.abs().max(fy.abs()) };
                            fx = m * if fx < 0.0 { -1.0 } else { 1.0 };
                            fy = m * if fy < 0.0 { -1.0 } else { 1.0 };
                        }
                        // Never collapse to zero: at least a pixel.
                        let size = (frame.as_coeffs()[0].hypot(frame.as_coeffs()[1]), frame.as_coeffs()[2].hypot(frame.as_coeffs()[3]));
                        let floor = |f: f64, s: f64| if f.abs() * s < 1.0 { (1.0 / s.max(1e-9)) * if f < 0.0 { -1.0 } else { 1.0 } } else { f };
                        let (fx, fy) = (floor(fx, size.0), floor(fy, size.1));
                        *frame * Affine::translate(anchor) * Affine::scale_non_uniform(fx, fy) * Affine::translate((-anchor.0, -anchor.1)) * frame.inverse()
                    }
                    TransformKind::Rotate => {
                        let c = *frame * Point::new(0.5, 0.5);
                        let mut th = (p - c).atan2() - (*start - c).atan2();
                        if mods(MOD_SHIFT) {
                            let base = rot0.unwrap_or(0.0);
                            th = ((base + th) / SNAP).round() * SNAP - base;
                        }
                        Affine::translate(c.to_vec2()) * Affine::rotate(th) * Affine::translate(-c.to_vec2())
                    }
                };
                let resize = matches!(kind, TransformKind::Resize { .. });
                let writes: Vec<(NodeId, &'static str, f64)> = parts.iter().flat_map(|part| part.apply(a, resize)).collect();
                preview_writes(&mut store.0, writes);
            }
            Gesture::Marquee { view, start, prefix, roots, base } => {
                let geo = Geo { store: &store.0, map: &map, globals: &globals, nodes: &nodes };
                let rect = Rect::from_points(*start, doc_point(view));
                tool.marquee = Some((view.panel.clone(), rect));
                let mut list = base.clone();
                for &n in roots {
                    if !geo.doc().live(n).is_some_and(|n| matches!(n.ty.name(), "rect" | "ellipse" | "path" | "slider" | "group" | "embed")) {
                        continue;
                    }
                    let mut chain = prefix.clone();
                    chain.push(n);
                    if geo.frame(&chain).is_some_and(|f| touches(f, rect)) {
                        let r = Reach { panel: view.panel.clone(), chain };
                        if !list.contains(&r) {
                            list.push(r);
                        }
                    }
                }
                set_selection(&mut sel, tool, list);
            }
            _ => {}
        }
    }

    // ---- Release ----
    if pointer.just_released_left {
        match std::mem::take(&mut tool.gesture) {
            Gesture::Press { then, .. } => {
                let click = match then {
                    AfterPress::Move { click } | AfterPress::Marquee { click, .. } => click,
                };
                match click {
                    Click::Keep => {}
                    Click::Only(r) => set_selection(&mut sel, tool, vec![r]),
                    Click::Remove(id) => {
                        let list = selected_reaches(&sel, tool).into_iter().filter(|r| r.node() != id).collect();
                        set_selection(&mut sel, tool, list);
                    }
                    Click::Clear => set_selection(&mut sel, tool, Vec::new()),
                }
            }
            Gesture::Move { label, drags, .. } => {
                for (h, _) in &drags {
                    store.0.end_drag(h);
                }
                gestures.0.push(GestureRequest::Commit(label));
            }
            Gesture::Transform { label, .. } => gestures.0.push(GestureRequest::Commit(label)),
            Gesture::Handle { .. } => gestures.0.push(GestureRequest::Commit("move handle".into())),
            Gesture::Shape { grip, .. } => {
                let label = if matches!(grip, ShapeGrip::Corner(_)) { "round corners" } else { "edit arc" };
                gestures.0.push(GestureRequest::Commit(label.into()));
            }
            Gesture::Marquee { .. } => tool.marquee = None,
            Gesture::Legacy => {
                if drag.target.is_some() {
                    commit.0 = drag.target;
                    drag.target = None;
                    drag.kind = None;
                    if let Some(h) = drag.handle.take() {
                        store.0.end_drag(&h);
                    }
                }
            }
            Gesture::Pan { .. } | Gesture::Idle => {}
        }
    }

    // ---- Hover and cursor, between gestures ----
    if matches!(tool.gesture, Gesture::Idle) {
        let geo = Geo { store: &store.0, map: &map, globals: &globals, nodes: &nodes };
        let reaches = selected_reaches(&sel, tool);
        let view = hits.panel.as_deref().and_then(|p| views.get(p));
        let mut cursor = if space { "grab" } else { "default" }.to_owned();
        let mut on_grip = false;
        if let Some(view) = view.filter(|v| v.dims == Dims::Two) {
            let mine = movers(&reaches, &view.panel);
            let cached = geometry_cache.selection(&geo, &view.panel, &mine, GeometryStamp::new(&store.0, geometry_revision.0), false);
            if let Some(frame) = cached.frame {
                let f = view.affine() * frame;
                let grip = grip_at(f, screen, cached.resizable, cached.rotatable);
                on_grip = matches!(grip, Some(Grip::Resize(..) | Grip::Rotate(..)));
                if !space {
                    cursor = cursor_for(f, grip);
                }
            }
        }
        let hover = match (view, top_hit(&hits)) {
            (Some(view), Some((chain, _))) if !on_grip => {
                let here: Vec<&Reach> = reaches.iter().filter(|r| r.panel == view.panel).collect();
                let k = target_index(geo.doc(), &chain, &here, mods(MOD_META));
                Some(Reach { panel: view.panel.clone(), chain: chain[..=k].to_vec() })
            }
            _ => None,
        };
        if tool.hover != hover {
            tool.hover = hover;
        }
        if tool.cursor != cursor {
            crate::web_ffi::send_cursor_from_worker(cursor.clone());
            tool.cursor = cursor;
        }
    } else {
        if !matches!(tool.gesture, Gesture::Press { .. }) && tool.hover.is_some() {
            tool.hover = None;
        }
        if matches!(tool.gesture, Gesture::Pan { .. }) && tool.cursor != "grabbing" {
            crate::web_ffi::send_cursor_from_worker("grabbing".into());
            tool.cursor = "grabbing".into();
        }
    }
    let hovered = tool.hover.as_ref().map(Reach::node);
    if sel.hovered != hovered {
        sel.hovered = hovered;
    }
}

/// Preview each leaf that differs from what it reads now. A locked leaf
/// holds still; why is logged at debug level, every frame it is asked.
/// What dragging a shape's handle to `p` (the shape's space) writes. A
/// corner dot sets every corner's radius (Alt: its own only) to the
/// pointer's distance along the corner's diagonal; the arc's handles set its
/// end or start by the pointer's angle (Shift: 15° steps) and its inner
/// ratio by the pointer's distance. Every arc leaf is written, so a commit
/// that adds the `arc` component carries all three.
fn shape_grip_writes(geo: &Geo, id: NodeId, grip: ShapeGrip, p: Point, arc: [f64; 3], alt: bool, shift: bool) -> Vec<(NodeId, &'static str, f64)> {
    use std::f64::consts::TAU;
    let Some((b, _)) = geo.local_box(id) else { return Vec::new() };
    let snap = |a: f64| if shift { (a / SNAP).round() * SNAP } else { a };
    match grip {
        ShapeGrip::Corner(i) => {
            let (u, v) = CORNERS[i];
            let corner = Point::new(b.x0 + u * b.width(), b.y0 + v * b.height());
            let d = p - corner;
            let r = ((d.x * (1.0 - 2.0 * u) + d.y * (1.0 - 2.0 * v)) * 0.5).clamp(0.0, b.width().min(b.height()) * 0.5);
            let all = ["corners.tl", "corners.tr", "corners.br", "corners.bl"];
            if alt { vec![(id, all[i], r)] } else { all.iter().map(|&leaf| (id, leaf, r)).collect() }
        }
        _ => {
            let (rx, ry) = (b.width().max(1e-9) * 0.5, b.height().max(1e-9) * 0.5);
            let t = snap((p.y / ry).atan2(p.x / rx));
            let [start, sweep, inner] = arc;
            // A sweep that closes up within a degree is the whole ellipse.
            let positive = |a: f64| {
                let a = a.rem_euclid(TAU);
                if a < 1f64.to_radians() || a > TAU - 1f64.to_radians() { TAU } else { a }
            };
            let (start, sweep, inner) = match grip {
                ShapeGrip::Sweep => (start, positive(t - start), inner),
                ShapeGrip::Start => (t, positive(start + sweep - t), inner),
                _ => (start, sweep, (p.x / rx).hypot(p.y / ry).clamp(0.0, 0.99)),
            };
            vec![(id, "arc.start", start), (id, "arc.sweep", sweep), (id, "arc.inner", inner)]
        }
    }
}

/// Edit a rect's or an ellipse's points: it becomes a path with the same id,
/// in one transaction (`iron_document::shape`). When that would cut a
/// binding or a clip, the shell asks first and converts on a yes.
fn edit_as_path(store: &Store, pending: &mut PendingTransactions, id: NodeId) {
    let ty = store.document().live(id).map_or("", |n| n.ty.name());
    match iron_document::shape::to_path(store, id) {
        Ok(c) if c.cuts.is_empty() => {
            pending.0.push(TransactionInput { label: format!("convert {ty} to path"), actor: Actor::Human, ts: 0, idempotency_key: None, ops: c.ops, exact: false });
        }
        Ok(c) => {
            info!("{id}: converting to a path would cut {}", c.cuts.join("; "));
            crate::web_ffi::send_convert_from_worker(serde_json::json!({"node": id.to_string(), "type": ty, "cuts": c.cuts}).to_string());
        }
        Err(e) => warn!("{e}"),
    }
}

pub(crate) fn preview_writes(store: &mut Store, writes: Vec<(NodeId, &'static str, f64)>) {
    for (id, path, v) in writes {
        let current = store.resolved(id, leaf(path)).and_then(|x| x.as_f64());
        if current.is_some_and(|c| (c - v).abs() < 1e-9) {
            continue;
        }
        if let Err(reason) = store.preview_target(id, leaf(path), Value::Number(v)) {
            debug!("{id}.{path}: {reason}");
        }
    }
}

/// Start a drag the box does not run: a mesh in 3D, content on a plane, a
/// slider's value.
#[allow(clippy::too_many_arguments)]
fn begin_legacy(
    entity: Entity,
    id: NodeId,
    panel: String,
    space: Affine,
    through: Option<(Entity, Entity)>,
    camera: Option<Entity>,
    slider: bool,
    pointer: &crate::PointerState,
    drag: &mut crate::DragState,
    store: &mut DocumentStore,
    provenance: &Query<&Provenance>,
    transforms: &Query<&GlobalTransform>,
    cameras: &DragCameras,
    views: &PanelViews,
    planes: &Query<(&GlobalTransform, &crate::document_bridge::planes::EmbedPlane)>,
) {
    let _ = provenance;
    drag.target = Some(entity);
    drag.handle = None;
    drag.panel = Some(panel);
    drag.space = space;
    drag.through = through;
    drag.camera = camera;
    drag.press_screen = pointer.screen;
    if slider {
        drag.kind = Some(crate::DragKind::SliderValue);
        return;
    }
    if camera.is_none() {
        // 2D content on a plane in 3D.
        let p = drag_point(drag, views, pointer.screen, cameras, planes).unwrap_or(Vec2::ZERO);
        let local = transforms.get(entity).map(|g| g.affine().inverse().transform_point3(p.extend(0.0))).unwrap_or(Vec3::ZERO);
        let h = store.0.begin_drag(id, [local.x as f64, local.y as f64]);
        match &h.mode {
            DragMode::Locked(reasons) => {
                for r in reasons {
                    info!("{id} locked for this drag: {r}");
                }
                drag.kind = None;
                drag.target = None;
            }
            _ => {
                drag.kind = Some(crate::DragKind::Overlay2D);
                drag.handle = Some(h);
            }
        }
        return;
    }
    if let Handle::Locked(reason) = store.0.handle(id, leaf("transform3d.pos")) {
        info!("{id}.transform3d.pos locked for this drag: {reason}");
        drag.kind = None;
        drag.target = None;
        return;
    }
    drag.kind = Some(crate::DragKind::World3D);
    // A camera-facing plane through the object; Ctrl drags on the ground.
    let Some((_, cam, cam_tf, rect, to_camera)) = camera.and_then(|c| cameras.get(c).ok()) else { return };
    let local = crate::bevy_app::scene3d::to_camera_px(pointer.screen, rect, to_camera);
    drag.plane_normal = if pointer.modifiers.ctrl { Vec3::Y } else { cam_tf.forward().as_vec3() }.normalize_or_zero();
    drag.plane_origin = transforms.get(entity).map(|t| t.translation()).unwrap_or(cam_tf.translation());
    drag.grab_offset_world = Vec3::ZERO;
    if let Some(ray) = crate::bevy_app::picking::camera_ray_from_window_px(cam, cam_tf, local)
        && let Some(hit) = intersect_ray_plane(ray, drag.plane_origin, drag.plane_normal)
    {
        if let Ok(t) = transforms.get(entity) {
            drag.grab_offset_world = t.translation() - hit;
        }
        drag.plane_origin = hit;
    }
}

/// The editor's feedback in each 2D panel: lit nodes' outlines and frames
/// (hover, selection and cross-probe highlights), the selection's box and
/// grips, path points, the marquee. Its own [`Overlay`] view over the
/// panel's, so a feedback change re-encodes only this list, never the
/// panel's content; an unchanged list stays clean.
#[derive(Component)]
pub struct SelectOverlay {
    panel: String,
}

#[allow(clippy::too_many_arguments)]
pub fn draw_select_overlay(
    mut commands: Commands,
    panel_layers: Query<(&DocumentPanelLayer, &VectorView), Without<SelectOverlay>>,
    mut overlays: Query<(Entity, &SelectOverlay, &mut DisplayList, &mut VectorView), Without<DocumentPanelLayer>>,
    tool: Res<SelectTool>,
    sel: Res<crate::SelectionState>,
    views: Res<PanelViews>,
    store: Res<DocumentStore>,
    map: Res<NodeMap>,
    globals: Query<&'static GlobalTransform>,
    nodes: Query<(&'static Doc2d, &'static GlobalTransform)>,
    (pen, shapes): (Res<crate::bevy_app::pen::PenTool>, Res<crate::bevy_app::shape_tool::ShapeTool>),
    active: Res<ActiveTool>,
    geometry_revision: Res<crate::document_bridge::GeometryRevision>,
    mut geometry_cache: ResMut<SelectionGeometryCache>,
    (highlights, spatial): (Res<crate::document_bridge::Highlights>, Res<crate::document_bridge::SpatialScene>),
) {
    geometry_cache.selection.retain(|panel, _| views.get(panel).is_some());
    geometry_cache.lit.retain(|panel, _| views.get(panel).is_some());
    for (e, o, ..) in &overlays {
        if !panel_layers.iter().any(|(l, _)| l.panel == o.panel) {
            commands.entity(e).despawn();
        }
    }
    for (layer, panel_view) in &panel_layers {
        match overlays.iter_mut().find(|(_, o, ..)| o.panel == layer.panel) {
            Some((.., mut v)) => {
                if *v != *panel_view {
                    *v = *panel_view;
                }
            }
            None => {
                let e = commands.spawn((DisplayList::default(), SelectOverlay { panel: layer.panel.clone() }, *panel_view, Overlay)).id();
                commands.entity(e).insert(VectorLayer::screen(order::EDITOR_OVERLAY).in_view(e));
            }
        }
    }
    let geo = Geo { store: &store.0, map: &map, globals: &globals, nodes: &nodes };
    let stamp = GeometryStamp::new(&store.0, geometry_revision.0);
    let reaches = selected_reaches(&sel, &tool);
    let blue = peniko::Color::new(BLUE);
    let thin = kurbo::Stroke::new(1.0);
    let square = Rect::new(0.0, 0.0, 1.0, 1.0).to_path(0.1);
    for (_, o, mut list, _) in &mut overlays {
        let Some(view) = views.get(&o.panel) else { continue };
        let to_screen = view.affine();
        // Lit leaves by their outline and lit containers by their box, in
        // screen px so the width holds under zoom. Frames already drawn as
        // the selection's box or the hover box are skipped.
        let boxed = reaches.iter().chain(tool.hover.as_ref()).filter(|r| r.panel == o.panel).map(|r| r.chain.clone()).collect();
        let lit: Vec<(peniko::Color, f64, kurbo::BezPath)> = geometry_cache
            .lit(&geo, view, spatial.embeds(), &highlights, boxed, stamp)
            .iter()
            .filter_map(|m| match &m.shape {
                LitShape::Outline(a, path) => {
                    let c = if m.selected { crate::document_bridge::LIT_SELECTED } else { crate::document_bridge::LIT_HOVERED };
                    Some((peniko::Color::new(c), 2.0, (to_screen * *a) * path.clone()))
                }
                LitShape::Frame(f) => Some((blue, if m.selected { 1.0 } else { 1.5 }, (to_screen * *f) * square.clone())),
            })
            .collect();
        let hover_frame = tool.hover.as_ref().filter(|h| h.panel == o.panel && !sel.selected.contains(&h.node()))
            .filter(|h| matches!(geo.ty(h.node()), "group" | "embed"))
            .and_then(|h| geometry_cache.hover(&geo, h, stamp));
        let mine = movers(&reaches, &o.panel);
        let cached = geometry_cache.selection(&geo, &o.panel, &mine, stamp, true);
        list.rebuild(|b| {
            b.clipped(Affine::IDENTITY, view.rect.to_kurbo(), |b| {
                for (c, width, path) in lit {
                    b.stroke(Affine::IDENTITY, kurbo::Stroke::new(width), c, path);
                }
                // The hovered node's box, for what has no outline of its own.
                if let Some(f) = hover_frame {
                    b.stroke(Affine::IDENTITY, kurbo::Stroke::new(1.5), blue, (to_screen * f) * square.clone());
                }
                if mine.len() > 1 {
                    for &f in &cached.frames {
                            b.stroke(Affine::IDENTITY, thin.clone(), blue, (to_screen * f) * square.clone());
                    }
                }
                if let Some(frame) = cached.frame {
                    let f = to_screen * frame;
                    b.stroke(Affine::IDENTITY, thin.clone(), blue, f * square.clone());
                    let fill = if cached.resizable { peniko::Color::new([1.0, 1.0, 1.0, 1.0]) } else { peniko::Color::new([0.8, 0.82, 0.85, 1.0]) };
                    let [a, bb, ..] = f.as_coeffs();
                    let angle = bb.atan2(a);
                    for (u, v) in [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
                        let c = f * Point::new(u, v);
                        let at = Affine::translate(c.to_vec2()) * Affine::rotate(angle);
                        let r = Rect::new(-HANDLE * 0.5, -HANDLE * 0.5, HANDLE * 0.5, HANDLE * 0.5);
                        b.fill(at, fill, r);
                        b.stroke(at, thin.clone(), blue, r);
                    }
                }
                // A path's points and handles: the lone selected path's, and
                // the one the pen is drawing.
                let white = peniko::Color::new([1.0, 1.0, 1.0, 1.0]);
                let draw_points = |b: &mut crate::vector::DisplayListBuilder<'_>, to_screen: Affine, path: NodeId| {
                    for (_, p, hi, ho) in geo.points(path) {
                        let c = to_screen * p;
                        for h in [hi, ho].into_iter().filter(|h| *h != kurbo::Vec2::ZERO) {
                            let e = to_screen * (p + h);
                            b.stroke(Affine::IDENTITY, thin.clone(), blue, kurbo::Line::new(c, e));
                            b.fill(Affine::IDENTITY, white, kurbo::Circle::new(e, HANDLE_END));
                            b.stroke(Affine::IDENTITY, thin.clone(), blue, kurbo::Circle::new(e, HANDLE_END));
                        }
                        let r = Rect::from_center_size(c, (POINT, POINT));
                        b.fill(Affine::IDENTITY, white, r);
                        b.stroke(Affine::IDENTITY, thin.clone(), blue, r);
                    }
                };
                // A lone shape's corner dots or arc handles.
                if *active == ActiveTool::Select
                    && let [r] = movers(&reaches, &o.panel)[..]
                    && matches!(geo.ty(r.node()), "rect" | "ellipse")
                    && let Some(space) = geo.space(&r.chain)
                {
                    for (_, at) in geo.shape_grips(r.node(), to_screen * space * geo.world(r.node())) {
                        b.fill(Affine::IDENTITY, white, kurbo::Circle::new(at, HANDLE_END));
                        b.stroke(Affine::IDENTITY, thin.clone(), blue, kurbo::Circle::new(at, HANDLE_END));
                    }
                }
                if *active == ActiveTool::Select
                    && let [r] = movers(&reaches, &o.panel)[..]
                    && geo.ty(r.node()) == "path"
                    && let Some(space) = geo.space(&r.chain)
                {
                    draw_points(b, to_screen * space * geo.world(r.node()), r.node());
                }
                if *active == ActiveTool::Pen
                    && let Some((path, panel, to_comp)) = pen.path.as_ref().filter(|(id, p, _)| p == &o.panel && geo.doc().live(*id).is_some())
                {
                    let _ = panel;
                    let to_screen = to_screen * to_comp.inverse() * geo.world(*path);
                    draw_points(b, to_screen, *path);
                    // The line from the last point to the pointer.
                    if let (Some((_, p, _, ho)), Some((_, at))) = (geo.points(*path).last().copied(), pen.pointer.as_ref().filter(|(pp, _)| pp == &o.panel)) {
                        let from = to_screen * p;
                        let mut line = kurbo::BezPath::new();
                        line.move_to(from);
                        if ho == kurbo::Vec2::ZERO { line.line_to(*at) } else { line.curve_to(to_screen * (p + ho), *at, *at) }
                        b.stroke(Affine::IDENTITY, thin.clone(), blue, line);
                    }
                }
                if *active == ActiveTool::Pen
                    && let Some((panel, start, now)) = &pen.press
                    && panel == &o.panel
                    && start.distance(*now) > 3.0
                {
                    let back = *start - (*now - *start);
                    b.stroke(Affine::IDENTITY, thin.clone(), blue, kurbo::Line::new(back, *now));
                    for e in [back, *now] {
                        b.fill(Affine::IDENTITY, white, kurbo::Circle::new(e, HANDLE_END));
                        b.stroke(Affine::IDENTITY, thin.clone(), blue, kurbo::Circle::new(e, HANDLE_END));
                    }
                }
                // The box the rectangle or ellipse tool is dragging out.
                if let Some(press) = shapes.press.as_ref().filter(|p| p.panel == o.panel)
                    && let Some(r) = press.rect
                {
                    let a = to_screen * press.to_comp.inverse();
                    let outline = if *active == ActiveTool::Ellipse { kurbo::Ellipse::from_rect(r).to_path(0.1) } else { r.to_path(0.1) };
                    b.stroke(Affine::IDENTITY, thin.clone(), blue, a * outline);
                }
                if let Some((p, r)) = &tool.marquee
                    && p == &o.panel
                {
                    let r = to_screen.transform_rect_bbox(*r);
                    b.fill(Affine::IDENTITY, peniko::Color::new([0.05, 0.6, 1.0, 0.08]), r);
                    b.stroke(Affine::IDENTITY, thin.clone(), blue, r);
                }
            });
        });
    }
}

/// Zoom limits, window px per document px.
const ZOOM: (f32, f32) = (0.05, 64.0);

/// Keep `doc` (document px) under `screen` (window px) at zoom `z`.
fn pan_keeping(view: &PanelView, doc: Vec2, screen: Vec2, z: f32) -> Vec2 {
    screen - Vec2::new(view.rect.x, view.rect.y) - doc * z
}

/// Pan and zoom 2D views, Figma's way: the wheel or a trackpad pans (Shift
/// turns vertical into horizontal), Ctrl/Cmd+wheel and pinch zoom about the
/// pointer, Shift+1 fits the content, Shift+2 fits the selection, Shift+0 is
/// 100%, +/− (with or without Cmd/Ctrl) zoom about the view's centre.
/// Space-drag and middle-drag pan (`select_tool_system`).
#[allow(clippy::too_many_arguments)]
pub fn navigate_views(
    pointer: Res<crate::PointerState>,
    hits: Res<crate::PointerHits>,
    scroll: Res<crate::bevy_app::AccumulatedScroll>,
    views: Res<PanelViews>,
    tool: Res<SelectTool>,
    sel: Res<crate::SelectionState>,
    mut keys: ResMut<ToolKeys>,
    mut cameras: ResMut<crate::document_bridge::ViewCameras>,
    store: Res<DocumentStore>,
    map: Res<NodeMap>,
    globals: Query<&'static GlobalTransform>,
    nodes: Query<(&'static Doc2d, &'static GlobalTransform)>,
) {
    let under = hits.panel.as_deref().and_then(|p| views.get(p)).filter(|v| v.dims == Dims::Two);
    if let Some(view) = under
        && scroll.delta != Vec2::ZERO
    {
        let px = match scroll.unit {
            bevy::input::mouse::MouseScrollUnit::Line => scroll.delta * 16.0,
            bevy::input::mouse::MouseScrollUnit::Pixel => scroll.delta,
        };
        let camera = cameras.0.entry(view.panel.clone()).or_default();
        if pointer.modifiers.ctrl || pointer.modifiers.meta {
            let doc = (pointer.screen - Vec2::new(view.rect.x, view.rect.y) - camera.pan) / camera.zoom;
            let z = (camera.zoom * (-px.y * 0.01).exp()).clamp(ZOOM.0, ZOOM.1);
            camera.pan = pan_keeping(view, doc, pointer.screen, z);
            camera.zoom = z;
        } else {
            let d = if pointer.modifiers.shift && px.x == 0.0 { Vec2::new(px.y, 0.0) } else { px };
            camera.pan -= d;
        }
    }

    // The zoom keys act on the view under the pointer, else the one holding
    // the selection, else the main viewer; Shift+2 prefers the selection's.
    let selection_view = sel.selected.first().and_then(|id| tool.reach.get(id)).and_then(|r| views.get(&r.panel));
    let to_selection = keys.0.iter().any(|(k, _)| k == "zoomsel");
    let target = (if to_selection { selection_view.or(under) } else { under.or(selection_view) })
        .or_else(|| views.get(crate::panels::VIEWER_PANEL))
        .filter(|v| v.dims == Dims::Two)
        .cloned();
    let mut rest = Vec::new();
    for (key, m) in std::mem::take(&mut keys.0) {
        let Some(view) = target.as_ref().filter(|_| matches!(key.as_str(), "zoomfit" | "zoomsel" | "zoom100" | "zoomin" | "zoomout")) else {
            rest.push((key, m));
            continue;
        };
        let size = Vec2::new(view.rect.w, view.rect.h);
        let centre = Vec2::new(view.rect.x, view.rect.y) + size * 0.5;
        let camera = cameras.0.entry(view.panel.clone()).or_default();
        let at_centre = (size * 0.5 - camera.pan) / camera.zoom;
        match key.as_str() {
            "zoomfit" | "zoomsel" => {
                let geo = Geo { store: &store.0, map: &map, globals: &globals, nodes: &nodes };
                // Boxes in the panel's document px: its top-level nodes, or
                // the selection as reached in this panel.
                let frames: Vec<Affine> = if key == "zoomsel" {
                    sel.selected.iter().filter_map(|id| tool.reach.get(id)).filter(|r| r.panel == view.panel).filter_map(|r| geo.frame(&r.chain)).collect()
                } else {
                    nest::panel_roots(&store.0.document(), view).iter().filter_map(|&n| geo.frame(&[n])).collect()
                };
                let Some(r) = frames
                    .iter()
                    .map(|f| f.transform_rect_bbox(Rect::new(0.0, 0.0, 1.0, 1.0)))
                    .reduce(|a, b| a.union(b))
                else {
                    continue;
                };
                let z = ((size.x / (r.width() as f32 * 1.1)).min(size.y / (r.height() as f32 * 1.1))).clamp(ZOOM.0, ZOOM.1);
                let c = Vec2::new(r.center().x as f32, r.center().y as f32);
                camera.pan = pan_keeping(view, c, centre, z);
                camera.zoom = z;
            }
            "zoom100" => {
                camera.pan = pan_keeping(view, at_centre, centre, 1.0);
                camera.zoom = 1.0;
            }
            _ => {
                let z = (camera.zoom * if key == "zoomin" { 2.0 } else { 0.5 }).clamp(ZOOM.0, ZOOM.1);
                camera.pan = pan_keeping(view, at_centre, centre, z);
                camera.zoom = z;
            }
        }
    }
    keys.0 = rest;
}

/// The selection's record is the store's session tier (a `selection` node
/// with `selects` relations, plans/specs/panels-and-session.md); `SelectionState`
/// and the tool's reaches are the frame's working copy. Whatever changed
/// the session elsewhere (a load, the shell, later undo) is read back;
/// otherwise a change here is written there, once per frame.
pub fn sync_session_selection(
    mut store: ResMut<DocumentStore>,
    mut sel: ResMut<crate::SelectionState>,
    mut tool: ResMut<SelectTool>,
    mut seen: Local<Option<u64>>,
) {
    let version = store.0.session_version();
    if seen.is_some_and(|v| v != version) {
        let doc = store.0.document();
        let mut reach = HashMap::default();
        let ids: Vec<NodeId> = store
            .0
            .selection()
            .into_iter()
            .map(|(id, data)| {
                let panel = data.get("panel").and_then(|v| v.as_str()).map(str::to_owned);
                let chain: Option<Vec<NodeId>> = data.get("chain").and_then(|v| v.as_str()).map(|c| c.split(' ').filter_map(|s| s.parse().ok()).collect());
                if let (Some(panel), Some(chain)) = (panel, chain)
                    && chain.last() == Some(&id)
                    && chain.iter().all(|&n| doc.live(n).is_some())
                {
                    reach.insert(id, Reach { panel, chain });
                }
                id
            })
            .collect();
        if sel.selected != ids {
            sel.selected = ids;
        }
        tool.reach = reach;
    } else if sel.is_changed() {
        let items: Vec<(NodeId, std::collections::BTreeMap<String, Value>)> = sel
            .selected
            .iter()
            .map(|id| {
                let mut data = std::collections::BTreeMap::new();
                if let Some(r) = tool.reach.get(id) {
                    data.insert("panel".to_owned(), Value::Str(r.panel.clone()));
                    data.insert("chain".to_owned(), Value::Str(r.chain.iter().map(|n| n.to_string()).collect::<Vec<_>>().join(" ")));
                }
                (*id, data)
            })
            .collect();
        if let Err(e) = store.0.set_selection(&items) {
            warn!("selection not recorded in the session: {e}");
        }
    }
    *seen = Some(store.0.session_version());
}

