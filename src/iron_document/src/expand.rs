//! Expansion helpers, override resolution, edit translation, and scope checks
//! (component-model.md §3, §6–§8; handoff-engine.md EN-13).
//!
//! Derived nodes live in [`crate::engine::views::VirtualViews`] (built by
//! [`crate::engine::owners::Owners`] over shared `Arc<Template>`s) rather than
//! materialized copies in a second `Document`. This module keeps the shared
//! definitions, override lookup, repeat/library predicates, edit translation
//! (`translate` / `refuse`), and scope/context checks.

use crate::definition;
use crate::document::RelKind;
use crate::expr::{Expr, Leaf};
use crate::facet::{FacetKind, Instance, LeafPath, LeafStruct, Override, Use};
use crate::id::NodeId;
use crate::op::Op;
use crate::order::OrderKey;
use crate::registry::TypeId;
use crate::value::{Slot, Value};
use crate::Tree;
use std::collections::BTreeMap;

#[cfg(feature = "differential")]
#[path = "expand_oracle.rs"]
mod oracle;
#[cfg(feature = "differential")]
pub(crate) use oracle::verify_tree;

/// Something expansion could not do, or did past a limit, on the node it
/// concerns (reactivity.md §4.3): shown on the node, never silent.
#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    pub node: NodeId,
    /// `depth`, `depth_blocked`, `budget`, `truncated`, `expansion`, `binding`,
    /// `constraint`.
    pub kind: &'static str,
    pub message: String,
}

/// Problems in a fixed order, by node, kind and message, whatever order they
/// were found in.
pub fn sort_problems(problems: &mut [Problem]) {
    problems.sort_by(|a, b| (a.node, a.kind, &a.message).cmp(&(b.node, b.kind, &b.message)));
}

/// Scopes (instances, repeat items) nested deeper than this are reported.
pub const DEPTH_REPORTED: usize = 16;
/// Scopes nested deeper than this are not expanded.
pub const DEPTH_MAX: usize = 128;
/// Independent derived-node safety cap per top-level authored owner (`EN-13+EN-7`).
pub const NODE_BUDGET: usize = 50_000;

/// The items each repeat and conditional has now, by key, in order.
pub type Items = BTreeMap<NodeId, Vec<String>>;

/// The definition a linked entity instantiates, if it is one.
/// Definitions are authored (`!def.is_expanded()`).
pub fn linked_definition(doc: Tree<'_>, entity: NodeId) -> Option<NodeId> {
    let inst = doc.get::<Instance>(entity)?;
    if inst.mode.constant().and_then(|v| v.as_str()) != Some("linked") {
        return None;
    }
    let def = inst.def.constant()?.as_ref()?;
    if def.is_expanded() {
        return None;
    }
    doc.live(def).filter(|n| n.ty.name() == "definition").map(|_| def)
}

/// A *reference* (instance-cache.md §3, phase D): a `use` whose own `instance`
/// facet names a definition, `linked`, with no entity figured.
pub fn reference_template(doc: Tree<'_>, u: NodeId) -> Option<NodeId> {
    if doc.live(u)?.ty.name() != "use" || doc.relations_from(u).any(|r| r.rel == RelKind::Figures) {
        return None;
    }
    let inst = doc.get::<Instance>(u)?;
    if inst.mode.constant().and_then(|v| v.as_str()) != Some("linked") {
        return None;
    }
    let def = inst.def.constant()?.as_ref()?;
    if def.is_expanded() {
        return None;
    }
    doc.live(def).filter(|n| n.ty.name() == "definition")?;
    let notation = doc.get::<Use>(u).and_then(|x| x.notation.constant()).and_then(|v| v.as_str()).unwrap_or("free");
    definition::template_of(doc, def, notation)
}

/// The definition a `use` is the main of (component-authoring.md §1): its
/// `instance` facet says `main`.
pub fn main_definition(doc: Tree<'_>, u: NodeId) -> Option<NodeId> {
    let inst = doc.get::<Instance>(u)?;
    if inst.mode.constant().and_then(|v| v.as_str()) != Some("main") {
        return None;
    }
    let def = inst.def.constant()?.as_ref()?;
    if def.is_expanded() {
        return None;
    }
    doc.live(def).filter(|n| n.ty.name() == "definition").map(|_| def)
}

/// The template a main shows, for its notation.
pub fn main_template(doc: Tree<'_>, u: NodeId) -> Option<NodeId> {
    let def = main_definition(doc, u)?;
    let notation = doc.get::<Use>(u)?.notation.constant()?.as_str()?.to_owned();
    definition::template_of(doc, def, &notation)
}

/// The entity a `use` figures.
pub fn use_entity(doc: Tree<'_>, u: NodeId) -> Option<NodeId> {
    doc.relations_from(u).find(|r| r.rel == RelKind::Figures && doc.is_live(r.to)).map(|r| r.to)
}

/// `n`, a node of a definition's content, as instance `owner` derives it:
/// `owner`'s path followed by `n`'s.
pub(crate) fn relocate(owner: NodeId, n: NodeId) -> Option<NodeId> {
    let mut path = owner.path();
    path.extend(n.path());
    NodeId::from_path(&path)
}

/// A definition's own content an entity derives: its port_specs, equations,
/// inner entities, connections, channels, machines, and reactive helpers.
pub(crate) fn is_body(doc: Tree<'_>, def: NodeId, node: NodeId) -> bool {
    doc.node(node).is_some_and(|n| {
        n.parent == Some(def)
            && matches!(
                n.ty.name(),
                "port_spec" | "equation" | "entity" | "connection" | "channel" | "machine" | "let" | "trace" | "accumulate" | "query"
            )
    })
}

/// Whether a facet makes its node's children a template whose copies are its
/// items: a repeat, or a conditional (`when`).
pub(crate) fn owns_items(kind: FacetKind) -> bool {
    matches!(kind, FacetKind::Repeat | FacetKind::When)
}

/// Whether `id` is a repeat or a conditional.
pub fn is_item_owner(doc: Tree<'_>, id: NodeId) -> bool {
    doc.has(id, FacetKind::Repeat) || doc.has(id, FacetKind::When)
}

/// Whether `id` is part of a repeat's template (plans/specs/repeat.md):
/// inside a group with a `repeat`/`when` facet but in none of its items.
pub fn in_repeat_template(doc: Tree<'_>, id: NodeId) -> bool {
    let mut at = doc.parent_of(id);
    while let Some(p) = at {
        if is_item_owner(doc, p) && id.item_of(p).is_none() {
            return true;
        }
        at = doc.parent_of(p);
    }
    false
}

/// Every drawn repeat of a document: authored and derived ones outside any
/// library or repeat template.
#[cfg(feature = "differential")]
pub(crate) fn drawn_repeats(doc: Tree<'_>) -> Vec<NodeId> {
    let group = TypeId::lookup("group").expect("registered");
    doc.nodes_of_type(group)
        .filter(|&g| doc.is_live(g) && is_item_owner(doc, g) && !g.path().first().is_some_and(|&root| in_library(doc, root)) && !in_repeat_template(doc, g))
        .collect()
}

/// An override node, as (node, target, leaf, value).
pub(crate) type OwnOverride = (NodeId, NodeId, LeafPath, Value);

/// An enclosing node's overrides indexed by target depth/first-step so looking
/// up overrides for `N` repeat items with `N` item-specific overrides is O(N)
/// instead of O(N²).
#[derive(Default)]
pub(crate) struct IndexedOverrides {
    /// 1-step targets (`target.path().len() == 1`), for direct children (`rest.is_empty()`).
    direct: Vec<OwnOverride>,
    /// Multi-step targets grouped by `target.path()[0]` (e.g. item scope `~key`).
    by_first: std::collections::HashMap<NodeId, Vec<OwnOverride>>,
}

impl IndexedOverrides {
    pub(crate) fn from_vec(list: Vec<OwnOverride>) -> IndexedOverrides {
        let mut out = IndexedOverrides::default();
        for item in list {
            let steps = item.1.path();
            if steps.len() == 1 {
                out.direct.push(item);
            } else if steps.len() > 1 {
                out.by_first.entry(steps[0]).or_default().push(item);
            }
        }
        out
    }
}

fn overrides(doc: Tree<'_>, owner: NodeId) -> Vec<OwnOverride> {
    overrides_by_indexed(owner, |n| std::rc::Rc::new(IndexedOverrides::from_vec(own_overrides(doc, n))))
}

/// Overrides reaching `owner`'s copies, weakest first, looking up only the
/// matching target-prefix bucket on each enclosing node (O(1) per repeat item).
pub(crate) fn overrides_by_indexed(owner: NodeId, mut own: impl FnMut(NodeId) -> std::rc::Rc<IndexedOverrides>) -> Vec<OwnOverride> {
    let path = owner.path();
    let mut out = Vec::new();
    for j in (0..path.len()).rev() {
        let rest = &path[j + 1..];
        let idx = own(path[j]);
        if rest.is_empty() {
            for (o, target, leaf, value) in &idx.direct {
                out.push((*o, *target, *leaf, value.clone()));
            }
        } else if let Some(bucket) = idx.by_first.get(&rest[0]) {
            for (o, target, leaf, value) in bucket {
                let steps = target.path();
                if steps.len() == rest.len() + 1 && steps[..rest.len()] == *rest {
                    out.push((*o, steps[rest.len()], *leaf, value.clone()));
                }
            }
        }
    }
    out
}

/// The overrides reaching `owner`'s copies, as (definition-local target, leaf,
/// value), weakest first: for render analysis (`analysis.rs`).
pub fn overrides_of(doc: Tree<'_>, owner: NodeId) -> Vec<(NodeId, LeafPath, Value)> {
    overrides(doc, owner).into_iter().map(|(_, t, l, v)| (t, l, v)).collect()
}

pub(crate) fn own_overrides(doc: Tree<'_>, owner: NodeId) -> Vec<OwnOverride> {
    if owner.is_expanded() {
        return Vec::new();
    }
    doc.authored_children(owner)
        .iter()
        .copied()
        .filter(|&c| doc.ty_of(c).is_some_and(|ty| ty.name() == "override") && doc.is_live(c))
        .filter_map(|c| {
            let o = doc.get::<Override>(c)?;
            let target = o.target.constant()?.as_ref()?;
            let leaf: LeafPath = o.leaf.constant()?.as_str()?.parse().ok()?;
            Some((c, target, leaf, o.value.constant()?.clone()))
        })
        .collect()
}

/// Overrides that apply nothing (component-model.md §5.3).
pub fn stale_overrides(doc: Tree<'_>) -> Vec<(NodeId, String)> {
    let ty = TypeId::lookup("override").expect("registered");
    let mut out = Vec::new();
    for o in doc.nodes_of_type(ty).filter(|&o| doc.is_live(o)) {
        let Some(owner) = doc.live(o).and_then(|n| n.parent) else { continue };
        if is_item_owner(doc, owner) {
            let target = doc.get::<Override>(o).and_then(|f| f.target.constant().and_then(|v| v.as_ref()));
            let derived = target.and_then(|t| NodeId::from_path(&[owner.path(), t.path()].concat()));
            if !derived.is_some_and(|d| doc.is_live(d)) {
                out.push((o, format!("{owner} has no such item now")));
            }
            continue;
        }
        let entity = if doc.live(owner).is_some_and(|n| n.ty.name() == "use") { use_entity(doc, owner) } else { Some(owner) };
        let Some(def) = entity.and_then(|e| linked_definition(doc, e)) else {
            out.push((o, format!("{owner} is not a linked instance")));
            continue;
        };
        let Some(facet) = doc.get::<Override>(o) else { continue };
        let target = facet.target.constant().and_then(|v| v.as_ref());
        let leaf = facet.leaf.constant().and_then(|v| v.as_str()).and_then(|l| l.parse::<LeafPath>().ok());
        let (Some(target), Some(leaf)) = (target, leaf) else {
            out.push((o, "its target or leaf is not well formed".into()));
            continue;
        };
        let derived = NodeId::from_path(&[owner.path(), target.path()].concat()).filter(|&d| doc.is_live(d));
        match derived {
            None => out.push((o, format!("{def} has no {target} for {owner} to derive"))),
            Some(d) if doc.leaf(d, leaf).is_none() => out.push((o, format!("{target} has no {leaf}"))),
            Some(_) => {}
        }
    }
    out
}

pub(crate) fn in_library(doc: Tree<'_>, mut id: NodeId) -> bool {
    while let Some(ty) = doc.ty_of(id) {
        if ty.name() == "library" {
            return true;
        }
        match doc.parent_of(id) {
            Some(p) => id = p,
            None => return false,
        }
    }
    false
}

/// Refuse direct structural/value ops targeting derived nodes or relations
/// (except `Op::Solve`, which writes to `Document::solved`).
pub fn refuse(op: &Op) -> Option<String> {
    if let Op::Link { rel } = op
        && rel.id.is_expanded()
    {
        return Some(format!("{} is a derived relation id: only an instance's expansion makes one", rel.id));
    }
    let target = match op {
        Op::Create { id, parent, .. } => [Some(*id), *parent].into_iter().flatten().find(|n| n.is_expanded()),
        Op::Delete { id } | Op::Reorder { id, .. } | Op::Set { id, .. } | Op::AddFacet { id, .. } | Op::RemoveFacet { id, .. } => Some(*id).filter(|n| n.is_expanded()),
        Op::Reparent { id, parent, .. } => [Some(*id), *parent].into_iter().flatten().find(|n| n.is_expanded()),
        Op::Link { rel } => Some(rel.from).filter(|n| n.is_expanded()),
        Op::Unlink { id } if id.is_expanded() => return Some(format!("{id} is a derived relation: its instance's definition makes it")),
        Op::Unlink { .. } | Op::Solve { .. } => None,
    }?;
    Some(match target.expanded_parts() {
        Some((owner, _)) => format!("{target} is part of instance {owner}, derived from its definition; edit the definition or detach the instance"),
        None => format!("{target} is a derived id: only an instance's expansion makes one"),
    })
}

/// Translate edits aimed at expanded nodes into authored ones (component-model.md §7).
pub fn translate(doc: Tree<'_>, reactive: &crate::graph::Reactive, ops: Vec<Op>) -> Result<Vec<Op>, crate::op::ApplyError> {
    use crate::op::{ApplyError, ErrorKind};
    for (i, op) in ops.iter().enumerate() {
        let target = match op {
            Op::Create { id, parent, .. } | Op::Reparent { id, parent, .. } => [Some(*id), *parent].into_iter().flatten().find(|n| n.repeat_item().is_some()),
            Op::Delete { id } | Op::Reorder { id, .. } | Op::RemoveFacet { id, .. } => Some(*id),
            Op::Set { id, .. } | Op::AddFacet { id, .. } => Some(*id).filter(|n| n.is_item_scope()),
            Op::Link { rel } => Some(rel.from),
            Op::Unlink { .. } | Op::Solve { .. } => None,
        };
        if let Some((target, (repeat, k, template))) = target.and_then(|t| t.repeat_item().map(|r| (t, r))) {
            let what = if template == repeat {
                format!("set {repeat}'s repeat.each, repeat.count or when.is")
            } else {
                format!("edit its template, {template}; only a copy's values can differ per item")
            };
            return Err(ApplyError {
                op: i,
                kind: ErrorKind::BadId,
                message: format!("{target} is item {k} of {repeat}, a copy: {what}"),
            });
        }
    }
    let mut fresh = doc.next_id().raw().max(
        ops.iter()
            .filter_map(|op| match op {
                Op::Create { id, .. } if id.client() == Some(doc.client()) => Some(id.raw() + 1),
                _ => None,
            })
            .max()
            .unwrap_or(0),
    );
    let mut out: Vec<Op> = Vec::with_capacity(ops.len());
    let mut made: Vec<(NodeId, NodeId, LeafPath, NodeId)> = Vec::new();
    let mut override_op = |owner: NodeId, local: NodeId, path: LeafPath, value: Value, out: &mut Vec<Op>| -> Result<(), String> {
        let mut steps = owner.path();
        if main_definition(doc, steps[0]).is_some() {
            steps.remove(0);
        }
        if steps.is_empty() {
            return Err(format!("{owner}: nothing to override"));
        }
        let owner = steps.remove(0);
        steps.push(local);
        let local = NodeId::from_path(&steps).ok_or_else(|| format!("{owner}: no path to {local}"))?;
        let existing = own_overrides(doc, owner)
            .into_iter()
            .find(|(_, t, l, _)| *t == local && *l == path)
            .map(|(o, ..)| o)
            .or_else(|| made.iter().find(|m| m.0 == owner && m.1 == local && m.2 == path).map(|m| m.3));
        match existing {
            Some(o) => out.push(Op::Set {
                id: o,
                path: "override.value".parse().expect("declared leaf"),
                slot: Slot::Const(value),
            }),
            None => {
                let o = NodeId::from_raw(fresh);
                fresh += 1;
                let after = doc.children(owner).iter().rev().find(|c| !c.is_expanded()).and_then(|&c| doc.live(c)).map(|n| n.order.clone());
                let mut order = after.map_or_else(OrderKey::first, |k| OrderKey::after(&k));
                for _ in made.iter().filter(|m| m.0 == owner) {
                    order = OrderKey::after(&order);
                }
                let mut f = Override::default();
                f.target = Slot::Const(Value::Ref(local));
                f.leaf = Slot::Const(Value::Str(path.to_string()));
                f.value = Slot::Const(value);
                made.push((owner, local, path, o));
                out.push(Op::Create {
                    id: o,
                    ty: TypeId::lookup("override").expect("registered"),
                    parent: Some(owner),
                    order,
                    facets: vec![f.wrap()],
                });
            }
        }
        Ok(())
    };
    let in_main = |id: NodeId| -> Option<NodeId> {
        let path = id.path();
        (path.len() == 2 && main_definition(doc, path[0]).is_some()).then(|| path[1])
    };
    let container = |p: NodeId| -> NodeId {
        if main_definition(doc, p).is_some() {
            return main_template(doc, p).unwrap_or(p);
        }
        in_main(p).unwrap_or(p)
    };
    for (i, op) in ops.into_iter().enumerate() {
        let op = match op {
            Op::Create { id, ty, parent, order, facets } => Op::Create { id, ty, parent: parent.map(container), order, facets },
            Op::Reparent { id, parent, order } => Op::Reparent { id: in_main(id).unwrap_or(id), parent: parent.map(container), order },
            Op::Delete { id } if in_main(id).is_some() => Op::Delete { id: in_main(id).unwrap_or(id) },
            Op::Reorder { id, order } if in_main(id).is_some() => Op::Reorder { id: in_main(id).unwrap_or(id), order },
            Op::AddFacet { id, facet } if in_main(id).is_some() => Op::AddFacet { id: in_main(id).unwrap_or(id), facet },
            Op::RemoveFacet { id, kind } if in_main(id).is_some() => Op::RemoveFacet { id: in_main(id).unwrap_or(id), kind },
            other => other,
        };
        if let Op::AddFacet { id, facet } = &op
            && let Some((owner, local)) = id.expanded_parts()
        {
            for field in facet.field_names() {
                if let Some(v) = facet.get(field).and_then(|s| s.constant()) {
                    override_op(owner, local, LeafPath::new(facet.kind(), field).expect("own field"), v.clone(), &mut out)
                        .map_err(|m| ApplyError { op: i, kind: ErrorKind::BadId, message: m })?;
                }
            }
            continue;
        }
        let Op::Set { id, path, slot } = &op else {
            out.push(op);
            continue;
        };
        let Some((owner, local)) = id.expanded_parts() else {
            out.push(op);
            continue;
        };
        let err = |m: String| ApplyError { op: i, kind: ErrorKind::BadId, message: m };
        if let Some(part) = in_main(*id)
            && slot.expr().is_some()
        {
            out.push(Op::Set { id: part, path: *path, slot: slot.clone() });
            continue;
        }
        let value = match slot {
            Slot::Const(v) => v.clone(),
            _ => {
                return Err(err(format!(
                    "{id} is inside instance {owner}: a binding from outside cannot reach it; give its definition an `in` port and bind that"
                )));
            }
        };
        let leaf = Leaf { node: *id, path: *path };
        match crate::handle::handle(doc, reactive, leaf) {
            crate::handle::Handle::Locked(reason) => return Err(err(reason)),
            crate::handle::Handle::Linear { .. } | crate::handle::Handle::Alias { .. } => {
                let (source, v) = crate::handle::resolve_target(doc, reactive, leaf, value).map_err(err)?;
                if source.node.is_expanded() {
                    return Err(err(format!("{leaf} resolves onto {source}, inside an instance")));
                }
                out.push(Op::Set { id: source.node, path: source.path, slot: Slot::Const(v) });
            }
            crate::handle::Handle::Direct => match in_main(*id) {
                Some(part) => out.push(Op::Set { id: part, path: *path, slot: Slot::Const(value) }),
                None => override_op(owner, local, *path, value, &mut out).map_err(err)?,
            },
        }
    }
    Ok(out)
}

/// The scope rules (component-model.md §8), for a binding written by `op`.
pub fn check_scope(doc: Tree<'_>, op: &Op) -> Result<(), String> {
    let (node, exprs): (NodeId, Vec<&Expr>) = match op {
        Op::Create { id, facets, .. } => (*id, facets.iter().flat_map(|f| f.field_names().into_iter().filter_map(move |n| f.get(n).and_then(|s| s.expr()))).collect()),
        Op::Set { id, slot, .. } => (*id, slot.expr().into_iter().collect()),
        Op::AddFacet { id, facet } => (*id, facet.field_names().into_iter().filter_map(|n| facet.get(n).and_then(|s| s.expr())).collect()),
        _ => return Ok(()),
    };
    if node.is_expanded() || exprs.is_empty() {
        return Ok(());
    }
    let home = definition_of(doc, node);
    if let Some(def) = home {
        for e in &exprs {
            context_fault(doc, def, node, e)?;
        }
    }
    for dep in exprs.iter().flat_map(|e| e.deps()) {
        match home {
            Some(def) => {
                if dep.node != def && !doc.is_ancestor(def, dep.node) {
                    return Err(format!("{node} is part of definition {def}: its bindings read only that definition's own nodes and ports, not {}", dep.node));
                }
            }
            None => {
                let place = doc.has(dep.node, crate::facet::FacetKind::Anchor)
                    && dep.path.facet == crate::facet::FacetKind::Anchor
                    && matches!(dep.path.field, "wx" | "wy" | "wrot");
                if let Some((owner, _)) = dep.node.expanded_parts()
                    && !place
                    && !doc.live(dep.node).is_some_and(|n| n.ty.name() == "port")
                {
                    return Err(format!("{node} reads {dep}, inside instance {owner}; read one of its ports, or give its definition an `out` port for this"));
                }
            }
        }
    }
    Ok(())
}

fn context_fault(doc: Tree<'_>, def: NodeId, node: NodeId, expr: &Expr) -> Result<(), String> {
    for name in expr.tokens() {
        if crate::palette::is_token(doc, &name) {
            continue;
        }
        let mut in_template = false;
        let mut at = Some(node);
        while let Some(n) = at.filter(|&a| a != def).and_then(|a| doc.node(a)) {
            in_template |= n.ty.name() == "template";
            at = n.parent;
        }
        if !in_template {
            return Err(format!("{node} reads @{name} outside a template: context depends on where a picture is placed, so only a definition's template may read it. Give the definition an input port for this instead"));
        }
        if !crate::palette::declared(doc, node, &name, def) {
            return Err(format!("{node} reads @{name}, which its definition does not declare: add <let name=\"{name}\" provide=\"default\" value=\"…\"/> to the template, the value used where nothing above provides {name}"));
        }
    }
    Ok(())
}

pub fn check_context(doc: Tree<'_>, node: NodeId) -> Result<(), String> {
    let Some(def) = definition_of(doc, node) else { return Ok(()) };
    // The definition's own bindings, not every binding in the document
    // (`EN-10`).
    for d in doc.descendants(def).into_iter().filter(|d| !d.is_expanded() && doc.is_live(*d)) {
        for facet in doc.facets_of(d) {
            for field in facet.field_names() {
                if let Some(expr) = facet.get(field).and_then(Slot::expr) {
                    context_fault(doc, def, d, expr)?;
                }
            }
        }
    }
    Ok(())
}

/// The definition a node belongs to, if it is part of one.
pub(crate) fn definition_of(doc: Tree<'_>, mut id: NodeId) -> Option<NodeId> {
    while let Some(ty) = doc.ty_of(id) {
        if ty.name() == "definition" {
            return Some(id);
        }
        id = doc.parent_of(id)?;
    }
    None
}
