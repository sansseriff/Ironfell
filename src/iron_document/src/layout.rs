//! Layout (plans/specs/layout-and-tables.md §2): groups that place their
//! children, through `iron_layout`.
//!
//! A group with a `layout` facet (mode not `none`) is a container. Its
//! children, minus `absolute` ones and bare empties, are its items; a child
//! group with a layout of its own is a nested container, laid out in the
//! same pass. Text is measured with `iron_text`, as `text::measure` does.
//!
//! The result is **published** under [`ProducerId::LAYOUT`], never stored:
//! each item's `transform2d.x`/`y`, the sizes a layout decides (`fill`
//! items, text whose width the layout chose, a hugging group's `size`).
//! So undo, save and the model's views see only what was authored, and a
//! laid-out position is locked against direct edits, as any published leaf
//! is. Layout reads what it does not publish, and beneath what it does
//! ([`base`]), so its own output never feeds back into it.

use std::collections::BTreeMap;

use iron_layout::{Align, Area, Ask, Container, Dim, Item, Justify, Mode, Node, Track, Tree};
use kurbo::Rect;

use crate::expr::Leaf;
use crate::facet::{FacetKind, LeafPath};
use crate::graph::ProducerId;
use crate::id::NodeId;
use crate::text::{self, Sizing};
use crate::{Store, TypeId, Value};

fn path(leaf: &str) -> LeafPath {
    leaf.parse().expect("declared leaf")
}

/// A leaf as layout must read it: beneath its own published value (the
/// authored constant, or the facet's default when the node lacks it).
pub fn base(store: &Store, id: NodeId, leaf: &str) -> Option<Value> {
    let p = path(leaf);
    match store.reactive.signal(Leaf { node: id, path: p }) {
        Some(s) if s.producer == ProducerId::LAYOUT => match store.doc.leaf(id, p) {
            Some(slot) => slot.constant().cloned(),
            None => p.facet.default_facet().get(p.field).and_then(|s| s.constant().cloned()),
        },
        _ => store.resolved(id, p),
    }
}

fn num(store: &Store, id: NodeId, leaf: &str, default: f64) -> f64 {
    base(store, id, leaf).and_then(|v| v.as_f64()).filter(|v| v.is_finite()).unwrap_or(default)
}

fn word(store: &Store, id: NodeId, leaf: &str) -> String {
    base(store, id, leaf).and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}

/// The container a group's `layout` facet describes, if it lays out.
fn container(store: &Store, id: NodeId) -> Option<Container> {
    store.doc.facet(id, FacetKind::Layout)?;
    let mode = match word(store, id, "layout.mode").as_str() {
        "stack_x" => Mode::StackX,
        "stack_y" => Mode::StackY,
        "grid" => Mode::Grid,
        _ => return None,
    };
    let f = |leaf: &str| num(store, id, leaf, 0.0).max(0.0) as f32;
    let (gap, row_gap) = (f("layout.gap"), f("layout.row_gap"));
    Some(Container {
        mode,
        // Taffy's gap is (between columns, between rows).
        gap: match mode {
            Mode::StackX => [gap, 0.0],
            Mode::StackY => [0.0, gap],
            Mode::Grid => [gap, row_gap],
        },
        pad: [f("layout.pad_t"), f("layout.pad_r"), f("layout.pad_b"), f("layout.pad_l")],
        align: align(&word(store, id, "layout.align")).unwrap_or_default(),
        justify: match word(store, id, "layout.justify").as_str() {
            "center" => Justify::Center,
            "end" => Justify::End,
            "between" => Justify::Between,
            _ => Justify::Start,
        },
        columns: tracks(&word(store, id, "layout.columns")),
        rows: tracks(&word(store, id, "layout.rows")),
    })
}

fn align(s: &str) -> Option<Align> {
    match s {
        "start" => Some(Align::Start),
        "center" => Some(Align::Center),
        "end" => Some(Align::End),
        "stretch" => Some(Align::Stretch),
        _ => None,
    }
}

/// `"auto 120 1fr 2fr"`: unreadable tokens are `auto`.
pub fn tracks(s: &str) -> Vec<Track> {
    s.split_whitespace()
        .map(|t| {
            if let Some(f) = t.strip_suffix("fr") {
                f.parse().map_or(Track::Auto, |f: f32| Track::Fr(f.max(0.0)))
            } else {
                t.trim_end_matches("px").parse().map_or(Track::Auto, |v: f32| Track::Px(v.max(0.0)))
            }
        })
        .collect()
}

/// What kind of item a node is, which decides what layout publishes for it.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    /// A group with a layout: its origin is its box's top-left.
    Container,
    /// A table (`table.rs`): a grid of its cells, its origin its top-left.
    Table,
    /// A table's cell: a text whose box is the whole cell, its words inset
    /// by the table's padding.
    Cell,
    Text,
    Rect,
    Ellipse,
    /// Anything else: placed by its bounds, never resized.
    Other,
}

/// One item's plan: where its box's top-left sits relative to its origin,
/// in its parent's space, for a fixed-size item.
struct Entry {
    id: NodeId,
    kind: Kind,
    offset: [f64; 2],
    fill: [bool; 2],
}

/// Whether a node takes part in its parent's layout.
fn is_item(store: &Store, id: NodeId) -> bool {
    let doc = &store.doc;
    let Some(n) = doc.live(id) else { return false };
    if !n.ty.spec().allows(FacetKind::Transform2d) || n.ty.name() == "empty" || n.ty.name() == "point" {
        return false;
    }
    base(store, id, "layout_item.absolute") != Some(Value::Bool(true))
}

/// A node's content box in its own local space: its geometry, or for a
/// group or `use`, its children's, through their transforms.
fn content_bounds(store: &Store, id: NodeId) -> Option<Rect> {
    if let Some(b) = crate::geom::local_bounds(store, id) {
        return Some(b.geometry);
    }
    let read = |l: Leaf| store.resolved(l.node, l.path);
    let mut out: Option<Rect> = None;
    for &c in store.doc.children(id) {
        let Some(r) = content_bounds(store, c) else { continue };
        let a = crate::geom::local_affine(&store.doc, &read, c);
        let pts = [[r.x0, r.y0], [r.x1, r.y0], [r.x0, r.y1], [r.x1, r.y1]].map(|p| a.apply(p));
        let (x0, x1) = pts.iter().map(|p| p[0]).fold((f64::MAX, f64::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
        let (y0, y1) = pts.iter().map(|p| p[1]).fold((f64::MAX, f64::MIN), |(lo, hi), y| (lo.min(y), hi.max(y)));
        let r = Rect::new(x0, y0, x1, y1);
        out = Some(out.map_or(r, |o| o.union(r)));
    }
    out
}

/// A fixed-size item's box under its own rotation and scale (translation
/// left out): its size, and where its top-left sits from its origin.
fn placed_box(store: &Store, id: NodeId, local: Rect) -> ([f64; 2], [f64; 2]) {
    let t = |f: &str, d: f64| num(store, id, &format!("transform2d.{f}"), d);
    let a = crate::geom::Affine2::from_trs(0.0, 0.0, t("rot", 0.0), t("sx", 1.0), t("sy", 1.0));
    let pts = [[local.x0, local.y0], [local.x1, local.y0], [local.x0, local.y1], [local.x1, local.y1]].map(|p| a.apply(p));
    let (x0, x1) = pts.iter().map(|p| p[0]).fold((f64::MAX, f64::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
    let (y0, y1) = pts.iter().map(|p| p[1]).fold((f64::MAX, f64::MIN), |(lo, hi), y| (lo.min(y), hi.max(y)));
    ([x1 - x0, y1 - y0], [x0, y0])
}

/// Lay a text out at `width` (`None`: unwrapped), as `text::measure` would.
fn text_layout(store: &Store, id: NodeId, width: Option<f64>) -> std::sync::Arc<iron_text::TextLayout> {
    iron_text::engine().layout(&text::content(store, id), &text::style(store, id), &text::runs(store, id), width)
}

fn sizing(store: &Store, id: NodeId) -> Sizing {
    Sizing::parse(&word(store, id, "text.sizing"))
}

struct Builder<'s> {
    store: &'s Store,
    tree: Tree,
    entries: Vec<Entry>,
    /// Each table's cell areas, from `table::place`.
    tables: BTreeMap<NodeId, Vec<crate::table::Area>>,
}

impl Builder<'_> {
    /// Add `id` and, for a container, its items; returns its index.
    fn add(&mut self, id: NodeId, kind: Kind) -> usize {
        let store = self.store;
        let fill = |leaf: &str| word(store, id, leaf) == "fill";
        let mut item = Item { align_self: align(&word(store, id, "layout_item.align")), ..Item::default() };
        let mut fills = [fill("layout_item.w"), fill("layout_item.h")];
        if store.doc.facet(id, FacetKind::LayoutItem).is_some() {
            let place = |start: &str, span: &str| {
                let s = num(store, id, start, 0.0).round();
                (s >= 1.0).then(|| Area { start: s as u16 - 1, span: num(store, id, span, 1.0).round().max(1.0) as u16 })
            };
            item.column = place("layout_item.column", "layout_item.span_x");
            item.row = place("layout_item.row", "layout_item.span_y");
        }
        let mut offset = [0.0, 0.0];
        let mut measured = false;
        let mut container_spec = None;
        match kind {
            Kind::Table => {
                let width = num(store, id, "table.width", 0.0);
                if width > 0.0 {
                    item.w = Dim::Px(width as f32);
                }
                let mut columns = tracks(&word(store, id, "table.columns"));
                if columns.is_empty() {
                    columns.push(Track::Auto);
                }
                container_spec = Some(Container { mode: Mode::Grid, gap: [0.0, 0.0], pad: [0.0; 4], align: Align::Stretch, justify: Justify::Start, columns, rows: Vec::new() });
            }
            Kind::Cell => measured = true,
            Kind::Container => {
                let fixed = |leaf: &str, size: &str, d: f64| (word(store, id, leaf) == "fixed").then(|| Dim::Px(num(store, id, size, d) as f32));
                item.w = fixed("layout.sizing_w", "size.w", 100.0).unwrap_or(Dim::Auto);
                item.h = fixed("layout.sizing_h", "size.h", 100.0).unwrap_or(Dim::Auto);
                container_spec = container(store, id);
            }
            Kind::Text => match sizing(store, id) {
                Sizing::AutoWidth => measured = true,
                Sizing::AutoHeight => {
                    item.w = Dim::Px(num(store, id, "size.w", 100.0) as f32);
                    measured = true;
                }
                Sizing::Fixed => {
                    item.w = Dim::Px(num(store, id, "size.w", 100.0) as f32);
                    item.h = Dim::Px(num(store, id, "size.h", 100.0) as f32);
                }
            },
            Kind::Rect => {
                let local = Rect::new(0.0, 0.0, num(store, id, "size.w", 100.0), num(store, id, "size.h", 100.0));
                let (size, at) = placed_box(store, id, local);
                (item.w, item.h, offset) = (Dim::Px(size[0] as f32), Dim::Px(size[1] as f32), at);
            }
            Kind::Ellipse => {
                let (rx, ry) = (num(store, id, "radius.rx", 50.0), num(store, id, "radius.ry", 50.0));
                let (size, at) = placed_box(store, id, Rect::new(-rx, -ry, rx, ry));
                (item.w, item.h, offset) = (Dim::Px(size[0] as f32), Dim::Px(size[1] as f32), at);
            }
            Kind::Other => {
                fills = [false, false];
                let local = content_bounds(store, id).unwrap_or(Rect::ZERO);
                let (size, at) = placed_box(store, id, local);
                (item.w, item.h, offset) = (Dim::Px(size[0] as f32), Dim::Px(size[1] as f32), at);
            }
        }
        if fills[0] {
            item.w = Dim::Fill;
        }
        if fills[1] {
            item.h = Dim::Fill;
        }
        let index = self.tree.nodes.len();
        self.tree.nodes.push(Node { key: id.raw(), item, container: container_spec, children: Vec::new(), measured });
        self.entries.push(Entry { id, kind, offset, fill: fills });
        if kind == Kind::Container {
            let kids: Vec<NodeId> = store.doc.children(id).iter().copied().filter(|&c| is_item(store, c)).collect();
            for c in kids {
                let k = kind_of(store, c);
                let i = self.add(c, k);
                self.tree.nodes[index].children.push(i);
            }
        }
        if kind == Kind::Table {
            // Cells in the areas `place` gives them, so merged cells and the
            // lines drawn around them agree.
            let (areas, _) = crate::table::place(store, id);
            for a in &areas {
                let i = self.add(a.id, Kind::Cell);
                self.tree.nodes[i].item.column = Some(Area { start: a.col as u16, span: a.span_x as u16 });
                self.tree.nodes[i].item.row = Some(Area { start: a.row as u16, span: a.span_y as u16 });
                self.tree.nodes[index].children.push(i);
            }
            self.tables.insert(id, areas);
        }
        index
    }
}

fn kind_of(store: &Store, id: NodeId) -> Kind {
    match store.doc.live(id).map(|n| n.ty.name()) {
        Some("group") if container(store, id).is_some() => Kind::Container,
        Some("table") => Kind::Table,
        Some("text") => Kind::Text,
        Some("rect") => Kind::Rect,
        Some("ellipse") => Kind::Ellipse,
        _ => Kind::Other,
    }
}

/// Every laid-out group's and table's placements, as the layout producer
/// publishes them, and every table's grid.
pub fn compute(store: &Store) -> (BTreeMap<Leaf, Value>, BTreeMap<NodeId, crate::table::Grid>) {
    let mut out = BTreeMap::new();
    let mut grids = BTreeMap::new();
    let doc = &store.doc;
    let group = TypeId::lookup("group").expect("registered");
    let table = TypeId::lookup("table").expect("registered");
    let is_container = |k: Kind| matches!(k, Kind::Container | Kind::Table);
    // Outermost containers: a container inside another's layout is laid
    // out in its parent's pass.
    let roots: Vec<(NodeId, Kind)> = doc
        .nodes_of_type(group)
        .filter(|&g| doc.is_live(g) && doc.facet(g, FacetKind::Layout).is_some() && container(store, g).is_some())
        .map(|g| (g, Kind::Container))
        .chain(doc.nodes_of_type(table).filter(|&t| doc.is_live(t)).map(|t| (t, Kind::Table)))
        .filter(|&(g, _)| !(doc.live(g).and_then(|n| n.parent).is_some_and(|p| is_container(kind_of(store, p))) && is_item(store, g)))
        .collect();
    for (root, kind) in roots {
        let mut b = Builder { store, tree: Tree::default(), entries: Vec::new(), tables: BTreeMap::new() };
        b.tree.root = b.add(root, kind);
        let by_key: BTreeMap<u64, (NodeId, [f64; 2])> = b.entries.iter().map(|e| (e.id.raw(), (e.id, if e.kind == Kind::Cell { text::cell_pad(store, e.id) } else { [0.0, 0.0] }))).collect();
        let result = b.tree.layout([None, None], &mut |key, ask: Ask| {
            let (id, [px, py]) = by_key[&key];
            let width = ask.known[0].map(f64::from).or(if ask.min_content { Some(0.0) } else { ask.available[0].map(f64::from) }).map(|w| (w - 2.0 * px).max(0.0));
            let natural = text_layout(store, id, None);
            let l = match width {
                Some(w) if w < natural.width => text_layout(store, id, Some(w)),
                _ => natural,
            };
            [ask.known[0].unwrap_or((l.width + 2.0 * px) as f32), ask.known[1].unwrap_or((l.height + 2.0 * py) as f32)]
        });
        for (&t, areas) in &b.tables {
            let tracks = result.tracks.get(&t.raw()).cloned().unwrap_or_default();
            let f = |v: &[(f32, f32)]| v.iter().map(|&(a, b)| (a as f64, b as f64)).collect();
            grids.insert(t, crate::table::Grid { columns: f(&tracks.columns), rows: f(&tracks.rows), cells: areas.clone() });
        }
        for (i, e) in b.entries.iter().enumerate() {
            let Some(p) = result.placed.get(&e.id.raw()) else { continue };
            let (x, y, w, h) = (p.x as f64, p.y as f64, p.w as f64, p.h as f64);
            let mut put = |leaf: &str, v: f64| {
                out.insert(Leaf { node: e.id, path: path(leaf) }, Value::Number(v));
            };
            if i != b.tree.root {
                let off = match (e.kind, e.fill) {
                    // A resized ellipse is centred on its new box.
                    (Kind::Ellipse, [fw, fh]) if fw || fh => [if fw { -w / 2.0 } else { e.offset[0] }, if fh { -h / 2.0 } else { e.offset[1] }],
                    _ => e.offset,
                };
                put("transform2d.x", x - off[0]);
                put("transform2d.y", y - off[1]);
            }
            match e.kind {
                Kind::Table => {
                    put("size.w", w);
                    put("size.h", h);
                }
                Kind::Cell => {
                    out.insert(Leaf { node: e.id, path: path("text.sizing") }, Value::Enum("fixed".into()));
                    out.insert(Leaf { node: e.id, path: path("size.w") }, Value::Number(w));
                    out.insert(Leaf { node: e.id, path: path("size.h") }, Value::Number(h));
                }
                Kind::Container => {
                    if word(store, e.id, "layout.sizing_w") != "fixed" || e.fill[0] {
                        put("size.w", w);
                    }
                    if word(store, e.id, "layout.sizing_h") != "fixed" || e.fill[1] {
                        put("size.h", h);
                    }
                }
                Kind::Rect => {
                    if e.fill[0] {
                        put("size.w", w);
                    }
                    if e.fill[1] {
                        put("size.h", h);
                    }
                }
                Kind::Ellipse => {
                    if e.fill[0] {
                        put("radius.rx", w / 2.0);
                    }
                    if e.fill[1] {
                        put("radius.ry", h / 2.0);
                    }
                }
                Kind::Text => {
                    // The layout chose this text's width (a column, a fill,
                    // a narrower stack): it wraps there, as auto height.
                    let authored = sizing(store, e.id);
                    let chosen = match authored {
                        Sizing::AutoWidth => (w - text_layout(store, e.id, None).width).abs() > 0.01,
                        Sizing::AutoHeight => (w - num(store, e.id, "size.w", 100.0)).abs() > 0.01,
                        Sizing::Fixed => e.fill[0] || e.fill[1],
                    };
                    if e.fill[1] {
                        out.insert(Leaf { node: e.id, path: path("text.sizing") }, Value::Enum("fixed".into()));
                        out.insert(Leaf { node: e.id, path: path("size.w") }, Value::Number(w));
                        out.insert(Leaf { node: e.id, path: path("size.h") }, Value::Number(h));
                    } else if chosen {
                        if authored != Sizing::AutoHeight {
                            out.insert(Leaf { node: e.id, path: path("text.sizing") }, Value::Enum("auto_height".into()));
                        }
                        out.insert(Leaf { node: e.id, path: path("size.w") }, Value::Number(w));
                    }
                }
                Kind::Other => {}
            }
        }
    }
    (out, grids)
}

/// Add auto layout (Figma's Shift+A; layout-and-tables.md §2.4): a group
/// that has no layout gets one; siblings are wrapped in a new laid-out
/// group at their top-left. The direction is the one they are spread
/// along, the gap the mean of the gaps between them, and they are ordered
/// as they sit, so the layout keeps the arrangement it was given. One
/// transaction's ops, and the group.
pub fn add_ops(doc: &crate::Document, extent: &dyn Fn(NodeId) -> Option<Rect>, selection: &[NodeId]) -> Result<(Vec<crate::Op>, NodeId), String> {
    use crate::facet::{Layout, LeafStruct, Name, Transform2d};
    use crate::{Op, OrderKey, Slot};
    let live: Vec<NodeId> = selection.iter().copied().filter(|&n| doc.is_live(n)).collect();
    // How a set of boxes is arranged: the axis they spread along, and the
    // mean gap between neighbours on it.
    let arrange = |boxes: &mut Vec<(NodeId, Rect)>| -> (&'static str, f64) {
        let spread = |f: &dyn Fn(&Rect) -> f64| {
            let (lo, hi) = boxes.iter().map(|(_, r)| f(r)).fold((f64::MAX, f64::MIN), |(lo, hi), v| (lo.min(v), hi.max(v)));
            hi - lo
        };
        let across = spread(&|r: &Rect| r.center().x) > spread(&|r: &Rect| r.center().y);
        boxes.sort_by(|a, b| if across { a.1.x0.total_cmp(&b.1.x0) } else { a.1.y0.total_cmp(&b.1.y0) });
        let gaps: Vec<f64> = boxes.windows(2).map(|w| if across { w[1].1.x0 - w[0].1.x1 } else { w[1].1.y0 - w[0].1.y1 }).collect();
        let gap = if gaps.is_empty() { 0.0 } else { (gaps.iter().sum::<f64>() / gaps.len() as f64).max(0.0).round() };
        (if across { "stack_x" } else { "stack_y" }, gap)
    };
    let facet = |mode: &str, gap: f64| {
        let mut l = Layout::default();
        l.mode = Slot::Const(Value::Enum(mode.into()));
        l.gap = Slot::Const(Value::Number(gap));
        l
    };
    if let [g] = live[..]
        && doc.live(g).is_some_and(|n| n.ty.name() == "group")
    {
        let mut boxes: Vec<(NodeId, Rect)> = doc.children(g).iter().filter_map(|&c| extent(c).map(|r| (c, r))).collect();
        let (mode, gap) = arrange(&mut boxes);
        let mut ops = Vec::new();
        match doc.facet(g, FacetKind::Layout) {
            Some(_) => {
                ops.push(Op::Set { id: g, path: path("layout.mode"), slot: Slot::Const(Value::Enum(mode.into())) });
                ops.push(Op::Set { id: g, path: path("layout.gap"), slot: Slot::Const(Value::Number(gap)) });
            }
            None => ops.push(Op::AddFacet { id: g, facet: facet(mode, gap).wrap() }),
        }
        // Children in the order they sit, which is the order they flow in.
        let mut key = OrderKey::first();
        for (c, _) in &boxes {
            ops.push(Op::Reorder { id: *c, order: key.clone() });
            key = OrderKey::after(&key);
        }
        return Ok((ops, g));
    }
    if live.is_empty() {
        return Err("nothing selected to lay out".into());
    }
    let parent = doc.live(live[0]).and_then(|n| n.parent).ok_or("lay out shapes that sit in a composition")?;
    if live.iter().any(|&r| doc.live(r).and_then(|n| n.parent) != Some(parent)) {
        return Err("the selection is in different groups; lay out siblings".into());
    }
    let mut boxes: Vec<(NodeId, Rect)> = live.iter().filter_map(|&n| extent(n).map(|r| (n, r))).collect();
    let all = boxes.iter().map(|(_, r)| *r).reduce(|a, b| a.union(b)).ok_or("the selection has no extent")?;
    let (mode, gap) = arrange(&mut boxes);
    let id = doc.next_id();
    let mut t = Transform2d::default();
    t.x = Slot::Const(Value::Number(all.x0));
    t.y = Slot::Const(Value::Number(all.y0));
    let mut name = Name::default();
    name.text = Slot::Const(Value::Str("stack".into()));
    let first = live.iter().filter_map(|&n| doc.live(n)).map(|n| n.order.clone()).min().expect("live");
    let mut ops = vec![Op::Create { id, ty: TypeId::lookup("group").expect("registered"), parent: Some(parent), order: OrderKey::before(&first), facets: vec![t.wrap(), facet(mode, gap).wrap(), name.wrap()] }];
    let mut key = OrderKey::first();
    for (n, _) in &boxes {
        ops.push(Op::Reparent { id: *n, parent: Some(id), order: key.clone() });
        key = OrderKey::after(&key);
        // Into the group's space, so leaving the layout (absolute) keeps
        // each where it was.
        for (field, d) in [("transform2d.x", all.x0), ("transform2d.y", all.y0)] {
            if let Some(v) = doc.leaf(*n, path(field)).and_then(|s| s.constant()).and_then(|v| v.as_f64()) {
                ops.push(Op::Set { id: *n, path: path(field), slot: Slot::Const(Value::Number(v - d)) });
            }
        }
    }
    Ok((ops, id))
}
