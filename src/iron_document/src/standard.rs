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
        "plot" => plot(&mut d, &mut ids, template),
        other => return Err(format!("no standard component {other:?}")),
    }
    Ok((d.ops, def))
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

/// `std:plot` (plans/specs/plot.md §4): series against x, drawn from SVG
/// path data, in a box of its own that reflows when resized. Ranges and
/// the cursor default to NaN, "auto" and "none". Template parts read the
/// box (`#T.size.w/h`); the ranges are outputs, so other components can
/// read them, and they do not depend on the box.
fn plot(d: &mut Def, ids: &mut Ids, template: impl Fn(&mut Def, &mut Ids) -> NodeId) {
    let def = d.id;
    let nan = || Slot::Const(Value::Number(f64::NAN));
    let text_ = |t: &str| Slot::Const(Value::Str(t.into()));
    let colour = |c: [f32; 4]| Slot::Const(Value::Color(c));
    let xs0: Vec<f64> = (0..=40).map(|i| i as f64 * 0.25).collect();
    let ys0: Vec<f64> = xs0.iter().map(|x| (x * 1.3).sin() * (-x / 5.0).exp()).collect();
    let none = || Slot::Const(Value::series(Vec::<f64>::new(), crate::units::Dim::NONE));
    let mut input = |d: &mut Def, name: &str, default: Slot| {
        let id = ids.id();
        d.child(id, "port_spec", def, vec![named(name), port("value", "in", default)]);
        id
    };
    let xs = input(d, "xs", Slot::Const(Value::series(xs0, crate::units::Dim::NONE)));
    let ys = input(d, "ys", Slot::Const(Value::series(ys0, crate::units::Dim::NONE)));
    let ys2 = input(d, "ys2", none());
    let ys3 = input(d, "ys3", none());
    let f = input(d, "f", nan());
    let style = input(d, "style", Slot::Const(Value::Enum("line".into())));
    let x_min = input(d, "x_min", nan());
    let x_max = input(d, "x_max", nan());
    let y_min = input(d, "y_min", nan());
    let y_max = input(d, "y_max", nan());
    let x_label = input(d, "x_label", text_(""));
    let y_label = input(d, "y_label", text_(""));
    let title = input(d, "title", text_(""));
    let cursor = input(d, "cursor", nan());
    let grid = input(d, "grid", Slot::Const(Value::Bool(true)));
    let color = input(d, "color", colour([0.184, 0.435, 0.871, 1.0]));
    let color2 = input(d, "color2", colour([0.91, 0.455, 0.231, 1.0]));
    let color3 = input(d, "color3", colour([0.098, 0.663, 0.475, 1.0]));
    let reveal = input(d, "reveal", n(1.0));
    let sig = |id: NodeId| format!("{id}.port.signal");
    let (xs, ys, ys2, ys3, f, style, x_min, x_max, y_min, y_max) = (sig(xs), sig(ys), sig(ys2), sig(ys3), sig(f), sig(style), sig(x_min), sig(x_max), sig(y_min), sig(y_max));
    let (x_label, y_label, title, cursor, grid, color, color2, color3, reveal) = (sig(x_label), sig(y_label), sig(title), sig(cursor), sig(grid), sig(color), sig(color2), sig(color3), sig(reveal));
    let is_fn = format!("kind({f}) == \"function\"");
    // Outputs: the ranges in use, and the curve's value at the cursor.
    let mut output = |d: &mut Def, name: &str, e: String| {
        let id = ids.id();
        d.child(id, "port_spec", def, vec![named(name), port("value", "out", bound(e))]);
        sig(id)
    };
    let x_lo = output(d, "x_lo", format!("isnan({x_min}) ? ({is_fn} ? 0 : min({xs})) : {x_min}"));
    let x_hi = output(d, "x_hi", format!("isnan({x_max}) ? ({is_fn} ? 10 : max({xs})) : {x_max}"));
    // The function sampled for the range, at a fixed count (outputs do not
    // see a picture's box); the picture samples it at its own width.
    let ys_all = format!("concat({is_fn} ? sample({f}, {x_lo}, {x_hi}, 200) : {ys}, {ys2}, {ys3})");
    let y_lo = output(d, "y_lo", format!("isnan({y_min}) ? nice_lo(min({ys_all}), max({ys_all}), 5) : {y_min}"));
    let y_hi = output(d, "y_hi", format!("isnan({y_max}) ? nice_hi(min({ys_all}), max({ys_all}), 5) : {y_max}"));
    let y_at = output(d, "y_at_cursor", format!("isnan({cursor}) ? 0 / 0 : ({is_fn} ? {f}({cursor}) : interp({xs}, {ys}, {cursor}))"));

    let t = template(d, ids);
    // The template's box: its picture's default size, read by every part.
    d.ops.iter_mut().for_each(|op| {
        if let Op::Create { id, facets, .. } = op
            && *id == t
        {
            facets.push(size(360.0, 240.0));
        }
    });
    let (w, h) = (format!("{t}.size.w"), format!("{t}.size.h"));
    let top = format!("({title} != \"\" ? 30 : 14)");
    let (left, right, bottom) = (54.0, 16.0, 40.0);
    let aw = format!("max(1, {w} - {})", left + right);
    let ah = format!("max(1, {h} - {top} - {bottom})");
    let samples = format!("clamp(floor({aw} / 2), 2, 2000)");
    let px1 = format!("({is_fn} ? range({x_lo}, {x_hi}, {samples}) : {xs})");
    let py1 = format!("({is_fn} ? sample({f}, {x_lo}, {x_hi}, {samples}) : {ys})");
    let xp = |v: &str| format!("scale({v}, {x_lo}, {x_hi}, 0, {aw})");
    let yp = |v: &str| format!("scale({v}, {y_lo}, {y_hi}, {ah}, 0)");
    let xt = format!("ticks({x_lo}, {x_hi}, clamp(floor({aw} / 70), 2, 8))");
    let yt = format!("ticks({y_lo}, {y_hi}, clamp(floor({ah} / 40), 2, 8))");
    let ink = [0.2, 0.22, 0.27, 1.0];
    let muted = [0.38, 0.41, 0.47, 1.0];

    let text_part = |d: &mut Def, ids: &mut Ids, parent: NodeId, name: &str, x: Slot, y: Slot, content: String, size_: f64, weight: f64, box_w: Option<(f64, &str)>, rot: f64, c: [f32; 4]| {
        let id = ids.id();
        let mut tr = Transform2d::default();
        tr.x = x;
        tr.y = y;
        tr.rot = n(rot);
        let mut words = Text::default();
        words.content = bound(content);
        let mut font = Font::default();
        font.size = n(size_);
        font.weight = n(weight);
        let mut facets = vec![tr.wrap()];
        if let Some((bw, align)) = box_w {
            words.sizing = Slot::Const(Value::Enum("fixed".into()));
            font.align = Slot::Const(Value::Enum(align.into()));
            facets.push(size(bw, size_ * 1.5));
        }
        facets.extend([words.wrap(), font.wrap(), paint(c), named(name)]);
        d.child(id, "text", parent, facets);
    };
    let path_part = |d: &mut Def, ids: &mut Ids, parent: NodeId, name: &str, dd: String, stroke: Stroke, fill: Fill, trim_end: Option<String>| {
        let id = ids.id();
        let mut shape = crate::facet::PathShape::default();
        shape.d = bound(dd);
        let mut facets = vec![at(0.0, 0.0).wrap(), shape.wrap(), stroke.wrap(), fill.wrap(), named(name)];
        // Drawn on by `trim.end` (Trim Paths).
        if let Some(e) = trim_end {
            let mut t = crate::facet::Trim::default();
            t.end = bound(e);
            facets.push(t.wrap());
        }
        d.child(id, "path", parent, facets);
    };
    let line = |c: [f32; 4], width: f64| {
        let mut s = Stroke::default();
        s.color = Slot::Const(Value::Color(c));
        s.width = n(width);
        s
    };
    let no_fill = || {
        let mut f = Fill::default();
        f.opacity = n(0.0);
        f
    };

    // 1. The frame, filling the box.
    let frame = ids.id();
    let mut corners = crate::facet::Corners::default();
    for c in [&mut corners.tl, &mut corners.tr, &mut corners.br, &mut corners.bl] {
        *c = n(6.0);
    }
    let mut box_ = Size::default();
    box_.w = bound(w.clone());
    box_.h = bound(h.clone());
    d.child(frame, "rect", t, vec![at(0.0, 0.0).wrap(), box_.wrap(), corners.wrap(), paint([1.0, 1.0, 1.0, 1.0]), line([0.82, 0.84, 0.88, 1.0], 1.0).wrap(), named("frame")]);
    // 2. The title.
    text_part(d, ids, t, "title", n(14.0), n(8.0), title.clone(), 13.0, 600.0, None, 0.0, ink);
    // 3. The plot area, clipped: grid, series, cursor.
    let area = ids.id();
    let mut area_at = Transform2d::default();
    area_at.x = n(left);
    area_at.y = bound(top.clone());
    let mut clip = crate::facet::GroupClip::default();
    clip.w = bound(aw.clone());
    clip.h = bound(ah.clone());
    d.child(area, "group", t, vec![area_at.wrap(), clip.wrap(), named("area")]);
    path_part(d, ids, area, "grid", format!("{grid} ? segments({xg}, 0, {xg}, {ah}) + ' ' + segments(0, {yg}, {aw}, {yg}) : \"\"", xg = xp(&xt), yg = yp(&yt)), line([0.9, 0.91, 0.94, 1.0], 1.0), no_fill(), None);
    for (k, (sx, sy, c)) in [(xs.clone(), ys3.clone(), color3.clone()), (xs.clone(), ys2.clone(), color2.clone()), (px1.clone(), py1.clone(), color.clone())].into_iter().enumerate() {
        let (x, y) = (xp(&sx), yp(&sy));
        let draw = format!(
            "len({sy}) == 0 ? \"\" : ({style} == 'scatter' ? circles({x}, {y}, 3) : ({style} == 'smooth' ? smooth({x}, {y}) : ({style} == 'area' ? area({x}, {y}, {ah}) : ({style} == 'bars' ? bars({x}, {y}, {ah}, max(1, {aw} / max(1, len({sy})) * 0.7)) : 'M ' + points({x}, {y})))))"
        );
        let solid = format!("({style} == 'scatter' || {style} == 'bars')");
        let mut stroke = line([0.0, 0.0, 0.0, 1.0], 2.0);
        stroke.color = bound(format!("{solid} ? rgb(0, 0, 0, 0) : {c}"));
        let mut fill = Fill::default();
        fill.color = bound(c.clone());
        fill.opacity = bound(format!("{solid} ? {reveal} : ({style} == 'area' ? 0.18 * {reveal} : 0)"));
        path_part(d, ids, area, &format!("series {}", 3 - k), draw, stroke, fill, Some(reveal.clone()));
    }
    let cx = xp(&cursor);
    let cy = yp(&y_at);
    let mut dashed = line(muted, 1.0);
    dashed.dash = Slot::Const(Value::Str("4 3".into()));
    path_part(d, ids, area, "cursor", format!("isnan({cursor}) ? \"\" : segments({cx}, 0, {cx}, {ah})"), dashed, no_fill(), None);
    let mut dot_fill = Fill::default();
    dot_fill.color = bound(color.clone());
    path_part(d, ids, area, "cursor dot", format!("isnan({cursor}) ? \"\" : circles({cx}, {cy}, 4)"), line([1.0, 1.0, 1.0, 1.0], 1.5), dot_fill, None);
    text_part(d, ids, area, "cursor value", bound(format!("isnan({cursor}) ? 0 : {cx} + 6")), bound(format!("isnan({cursor}) ? 0 : {cy} - 20")), format!("isnan({cursor}) ? \"\" : fmt(sig({y_at}, 3))"), 11.0, 600.0, None, 0.0, ink);
    // 4. Axes, tick marks and labels, unclipped, in the same space.
    let axes = ids.id();
    let mut axes_at = Transform2d::default();
    axes_at.x = n(left);
    axes_at.y = bound(top.clone());
    d.child(axes, "group", t, vec![axes_at.wrap(), named("axes")]);
    path_part(d, ids, axes, "axis lines", format!("'M 0,0 L 0,' + str({ah}) + ' L ' + str({aw}) + ',' + str({ah})"), line([0.55, 0.57, 0.62, 1.0], 1.0), no_fill(), None);
    path_part(d, ids, axes, "tick marks", format!("segments({xg}, {ah}, {xg}, {ah} + 4) + ' ' + segments(-4, {yg}, 0, {yg})", xg = xp(&xt), yg = yp(&yt)), line([0.55, 0.57, 0.62, 1.0], 1.0), no_fill(), None);
    let x_unit = format!("unit_of({px1})");
    let y_unit = format!("unit_of({py1})");
    for i in 0..8 {
        let v = format!("at({xt}, {i})");
        text_part(d, ids, axes, &format!("x tick {i}"), bound(format!("{} - 28", xp(&v))), bound(format!("{ah} + 7")), format!("{i} < len({xt}) ? fmt(in_unit({v}, {x_unit})) : \"\""), 10.0, 400.0, Some((56.0, "center")), 0.0, muted);
    }
    for i in 0..8 {
        let v = format!("at({yt}, {i})");
        text_part(d, ids, axes, &format!("y tick {i}"), n(-50.0), bound(format!("{} - 7", yp(&v))), format!("{i} < len({yt}) ? fmt(in_unit({v}, {y_unit})) : \"\""), 10.0, 400.0, Some((44.0, "right")), 0.0, muted);
    }
    // 5. Axis titles: the label given, else the data's unit.
    text_part(d, ids, t, "x title", bound(format!("{left} + {aw} / 2 - 120")), bound(format!("{h} - 18")), format!("{x_label} != \"\" ? {x_label} : {x_unit}"), 11.0, 500.0, Some((240.0, "center")), 0.0, muted);
    let mut y_title_rot = -std::f64::consts::FRAC_PI_2;
    y_title_rot = (y_title_rot * 1e9).round() / 1e9;
    text_part(d, ids, t, "y title", n(4.0), bound(format!("{top} + {ah} / 2 + 120")), format!("{y_label} != \"\" ? {y_label} : {y_unit}"), 11.0, 500.0, Some((240.0, "center")), y_title_rot, muted);
}

