//! Definitions as data (entity-composition-model.md §4).
//!
//! A definition is a subtree under a `library` root:
//!
//! ```text
//! library
//!   definition "bar"
//!     param_spec "level"      kind=number default=0.4
//!     depiction  notation="free"
//!       rect "bar"  h.bind = #<level spec>.param_spec.default * 300   (the template)
//! ```
//!
//! An `entity` instantiates a definition: its `instance.def` names it and its
//! `params` holds one slot per `param_spec`, each bindable. Placing a
//! depiction copies the template subtree into the composition as ordinary
//! authored nodes, rewriting every reference to a spec's `default` into a
//! reference to the entity's parameter, and links each copied root to the
//! entity with a `depicts` relation. So a template is itself a live,
//! evaluable subtree (it shows the defaults), and an instance is plain
//! nodes and plain bindings: nothing downstream knows about definitions.
//!
//! Validation (`validate_entity`) runs at apply time whenever an entity's
//! instance or params change: the definition must exist, the parameter
//! names must match the specs exactly, and a constant value must have the
//! spec's kind (and, for a quantity, the default's dimension).

use crate::component::{Component, ComponentKind, Depiction, Instance, LeafPath, LeafStruct, Name, ParamSpec, Params};
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

/// The `depiction` child of `def` for `notation`.
pub fn template_of(doc: &Document, def: NodeId, notation: &str) -> Option<NodeId> {
    doc.children(def).iter().copied().find(|&c| {
        doc.get::<Depiction>(c).and_then(|d| d.notation.constant()).and_then(|v| v.as_str()) == Some(notation)
    })
}

/// Where an instance goes: the entity under `entity_parent`, the copied
/// template roots under `depiction_parent`, each placed after the given
/// key (or first, for `None`).
pub struct Placement {
    pub entity_parent: Option<NodeId>,
    pub entity_after: Option<OrderKey>,
    pub depiction_parent: Option<NodeId>,
    pub depiction_after: Option<OrderKey>,
}

/// Ops that instantiate `def`: an entity named `name` with every parameter
/// at its default, a copy of the `notation` template with its bindings
/// rewritten onto the entity, and a `depicts` relation from each copied
/// root to the entity. Ids and relation ids are taken from `doc`, so these
/// ops must be applied before anything else allocates.
pub fn instantiate(doc: &Document, def: NodeId, notation: &str, name: &str, place: &Placement) -> Result<(NodeId, Vec<Op>), String> {
    let specs = params_of(doc, def)?;
    let template = template_of(doc, def, notation).ok_or_else(|| format!("{def} has no depiction for notation {notation:?}"))?;
    let roots: Vec<NodeId> = doc.children(template).to_vec();
    let mut copied: Vec<NodeId> = Vec::new();
    for &r in &roots {
        copied.push(r);
        copied.extend(doc.descendants(r));
    }
    let ids = doc.next_ids(1 + copied.len());
    let entity = ids[0];
    let new_id: BTreeMap<NodeId, NodeId> = copied.iter().copied().zip(ids[1..].iter().copied()).collect();
    let rel_ids = doc.next_relation_ids(roots.len());

    // Every spec's default leaf becomes the entity's parameter; every
    // reference between template nodes follows the copy.
    let rewrite = |leaf: Leaf| -> Leaf {
        if leaf.path.component == ComponentKind::ParamSpec
            && let Some(spec) = specs.iter().find(|s| s.spec == leaf.node)
        {
            return Leaf { node: entity, path: LeafPath::new(ComponentKind::Params, spec.name).expect("parameter path") };
        }
        match new_id.get(&leaf.node) {
            Some(&n) => Leaf { node: n, path: leaf.path },
            None => leaf,
        }
    };

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
        components: vec![instance.wrap(), params.wrap(), entity_name.wrap()],
    });

    let mut key = place.depiction_after.clone();
    for &old in &copied {
        let node = doc.live(old).expect("copied node is live");
        let is_root = roots.contains(&old);
        let (parent, order) = if is_root {
            key = Some(match &key {
                Some(k) => OrderKey::after(k),
                None => OrderKey::first(),
            });
            (place.depiction_parent, key.clone().expect("just set"))
        } else {
            (node.parent.map(|p| new_id[&p]), node.order.clone())
        };
        let components: Vec<Component> = doc
            .components_of(old)
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
            .collect();
        ops.push(Op::Create { id: new_id[&old], ty: node.ty, parent, order, components });
    }
    for (i, &r) in roots.iter().enumerate() {
        ops.push(Op::Link {
            rel: Relation { id: rel_ids[i], from: new_id[&r], to: entity, rel: RelKind::Depicts, data: BTreeMap::new() },
        });
    }
    Ok((entity, ops))
}
