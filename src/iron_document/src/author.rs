//! Authoring components from the UI (component-authoring.md §2–§4): ports
//! placed on a main, inputs added to a definition, a port's spec edited,
//! shapes wrapped in an empty, and two ports connected by a line.
//!
//! A definition's inputs and its instances' arguments must agree
//! (`definition::validate_entity`), so every edit that changes the inputs
//! also rewrites the arguments of every instance of the definition, in the
//! same transaction: a new input gets its default, a renamed one keeps its
//! value, a removed one is dropped.

use crate::definition;
use crate::document::Document;
use crate::expand;
use crate::expr::Leaf;
use crate::facet::{Anchor, Args, Facet, FacetKind, Instance, LeafPath, LeafStruct, Name, Port, Transform2d};
use crate::id::NodeId;
use crate::op::{self, Op};
use crate::order::OrderKey;
use crate::registry::TypeId;
use crate::value::{Slot, Value};

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).expect("registered type")
}

fn named(text: &str) -> Facet {
    let mut n = Name::default();
    n.text = Slot::Const(Value::Str(text.to_owned()));
    n.wrap()
}

fn name_of(doc: &Document, id: NodeId) -> Option<String> {
    doc.get::<Name>(id)?.text.constant()?.as_str().map(str::to_owned)
}

/// A name usable as an argument: letters, digits and `_`.
fn ident(name: &str) -> String {
    let s: String = name.trim().chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' }).collect();
    if s.is_empty() || s.starts_with(|c: char| c.is_ascii_digit()) { format!("p_{s}") } else { s }
}

/// `base`, or `base2`, `base3`…: the first name no port_spec of `def` has.
fn fresh_name(doc: &Document, def: NodeId, base: &str) -> String {
    let taken = |n: &str| doc.children(def).iter().any(|&c| doc.live(c).is_some_and(|x| x.ty.name() == "port_spec") && name_of(doc, c).as_deref() == Some(n));
    if !taken(base) {
        return base.to_owned();
    }
    (2..).map(|i| format!("{base}{i}")).find(|n| !taken(n)).expect("a free name")
}

/// The authored entities that instantiate `def`.
pub fn entities_of(doc: &Document, def: NodeId) -> Vec<NodeId> {
    doc.nodes_of_type(ty("entity"))
        .filter(|&e| !e.is_expanded() && doc.is_live(e))
        .filter(|&e| doc.get::<Instance>(e).and_then(|i| i.def.constant()).and_then(|v| v.as_ref()) == Some(def))
        .collect()
}

/// The ops `ops` need so every instance of `def` keeps valid arguments
/// after them: each entity's `args` rebuilt from the inputs `ops` leave,
/// keeping values by name (`renamed` maps old names to new).
fn resync_args(doc: &Document, def: NodeId, ops: &[Op], renamed: &[(String, String)]) -> Result<Vec<Op>, String> {
    let mut after = doc.clone();
    let stamp = after.version().next();
    for (i, op) in ops.iter().enumerate() {
        op::apply_op(&mut after, i, op, stamp).map_err(|e| e.message)?;
    }
    let inputs = definition::inputs_of(&after, def)?;
    let mut out = Vec::new();
    for e in entities_of(doc, def) {
        let old = doc.get::<Args>(e).cloned().unwrap_or_default();
        let mut new = Args::default();
        for input in &inputs {
            let was = renamed.iter().find(|(_, n)| n == input.name).map(|(o, _)| o.as_str()).unwrap_or(input.name);
            let slot = old.slots.get(was).cloned().filter(|s| s.constant().is_none_or(|v| v.kind() == input.default.kind())).unwrap_or(Slot::Const(input.default.clone()));
            new.insert(input.name, slot);
        }
        if new.slots != old.slots {
            out.push(Op::AddFacet { id: e, facet: new.wrap() });
        }
    }
    Ok(out)
}

/// What a port is by default for its kind: zero in the kind's unit.
fn zero_of(kind: &str) -> Value {
    match crate::port::kind(kind).and_then(|k| k.unit) {
        Some(unit) => Value::parse_quantity(&format!("0 {unit}")).unwrap_or(Value::Number(0.0)),
        None => Value::Number(0.0),
    }
}

/// Place a port on main `main` at `at` (the main's own space): a
/// `port_spec` in its definition and an anchor empty in the template there
/// (component-authoring.md §2). Returns the ops, the spec and the anchor.
pub fn add_port(doc: &Document, main: NodeId, at: [f64; 2], dir: &str, follow: bool) -> Result<(Vec<Op>, NodeId, NodeId), String> {
    let def = expand::main_definition(doc, main).ok_or_else(|| format!("{main} is not a main: ports are placed on a component's main"))?;
    let template = expand::main_template(doc, main).ok_or_else(|| format!("{main} shows no template"))?;
    let ids = doc.next_ids(2);
    let (spec, anchor) = (ids[0], ids[1]);
    let name = fresh_name(doc, def, if dir == "out" { "out" } else { "in" });
    let last_spec = doc.children(def).iter().filter(|&&c| doc.live(c).is_some_and(|n| n.ty.name() == "port_spec")).filter_map(|&c| doc.live(c)).map(|n| n.order.clone()).max();
    let spec_order = last_spec.map_or_else(|| doc.children(def).iter().filter_map(|&c| doc.live(c)).map(|n| n.order.clone()).min().map_or_else(OrderKey::first, |k| OrderKey::before(&k)), |k| OrderKey::after(&k));
    let mut p = Port::default();
    p.kind = Slot::Const(Value::Str("value".into()));
    p.dir = Slot::Const(Value::Enum(dir.into()));
    p.signal = Slot::Const(Value::Number(0.0));
    let mut t = Transform2d::default();
    t.x = Slot::Const(Value::Number(at[0]));
    t.y = Slot::Const(Value::Number(at[1]));
    let mut a = Anchor::default();
    a.port = Slot::Const(Value::Ref(spec));
    a.follow = Slot::Const(Value::Bool(follow));
    let last = doc.children(template).iter().filter(|c| !c.is_expanded()).filter_map(|&c| doc.live(c)).map(|n| n.order.clone()).max();
    let mut ops = vec![
        Op::Create { id: spec, ty: ty("port_spec"), parent: Some(def), order: spec_order, facets: vec![named(&name), p.wrap()] },
        Op::Create { id: anchor, ty: ty("empty"), parent: Some(template), order: last.map_or_else(OrderKey::first, |k| OrderKey::after(&k)), facets: vec![t.wrap(), a.wrap(), named(&name)] },
    ];
    let args = resync_args(doc, def, &ops, &[])?;
    ops.extend(args);
    Ok((ops, spec, anchor))
}

/// Add a setting to `def`: an `in` port with no anchor (component-authoring.md
/// §2), named `name` (made unique), defaulting to `default`.
pub fn add_input(doc: &Document, def: NodeId, name: &str, default: Value) -> Result<(Vec<Op>, NodeId), String> {
    if doc.live(def).map(|n| n.ty.name()) != Some("definition") {
        return Err(format!("{def} is not a definition"));
    }
    let spec = doc.next_id();
    let name = fresh_name(doc, def, &ident(name));
    let mut p = Port::default();
    p.kind = Slot::Const(Value::Str("value".into()));
    p.dir = Slot::Const(Value::Enum("in".into()));
    p.signal = Slot::Const(default);
    let first = doc.children(def).iter().filter_map(|&c| doc.live(c)).map(|n| n.order.clone()).min();
    let last_spec = doc.children(def).iter().filter(|&&c| doc.live(c).is_some_and(|n| n.ty.name() == "port_spec")).filter_map(|&c| doc.live(c)).map(|n| n.order.clone()).max();
    let order = match (last_spec, first) {
        (Some(k), _) => OrderKey::after(&k),
        (None, Some(k)) => OrderKey::before(&k),
        (None, None) => OrderKey::first(),
    };
    let mut ops = vec![Op::Create { id: spec, ty: ty("port_spec"), parent: Some(def), order, facets: vec![named(&name), p.wrap()] }];
    ops.extend(resync_args(doc, def, &ops, &[])?);
    Ok((ops, spec))
}

/// Changes to a port's spec from the inspector's Port section.
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
pub struct PortPatch {
    pub name: Option<String>,
    pub kind: Option<String>,
    pub dir: Option<String>,
    pub follow: Option<bool>,
    pub connector: Option<String>,
    pub gender: Option<String>,
    /// The default (an input's) or the value (an output's), as markup:
    /// `10 mW`, or `{= #power.port.signal}` for an expression.
    pub signal: Option<String>,
}

/// The ops for `patch` on port_spec `spec`, and on its anchors and its
/// definition's instances.
pub fn edit_port(doc: &Document, spec: NodeId, patch: &PortPatch) -> Result<Vec<Op>, String> {
    if doc.live(spec).map(|n| n.ty.name()) != Some("port_spec") {
        return Err(format!("{spec} is not a port_spec"));
    }
    let def = doc.live(spec).and_then(|n| n.parent).ok_or("a port_spec belongs to a definition")?;
    let path = |f: &str| LeafPath::new(FacetKind::Port, f).expect("declared");
    let mut ops = Vec::new();
    let mut renamed = Vec::new();
    if let Some(name) = &patch.name {
        let old = name_of(doc, spec).unwrap_or_default();
        let new = ident(name);
        if new != old {
            if doc.children(def).iter().any(|&c| c != spec && doc.live(c).is_some_and(|n| n.ty.name() == "port_spec") && name_of(doc, c).as_deref() == Some(new.as_str())) {
                return Err(format!("{def} already has a port {new:?}"));
            }
            ops.push(Op::Set { id: spec, path: LeafPath::new(FacetKind::Name, "text").expect("declared"), slot: Slot::Const(Value::Str(new.clone())) });
            renamed.push((old, new));
        }
    }
    if let Some(kind) = &patch.kind {
        crate::port::kind(kind).ok_or_else(|| {
            let names: Vec<&str> = crate::port::KINDS.iter().map(|k| k.name).collect();
            format!("unknown port kind {kind:?}; kinds: {}", names.join(", "))
        })?;
        ops.push(Op::Set { id: spec, path: path("kind"), slot: Slot::Const(Value::Str(kind.clone())) });
        // A new kind's values are in its unit.
        if patch.signal.is_none() && doc.leaf(spec, path("signal")).is_some_and(|s| s.constant().is_some()) {
            ops.push(Op::Set { id: spec, path: path("signal"), slot: Slot::Const(zero_of(kind)) });
        }
    }
    if let Some(dir) = &patch.dir {
        if !matches!(dir.as_str(), "in" | "out" | "pair") {
            return Err(format!("a port's direction is in, out or pair, not {dir:?}"));
        }
        ops.push(Op::Set { id: spec, path: path("dir"), slot: Slot::Const(Value::Enum(dir.clone())) });
    }
    if let Some(c) = &patch.connector {
        ops.push(Op::Set { id: spec, path: path("connector"), slot: Slot::Const(Value::Str(c.trim().to_owned())) });
    }
    if let Some(g) = &patch.gender {
        if !matches!(g.as_str(), "any" | "male" | "female" | "neutral") {
            return Err(format!("a gender is any, male, female or neutral, not {g:?}"));
        }
        ops.push(Op::Set { id: spec, path: path("gender"), slot: Slot::Const(Value::Enum(g.clone())) });
    }
    if let Some(text) = &patch.signal {
        let text = text.trim();
        let slot = match text.strip_prefix("{=").and_then(|t| t.strip_suffix('}')) {
            Some(e) => Slot::Bound(crate::expr::Expr::parse(e.trim()).map_err(|e| format!("signal: {e}"))?),
            None => Slot::Const(text.parse::<f64>().map(Value::Number).or_else(|_| Value::parse_quantity(text)).map_err(|_| format!("signal: {text:?} is not a number or quantity"))?),
        };
        ops.push(Op::Set { id: spec, path: path("signal"), slot });
    }
    if let Some(follow) = patch.follow {
        for a in doc.nodes_with(FacetKind::Anchor) {
            if !a.is_expanded() && doc.is_live(a) && doc.get::<Anchor>(a).and_then(|x| x.port.constant()).and_then(|v| v.as_ref()) == Some(spec) {
                ops.push(Op::Set { id: a, path: LeafPath::new(FacetKind::Anchor, "follow").expect("declared"), slot: Slot::Const(Value::Bool(follow)) });
                if let Some(name) = renamed.last().map(|(_, n)| n.clone()) {
                    ops.push(Op::Set { id: a, path: LeafPath::new(FacetKind::Name, "text").expect("declared"), slot: Slot::Const(Value::Str(name)) });
                }
            }
        }
    } else if let Some(name) = renamed.last().map(|(_, n)| n.clone()) {
        for a in doc.nodes_with(FacetKind::Anchor) {
            if !a.is_expanded() && doc.is_live(a) && doc.get::<Anchor>(a).and_then(|x| x.port.constant()).and_then(|v| v.as_ref()) == Some(spec) {
                ops.push(Op::Set { id: a, path: LeafPath::new(FacetKind::Name, "text").expect("declared"), slot: Slot::Const(Value::Str(name.clone())) });
            }
        }
    }
    let args = resync_args(doc, def, &ops, &renamed)?;
    ops.extend(args);
    Ok(ops)
}

/// Wrap `selection` (siblings) in a new empty at the centre of their extent
/// (component-authoring.md §2): the empty is created beside them, they move
/// into it, and their positions shift by the centre, so nothing moves on
/// screen. `extent` gives a node's box in its parent's space. Inside a main,
/// the ops name derived nodes and `expand::translate` sends them to the
/// definition.
pub fn wrap_in_empty(doc: &Document, extent: &dyn Fn(NodeId) -> Option<kurbo::Rect>, selection: &[NodeId]) -> Result<(Vec<Op>, NodeId), String> {
    let mut roots: Vec<NodeId> = selection.iter().copied().filter(|&n| doc.is_live(n)).collect();
    if roots.is_empty() {
        return Err("nothing selected to wrap".into());
    }
    let parent = doc.live(roots[0]).and_then(|n| n.parent).ok_or("wrap shapes that sit in a composition")?;
    if roots.iter().any(|&r| doc.live(r).and_then(|n| n.parent) != Some(parent)) {
        return Err("the selection is in different groups; wrap siblings".into());
    }
    roots.sort_by(|a, b| doc.live(*a).map(|n| n.order.clone()).cmp(&doc.live(*b).map(|n| n.order.clone())));
    let r = roots.iter().filter_map(|&n| extent(n)).reduce(|a, b| a.union(b)).ok_or("the selection has no extent")?;
    let c = r.center();
    let id = doc.next_id();
    let mut t = Transform2d::default();
    t.x = Slot::Const(Value::Number(c.x));
    t.y = Slot::Const(Value::Number(c.y));
    let order = doc.live(roots[0]).expect("live").order.clone();
    let mut ops = vec![Op::Create { id, ty: ty("empty"), parent: Some(parent), order: OrderKey::before(&order), facets: vec![t.wrap(), named("empty")] }];
    let mut key = OrderKey::first();
    for &n in &roots {
        ops.push(Op::Reparent { id: n, parent: Some(id), order: key.clone() });
        key = OrderKey::after(&key);
        for (field, d) in [("x", c.x), ("y", c.y)] {
            let path = LeafPath::new(FacetKind::Transform2d, field).expect("declared");
            if let Some(v) = doc.leaf(n, path).and_then(|s| s.constant()).and_then(|v| v.as_f64()) {
                ops.push(Op::Set { id: n, path, slot: Slot::Const(Value::Number(v - d)) });
            }
        }
    }
    Ok((ops, id))
}

/// Connect two ports by a line (component-authoring.md §3): settings and
/// outputs with no anchor, or any two compatible ports. `a` and `b` are
/// ports (an instance's, `#entity/spec`); the kinds must agree, a one-way
/// pair must run out to in, and `value` ports must carry one dimension.
pub fn connect(doc: &Document, read: &dyn Fn(Leaf) -> Option<Value>, a: NodeId, b: NodeId) -> Result<Vec<Op>, String> {
    let port = |p: NodeId| doc.get::<Port>(p).ok_or_else(|| format!("{p} is not a port"));
    let text = |p: &Port, f: &str| match f {
        "kind" => p.kind.constant(),
        _ => p.dir.constant(),
    }
    .and_then(|v| v.as_str())
    .unwrap_or("")
    .to_owned();
    let (pa, pb) = (port(a)?, port(b)?);
    let (ka, kb) = (text(pa, "kind"), text(pb, "kind"));
    if ka != kb {
        return Err(format!("{a} carries {ka}, {b} carries {kb}"));
    }
    let signal = |p: NodeId| read(Leaf { node: p, path: LeafPath::new(FacetKind::Port, "signal").expect("declared") }).and_then(|v| v.as_quantity()).map(|(_, d)| d);
    if let (Some(da), Some(db)) = (signal(a), signal(b))
        && da != db
    {
        return Err(format!("{a} is in {}, {b} in {}", da.canonical_unit(), db.canonical_unit()));
    }
    for p in [a, b] {
        if crate::plug::connection_of(doc, p).is_some() && text(port(p)?, "dir") == "in" {
            return Err(format!("{p} already has a connection feeding it"));
        }
    }
    let id = doc.next_id();
    let rels = doc.next_relation_ids(2);
    crate::plug::plug_ops(doc, a, b, None, id, [rels[0], rels[1]])
}

/// The port_specs of `def`, for an inspector: name, direction and whether
/// it is placed on a picture.
pub fn ports_of(doc: &Document, def: NodeId) -> Vec<(NodeId, String, String, bool)> {
    let placed: Vec<NodeId> = doc.nodes_with(FacetKind::Anchor).filter(|a| !a.is_expanded() && doc.is_live(*a)).filter_map(|a| doc.get::<Anchor>(a).and_then(|x| x.port.constant()).and_then(|v| v.as_ref())).collect();
    doc.children(def)
        .iter()
        .copied()
        .filter(|&c| doc.live(c).is_some_and(|n| n.ty.name() == "port_spec"))
        .map(|c| {
            let dir = doc.get::<Port>(c).and_then(|p| p.dir.constant()).and_then(|v| v.as_str()).unwrap_or("in").to_owned();
            (c, name_of(doc, c).unwrap_or_default(), dir, placed.contains(&c))
        })
        .collect()
}
