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

use crate::facet::{Constraint, LeafPath};
use crate::document::{Document, RelKind};
use crate::expr::Leaf;
use crate::id::NodeId;
use crate::registry::TypeId;
use crate::value::Value;
use std::collections::BTreeSet;

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
pub fn read(doc: &Document, read_value: &dyn Fn(Leaf) -> Option<Value>, id: NodeId) -> Result<Option<ConstraintInfo>, String> {
    let Some(node) = doc.live(id) else { return Ok(None) };
    if node.ty != constraint_type() {
        return Ok(None);
    }
    let c = doc.get::<Constraint>(id).ok_or_else(|| format!("{id} has no constraint component"))?;
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
        if doc.facet(r.to, crate::facet::FacetKind::Transform2d).is_none() {
            return Err(format!("{id} ({kind}): operand {} has no transform2d", r.to));
        }
        operands.push(Operand { role, node: r.to, anchor, at });
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

/// Every live constraint, malformed ones as errors.
pub fn all(doc: &Document, read_value: &dyn Fn(Leaf) -> Option<Value>) -> Result<Vec<ConstraintInfo>, String> {
    let mut out = Vec::new();
    for id in doc.nodes_of_type(constraint_type()) {
        if let Some(c) = read(doc, read_value, id)? {
            out.push(c);
        }
    }
    Ok(out)
}

/// Validate every constraint: kinds, roles, anchors, and no duplicates (two
/// constraints of one kind over the same anchored operands). Inkling unified
/// duplicates silently; for an author that is an LLM, an explicit error
/// teaches more.
pub fn validate_all(doc: &Document, read_value: &dyn Fn(Leaf) -> Option<Value>) -> Result<(), String> {
    let all = all(doc, read_value)?;
    let mut seen: BTreeSet<(&'static str, Vec<(NodeId, [i64; 2])>)> = BTreeSet::new();
    for c in &all {
        if c.spec.kind == "fix" {
            continue;
        }
        let key: Vec<(NodeId, [i64; 2])> =
            c.operands.iter().map(|o| (o.node, [(o.anchor[0] * 1e6) as i64, (o.anchor[1] * 1e6) as i64])).collect();
        let mut sorted = key.clone();
        sorted.sort();
        if !seen.insert((c.spec.kind, sorted)) {
            let other = all.iter().find(|o| o.id != c.id && o.spec.kind == c.spec.kind && same_operands(o, c)).map(|o| o.id);
            return Err(format!(
                "{} ({}): already constrained by {}",
                c.id,
                c.spec.kind,
                other.map(|o| o.to_string()).unwrap_or_default()
            ));
        }
    }
    Ok(())
}

fn same_operands(a: &ConstraintInfo, b: &ConstraintInfo) -> bool {
    let key = |c: &ConstraintInfo| {
        let mut k: Vec<(NodeId, [i64; 2])> =
            c.operands.iter().map(|o| (o.node, [(o.anchor[0] * 1e6) as i64, (o.anchor[1] * 1e6) as i64])).collect();
        k.sort();
        k
    };
    key(a) == key(b)
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
