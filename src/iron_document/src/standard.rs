//! The app's standard components (component-authoring.md §6): definitions
//! the Library tab offers, built from code at startup like the node types.
//! Dragging one into a composition copies its definition into the
//! document's library on first use (marked `import.source = std:<key>`, so
//! later drags reuse it) and places a linked instance. The Library tab shows
//! each as a baked preview: its picture with resolved values, in a session
//! composition.

use crate::definition::{self, Placement};
use crate::document::Document;
use crate::expr::Expr;
use crate::facet::{Anchor, Composition, Facet, Fill, Font, Import, LeafStruct, Name, Port, Radius, Size, Slider, Stroke, Template, Text, Transform2d};
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
fn q(text: &str) -> Value {
    Value::parse_quantity(text).expect("standard quantity")
}
fn bound(e: String) -> Slot {
    Slot::Bound(Expr::parse(&e).unwrap_or_else(|err| panic!("standard binding {e}: {err}")))
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
fn port(kind: &str, dir: &str, signal: Slot) -> Facet {
    let mut p = Port::default();
    p.kind = Slot::Const(Value::Str(kind.into()));
    p.dir = Slot::Const(Value::Enum(dir.into()));
    p.signal = signal;
    p.wrap()
}
fn anchor(port: NodeId, follow: bool) -> Facet {
    let mut a = Anchor::default();
    a.port = Slot::Const(Value::Ref(port));
    a.follow = Slot::Const(Value::Bool(follow));
    a.wrap()
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

/// A definition being built: its node, and children in order.
struct Def {
    id: NodeId,
    ops: Vec<Op>,
    key: OrderKey,
}
impl Def {
    fn child(&mut self, id: NodeId, ty_: &str, parent: NodeId, facets: Vec<Facet>) -> NodeId {
        self.ops.push(Op::Create { id, ty: ty(ty_), parent: Some(parent), order: self.key.clone(), facets });
        self.key = OrderKey::after(&self.key);
        id
    }
}

/// The definition `key` already copied into `doc`, if any.
pub fn find(doc: &Document, key: &str) -> Option<NodeId> {
    let source = format!("std:{key}");
    doc.nodes_of_type(ty("definition")).find(|&d| doc.is_live(d) && doc.get::<Import>(d).and_then(|i| i.source.constant()).and_then(|v| v.as_str()) == Some(source.as_str()))
}

/// The ops creating standard definition `key` under `library`, with ids
/// from `first`. Returns the ops and the definition.
pub fn definition_ops(key: &str, library: NodeId, order: OrderKey, first: u64) -> Result<(Vec<Op>, NodeId), String> {
    let entry = ENTRIES.iter().find(|e| e.key == key).ok_or_else(|| format!("no standard component {key:?}"))?;
    let mut ids = Ids(first);
    let def = ids.id();
    let mut imp = Import::default();
    imp.source = Slot::Const(Value::Str(format!("std:{key}")));
    imp.version = Slot::Const(Value::Str("1".into()));
    let mut d = Def { id: def, ops: vec![Op::Create { id: def, ty: ty("definition"), parent: Some(library), order, facets: vec![named(&entry.name.to_lowercase()), imp.wrap()] }], key: OrderKey::first() };
    let template = |d: &mut Def, ids: &mut Ids| {
        let mut t = Template::default();
        t.notation = Slot::Const(Value::Str(NOTATION.into()));
        let id = ids.id();
        d.child(id, "template", d.id, vec![t.wrap()])
    };
    match key {
        "slider" => {
            let (level, min, max, unit, out) = (ids.id(), ids.id(), ids.id(), ids.id(), ids.id());
            d.child(level, "port_spec", def, vec![named("level"), port("value", "in", n(10.0))]);
            d.child(min, "port_spec", def, vec![named("min"), port("value", "in", n(0.0))]);
            d.child(max, "port_spec", def, vec![named("max"), port("value", "in", n(20.0))]);
            d.child(unit, "port_spec", def, vec![named("unit"), port("value", "in", Slot::Const(q("1 mW")))]);
            d.child(out, "port_spec", def, vec![named("value"), port("value", "out", bound(format!("{level}.port.signal * {unit}.port.signal")))]);
            let t = template(&mut d, &mut ids);
            let mut s = Slider::default();
            s.min = bound(format!("{min}.port.signal"));
            s.max = bound(format!("{max}.port.signal"));
            s.step = n(0.1);
            s.value = bound(format!("{level}.port.signal"));
            let knob = ids.id();
            d.child(knob, "slider", t, vec![at(0.0, 0.0).wrap(), size(200.0, 24.0), s.wrap(), named("slider")]);
        }
        "readout" => {
            let value = ids.id();
            d.child(value, "port_spec", def, vec![named("value"), port("value", "in", n(0.0))]);
            let t = template(&mut d, &mut ids);
            let mut words = Text::default();
            words.content = bound(format!("fmt({value}.port.signal)"));
            let text = ids.id();
            d.child(text, "text", t, vec![at(0.0, 0.0).wrap(), words.wrap(), Font::default().wrap(), paint([0.1, 0.1, 0.12, 1.0]), named("readout")]);
        }
        "laser" => {
            let (power, out) = (ids.id(), ids.id());
            d.child(power, "port_spec", def, vec![named("power"), port("value", "in", Slot::Const(q("10 mW")))]);
            d.child(out, "port_spec", def, vec![named("out"), port("optical_power", "out", bound(format!("{power}.port.signal")))]);
            let t = template(&mut d, &mut ids);
            let mut glow = Fill::default();
            glow.color = Slot::Const(Value::Color([0.95, 0.15, 0.1, 1.0]));
            glow.opacity = bound(format!("clamp({power}.port.signal / 20 mW, 0.15, 1)"));
            let (body, beam, out_at) = (ids.id(), ids.id(), ids.id());
            d.child(body, "rect", t, vec![at(0.0, 0.0).wrap(), size(130.0, 70.0), paint([0.18, 0.2, 0.26, 1.0]), named("body")]);
            d.child(beam, "rect", t, vec![at(130.0, 28.0).wrap(), size(50.0, 14.0), glow.wrap(), named("beam")]);
            d.child(out_at, "empty", t, vec![at(180.0, 35.0).wrap(), anchor(out, false), named("out")]);
        }
        "fiber" => {
            let (loss, length, input, output) = (ids.id(), ids.id(), ids.id(), ids.id());
            d.child(loss, "port_spec", def, vec![named("loss_db_per_km"), port("value", "in", n(0.5))]);
            d.child(length, "port_spec", def, vec![named("length_km"), port("value", "in", n(2.0))]);
            d.child(input, "port_spec", def, vec![named("in"), port("optical_power", "in", Slot::Const(q("0 W")))]);
            d.child(output, "port_spec", def, vec![named("out"), port("optical_power", "out", bound(format!("{input}.port.signal * pow(10, -{loss}.port.signal * {length}.port.signal / 10)")))]);
            let t = template(&mut d, &mut ids);
            let (in_at, out_at, curve, contour, a, b) = (ids.id(), ids.id(), ids.id(), ids.id(), ids.id(), ids.id());
            d.child(in_at, "empty", t, vec![at(0.0, 0.0).wrap(), anchor(input, true), named("in")]);
            d.child(out_at, "empty", t, vec![at(300.0, 0.0).wrap(), anchor(output, true), named("out")]);
            let mut line = Stroke::default();
            line.color = Slot::Const(Value::Color([0.96, 0.7, 0.15, 1.0]));
            line.width = n(6.0);
            d.child(curve, "path", t, vec![at(0.0, 0.0).wrap(), crate::facet::PathShape::default().wrap(), line.wrap(), named("fiber")]);
            d.child(contour, "contour", curve, vec![crate::facet::Contour::default().wrap()]);
            for (p, end) in [(a, in_at), (b, out_at)] {
                let mut t2 = Transform2d::default();
                t2.x = bound(format!("{end}.transform2d.x"));
                t2.y = bound(format!("{end}.transform2d.y"));
                let half = format!("({out_at}.transform2d.x - {in_at}.transform2d.x) / 2");
                let mut h = crate::facet::Handles::default();
                h.ix = bound(format!("-{half}"));
                h.iy = n(0.0);
                h.ox = bound(half);
                h.oy = n(0.0);
                d.child(p, "point", contour, vec![t2.wrap(), h.wrap()]);
            }
        }
        "meter" => {
            let (input, range) = (ids.id(), ids.id());
            d.child(input, "port_spec", def, vec![named("in"), port("optical_power", "in", Slot::Const(q("0 W")))]);
            d.child(range, "port_spec", def, vec![named("range"), port("value", "in", Slot::Const(q("20 mW")))]);
            let t = template(&mut d, &mut ids);
            let (dial, in_at, pivot, needle, readout) = (ids.id(), ids.id(), ids.id(), ids.id(), ids.id());
            let mut r = Radius::default();
            r.rx = n(55.0);
            r.ry = n(55.0);
            let mut rim = Stroke::default();
            rim.color = Slot::Const(Value::Color([0.3, 0.32, 0.38, 1.0]));
            rim.width = n(3.0);
            d.child(dial, "ellipse", t, vec![at(55.0, 55.0).wrap(), r.wrap(), paint([0.95, 0.96, 0.98, 1.0]), rim.wrap(), named("dial")]);
            let mut facing = at(0.0, 55.0);
            facing.rot = n(std::f64::consts::PI);
            d.child(in_at, "empty", t, vec![facing.wrap(), anchor(input, false), named("in")]);
            let mut turn = at(55.0, 55.0);
            turn.rot = bound(format!("-3.14159 + {input}.port.signal / {range}.port.signal * 3.14159"));
            d.child(pivot, "empty", t, vec![turn.wrap(), named("pivot")]);
            d.child(needle, "rect", pivot, vec![at(0.0, -2.0).wrap(), size(50.0, 4.0), paint([0.85, 0.2, 0.15, 1.0]), named("needle")]);
            let mut words = Text::default();
            words.content = bound(format!("fmt({input}.port.signal, \"mW\", 2)"));
            d.child(readout, "text", t, vec![at(14.0, 118.0).wrap(), words.wrap(), Font::default().wrap(), paint([0.1, 0.1, 0.12, 1.0]), named("readout")]);
        }
        other => return Err(format!("no standard component {other:?}")),
    }
    Ok((d.ops, def))
}

fn root_of(doc: &Document, ty_: &str) -> Option<NodeId> {
    doc.roots().iter().copied().find(|&r| !r.is_session() && doc.live(r).is_some_and(|n| n.ty.name() == ty_))
}

fn after_last(doc: &Document, parent: Option<NodeId>) -> OrderKey {
    let kids: Vec<NodeId> = match parent {
        Some(p) => doc.children(p).to_vec(),
        None => doc.roots().to_vec(),
    };
    kids.iter().filter(|c| !c.is_expanded()).filter_map(|&c| doc.live(c)).map(|n| n.order.clone()).max().map_or_else(OrderKey::first, |k| OrderKey::after(&k))
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

/// The Stats composition's name (free-running.md §4).
pub const STATS: &str = "stats";
/// Its readout lines, top to bottom: the text nodes the `stats` producer
/// publishes `text.content` onto, by name.
pub const STATS_LINES: [&str; 4] = ["fps", "renderer", "build", "canvas"];

/// The ops building the Stats composition: a session-tier 2D composition,
/// never saved, with a dark card and one monospaced text line per entry of
/// `STATS_LINES`. Its text is runtime signals; the authored content is a
/// placeholder shown before the first publish.
pub fn stats_ops(first: NodeId, order: OrderKey) -> Vec<Op> {
    let mut ids = Ids(first.raw());
    let comp = ids.id();
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    let mut ops = vec![Op::Create { id: comp, ty: ty("composition"), parent: None, order, facets: vec![named(STATS), c.wrap()] }];
    let (line, pad, size_px) = (18.0, 10.0, 12.0);
    let card = ids.id();
    let mut key = OrderKey::first();
    ops.push(Op::Create { id: card, ty: ty("rect"), parent: Some(comp), order: key.clone(), facets: vec![at(8.0, 8.0).wrap(), size(330.0, 2.0 * pad + line * STATS_LINES.len() as f64), paint([0.08, 0.08, 0.1, 0.9]), named("card")] });
    for (i, name) in STATS_LINES.iter().enumerate() {
        key = OrderKey::after(&key);
        let mut body = Text::default();
        body.content = Slot::Const(Value::Str("…".into()));
        let mut font = Font::default();
        font.family = Slot::Const(Value::Str("JetBrains Mono".into()));
        font.size = n(size_px);
        font.tabular = Slot::Const(Value::Bool(true));
        let colour = if i == 0 { [0.35, 0.95, 0.95, 1.0] } else { [0.92, 0.92, 0.94, 1.0] };
        ops.push(Op::Create { id: ids.id(), ty: ty("text"), parent: Some(comp), order: key.clone(), facets: vec![at(8.0 + pad, 8.0 + pad + line * i as f64).wrap(), body.wrap(), font.wrap(), paint(colour), named(name)] });
    }
    ops
}

/// The ops building the Library tab's session composition: a grid of
/// cells, each an `empty` named for its entry's key holding a baked preview
/// (scaled to fit) and a label. Ids from `first`, a session id. Previews are
/// each standard picture's resolved shapes, with no bindings and no ports.
pub fn library_ops(first: NodeId, order: OrderKey) -> Vec<Op> {
    let mut ids = Ids(first.raw());
    let comp = ids.id();
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    let mut ops = vec![Op::Create { id: comp, ty: ty("composition"), parent: None, order, facets: vec![named(LIBRARY), c.wrap()] }];
    let (cell_w, cell_h, preview_w, preview_h) = (175.0, 150.0, 150.0, 96.0);
    let mut key = OrderKey::first();
    for (i, entry) in ENTRIES.iter().enumerate() {
        let (col, row) = ((i % 2) as f64, (i / 2) as f64);
        let cell = ids.id();
        ops.push(Op::Create { id: cell, ty: ty("empty"), parent: Some(comp), order: key.clone(), facets: vec![at(20.0 + col * cell_w, 20.0 + row * cell_h).wrap(), named(entry.key)] });
        key = OrderKey::after(&key);
        // A card behind the preview, the preview fitted into it, a label.
        let card = ids.id();
        let mut corners = crate::facet::Corners::default();
        corners.tl = n(8.0);
        corners.tr = n(8.0);
        corners.br = n(8.0);
        corners.bl = n(8.0);
        ops.push(Op::Create { id: card, ty: ty("rect"), parent: Some(cell), order: OrderKey::first(), facets: vec![at(0.0, 0.0).wrap(), size(preview_w + 10.0, preview_h + 10.0), paint([0.965, 0.965, 0.975, 1.0]), corners.wrap(), named("card")] });
        let baked = bake(entry.key);
        if let Some(b) = baked.bounds {
            let s = (preview_w / b.width().max(1.0)).min(preview_h / b.height().max(1.0)).min(1.0);
            let holder = ids.id();
            let mut t = at(5.0 + (preview_w - b.width() * s) / 2.0 - b.x0 * s, 5.0 + (preview_h - b.height() * s) / 2.0 - b.y0 * s);
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
        font.size = n(13.0);
        ops.push(Op::Create { id: label, ty: ty("text"), parent: Some(cell), order: OrderKey::after(&OrderKey::after(&OrderKey::first())), facets: vec![at(4.0, preview_h + 18.0).wrap(), words.wrap(), font.wrap(), paint([0.25, 0.25, 0.3, 1.0]), named("label")] });
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
        // Anchors mark ports; a preview has none.
        if doc.facet(id, crate::facet::FacetKind::Anchor).is_some() && doc.children(id).is_empty() {
            continue;
        }
        let parent = node.parent.and_then(|p| index.iter().position(|&x| x == p));
        let facets: Vec<Facet> = doc
            .facets_of(id)
            .filter(|f| !matches!(f, Facet::Anchor(_)))
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
