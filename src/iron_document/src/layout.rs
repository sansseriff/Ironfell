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

use crate::tree::Tree as DocTree;
use std::collections::{BTreeMap, BTreeSet};

use iron_layout::{Align, Area, Ask, Container, Dim, Item, Justify, Mode, Node, Track, Tree};
use kurbo::Rect;

use crate::expr::Leaf;
use crate::facet::{FacetKind, LeafPath};
use crate::id::NodeId;
use crate::text::{self, Sizing};
use crate::graph::ProducerId;
use crate::reads::Reads;
use crate::{TypeId, Value};

fn path(leaf: &str) -> LeafPath {
    leaf.parse().expect("declared leaf")
}

/// A leaf as layout must read it: beneath its own published value (the
/// authored constant, or the facet's default when the node lacks it).
pub fn base(store: &dyn Reads, id: NodeId, leaf: &str) -> Option<Value> {
    store.layout_base(id, path(leaf))
}

fn num(store: &dyn Reads, id: NodeId, leaf: &str, default: f64) -> f64 {
    base(store, id, leaf).and_then(|v| v.as_f64()).filter(|v| v.is_finite()).unwrap_or(default)
}

fn word(store: &dyn Reads, id: NodeId, leaf: &str) -> String {
    base(store, id, leaf).and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}

/// A container laid out in a previewed order (`Store::set_layout_preview`):
/// its children as `order` lists them (others after, as they are), and
/// `floating`, if set, published where the pointer holds it, in the
/// container's space, while still taking its place in the flow.
#[derive(Clone, Debug, PartialEq)]
pub struct Preview {
    pub container: NodeId,
    pub order: Vec<NodeId>,
    pub floating: Option<(NodeId, [f64; 2])>,
}

/// The container a group's `layout` facet describes, if it lays out.
fn container(store: &dyn Reads, id: NodeId) -> Option<Container> {
    if !store.tree().has(id, FacetKind::Layout) {
        return None;
    }
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
fn is_item(store: &dyn Reads, id: NodeId) -> bool {
    let doc = store.tree();
    let Some(n) = doc.live(id) else { return false };
    if !n.ty.spec().allows(FacetKind::Transform2d) || n.ty.name() == "empty" || n.ty.name() == "point" {
        return false;
    }
    base(store, id, "layout_item.absolute") != Some(Value::Bool(true))
}

/// A node's content box in its own local space: its geometry, or for a
/// group or `use`, its children's, through their transforms.
fn content_bounds(store: &dyn Reads, id: NodeId) -> Option<Rect> {
    if let Some(b) = crate::geom::local_bounds(store, id) {
        return Some(b.geometry);
    }
    let read = |l: Leaf| store.resolved(l.node, l.path);
    let mut out: Option<Rect> = None;
    for &c in store.tree().children(id).iter() {
        let Some(r) = content_bounds(store, c) else { continue };
        let a = crate::geom::local_affine(store.tree(), &read, c);
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
fn placed_box(store: &dyn Reads, id: NodeId, local: Rect) -> ([f64; 2], [f64; 2]) {
    let t = |f: &str, d: f64| num(store, id, &format!("transform2d.{f}"), d);
    let a = crate::geom::Affine2::from_trs(0.0, 0.0, t("rot", 0.0), t("sx", 1.0), t("sy", 1.0));
    let pts = [[local.x0, local.y0], [local.x1, local.y0], [local.x0, local.y1], [local.x1, local.y1]].map(|p| a.apply(p));
    let (x0, x1) = pts.iter().map(|p| p[0]).fold((f64::MAX, f64::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
    let (y0, y1) = pts.iter().map(|p| p[1]).fold((f64::MAX, f64::MIN), |(lo, hi), y| (lo.min(y), hi.max(y)));
    ([x1 - x0, y1 - y0], [x0, y0])
}

/// Lay a text out at `width` (`None`: unwrapped), as `text::measure` would.
fn text_layout(store: &dyn Reads, id: NodeId, width: Option<f64>) -> std::sync::Arc<iron_text::TextLayout> {
    iron_text::engine().layout(&text::content(store, id), &text::style(store, id), &text::runs(store, id), width)
}

fn sizing(store: &dyn Reads, id: NodeId) -> Sizing {
    Sizing::parse(&word(store, id, "text.sizing"))
}

struct Builder<'s> {
    store: &'s dyn Reads,
    /// The run's root: a contained box is a leaf of every run but its own.
    root: NodeId,
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
        if store.tree().has(id, FacetKind::LayoutItem) {
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
            Kind::Container if contained(store, id) && id != self.root => {
                // A contained box (reactive-engine.md §9.2): a leaf of its
                // own size here, sized as if empty where it hugs; its
                // interior is a run of its own, after this one.
                let pad = |a: &str, b: &str| num(store, id, a, 0.0).max(0.0) + num(store, id, b, 0.0).max(0.0);
                let own = |leaf: &str, size: &str, d: f64, empty: f64| Dim::Px(if word(store, id, leaf) == "fixed" { num(store, id, size, d) } else { empty } as f32);
                item.w = own("layout.sizing_w", "size.w", 100.0, pad("layout.pad_l", "layout.pad_r"));
                item.h = own("layout.sizing_h", "size.h", 100.0, pad("layout.pad_t", "layout.pad_b"));
                item.keep = true;
            }
            Kind::Container if contained(store, id) => {
                // Its interior, at the size its parent gave it.
                // Allocated by its parent's run when it is an item there (fill
                // included); else its own: fixed, or as if empty.
                let allocated = store.tree().live(id).and_then(|n| n.parent).is_some_and(|p| matches!(kind_of(store, p), Kind::Container | Kind::Table)) && is_item(store, id);
                let pad = |a: &str, b: &str| num(store, id, a, 0.0).max(0.0) + num(store, id, b, 0.0).max(0.0);
                let size = |leaf: &str, size: &str, d: f64, empty: f64| {
                    if allocated {
                        store.resolved(id, path(size)).and_then(|v| v.as_f64()).unwrap_or(empty)
                    } else if word(store, id, leaf) == "fixed" {
                        num(store, id, size, d)
                    } else {
                        empty
                    }
                };
                item.w = Dim::Px(size("layout.sizing_w", "size.w", 100.0, pad("layout.pad_l", "layout.pad_r")) as f32);
                item.h = Dim::Px(size("layout.sizing_h", "size.h", 100.0, pad("layout.pad_t", "layout.pad_b")) as f32);
                // Its parent's run gave it its size: nothing fills here.
                fills = [false, false];
                container_spec = container(store, id);
            }
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
        if kind == Kind::Container && self.tree.nodes[index].container.is_some() {
            let mut kids: Vec<NodeId> = store.tree().children(id).iter().copied().filter(|&c| is_item(store, c)).collect();
            if let Some(p) = store.layout_preview().filter(|p| p.container == id) {
                kids.sort_by_key(|k| p.order.iter().position(|o| o == k).unwrap_or(usize::MAX));
            }
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

/// Whether a group is contained (`layout.contain = both`).
fn contained(store: &dyn Reads, id: NodeId) -> bool {
    word(store, id, "layout.contain") == "both"
}

fn kind_of(store: &dyn Reads, id: NodeId) -> Kind {
    match store.tree().live(id).map(|n| n.ty.name()) {
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
/// What layout reads while it runs: what this run has placed already, over
/// the store. A root nested in a plain group inside another root is laid out
/// first, and the outer one reads its placements through the group's bounds
/// in the same run (reactive-engine handoff §4.10, RE-5.4 step 2).
struct Fresh<'a> {
    store: &'a dyn Reads,
    out: &'a BTreeMap<Leaf, Value>,
}

impl Reads for Fresh<'_> {
    fn tree(&self) -> DocTree<'_> {
        self.store.tree()
    }
    fn resolved(&self, id: NodeId, path: LeafPath) -> Option<Value> {
        self.out.get(&Leaf { node: id, path }).cloned().or_else(|| self.store.resolved(id, path))
    }
    fn layout_base(&self, id: NodeId, path: LeafPath) -> Option<Value> {
        self.store.layout_base(id, path)
    }
    fn layout_preview(&self) -> Option<&Preview> {
        self.store.layout_preview()
    }
}

/// Whether a node places its children: a group with a `layout` facet, or a
/// table.
pub(crate) fn lays_out(doc: DocTree<'_>, n: NodeId) -> bool {
    doc.live(n).is_some_and(|x| match x.ty.name() {
        "group" => doc.has(n, FacetKind::Layout),
        "table" => true,
        _ => false,
    })
}

/// How many ancestors a node has.
fn depth(doc: DocTree<'_>, n: NodeId) -> usize {
    std::iter::successors(doc.live(n).and_then(|x| x.parent), |p| doc.live(*p).and_then(|x| x.parent)).count()
}

/// Lay out structural root `root`: each container in it that a run lays out
/// from its top (one inside another's layout is laid out in its parent's
/// tree), deepest first: a layout native's run.
pub(crate) fn lay_out(store: &dyn Reads, root: NodeId) -> (BTreeMap<Leaf, Value>, BTreeMap<NodeId, crate::table::Grid>) {
    LAY_OUTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut out = BTreeMap::new();
    let mut grids = BTreeMap::new();
    let doc = store.tree();
    let is_container = |k: Kind| matches!(k, Kind::Container | Kind::Table);
    // The nodes that place children, reached through nodes that do.
    let mut placing = Vec::new();
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        placing.push(n);
        stack.extend(doc.children(n).iter().copied().filter(|&c| lays_out(doc, c)));
    }
    let mut roots: Vec<(NodeId, Kind)> = placing
        .into_iter()
        .filter_map(|n| match doc.live(n)?.ty.name() {
            "table" => Some((n, Kind::Table)),
            _ => container(store, n).map(|_| (n, Kind::Container)),
        })
        .filter(|&(g, _)| contained(store, g) || !(doc.live(g).and_then(|n| n.parent).is_some_and(|p| is_container(kind_of(store, p))) && is_item(store, g)))
        .collect();
    // Deepest first, but a contained box after the run that sizes it,
    // shallowest first.
    roots.sort_by_key(|&(r, _)| if contained(store, r) { (1, depth(doc, r) as i64, r) } else { (0, -(depth(doc, r) as i64), r) });
    for (root, kind) in roots {
        let mut mine: BTreeMap<Leaf, Value> = BTreeMap::new();
        let fresh = Fresh { store, out: &out };
        let store: &dyn Reads = &fresh;
        let mut b = Builder { store, root, tree: Tree::default(), entries: Vec::new(), tables: BTreeMap::new() };
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
                mine.insert(Leaf { node: e.id, path: path(leaf) }, Value::Number(v));
            };
            if i != b.tree.root {
                let off = match (e.kind, e.fill) {
                    // A resized ellipse is centred on its new box.
                    (Kind::Ellipse, [fw, fh]) if fw || fh => [if fw { -w / 2.0 } else { e.offset[0] }, if fh { -h / 2.0 } else { e.offset[1] }],
                    _ => e.offset,
                };
                match store.layout_preview().and_then(|p| p.floating).filter(|(n, _)| *n == e.id) {
                    Some((_, [fx, fy])) => {
                        put("transform2d.x", fx);
                        put("transform2d.y", fy);
                    }
                    None => {
                        put("transform2d.x", x - off[0]);
                        put("transform2d.y", y - off[1]);
                    }
                }
            }
            match e.kind {
                Kind::Table => {
                    put("size.w", w);
                    put("size.h", h);
                }
                Kind::Cell => {
                    mine.insert(Leaf { node: e.id, path: path("text.sizing") }, Value::Enum("fixed".into()));
                    mine.insert(Leaf { node: e.id, path: path("size.w") }, Value::Number(w));
                    mine.insert(Leaf { node: e.id, path: path("size.h") }, Value::Number(h));
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
                        mine.insert(Leaf { node: e.id, path: path("text.sizing") }, Value::Enum("fixed".into()));
                        mine.insert(Leaf { node: e.id, path: path("size.w") }, Value::Number(w));
                        mine.insert(Leaf { node: e.id, path: path("size.h") }, Value::Number(h));
                    } else if chosen {
                        if authored != Sizing::AutoHeight {
                            mine.insert(Leaf { node: e.id, path: path("text.sizing") }, Value::Enum("auto_height".into()));
                        }
                        mine.insert(Leaf { node: e.id, path: path("size.w") }, Value::Number(w));
                    }
                }
                Kind::Other => {}
            }
        }
        out.extend(mine);
    }
    (out, grids)
}

/// Figma's constraints (frame-constraints.md §3): for each `use` in `uses`
/// that is a resized picture of a component, each part with a `constraints`
/// facet placed for the new box. Read from the picture's own copy (so an
/// instance's overrides count) and published on it; a bound leaf keeps its
/// binding (one writer, F5).
pub(crate) fn constrained_of(store: &dyn Reads, uses: &[NodeId]) -> BTreeMap<Leaf, Value> {
    let doc = store.tree();
    let mut out = BTreeMap::new();
    for &u in uses {
        if !doc.is_live(u) || !doc.has(u, FacetKind::Size) {
            continue;
        }
        let def = crate::expand::main_definition(doc, u).or_else(|| crate::expand::use_entity(doc, u).and_then(|e| crate::expand::linked_definition(doc, e)));
        let Some(def) = def else { continue };
        let notation = doc.get::<crate::facet::Use>(u).and_then(|x| x.notation.constant()).and_then(|v| v.as_str()).unwrap_or("free").to_owned();
        let Some(t) = crate::definition::template_of(doc, def, &notation) else { continue };
        let constant = |id: NodeId, leaf: &str| if doc.leaf(id, path(leaf)).is_some() { store.constant(id, path(leaf)).and_then(|v| v.as_f64()) } else { None };
        let (Some(w0), Some(h0)) = (constant(t, "size.w"), constant(t, "size.h")) else { continue };
        let (w, h) = (num(store, u, "size.w", w0), num(store, u, "size.h", h0));
        if (w - w0).abs() < 1e-9 && (h - h0).abs() < 1e-9 {
            continue;
        }
        for &part in doc.children(t).iter() {
            if !doc.has(part, FacetKind::Constraints) {
                continue;
            }
            let Some(copy) = NodeId::expanded(u, part) else { continue };
            if !doc.is_live(copy) {
                continue;
            }
            // Read as values: an edit of a part's constraint moves its copy.
            let mode = |field: &str, d: &str| store.constant(part, path(field)).and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_else(|| d.to_owned());
            let (mh, mv) = (mode("constraints.horizontal", "left"), mode("constraints.vertical", "top"));
            let ty = doc.live(copy).map_or("", |n| n.ty.name());
            // The part's box: a rect's (and anything sized) from its corner,
            // an ellipse's around its centre; anything else a point.
            let (bx, by, bw, bh, sized) = match ty {
                "ellipse" => {
                    let (rx, ry) = (constant(copy, "radius.rx").unwrap_or(0.0), constant(copy, "radius.ry").unwrap_or(0.0));
                    let (cx, cy) = (constant(copy, "transform2d.x").unwrap_or(0.0), constant(copy, "transform2d.y").unwrap_or(0.0));
                    (cx - rx, cy - ry, 2.0 * rx, 2.0 * ry, true)
                }
                _ => {
                    let x = constant(copy, "transform2d.x").unwrap_or(0.0);
                    let y = constant(copy, "transform2d.y").unwrap_or(0.0);
                    match (constant(copy, "size.w"), constant(copy, "size.h")) {
                        (Some(sw), Some(sh)) if ty != "text" || word(store, copy, "text.sizing") == "fixed" => (x, y, sw, sh, true),
                        _ => (x, y, 0.0, 0.0, false),
                    }
                }
            };
            let axis = |m: &str, pos: f64, len: f64, d0: f64, d: f64| -> (f64, f64) {
                let dd = d - d0;
                match m {
                    "right" | "bottom" => (pos + dd, len),
                    "left_right" | "top_bottom" => (pos, (len + dd).max(0.0)),
                    "center" => (pos + dd / 2.0, len),
                    "scale" if d0 > 0.0 => (pos * d / d0, len * d / d0),
                    _ => (pos, len),
                }
            };
            let (nx, nw) = axis(&mh, bx, bw, w0, w);
            let (ny, nh) = axis(&mv, by, bh, h0, h);
            let free = |leaf: &str| {
                // Bound or not is read as a value too (tracked).
                let _ = store.constant(copy, path(leaf));
                doc.leaf(copy, path(leaf)).is_none_or(|s| s.constant().is_some())
            };
            let mut put = |leaf: &str, v: f64| {
                if free(leaf) {
                    out.insert(Leaf { node: copy, path: path(leaf) }, Value::Number(v));
                }
            };
            if ty == "ellipse" {
                put("transform2d.x", nx + nw / 2.0);
                put("transform2d.y", ny + nh / 2.0);
                put("radius.rx", nw / 2.0);
                put("radius.ry", nh / 2.0);
            } else {
                put("transform2d.x", nx);
                put("transform2d.y", ny);
                if sized {
                    put("size.w", nw);
                    put("size.h", nh);
                }
            }
        }
    }
    out
}

/// Add auto layout (Figma's Shift+A; layout-and-tables.md §2.4): a group
/// that has no layout gets one; siblings are wrapped in a new laid-out
/// group at their top-left. The direction is the one they are spread
/// along, the gap the mean of the gaps between them, and they are ordered
/// as they sit, so the layout keeps the arrangement it was given. One
/// transaction's ops, and the group.
pub fn add_ops(doc: DocTree<'_>, extent: &dyn Fn(NodeId) -> Option<Rect>, selection: &[NodeId]) -> Result<(Vec<crate::Op>, NodeId), String> {
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

/// Structural roots laid out since the process started: what RE-5.4's
/// demonstrations count (`tests/cells_demo.rs`).
static LAY_OUTS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[doc(hidden)]
pub fn lay_out_count() -> u64 {
    LAY_OUTS.load(std::sync::atomic::Ordering::Relaxed)
}

/// The leaves layout may write on a node: its place, and the sizes a layout
/// decides.
const WRITES: [&str; 7] = ["transform2d.x", "transform2d.y", "size.w", "size.h", "text.sizing", "radius.rx", "radius.ry"];

/// The pinned parts of one `use`: the parts of a resized picture that
/// `constrained_of` places, copies of template parts with a `constraints`
/// facet. Their placement is the pins', not a container's (it overrode the
/// container's before natives). None unless the use is live and sized.
/// Uses `has` (recording `Key::FacetSet`, not `Key::Facet`) for `Size` and
/// `Constraints` so value edits to box sizes or constraint modes do not
/// rerun the layout maker.
pub(crate) fn pinned_parts_of(doc: DocTree<'_>, u: NodeId) -> BTreeSet<NodeId> {
    let mut out = BTreeSet::new();
    let use_ty = TypeId::lookup("use").expect("registered");
    if doc.node(u).is_some_and(|n| n.ty == use_ty) && doc.is_live(u) && doc.has(u, FacetKind::Size) {
        let def = crate::expand::main_definition(doc, u).or_else(|| crate::expand::use_entity(doc, u).and_then(|e| crate::expand::linked_definition(doc, e)));
        let Some(def) = def else { return out };
        let notation = doc.get::<crate::facet::Use>(u).and_then(|x| x.notation.constant()).and_then(|v| v.as_str()).unwrap_or("free").to_owned();
        let Some(t) = crate::definition::template_of(doc, def, &notation) else { return out };
        for &part in doc.children(t).iter() {
            if doc.has(part, FacetKind::Constraints)
                && let Some(copy) = NodeId::expanded(u, part)
                && doc.is_live(copy)
            {
                out.insert(copy);
            }
        }
    }
    out
}

/// A structural root's layout as a native cell (engine.md §3.3, EN-2a): it
/// lays the root out through `lay_out`, reading every value and structural
/// key through its `Cx`, and claims the placements. Its outputs are fixed by
/// structure; if any node in its subtree is a pinned part, `uses` names the
/// owning `use` nodes so `constrained_of` runs keyed with no document scan.
#[derive(Debug, PartialEq)]
pub(crate) struct LayoutNative {
    root: NodeId,
    outputs: BTreeSet<Leaf>,
    /// The `use` nodes whose pinned parts lie in `outputs` (sorted, deduped).
    uses: Vec<NodeId>,
}

impl LayoutNative {
    pub(crate) fn new(doc: DocTree<'_>, root: NodeId, pinned_owner: &BTreeMap<NodeId, NodeId>) -> (LayoutNative, BTreeSet<NodeId>) {
        let mut nodes = BTreeSet::new();
        let mut stack = vec![root];
        while let Some(n) = stack.pop() {
            nodes.insert(n);
            // A table places its cells, in its rows.
            if doc.live(n).is_some_and(|x| x.ty.name() == "table") {
                nodes.extend(crate::table::rows(doc, n));
                nodes.extend(crate::table::cells(doc, n));
            }
            for &c in doc.children(n).iter().filter(|&&c| doc.is_live(c)) {
                nodes.insert(c);
                if lays_out(doc, c) {
                    stack.push(c);
                }
            }
        }
        let mut uses: Vec<NodeId> = nodes.iter().filter_map(|n| pinned_owner.get(n).copied()).collect();
        uses.sort();
        uses.dedup();
        let writes: [LeafPath; 7] = WRITES.map(path);
        let outputs = nodes.iter().copied().flat_map(|n| writes.into_iter().map(move |path| Leaf { node: n, path })).collect();
        (LayoutNative { root, outputs, uses }, nodes)
    }

    #[cfg(feature = "structure-oracle")]
    pub(crate) fn uses(&self) -> &[NodeId] {
        &self.uses
    }
}

impl crate::graph::Native for LayoutNative {
    fn producer(&self) -> ProducerId {
        ProducerId::LAYOUT
    }
    fn outputs(&self) -> &BTreeSet<Leaf> {
        &self.outputs
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn keyed(&self) -> bool {
        true
    }
    fn compute(&self, cx: &mut crate::graph::Cx<'_>) -> Result<BTreeMap<Leaf, Value>, crate::graph::Failure> {
        // Text measures through the engine's fonts.
        let _ = cx.input::<u64>(crate::graph::InputKey::Fonts);
        let (mut values, grids) = lay_out(&*cx, self.root);
        debug_assert!(values.keys().all(|l| self.outputs.contains(l)), "layout of {} wrote outside its outputs", self.root);
        if !self.uses.is_empty() {
            values.extend(constrained_of(&*cx, &self.uses));
        }
        // A table whose tracks moved redraws its lines.
        let columns: LeafPath = path("table.columns");
        let before = cx.previous_extra::<BTreeMap<NodeId, crate::table::Grid>>();
        let moved: Vec<NodeId> = grids.iter().filter(|(t, g)| before.and_then(|b| b.get(t)) != Some(g)).map(|(t, _)| *t).collect();
        for t in moved {
            cx.mark(Leaf { node: t, path: columns });
        }
        cx.set_extra(std::sync::Arc::new(grids));
        Ok(values.into_iter().filter(|(l, _)| self.outputs.contains(l)).collect())
    }
}

/// `constrained_of` as a native cell, scoped to one top-level root, for the
/// pinned parts under that root no layout native holds.
#[derive(Debug, PartialEq)]
pub(crate) struct PinsNative {
    root: NodeId,
    uses: Vec<NodeId>,
    outputs: BTreeSet<Leaf>,
}

impl PinsNative {
    pub(crate) fn new(root: NodeId, uses: Vec<NodeId>, pinned: &BTreeSet<NodeId>) -> PinsNative {
        let writes: [LeafPath; 7] = WRITES.map(path);
        PinsNative {
            root,
            uses,
            outputs: pinned.iter().copied().flat_map(|node| writes.into_iter().map(move |path| Leaf { node, path })).collect(),
        }
    }

    #[cfg(feature = "structure-oracle")]
    pub(crate) fn uses(&self) -> &[NodeId] {
        &self.uses
    }
}

impl crate::graph::Native for PinsNative {
    fn producer(&self) -> ProducerId {
        ProducerId::LAYOUT
    }
    fn outputs(&self) -> &BTreeSet<Leaf> {
        &self.outputs
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn keyed(&self) -> bool {
        true
    }
    fn compute(&self, cx: &mut crate::graph::Cx<'_>) -> Result<BTreeMap<Leaf, Value>, crate::graph::Failure> {
        Ok(constrained_of(&*cx, &self.uses).into_iter().filter(|(l, _)| self.outputs.contains(l)).collect())
    }
}

/// A native produced by the per-root layout maker (`make_root`).
#[derive(Debug, PartialEq)]
pub(crate) enum LayoutRule {
    Layout(LayoutNative),
    Pins(PinsNative),
}

impl LayoutRule {
    pub(crate) fn same_as(&self, existing: Option<&dyn crate::graph::Native>) -> bool {
        let Some(existing) = existing else { return false };
        match self {
            LayoutRule::Layout(n) => existing.as_any().downcast_ref::<LayoutNative>() == Some(n),
            LayoutRule::Pins(n) => existing.as_any().downcast_ref::<PinsNative>() == Some(n),
        }
    }

    pub(crate) fn into_arc(self) -> std::sync::Arc<dyn crate::graph::Native> {
        match self {
            LayoutRule::Layout(n) => std::sync::Arc::new(n),
            LayoutRule::Pins(n) => std::sync::Arc::new(n),
        }
    }
}

/// Stage 2 layout maker for one top-level root (engine.md §3.3, EN-2a):
/// reads only through `MakerCx`'s recording `Tree` and scoped indexes, and
/// returns the exact set of `Layout(_)` and `Pins(root)` natives for `root`.
pub(crate) fn make_root(cx: &crate::MakerCx<'_>, root: NodeId) -> BTreeMap<crate::graph::NativeKey, LayoutRule> {
    let doc = cx.tree();
    let table_ty = TypeId::lookup("table").expect("registered");
    let use_ty = TypeId::lookup("use").expect("registered");

    let mut pinned_owner: BTreeMap<NodeId, NodeId> = BTreeMap::new();
    for u in doc.nodes_of_type_in(root, use_ty) {
        for part in pinned_parts_of(doc, u) {
            pinned_owner.insert(part, u);
        }
    }

    let mut roots: BTreeSet<NodeId> = BTreeSet::new();
    for n in doc.nodes_with_in(root, FacetKind::Layout).into_iter().chain(doc.nodes_of_type_in(root, table_ty)) {
        if lays_out(doc, n) && !doc.live(n).and_then(|x| x.parent).is_some_and(|p| lays_out(doc, p)) {
            roots.insert(n);
        }
    }

    let mut out: BTreeMap<crate::graph::NativeKey, LayoutRule> = BTreeMap::new();
    let mut covered: BTreeSet<NodeId> = BTreeSet::new();
    for r in roots {
        let (native, nodes) = LayoutNative::new(doc, r, &pinned_owner);
        covered.extend(nodes);
        out.insert(crate::graph::NativeKey::Layout(r), LayoutRule::Layout(native));
    }

    let mut uncovered: BTreeSet<NodeId> = BTreeSet::new();
    let mut pin_uses: Vec<NodeId> = Vec::new();
    for (&part, &u) in &pinned_owner {
        if !covered.contains(&part) {
            uncovered.insert(part);
            pin_uses.push(u);
        }
    }
    if !uncovered.is_empty() {
        pin_uses.sort();
        pin_uses.dedup();
        out.insert(crate::graph::NativeKey::Pins(root), LayoutRule::Pins(PinsNative::new(root, pin_uses, &uncovered)));
    }
    out
}
