//! The app's standard components (component-authoring.md §6): definitions
//! the Library tab offers, built from code at startup like the node types.
//! Dragging one into a composition copies its definition into the
//! document's library on first use (marked `import.source = std:<key>`, so
//! later drags reuse it) and places a linked instance. The Library tab shows
//! each as a baked preview: its picture with resolved values, in a session
//! composition.

use crate::definition::{self, Placement};
use crate::document::Document;
use crate::facet::{Composition, Facet, FacetKind, Fill, Font, Import, LeafPath, LeafStruct, Name, Port, Size, Stroke, Text, Transform2d};
use crate::id::NodeId;
use crate::op::Op;
use crate::order::OrderKey;
use crate::registry::TypeId;
use crate::value::{Slot, Value};

/// One entry of the Library tab.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Entry {
    pub key: &'static str,
    pub name: &'static str,
}

pub const ENTRIES: &[Entry] = &[
    Entry { key: "slider", name: "Slider" },
    Entry { key: "readout", name: "Readout" },
    Entry { key: "laser", name: "Laser" },
    Entry { key: "fiber", name: "Fiber" },
    Entry { key: "meter", name: "Dial meter" },
    Entry { key: "plot", name: "Plot" },
];

/// The name of the session composition the Library tab shows.
pub const LIBRARY: &str = "library";
/// The notation standard templates are drawn in.
const NOTATION: &str = "free";

fn ty(n: &str) -> TypeId {
    TypeId::lookup(n).expect("registered type")
}
fn named(t: &str) -> Facet {
    let mut n = Name::default();
    n.text = Slot::Const(Value::Str(t.into()));
    n.wrap()
}
fn n(v: f64) -> Slot {
    Slot::Const(Value::Number(v))
}
fn at(x: f64, y: f64) -> Transform2d {
    let mut t = Transform2d::default();
    t.x = n(x);
    t.y = n(y);
    t
}
fn size(w: f64, h: f64) -> Facet {
    let mut s = Size::default();
    s.w = n(w);
    s.h = n(h);
    s.wrap()
}
fn paint(c: [f32; 4]) -> Facet {
    let mut f = Fill::default();
    f.color = Slot::Const(Value::Color(c));
    f.wrap()
}

/// Fresh ids in creation order, from `next`.
struct Ids(u64);
impl Ids {
    fn id(&mut self) -> NodeId {
        let id = NodeId::from_raw(self.0);
        self.0 += 1;
        id
    }
}

/// Each standard component's markup (markup.rs): definitions as data, read
/// at startup like the node types. Nothing here knows what one draws.
fn source(key: &str) -> Option<&'static str> {
    Some(match key {
        "slider" => include_str!("std/slider.xml"),
        "readout" => include_str!("std/readout.xml"),
        "laser" => include_str!("std/laser.xml"),
        "fiber" => include_str!("std/fiber.xml"),
        "meter" => include_str!("std/meter.xml"),
        "plot" => include_str!("std/plot.xml"),
        _ => return None,
    })
}

/// The definition `key` already copied into `doc`, if any.
pub fn find(doc: &Document, key: &str) -> Option<NodeId> {
    let source = format!("std:{key}");
    doc.nodes_of_type(ty("definition")).find(|&d| doc.is_live(d) && doc.get::<Import>(d).and_then(|i| i.source.constant()).and_then(|v| v.as_str()) == Some(source.as_str()))
}

/// The ops creating standard definition `key` under `library`, with ids
/// from `first`: its markup, marked `import.source = std:<key>` so later
/// placements reuse the copy. Returns the ops and the definition.
pub fn definition_ops(key: &str, library: NodeId, order: OrderKey, first: u64) -> Result<(Vec<Op>, NodeId), String> {
    let src = source(key).filter(|_| ENTRIES.iter().any(|e| e.key == key)).ok_or_else(|| format!("no standard component {key:?}"))?;
    let mut built = crate::markup::ops(src, Some(library), order, NodeId::from_raw(first)).map_err(|e| format!("std:{key}: {e}"))?;
    let def = match built.roots.as_slice() {
        [d] => *d,
        _ => return Err(format!("std:{key}: the markup must hold one definition")),
    };
    let mut imp = Import::default();
    imp.source = Slot::Const(Value::Str(format!("std:{key}")));
    imp.version = Slot::Const(Value::Str("1".into()));
    if let Some(Op::Create { facets, .. }) = built.ops.first_mut() {
        facets.push(imp.wrap());
    }
    Ok((built.ops, def))
}

pub(crate) fn root_of(doc: &Document, ty_: &str) -> Option<NodeId> {
    doc.roots().iter().copied().find(|&r| !r.is_session() && doc.live(r).is_some_and(|n| n.ty.name() == ty_))
}

pub(crate) fn after_last(doc: &Document, parent: Option<NodeId>) -> OrderKey {
    let kids: Vec<NodeId> = match parent {
        Some(p) => doc.children(p).to_vec(),
        None => doc.roots().to_vec(),
    };
    kids.iter().filter(|c| !c.is_expanded()).filter_map(|&c| doc.live(c)).map(|n| n.order.clone()).max().map_or_else(OrderKey::first, |k| OrderKey::after(&k))
}

/// One port of a standard component, for the model's manifest: its name,
/// `in` or `out`, its kind, and an input's default as the views print it.
#[derive(Clone, Debug, PartialEq)]
pub struct PortInfo {
    pub name: String,
    pub dir: String,
    pub kind: String,
    pub default: Option<String>,
}

/// Every standard component's ports, read from the definition it builds,
/// so the manifest cannot drift from what `std:<key>` places.
pub fn interfaces() -> Vec<(&'static str, Vec<PortInfo>)> {
    ENTRIES
        .iter()
        .map(|e| {
            let mut store = crate::Store::new();
            let doc = store.document();
            let (library, first) = (doc.next_id(), doc.next_id().raw() + 1);
            let mut ops = vec![Op::Create { id: library, ty: ty("library"), parent: None, order: OrderKey::first(), facets: vec![] }];
            let (def_ops, def) = definition_ops(e.key, library, OrderKey::first(), first).expect("standard definition");
            ops.extend(def_ops);
            store.apply(crate::TransactionInput { label: "interface".into(), actor: crate::Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }).expect("standard definition applies");
            let doc = store.document();
            let ports = doc
                .children(def)
                .iter()
                .filter_map(|&c| {
                    let p = doc.get::<Port>(c)?;
                    let name = doc.get::<Name>(c)?.text.constant()?.as_str()?.to_owned();
                    let text = |s: &Slot| s.constant().and_then(|v| v.as_str()).unwrap_or("").to_owned();
                    let dir = text(&p.dir);
                    let default = (dir == "in").then(|| p.signal.constant().map(ToString::to_string)).flatten();
                    Some(PortInfo { name, dir, kind: text(&p.kind), default })
                })
                .collect();
            (e.key, ports)
        })
        .collect()
}

impl crate::Store {
    /// Place standard component `key` in `composition` at `at` (its
    /// picture's origin), as one history entry: its definition copied in on
    /// first use, then a linked instance. Returns the instance's `use`.
    pub fn place_standard(&mut self, key: &str, composition: NodeId, at: [f64; 2], actor: crate::Actor) -> Result<NodeId, String> {
        let name = ENTRIES.iter().find(|e| e.key == key).map(|e| e.name.to_lowercase()).ok_or_else(|| format!("no standard component {key:?}"))?;
        let err = |e: Vec<crate::op::ApplyError>| e.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("; ");
        let tx = |label: String, ops| crate::TransactionInput { label, actor, ts: 0, idempotency_key: None, ops, exact: false };
        let mut first = None;
        let def = match find(&self.doc, key) {
            Some(d) => d,
            None => {
                let mut ops = Vec::new();
                let mut next = self.doc.next_id().raw();
                let library = match root_of(&self.doc, "library") {
                    Some(l) => l,
                    None => {
                        let l = NodeId::from_raw(next);
                        next += 1;
                        ops.push(Op::Create { id: l, ty: ty("library"), parent: None, order: after_last(&self.doc, None), facets: vec![] });
                        l
                    }
                };
                let order = if ops.is_empty() { after_last(&self.doc, Some(library)) } else { OrderKey::first() };
                let (def_ops, def) = definition_ops(key, library, order, next)?;
                ops.extend(def_ops);
                first = Some(self.apply(tx(format!("add {name}"), ops)).map_err(err)?.version);
                def
            }
        };
        let doc = &self.doc;
        let mut ops = Vec::new();
        let model = match root_of(doc, "model") {
            Some(m) => m,
            None => {
                let m = doc.next_id();
                ops.push(Op::Create { id: m, ty: ty("model"), parent: None, order: after_last(doc, None), facets: vec![] });
                m
            }
        };
        let place = Placement { entity_parent: Some(model), entity_after: doc.children(model).iter().filter_map(|&c| doc.live(c)).map(|n| n.order.clone()).max(), figure_parent: Some(composition), figure_after: doc.children(composition).iter().filter(|c| !c.is_expanded()).filter_map(|&c| doc.live(c)).map(|n| n.order.clone()).max() };
        let (_, mut inst) = if ops.is_empty() {
            definition::instantiate(doc, def, NOTATION, &name, &place)?
        } else {
            // A model made in this transaction: instantiate's ids come after it.
            let mut scratch = doc.clone();
            let stamp = scratch.version().next();
            crate::op::apply_op(&mut scratch, 0, &ops[0], stamp).map_err(|e| e.message)?;
            definition::instantiate(&scratch, def, NOTATION, &name, &place)?
        };
        for op in inst.iter_mut() {
            if let Op::Create { ty: t, facets, .. } = op
                && t.name() == "use"
            {
                for f in facets.iter_mut() {
                    if let Facet::Transform2d(tr) = f {
                        tr.x = n(at[0]);
                        tr.y = n(at[1]);
                    }
                }
            }
        }
        let u = inst.iter().find_map(|op| match op {
            Op::Create { id, ty: t, .. } if t.name() == "use" => Some(*id),
            _ => None,
        });
        ops.extend(inst);
        let applied = self.apply(tx(format!("add {name}"), ops)).map_err(err)?;
        if let Some(v) = first {
            self.merge_with_previous(v, format!("add {name}"));
        }
        let _ = applied;
        u.ok_or_else(|| "no picture placed".into())
    }
}

/// The name of the Latency view's session composition (src-ui/views/html/LatencyView.svelte).
pub const LATENCY: &str = "latency";
/// The draggable square in [`LATENCY`], by name.
pub const LATENCY_TARGET: &str = "latency target";
/// The square's side, in document px (CSS px at 100%). Under the select
/// tool's 48 px for corner-radius dots, so a press anywhere on it moves it.
pub const LATENCY_TARGET_SIZE: f64 = 40.0;

/// The ops building the Latency view's composition: a session-tier 2D
/// composition, never saved and never in history, holding one yellow
/// square the view's tests drag through the ordinary select tool. The view
/// recentres it (`engine_api::document_latency`).
pub fn latency_ops(first: NodeId, order: OrderKey) -> Vec<Op> {
    let mut ids = Ids(first.raw());
    let comp = ids.id();
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    let side = LATENCY_TARGET_SIZE;
    vec![
        Op::Create { id: comp, ty: ty("composition"), parent: None, order, facets: vec![named(LATENCY), c.wrap()] },
        Op::Create { id: ids.id(), ty: ty("rect"), parent: Some(comp), order: OrderKey::first(), facets: vec![at(200.0, 200.0).wrap(), size(side, side), paint([1.0, 0.84, 0.0, 1.0]), named(LATENCY_TARGET)] },
    ]
}

/// The Library's grid, in document px (CSS px at 100%): two columns of
/// cards that fit a 272 px panel whole (12 + 120 + 8 + 120 + 12), the
/// left side's width at a 1440 px window (src-ui/views/workspace.ts).
const LIB_MARGIN: f64 = 12.0;
const LIB_CARD: (f64, f64) = (120.0, 80.0);
const LIB_GAP: f64 = 8.0;
/// A cell's card, then its label under it, then the gap to the next row.
const LIB_ROW: f64 = 116.0;
/// The card's inset around its preview.
const LIB_INSET: f64 = 6.0;

/// The Library's card and label colours for a theme, in sRGB.
/// (fill, edge, label), from the shell's tokens (src-ui/app.css): the card
/// is `--bg-sunken` with a `--border` edge in the light theme, `--raised`
/// with a `--border-strong` edge in the dark one (a tile lifted off the
/// panel without glaring, as Figma's dark assets panel), and the label is
/// `--fg-muted`. The previews inside are recoloured to match
/// ([`PREVIEW_DARK`]).
pub fn library_colours(light: bool) -> ([f32; 4], [f32; 4], [f32; 4]) {
    let hex = |c: u32| [((c >> 16) & 0xff) as f32 / 255.0, ((c >> 8) & 0xff) as f32 / 255.0, (c & 0xff) as f32 / 255.0, 1.0];
    if light { (hex(0xf3f3f3), hex(0xe5e5e5), hex(0x616161)) } else { (hex(0x2a2b2e), hex(0x3c3c3c), hex(0x9d9d9d)) }
}

/// The previews are baked in the light theme's colours; in the dark one each
/// colour here (light, dark) becomes its partner, and back. They are the
/// colours the baked previews hold: the Readout's and Dial meter's ink, the
/// Laser's body, the dial's face and rim, and the Plot's theme tokens as
/// baked (palette.rs `BUILT_IN`: surface, grid, ink, axis). Accents (the
/// beam, needle, fiber, series) and `muted` read on both and stay. Every
/// value is distinct, so the mapping runs either way.
const PREVIEW_DARK: &[(u32, u32)] = &[
    (0x1a1a1f, 0xe8e8ea), // text ink
    (0x2e3342, 0x8a93a8), // laser body
    (0xf2f5fa, 0x1c1d1f), // dial face
    (0x4d5261, 0x8a90a0), // dial rim
    (0xfcfcfb, 0x1a1a19), // @surface
    (0xe1e0d9, 0x2c2c2a), // @grid
    (0x0b0b0b, 0xf0efec), // @ink
    (0xc3c2b7, 0x383835), // @axis
];

/// A preview colour in the theme (`light`): its partner in
/// [`PREVIEW_DARK`] when it holds the other theme's, else itself.
fn preview_colour(c: [f32; 4], light: bool) -> [f32; 4] {
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u32;
    let rgb = (byte(c[0]) << 16) | (byte(c[1]) << 8) | byte(c[2]);
    let found = PREVIEW_DARK.iter().find_map(|&(l, d)| if light && rgb == d { Some(l) } else if !light && rgb == l { Some(d) } else { None });
    match found {
        Some(x) => [((x >> 16) & 0xff) as f32 / 255.0, ((x >> 8) & 0xff) as f32 / 255.0, (x & 0xff) as f32 / 255.0, c[3]],
        None => c,
    }
}

/// The ops building the Library tab's session composition: a grid of
/// cells, each an `empty` named for its entry's key holding a card, a baked
/// preview (scaled to fit) and a label, in the light theme's colours
/// ([`library_theme_ops`] recolours them). Ids from `first`, a session id.
/// Previews are each standard picture's resolved shapes, with no bindings
/// and no ports.
pub fn library_ops(first: NodeId, order: OrderKey) -> Vec<Op> {
    let mut ids = Ids(first.raw());
    let comp = ids.id();
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    let mut ops = vec![Op::Create { id: comp, ty: ty("composition"), parent: None, order, facets: vec![named(LIBRARY), c.wrap()] }];
    let (card_w, card_h) = LIB_CARD;
    let (preview_w, preview_h) = (card_w - 2.0 * LIB_INSET, card_h - 2.0 * LIB_INSET);
    let (fill, edge, ink) = library_colours(true);
    let mut key = OrderKey::first();
    for (i, entry) in ENTRIES.iter().enumerate() {
        let (col, row) = ((i % 2) as f64, (i / 2) as f64);
        let cell = ids.id();
        ops.push(Op::Create { id: cell, ty: ty("empty"), parent: Some(comp), order: key.clone(), facets: vec![at(LIB_MARGIN + col * (card_w + LIB_GAP), LIB_MARGIN + row * LIB_ROW).wrap(), named(entry.key)] });
        key = OrderKey::after(&key);
        // A card behind the preview, the preview fitted into it, a label.
        let card = ids.id();
        let mut corners = crate::facet::Corners::default();
        corners.tl = n(8.0);
        corners.tr = n(8.0);
        corners.br = n(8.0);
        corners.bl = n(8.0);
        let mut stroke = Stroke::default();
        stroke.color = Slot::Const(Value::Color(edge));
        stroke.width = n(1.0);
        ops.push(Op::Create { id: card, ty: ty("rect"), parent: Some(cell), order: OrderKey::first(), facets: vec![at(0.0, 0.0).wrap(), size(card_w, card_h), paint(fill), stroke.wrap(), corners.wrap(), named("card")] });
        let baked = bake(entry.key);
        if let Some(b) = baked.bounds {
            let s = (preview_w / b.width().max(1.0)).min(preview_h / b.height().max(1.0)).min(1.0);
            let holder = ids.id();
            let mut t = at(LIB_INSET + (preview_w - b.width() * s) / 2.0 - b.x0 * s, LIB_INSET + (preview_h - b.height() * s) / 2.0 - b.y0 * s);
            t.sx = n(s);
            t.sy = n(s);
            ops.push(Op::Create { id: holder, ty: ty("empty"), parent: Some(cell), order: OrderKey::after(&OrderKey::first()), facets: vec![t.wrap(), named("preview")] });
            let mut made: Vec<NodeId> = Vec::new();
            let mut k = OrderKey::first();
            for (t_, facets, parent) in baked.nodes {
                let id = ids.id();
                let p = parent.map_or(holder, |i| made[i]);
                ops.push(Op::Create { id, ty: t_, parent: Some(p), order: k.clone(), facets });
                k = OrderKey::after(&k);
                made.push(id);
            }
        }
        let label = ids.id();
        let mut words = Text::default();
        words.content = Slot::Const(Value::Str(entry.name.into()));
        let mut font = Font::default();
        font.size = n(12.0);
        ops.push(Op::Create { id: label, ty: ty("text"), parent: Some(cell), order: OrderKey::after(&OrderKey::after(&OrderKey::first())), facets: vec![at(0.0, card_h + 6.0).wrap(), words.wrap(), font.wrap(), paint(ink), named("label")] });
    }
    ops
}

/// The session ops recolouring the Library's cards, labels and previews
/// for the theme (`light`), or none when they already wear it. Only the
/// Library's session composition is touched, never a document's components. The Library's
/// colours are constants (its previews are baked), so a theme switch sets
/// them, as the timeline regenerates in its palette (timeline_view.rs).
pub fn library_theme_ops(doc: &Document, light: bool) -> Vec<Op> {
    let (fill, edge, ink) = library_colours(light);
    let Some(&lib) = doc.roots().iter().find(|&&r| r.is_session() && doc.get::<Name>(r).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(LIBRARY)) else {
        return Vec::new();
    };
    let mut ops = Vec::new();
    let mut set = |id: NodeId, kind: FacetKind, field: &'static str, want: [f32; 4], have: Option<&Value>| {
        if have != Some(&Value::Color(want)) {
            ops.push(Op::Set { id, path: LeafPath::new(kind, field).expect("declared"), slot: Slot::Const(Value::Color(want)) });
        }
    };
    for &cell in doc.children(lib) {
        for &c in doc.children(cell) {
            let name = doc.get::<Name>(c).and_then(|n| n.text.constant()).and_then(|v| v.as_str()).map(str::to_owned);
            let have_fill = doc.get::<Fill>(c).and_then(|f| f.color.constant().cloned());
            match name.as_deref() {
                Some("card") => {
                    set(c, FacetKind::Fill, "color", fill, have_fill.as_ref());
                    let have_edge = doc.get::<Stroke>(c).and_then(|s| s.color.constant().cloned());
                    set(c, FacetKind::Stroke, "color", edge, have_edge.as_ref());
                }
                Some("label") => set(c, FacetKind::Fill, "color", ink, have_fill.as_ref()),
                Some("preview") => {
                    let mut stack = doc.children(c).to_vec();
                    while let Some(id) = stack.pop() {
                        stack.extend(doc.children(id).iter().copied());
                        let fill = doc.get::<Fill>(id).and_then(|f| f.color.constant().cloned());
                        let edge = doc.get::<Stroke>(id).and_then(|s| s.color.constant().cloned());
                        for (kind, have) in [(FacetKind::Fill, fill), (FacetKind::Stroke, edge)] {
                            if let Some(Value::Color(was)) = have {
                                set(id, kind, "color", preview_colour(was, light), Some(&Value::Color(was)));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
    }
    ops
}

/// A standard picture, resolved: (type, facets with constant slots, index
/// of the parent among earlier nodes), and its bounds.
struct Baked {
    nodes: Vec<(TypeId, Vec<Facet>, Option<usize>)>,
    bounds: Option<kurbo::Rect>,
}

fn bake(key: &str) -> Baked {
    let mut store = crate::Store::new();
    let ids = store.document().next_ids(3);
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    let setup = vec![
        Op::Create { id: ids[0], ty: ty("library"), parent: None, order: OrderKey::first(), facets: vec![] },
        Op::Create { id: ids[1], ty: ty("composition"), parent: None, order: OrderKey::after(&OrderKey::first()), facets: vec![named("bake"), c.wrap()] },
        Op::Create { id: ids[2], ty: ty("model"), parent: None, order: OrderKey::after(&OrderKey::after(&OrderKey::first())), facets: vec![] },
    ];
    let empty = Baked { nodes: Vec::new(), bounds: None };
    if store.apply(crate::TransactionInput { label: "bake".into(), actor: crate::Actor::System, ts: 0, idempotency_key: None, ops: setup, exact: false }).is_err() {
        return empty;
    }
    let Ok(u) = store.place_standard(key, ids[1], [0.0, 0.0], crate::Actor::System) else { return empty };
    let _ = store.refresh_text();
    let doc = store.document();
    let mut nodes = Vec::new();
    let mut index: Vec<NodeId> = Vec::new();
    let mut stack: Vec<NodeId> = doc.children(u).iter().rev().copied().collect();
    while let Some(id) = stack.pop() {
        let Some(node) = doc.live(id) else { continue };
        // A repeat's template is not drawn, nor is a let: the copies are
        // baked as plain shapes.
        if crate::expand::in_repeat_template(doc, id) || matches!(node.ty.name(), "let" | "on" | "set" | "trace" | "accumulate" | "query") {
            continue;
        }
        // Anchors mark ports; a preview has none.
        if doc.facet(id, crate::facet::FacetKind::Anchor).is_some() && doc.children(id).is_empty() {
            continue;
        }
        let parent = node.parent.and_then(|p| index.iter().position(|&x| x == p));
        let facets: Vec<Facet> = doc
            .facets_of(id)
            .filter(|f| !matches!(f, Facet::Anchor(_) | Facet::Repeat(_) | Facet::Item(_) | Facet::When(_) | Facet::Selection(_)))
            .map(|f| {
                let mut f = f.clone();
                for field in f.field_names() {
                    if f.get(field).and_then(|s| s.expr()).is_some() {
                        let path = crate::facet::LeafPath::new(f.kind(), field).expect("own field");
                        if let Some(v) = store.resolved(id, path) {
                            let _ = f.set(field, Slot::Const(v));
                        }
                    }
                }
                f
            })
            .collect();
        nodes.push((node.ty, facets, parent));
        index.push(id);
        stack.extend(doc.children(id).iter().rev().copied());
    }
    // In the use's parent space; the use sits at the origin.
    let bounds = store.extent(u);
    Baked { nodes, bounds }
}
