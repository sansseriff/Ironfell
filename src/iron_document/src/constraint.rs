//! Constraints in the document (constraint-system.md §4).
//!
//! A constraint is a `constraint` node with a `constraint` facet (kind,
//! value, enabled) and one `constrains` relation per operand, whose `data`
//! carries the operand's `role` (`a`, `b`) and `anchor` (a point in the
//! operand's local space; `[0, 0]` by default). A `pin` carries its world
//! point as `at` on its operand's relation. Constraints have no direction:
//! which side moves is decided per solve (`solve.rs`).
//!
//! The kind vocabulary is code, like the node types; [`KINDS`] is the one
//! table validation, views and the solver read.

use crate::Tree;
use crate::facet::{Constraint, LeafPath};
use crate::document::RelKind;
use crate::expr::Leaf;
use crate::id::NodeId;
use crate::registry::TypeId;
use crate::value::Value;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConstraintSpec {
    pub kind: &'static str,
    /// Operand roles, each required exactly once.
    pub roles: &'static [&'static str],
    /// Whether `constraint.value` means anything (a distance, an angle).
    pub uses_value: bool,
    /// Residual equations the kind contributes.
    pub equations: usize,
}

pub const KINDS: &[ConstraintSpec] = &[
    ConstraintSpec { kind: "coincident", roles: &["a", "b"], uses_value: false, equations: 2 },
    ConstraintSpec { kind: "distance", roles: &["a", "b"], uses_value: true, equations: 1 },
    ConstraintSpec { kind: "pin", roles: &["a"], uses_value: false, equations: 2 },
    ConstraintSpec { kind: "fix", roles: &["a"], uses_value: false, equations: 0 },
    ConstraintSpec { kind: "angle", roles: &["a", "b"], uses_value: true, equations: 1 },
    ConstraintSpec { kind: "horizontal", roles: &["a", "b"], uses_value: false, equations: 1 },
    ConstraintSpec { kind: "vertical", roles: &["a", "b"], uses_value: false, equations: 1 },
    // Two frames held together, facing each other: a port plugged into a
    // port (component-model.md §4.3). Connections imply them (`plug.rs`).
    ConstraintSpec { kind: "mate", roles: &["a", "b"], uses_value: false, equations: 3 },
];

pub fn spec(kind: &str) -> Option<&'static ConstraintSpec> {
    KINDS.iter().find(|s| s.kind == kind)
}

#[derive(Clone, Debug, PartialEq)]
pub struct Operand {
    pub role: &'static str,
    pub node: NodeId,
    pub anchor: [f64; 2],
    /// A `pin`'s world point.
    pub at: Option<[f64; 2]>,
    /// A `mate`'s facing at the anchor, in the operand's space (radians).
    pub turn: f64,
}

/// One constraint, read from the document and resolved values.
#[derive(Clone, Debug, PartialEq)]
pub struct ConstraintInfo {
    pub id: NodeId,
    pub spec: &'static ConstraintSpec,
    /// Resolved `constraint.value` (a binding's value if bound).
    pub value: f64,
    pub enabled: bool,
    /// In role order.
    pub operands: Vec<Operand>,
}

impl ConstraintInfo {
    pub fn operand(&self, role: &str) -> &Operand {
        self.operands.iter().find(|o| o.role == role).expect("validated role")
    }

    pub fn nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.operands.iter().map(|o| o.node)
    }
}

pub fn constraint_type() -> TypeId {
    TypeId::lookup("constraint").expect("registered")
}

/// The value leaf of a constraint node.
pub fn value_leaf(id: NodeId) -> Leaf {
    Leaf { node: id, path: LeafPath::new(crate::facet::FacetKind::Constraint, "value").expect("declared") }
}

fn vec2(v: Option<&Value>) -> Option<[f64; 2]> {
    v.and_then(|v| v.as_vec2())
}

/// Read one constraint. `Err` describes a malformed one; `Ok(None)` is a
/// constraint that is not a live constraint node.
pub fn read(doc: Tree<'_>, read_value: &dyn Fn(Leaf) -> Option<Value>, id: NodeId) -> Result<Option<ConstraintInfo>, String> {
    let Some(node) = doc.live(id) else { return Ok(None) };
    if node.ty != constraint_type() {
        return Ok(None);
    }
    let c = doc.get::<Constraint>(id).ok_or_else(|| format!("{id} has no constraint component"))?;
    // Read through `read_value` too, though only their constants count: a
    // native grouping or validating constraints follows their edits.
    for f in ["kind", "enabled"] {
        let _ = read_value(Leaf { node: id, path: crate::facet::LeafPath::new(crate::facet::FacetKind::Constraint, f).expect("declared") });
    }
    // A bound kind is reported on its own leaf (`LeafPath::is_structural`),
    // not again as an unknown kind here.
    if c.kind.expr().is_some() {
        return Ok(None);
    }
    let kind = c.kind.constant().and_then(|v| v.as_str()).unwrap_or("");
    let spec = spec(kind).ok_or_else(|| {
        let names: Vec<&str> = KINDS.iter().map(|s| s.kind).collect();
        format!("{id}: unknown constraint kind {kind:?}; kinds: {}", names.join(", "))
    })?;
    let enabled = c.enabled.constant().and_then(|v| v.as_bool()).unwrap_or(true);
    let value = match read_value(value_leaf(id)) {
        Some(Value::Number(n)) => n,
        Some(v) => return Err(format!("{id}: value must be a number, got {}", v.kind().name())),
        None => 0.0,
    };
    let mut operands = Vec::new();
    for r in doc.relations_from(id).filter(|r| r.rel == RelKind::Constrains) {
        let role = r.data.get("role").and_then(|v| v.as_str()).unwrap_or("a");
        let role = spec
            .roles
            .iter()
            .copied()
            .find(|x| *x == role)
            .ok_or_else(|| format!("{id} ({kind}): no role {role:?}; roles: {}", spec.roles.join(", ")))?;
        if operands.iter().any(|o: &Operand| o.role == role) {
            return Err(format!("{id} ({kind}): role {role:?} given twice"));
        }
        let anchor = match r.data.get("anchor") {
            None => [0.0, 0.0],
            Some(v) => v.as_vec2().ok_or_else(|| format!("{id}: anchor must be a vec2"))?,
        };
        let at = match r.data.get("at") {
            None => None,
            Some(v) => Some(v.as_vec2().ok_or_else(|| format!("{id}: at must be a vec2"))?),
        };
        let turn = match r.data.get("turn") {
            None => 0.0,
            Some(v) => v.as_f64().ok_or_else(|| format!("{id}: turn must be a number"))?,
        };
        // A copy that went (an instance deleted, a repeat's item gone)
        // leaves the operand dead, and the constraint inactive (`solve`).
        let gone_copy = r.to.is_expanded() && !doc.is_live(r.to);
        if !gone_copy && doc.facet(r.to, crate::facet::FacetKind::Transform2d).is_none() {
            return Err(format!("{id} ({kind}): operand {} has no transform2d", r.to));
        }
        operands.push(Operand { role, node: r.to, anchor, at, turn });
    }
    let _ = vec2;
    for role in spec.roles {
        if !operands.iter().any(|o| o.role == *role) {
            return Err(format!("{id} ({kind}): missing operand {role:?} (link it with a constrains relation)"));
        }
    }
    if kind == "pin" && operands[0].at.is_none() {
        return Err(format!("{id} (pin): the operand's relation needs `at`, the world point"));
    }
    operands.sort_by_key(|o| spec.roles.iter().position(|r| *r == o.role));
    Ok(Some(ConstraintInfo { id, spec, value, enabled, operands }))
}

/// Every live constraint that can be read, and a problem for each that
/// cannot: one malformed constraint leaves the rest usable (DS-7).
pub fn all(doc: Tree<'_>, read_value: &dyn Fn(Leaf) -> Option<Value>) -> (Vec<ConstraintInfo>, Vec<(NodeId, String)>) {
    let mut out = Vec::new();
    let mut problems = Vec::new();
    for id in doc.nodes_of_type(constraint_type()) {
        match read(doc, read_value, id) {
            Ok(Some(c)) => out.push(c),
            Ok(None) => {}
            Err(e) => problems.push((id, e)),
        }
    }
    (out, problems)
}

/// The constraints the solver may use, and a problem for each it may not:
/// a malformed one, or the second of two of one kind over the same
/// anchored operands (Inkling unified duplicates silently; for an author
/// that is an LLM, an explicit problem teaches more). The first of a pair,
/// in id order, is kept.
pub fn usable(doc: Tree<'_>, read_value: &dyn Fn(Leaf) -> Option<Value>) -> (Vec<ConstraintInfo>, Vec<(NodeId, String)>) {
    let (mut all, mut problems) = all(doc, read_value);
    all.sort_by_key(|c| c.id);
    let mut seen: BTreeMap<(&'static str, Vec<(NodeId, [i64; 2])>), NodeId> = BTreeMap::new();
    let mut kept = Vec::with_capacity(all.len());
    for c in all {
        if c.spec.kind != "fix" {
            let mut key: Vec<(NodeId, [i64; 2])> = c.operands.iter().map(|o| (o.node, [(o.anchor[0] * 1e6) as i64, (o.anchor[1] * 1e6) as i64])).collect();
            key.sort();
            if let Some(first) = seen.get(&(c.spec.kind, key.clone())) {
                problems.push((c.id, format!("{} ({}): already constrained by {first}", c.id, c.spec.kind)));
                continue;
            }
            seen.insert((c.spec.kind, key), c.id);
        }
        kept.push(c);
    }
    problems.sort_by_key(|(id, _)| *id);
    (kept, problems)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_are_consistent() {
        for s in KINDS {
            assert_eq!(spec(s.kind), Some(s));
            assert!(!s.roles.is_empty());
        }
        assert!(spec("weld").is_none());
    }
}
