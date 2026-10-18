//! Definitions as data (entity-composition-model.md §4).
//!
//! A definition is a subtree under a `library` root:
//!
//! ```text
//! library
//!   definition "bar"
//!     param_spec "level"      kind=number default=0.4
//!     template   notation="free"
//!       rect "bar"  h.bind = #<level spec>.param_spec.default * 300   (the template)
//! ```
//!
//! An `entity` instantiates a definition: its `instance.def` names it and its
//! `params` holds one slot per `param_spec`, each bindable. Placing a
//! figure copies the template subtree into the composition as ordinary
//! authored nodes, rewriting every reference to a spec's `default` into a
//! reference to the entity's parameter, and links each copied root to the
//! entity with a `figures` relation. So a template is itself a live,
//! evaluable subtree (it shows the defaults), and an instance is plain
//! nodes and plain bindings: nothing downstream knows about definitions.
//!
//! Validation (`validate_entity`) runs at apply time whenever an entity's
//! instance or params change: the definition must exist, the parameter
//! names must match the specs exactly, and a constant value must have the
//! spec's kind (and, for a quantity, the default's dimension).

use crate::facet::{Facet, FacetKind, Template, Instance, LeafPath, LeafStruct, Name, ParamSpec, Params};
use crate::document::{Document, RelKind, Relation};
use crate::expr::Leaf;
use crate::id::NodeId;
use crate::op::Op;
use crate::order::OrderKey;
use crate::registry::TypeId;
use crate::value::{Slot, Value, ValueKind};
use std::collections::BTreeMap;

/// One declared parameter.
#[derive(Clone, Debug, PartialEq)]
pub struct ParamInfo {
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

/// The parameters a definition declares, in child order.
pub fn params_of(doc: &Document, def: NodeId) -> Result<Vec<ParamInfo>, String> {
    let node = doc.live(def).ok_or_else(|| format!("{def} is not a live node"))?;
    if node.ty != ty("definition") {
        return Err(format!("{def} is a {}, not a definition", node.ty));
    }
    let mut out = Vec::new();
    for &child in doc.children(def) {
        let Some(spec) = doc.get::<ParamSpec>(child) else { continue };
        let name = name_of(doc, child).filter(|n| !n.is_empty()).ok_or_else(|| format!("param_spec {child} has no name"))?;
        let kind_name = spec.kind.constant().and_then(|v| v.as_str()).unwrap_or("any");
        let kind = ValueKind::from_name(kind_name).ok_or_else(|| format!("param_spec {child} ({name}): unknown kind {kind_name:?}"))?;
        let default = spec.default.constant().cloned().ok_or_else(|| format!("param_spec {child} ({name}): default must be a constant"))?;
        if kind != ValueKind::Any && default.kind() != kind {
            return Err(format!("param_spec {child} ({name}): default is {}, kind says {}", default.kind().name(), kind.name()));
        }
        if out.iter().any(|p: &ParamInfo| p.name == name) {
            return Err(format!("definition {def} declares {name:?} twice"));
        }
        out.push(ParamInfo { spec: child, name: crate::sym::intern(&name), kind, default });
    }
    Ok(out)
}

/// Check an entity's instance and params against its definition.
pub fn validate_entity(doc: &Document, id: NodeId) -> Result<(), String> {
    let def = doc
        .get::<Instance>(id)
        .and_then(|i| i.def.constant())
        .and_then(|v| v.as_ref())
        .ok_or_else(|| format!("{id}: instance.def must be a constant reference to a definition"))?;
    let specs = params_of(doc, def).map_err(|e| format!("{id}: {e}"))?;
    let params = doc.get::<Params>(id).ok_or_else(|| format!("{id}: an entity needs a params component"))?;
    for spec in &specs {
        let Some(slot) = params.slots.get(spec.name) else {
            return Err(format!("{id}: missing parameter {:?} declared by {def}", spec.name));
        };
        if let Some(v) = slot.constant() {
            check_kind(spec, v).map_err(|e| format!("{id}.params.{}: {e}", spec.name))?;
        }
    }
    for name in params.slots.keys() {
        if !specs.iter().any(|s| s.name == *name) {
            let declared: Vec<&str> = specs.iter().map(|s| s.name).collect();
            return Err(format!("{id}: {def} declares no parameter {name:?}; declared: {}", declared.join(", ")));
        }
    }
    Ok(())
}

fn check_kind(spec: &ParamInfo, v: &Value) -> Result<(), String> {
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

/// Where an instance goes: the entity under `entity_parent`, the copied
/// template roots under `figure_parent`, each placed after the given
/// key (or first, for `None`).
pub struct Placement {
    pub entity_parent: Option<NodeId>,
    pub entity_after: Option<OrderKey>,
    pub figure_parent: Option<NodeId>,
    pub figure_after: Option<OrderKey>,
}

/// Ops that instantiate `def`: an entity named `name` with every parameter
/// at its default, one `port` per `port_spec` with its behaviour binding
/// rewritten onto the entity (ECM §6.1), a copy of the `notation` template
/// with its bindings rewritten the same way, and a `figures` relation from
/// each copied root to the entity. Ids and relation ids are taken from
/// `doc`, so these ops must be applied before anything else allocates.
pub fn instantiate(doc: &Document, def: NodeId, notation: &str, name: &str, place: &Placement) -> Result<(NodeId, Vec<Op>), String> {
    let specs = params_of(doc, def)?;
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
    let mut params = Params::default();
    for s in &specs {
        params.insert(s.name, Slot::Const(s.default.clone()));
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
        facets: vec![instance.wrap(), params.wrap(), entity_name.wrap()],
    });
    for &spec in &port_specs {
        let node = doc.live(spec).expect("live port_spec");
        let copied_ty = if node.ty == ty("port_spec") { ty("port") } else { node.ty };
        ops.push(Op::Create { id: new_id[&spec], ty: copied_ty, parent: Some(entity), order: node.order.clone(), facets: rewritten(doc, spec, &specs, entity, &new_id) });
    }
    ops.extend(copy_template(doc, &specs, entity, &new_id, &roots, &copied, place.figure_parent, place.figure_after.clone(), &rel_ids));
    Ok((entity, ops))
}

/// Ops that draw an existing entity in another notation: a copy of its
/// definition's `notation` template under `parent` (after `after`), its
/// bindings reading the entity's parameters and ports, each root a figure
/// of the entity (ECM §5: one entity, many figures).
pub fn place_figures(doc: &Document, entity: NodeId, notation: &str, parent: Option<NodeId>, after: Option<OrderKey>) -> Result<Vec<Op>, String> {
    let def = doc
        .get::<Instance>(entity)
        .and_then(|i| i.def.constant())
        .and_then(|v| v.as_ref())
        .ok_or_else(|| format!("{entity} is not an entity with a definition"))?;
    let specs = params_of(doc, def)?;
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
    Ok(copy_template(doc, &specs, entity, &new_id, &roots, &copied, parent, after, &rel_ids))
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

/// A node's facets with every binding rewritten: a spec's default
/// becomes the entity's parameter; a reference to a copied node (or a
/// port_spec) follows it.
fn rewritten(doc: &Document, old: NodeId, specs: &[ParamInfo], entity: NodeId, new_id: &BTreeMap<NodeId, NodeId>) -> Vec<Facet> {
    let rewrite = |leaf: Leaf| -> Leaf {
        if leaf.path.facet == FacetKind::ParamSpec
            && let Some(spec) = specs.iter().find(|s| s.spec == leaf.node)
        {
            return Leaf { node: entity, path: LeafPath::new(FacetKind::Params, spec.name).expect("parameter path") };
        }
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
    specs: &[ParamInfo],
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
        ops.push(Op::Create { id: new_id[&old], ty: node.ty, parent: p, order, facets: rewritten(doc, old, specs, entity, new_id) });
    }
    for (i, &r) in roots.iter().enumerate() {
        ops.push(Op::Link {
            rel: Relation { id: rel_ids[i], from: new_id[&r], to: entity, rel: RelKind::Figures, data: BTreeMap::new() },
        });
    }
    ops
}
