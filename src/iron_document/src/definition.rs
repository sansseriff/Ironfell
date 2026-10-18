//! Definitions as data (entity-composition-model.md §4, component-model.md §4).
//!
//! A definition is a subtree under a `library` root:
//!
//! ```text
//! library
//!   definition "bar"
//!     port_spec "level"   kind=value dir=in signal=0.4      (an input and its default)
//!     template  notation="free"
//!       rect "bar"  h.bind = #<level spec>.port.signal * 300   (the template)
//! ```
//!
//! Its interface is its `port_spec`s: `in` ports, each with a default in
//! `port.signal`; `out` ports, whose `port.signal` is the behaviour, a
//! binding; and across/through pairs. An `entity` instantiates a definition:
//! its `instance.def` names it and its `args` holds one slot per `in` port,
//! each bindable. A *linked* instance (`instantiate`, the default) stores
//! only that entity and a `use` per picture; its ports and pictures are
//! expanded from the definition (`expand.rs`), an `in` port reading its
//! argument (`#entity.args.level`) unless a connection feeds it. A *static*
//! instance (`instantiate_static`, and `detach_ops` for a linked one) copies
//! them in as ordinary authored nodes instead, with every reference to a spec
//! rewritten to the entity's copy. Either way a template is itself a live,
//! evaluable subtree (it shows the defaults).
//!
//! Validation (`validate_entity`) runs at apply time whenever an entity's
//! instance or args change: the definition must exist, the argument names
//! must match its `in` ports exactly, and a constant value must have the
//! default's kind (and, for a quantity, its dimension).

use crate::facet::{Args, Facet, FacetKind, Template, Instance, LeafPath, LeafStruct, Name, Port, Use};
use crate::expr::{Expr, Leaf};
use crate::document::{Document, RelKind, Relation};
use crate::id::NodeId;
use crate::op::Op;
use crate::order::OrderKey;
use crate::registry::TypeId;
use crate::value::{Slot, Value, ValueKind};
use std::collections::BTreeMap;

/// One declared input: an `in` port_spec and its default.
#[derive(Clone, Debug, PartialEq)]
pub struct InputInfo {
    pub spec: NodeId,
    pub name: &'static str,
    pub kind: ValueKind,
    pub default: Value,
}

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).expect("registered type")
}

fn name_of(doc: &Document, id: NodeId) -> Option<String> {
    doc.get::<Name>(id)?.text.constant()?.as_str().map(str::to_owned)
}

/// An `in` port of a one-way kind; across/through terminals are not inputs.
fn is_input(port: &Port) -> bool {
    let one_way = port.kind.constant().and_then(|v| v.as_str()).and_then(crate::port::kind).is_some_and(|k| k.flow == crate::port::Flow::Signal);
    one_way && port.dir.constant().and_then(|v| v.as_str()) == Some("in")
}

/// The inputs a definition declares (its `in` port_specs), in child order.
pub fn inputs_of(doc: &Document, def: NodeId) -> Result<Vec<InputInfo>, String> {
    let node = doc.live(def).ok_or_else(|| format!("{def} is not a live node"))?;
    if node.ty != ty("definition") {
        return Err(format!("{def} is a {}, not a definition", node.ty));
    }
    let mut out = Vec::new();
    for &child in doc.children(def) {
        let Some(port) = doc.get::<Port>(child).filter(|p| is_input(p)) else { continue };
        if !doc.is_live(child) {
            continue;
        }
        let name = name_of(doc, child).filter(|n| !n.is_empty()).ok_or_else(|| format!("port_spec {child} has no name"))?;
        let default = port.signal.constant().cloned().ok_or_else(|| format!("port_spec {child} ({name}): an input's default must be a constant"))?;
        if out.iter().any(|p: &InputInfo| p.name == name) {
            return Err(format!("definition {def} declares {name:?} twice"));
        }
        out.push(InputInfo { spec: child, name: crate::sym::intern(&name), kind: default.kind(), default });
    }
    Ok(out)
}

/// Check an entity's instance and args against its definition.
pub fn validate_entity(doc: &Document, id: NodeId) -> Result<(), String> {
    let def = doc
        .get::<Instance>(id)
        .and_then(|i| i.def.constant())
        .and_then(|v| v.as_ref())
        .ok_or_else(|| format!("{id}: instance.def must be a constant reference to a definition"))?;
    let specs = inputs_of(doc, def).map_err(|e| format!("{id}: {e}"))?;
    let args = doc.get::<Args>(id).ok_or_else(|| format!("{id}: an entity needs an args facet"))?;
    for spec in &specs {
        let Some(slot) = args.slots.get(spec.name) else {
            return Err(format!("{id}: missing argument {:?} for an in port of {def}", spec.name));
        };
        if let Some(v) = slot.constant() {
            check_kind(spec, v).map_err(|e| format!("{id}.args.{}: {e}", spec.name))?;
        }
    }
    for name in args.slots.keys() {
        if !specs.iter().any(|s| s.name == *name) {
            let declared: Vec<&str> = specs.iter().map(|s| s.name).collect();
            return Err(format!("{id}: {def} declares no in port {name:?}; declared: {}", declared.join(", ")));
        }
    }
    Ok(())
}

fn check_kind(spec: &InputInfo, v: &Value) -> Result<(), String> {
    if spec.kind == ValueKind::Any {
        return Ok(());
    }
    if v.kind() != spec.kind {
        return Err(format!("expects {}, got {}", spec.kind.name(), v.kind().name()));
    }
    if let (Value::Quantity { dim: want, .. }, Value::Quantity { dim: got, .. }) = (&spec.default, v)
        && want != got
    {
        return Err(format!("expects {}, got {}", want.canonical_unit(), got.canonical_unit()));
    }
    Ok(())
}

/// The `template` child of `def` for `notation`.
pub fn template_of(doc: &Document, def: NodeId, notation: &str) -> Option<NodeId> {
    doc.children(def).iter().copied().find(|&c| {
        doc.get::<Template>(c).and_then(|d| d.notation.constant()).and_then(|v| v.as_str()) == Some(notation)
    })
}

/// Where an instance goes: the entity under `entity_parent`, its `use`
/// under `figure_parent`, each placed after the given key (or first, for
/// `None`).
pub struct Placement {
    pub entity_parent: Option<NodeId>,
    pub entity_after: Option<OrderKey>,
    pub figure_parent: Option<NodeId>,
    pub figure_after: Option<OrderKey>,
}

/// Ops that instantiate `def` as a linked instance (component-model.md
/// §5): an entity named `name` with every argument at its default, and a
/// `use` of it in `notation` under `place.figure_parent`. Nothing is
/// copied: the entity's ports and the use's picture are expanded from the
/// definition when the ops apply. Ids and relation ids are taken from
/// `doc`, so these ops must be applied before anything else allocates.
pub fn instantiate(doc: &Document, def: NodeId, notation: &str, name: &str, place: &Placement) -> Result<(NodeId, Vec<Op>), String> {
    let specs = inputs_of(doc, def)?;
    template_of(doc, def, notation).ok_or_else(|| format!("{def} has no template for notation {notation:?}"))?;
    let ids = doc.next_ids(2);
    let (entity, u) = (ids[0], ids[1]);
    let mut instance = Instance::default();
    instance.def = Slot::Const(Value::Ref(def));
    let mut args = Args::default();
    for s in &specs {
        args.insert(s.name, Slot::Const(s.default.clone()));
    }
    let mut entity_name = Name::default();
    entity_name.text = Slot::Const(Value::Str(name.to_owned()));
    let mut ops = vec![Op::Create {
        id: entity,
        ty: ty("entity"),
        parent: place.entity_parent,
        order: match &place.entity_after {
            Some(k) => OrderKey::after(k),
            None => OrderKey::first(),
        },
        facets: vec![instance.wrap(), args.wrap(), entity_name.wrap()],
    }];
    ops.extend(use_ops(doc, u, doc.next_relation_id(), entity, notation, name, place.figure_parent, place.figure_after.clone()));
    Ok((entity, ops))
}

/// Ops that draw an existing entity in another notation: a `use` of it
/// under `parent` (after `after`), whose picture is expanded from its
/// definition's `notation` template (ECM §5: one entity, many figures).
pub fn place_figures(doc: &Document, entity: NodeId, notation: &str, parent: Option<NodeId>, after: Option<OrderKey>) -> Result<Vec<Op>, String> {
    let def = doc
        .get::<Instance>(entity)
        .and_then(|i| i.def.constant())
        .and_then(|v| v.as_ref())
        .ok_or_else(|| format!("{entity} is not an entity with a definition"))?;
    template_of(doc, def, notation).ok_or_else(|| format!("{def} has no template for notation {notation:?}"))?;
    let name = name_of(doc, entity).unwrap_or_default();
    Ok(use_ops(doc, doc.next_id(), doc.next_relation_id(), entity, notation, &name, parent, after))
}

/// A `use` node figuring `entity` in `notation`, at the origin.
#[allow(clippy::too_many_arguments)]
fn use_ops(doc: &Document, u: NodeId, rel: crate::id::RelationId, entity: NodeId, notation: &str, name: &str, parent: Option<NodeId>, after: Option<OrderKey>) -> Vec<Op> {
    let _ = doc;
    let mut placed = Use::default();
    placed.notation = Slot::Const(Value::Str(notation.to_owned()));
    let mut use_name = Name::default();
    use_name.text = Slot::Const(Value::Str(name.to_owned()));
    vec![
        Op::Create {
            id: u,
            ty: ty("use"),
            parent,
            order: match &after {
                Some(k) => OrderKey::after(k),
                None => OrderKey::first(),
            },
            facets: vec![placed.wrap(), crate::facet::Transform2d::default().wrap(), use_name.wrap()],
        },
        Op::Link { rel: Relation { id: rel, from: u, to: entity, rel: RelKind::Figures, data: BTreeMap::new() } },
    ]
}

/// Ops that instantiate `def` as a static instance (component-model.md
/// §5.2, "load as static asset"): a `detached` entity named `name` with
/// every argument at its default, one `port` per `port_spec` with its
/// behaviour binding rewritten onto the entity (ECM §6.1), a copy of the
/// `notation` template with its bindings rewritten the same way, and a
/// `figures` relation from each copied root to the entity. Everything is
/// authored: editing the definition later changes nothing here. Ids and
/// relation ids are taken from `doc`, so these ops must be applied before
/// anything else allocates.
pub fn instantiate_static(doc: &Document, def: NodeId, notation: &str, name: &str, place: &Placement) -> Result<(NodeId, Vec<Op>), String> {
    let specs = inputs_of(doc, def)?;
    // Ports and behaviour equations are copied onto the entity.
    let port_specs: Vec<NodeId> = doc.children(def).iter().copied().filter(|&c| doc.live(c).is_some_and(|n| n.ty == ty("port_spec") || n.ty == ty("equation"))).collect();
    let template = template_of(doc, def, notation).ok_or_else(|| format!("{def} has no template for notation {notation:?}"))?;
    let (roots, copied) = template_nodes(doc, template);
    let ids = doc.next_ids(1 + port_specs.len() + copied.len());
    let entity = ids[0];
    let mut new_id: BTreeMap<NodeId, NodeId> = port_specs.iter().copied().zip(ids[1..].iter().copied()).collect();
    new_id.extend(copied.iter().copied().zip(ids[1 + port_specs.len()..].iter().copied()));
    let rel_ids = doc.next_relation_ids(roots.len());

    let mut ops = Vec::new();
    let mut instance = Instance::default();
    instance.def = Slot::Const(Value::Ref(def));
    instance.mode = Slot::Const(Value::Enum("detached".into()));
    let mut args = Args::default();
    for s in &specs {
        args.insert(s.name, Slot::Const(s.default.clone()));
    }
    let mut entity_name = Name::default();
    entity_name.text = Slot::Const(Value::Str(name.to_owned()));
    ops.push(Op::Create {
        id: entity,
        ty: ty("entity"),
        parent: place.entity_parent,
        order: match &place.entity_after {
            Some(k) => OrderKey::after(k),
            None => OrderKey::first(),
        },
        facets: vec![instance.wrap(), args.wrap(), entity_name.wrap()],
    });
    for &spec in &port_specs {
        let node = doc.live(spec).expect("live port_spec");
        let copied_ty = if node.ty == ty("port_spec") { ty("port") } else { node.ty };
        let mut facets = rewritten(doc, spec, &new_id);
        // An input reads its argument; a connection, when there is one,
        // takes its place in the graph (`port::derived_signals`).
        if let Some(input) = specs.iter().find(|s| s.spec == spec) {
            let arg = Leaf { node: entity, path: LeafPath::new(FacetKind::Args, input.name).expect("argument path") };
            for f in &mut facets {
                if let Facet::Port(p) = f {
                    p.signal = Slot::Bound(Expr::Ref(arg));
                }
            }
        }
        ops.push(Op::Create { id: new_id[&spec], ty: copied_ty, parent: Some(entity), order: node.order.clone(), facets });
    }
    ops.extend(copy_template(doc, entity, &new_id, &roots, &copied, place.figure_parent, place.figure_after.clone(), &rel_ids));
    Ok((entity, ops))
}

/// Ops that draw an existing static entity in another notation: a copy of
/// its definition's `notation` template under `parent` (after `after`), its
/// bindings reading the entity's ports, each root a figure of the entity
/// (ECM §5: one entity, many figures).
pub fn place_static_figures(doc: &Document, entity: NodeId, notation: &str, parent: Option<NodeId>, after: Option<OrderKey>) -> Result<Vec<Op>, String> {
    let def = doc
        .get::<Instance>(entity)
        .and_then(|i| i.def.constant())
        .and_then(|v| v.as_ref())
        .ok_or_else(|| format!("{entity} is not an entity with a definition"))?;
    let template = template_of(doc, def, notation).ok_or_else(|| format!("{def} has no template for notation {notation:?}"))?;
    let (roots, copied) = template_nodes(doc, template);
    // Each port_spec maps to the entity's port of the same name.
    let mut new_id: BTreeMap<NodeId, NodeId> = BTreeMap::new();
    for &spec in doc.children(def).iter().filter(|&&c| doc.live(c).is_some_and(|n| n.ty == ty("port_spec"))) {
        let port = doc.children(entity).iter().copied().find(|&p| name_of(doc, p) == name_of(doc, spec)).ok_or_else(|| format!("{entity} has no port named like {spec}"))?;
        new_id.insert(spec, port);
    }
    new_id.extend(copied.iter().copied().zip(doc.next_ids(copied.len())));
    let rel_ids = doc.next_relation_ids(roots.len());
    Ok(copy_template(doc, entity, &new_id, &roots, &copied, parent, after, &rel_ids))
}

/// A template's roots, and every node to copy (roots and descendants).
fn template_nodes(doc: &Document, template: NodeId) -> (Vec<NodeId>, Vec<NodeId>) {
    let roots: Vec<NodeId> = doc.children(template).to_vec();
    let mut copied: Vec<NodeId> = Vec::new();
    for &r in &roots {
        copied.push(r);
        copied.extend(doc.descendants(r));
    }
    (roots, copied)
}

/// A node's facets with every binding rewritten: a reference to a copied
/// node, or to a port_spec, follows it to the instance's copy.
fn rewritten(doc: &Document, old: NodeId, new_id: &BTreeMap<NodeId, NodeId>) -> Vec<Facet> {
    let rewrite = |leaf: Leaf| -> Leaf {
        match new_id.get(&leaf.node) {
            Some(&n) => Leaf { node: n, path: leaf.path },
            None => leaf,
        }
    };
    doc.facets_of(old)
        .map(|c| {
            let mut c = c.clone();
            for field in c.field_names() {
                if let Some(expr) = c.get(field).and_then(|s| s.expr()) {
                    let rewritten = expr.map_refs(&rewrite);
                    c.set(field, Slot::Bound(rewritten)).expect("field exists");
                }
            }
            c
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn copy_template(
    doc: &Document,
    entity: NodeId,
    new_id: &BTreeMap<NodeId, NodeId>,
    roots: &[NodeId],
    copied: &[NodeId],
    parent: Option<NodeId>,
    after: Option<OrderKey>,
    rel_ids: &[crate::id::RelationId],
) -> Vec<Op> {
    let mut ops = Vec::new();
    let mut key = after;
    for &old in copied {
        let node = doc.live(old).expect("copied node is live");
        let (p, order) = if roots.contains(&old) {
            key = Some(match &key {
                Some(k) => OrderKey::after(k),
                None => OrderKey::first(),
            });
            (parent, key.clone().expect("just set"))
        } else {
            (node.parent.map(|p| new_id[&p]), node.order.clone())
        };
        ops.push(Op::Create { id: new_id[&old], ty: node.ty, parent: p, order, facets: rewritten(doc, old, new_id) });
    }
    for (i, &r) in roots.iter().enumerate() {
        ops.push(Op::Link {
            rel: Relation { id: rel_ids[i], from: new_id[&r], to: entity, rel: RelKind::Figures, data: BTreeMap::new() },
        });
    }
    ops
}

/// Ops that detach a linked instance (component-model.md §5.2): what it
/// derives becomes authored nodes with fresh ids, and it stops following its
/// definition. Its entity keeps `instance.def` (where it came from) with
/// `instance.mode = detached`; each `use` keeps figuring it and gains the
/// copied picture as authored children. Relations to the derived nodes (a
/// connection to a port, an annotation's target) and bindings that read
/// them (a readout of a port's signal) are re-pointed at the copies, so
/// nothing outside the instance notices. One transaction; undo restores the
/// link.
pub fn detach_ops(doc: &Document, entity: NodeId) -> Result<Vec<Op>, String> {
    if crate::expand::linked_definition(doc, entity).is_none() {
        return Err(format!("{entity} is not a linked instance"));
    }
    // What it derives, parents first: its ports and equations, then each
    // use's picture.
    let mut derived: Vec<NodeId> = doc.children(entity).iter().copied().filter(|c| c.is_expanded()).collect();
    let uses: Vec<NodeId> = doc.relations_to(entity).filter(|r| r.rel == RelKind::Figures && doc.is_live(r.from) && doc.live(r.from).is_some_and(|n| n.ty == ty("use"))).map(|r| r.from).collect();
    for &u in &uses {
        derived.extend(doc.descendants(u).into_iter().filter(|d| d.is_expanded()));
    }
    let fresh = doc.next_ids(derived.len());
    let map: BTreeMap<NodeId, NodeId> = derived.iter().copied().zip(fresh.iter().copied()).collect();
    let follow = |l: Leaf| -> Leaf { map.get(&l.node).map_or(l, |&n| Leaf { node: n, path: l.path }) };
    let rewrite = |f: &Facet| -> Facet {
        let mut f = f.clone();
        for field in f.field_names() {
            if let Some(expr) = f.get(field).and_then(|s| s.expr()) {
                let e = expr.map_refs(&follow);
                f.set(field, Slot::Bound(e)).expect("field exists");
            }
        }
        f
    };
    let mut ops = vec![Op::Set { id: entity, path: "instance.mode".parse().expect("declared leaf"), slot: Slot::Const(Value::Enum("detached".into())) }];
    for &old in &derived {
        let node = doc.live(old).expect("derived node is live");
        let parent = node.parent.map(|p| map.get(&p).copied().unwrap_or(p));
        ops.push(Op::Create { id: map[&old], ty: node.ty, parent, order: node.order.clone(), facets: doc.facets_of(old).map(&rewrite).collect() });
    }
    // Relations into the instance follow the copies.
    let into: Vec<&Relation> = derived.iter().flat_map(|&d| doc.relations_to(d)).filter(|r| !r.from.is_expanded() && !r.id.is_session()).collect();
    let rel_ids = doc.next_relation_ids(into.len());
    for (r, id) in into.into_iter().zip(rel_ids) {
        ops.push(Op::Unlink { id: r.id });
        ops.push(Op::Link { rel: Relation { id, from: r.from, to: map[&r.to], rel: r.rel, data: r.data.clone() } });
    }
    // So do bindings outside it that read it.
    for (leaf, expr) in doc.bound_slots() {
        if leaf.node.is_expanded() || !expr.deps().iter().any(|d| map.contains_key(&d.node)) {
            continue;
        }
        ops.push(Op::Set { id: leaf.node, path: leaf.path, slot: Slot::Bound(expr.map_refs(&follow)) });
    }
    Ok(ops)
}
