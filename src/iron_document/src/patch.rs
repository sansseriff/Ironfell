//! Patches, axes, regions, strength, and the dedicated solve layer
//! (plans/specs/patches.md, plans/specs/engine.md §2, EN-4).
//!
//! One variation algebra for instance overrides, variants, state poses, clips,
//! narrative extents, slides, takes, and the session layer, verified against
//! `src/iron_patch` (20 worked examples + 21 x 10,000-script laws).

use crate::expr::Leaf;
use crate::facet::{FacetKind, LeafPath};
use crate::id::{NodeId, base62, parse_base62};
use crate::op::Op;
use crate::order::OrderKey;
use crate::value::Slot;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::str::FromStr;

// ---------------------------------------------------------------------------
// Stable typed identifiers (base62, canonical JSON map keys)
// ---------------------------------------------------------------------------

macro_rules! patch_id_type {
    ($name:ident, $prefix:literal, $doc:literal) => {
        #[doc = $doc]
        #[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub u64);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}{}", $prefix, base62(self.0))
            }
        }

        impl FromStr for $name {
            type Err = String;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let rest = s
                    .strip_prefix($prefix)
                    .ok_or_else(|| format!("expected prefix {:?} in {:?}", $prefix, s))?;
                let n = parse_base62(rest).ok_or_else(|| format!("invalid base62 id {:?}", s))?;
                Ok($name(n))
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(&self.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let text = String::deserialize(d)?;
                text.parse().map_err(serde::de::Error::custom)
            }
        }
    };
}

patch_id_type!(AxisId, "a", "Stable identifier for a variation axis.");
patch_id_type!(AxisValueId, "v", "Stable identifier for a value on a variation axis (P2).");
patch_id_type!(PatchId, "p", "Stable identifier for a patch.");

/// Outermost layer: session patches (scratch and live gesture previews; P8).
/// Never saved in canonical JSON and never recorded in undo history.
pub const SESSION_LAYER: u32 = u32::MAX;

// ---------------------------------------------------------------------------
// Axes and points (patches.md §3)
// ---------------------------------------------------------------------------

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AxisKind {
    /// Slides, takes, narrative joints: regions are intervals by endpoint ids.
    Ordered,
    /// Machine states, modes: regions are sets or cofinite sets.
    Unordered,
    /// Component placements: paths of placement steps, regions are suffix patterns.
    Instance,
    /// A clock's continuous time: regions are real intervals.
    Time,
}

/// One position on an axis. Deleted values remain as tombstones in their place
/// so intervals naming them keep their meaning (P3).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AxisValue {
    pub id: AxisValueId,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    #[serde(default = "default_true")]
    pub live: bool,
    /// Placement path, outermost first (`AxisKind::Instance` only).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub path: Vec<NodeId>,
    /// Parallel to `path`: which scope declares each step (P18 anchoring).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scopes: Vec<NodeId>,
}

fn default_true() -> bool {
    true
}

impl AxisValue {
    pub fn new(id: AxisValueId, name: impl Into<String>) -> Self {
        AxisValue { id, name: name.into(), live: true, path: Vec::new(), scopes: Vec::new() }
    }

    pub fn with_path(id: AxisValueId, name: impl Into<String>, path: Vec<NodeId>, scopes: Vec<NodeId>) -> Self {
        debug_assert!(scopes.is_empty() || scopes.len() == path.len());
        AxisValue { id, name: name.into(), live: true, path, scopes }
    }
}

/// One variation axis (document-level when `owner == None`, component-local
/// when `owner == Some(def)`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Axis {
    pub id: AxisId,
    pub name: String,
    pub kind: AxisKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<NodeId>,
    #[serde(default)]
    pub values: Vec<AxisValue>,
}

impl Axis {
    pub fn new(id: AxisId, name: impl Into<String>, kind: AxisKind, owner: Option<NodeId>) -> Self {
        Axis { id, name: name.into(), kind, owner, values: Vec::new() }
    }

    pub fn push(&mut self, value: AxisValue) {
        self.values.push(value);
    }

    pub fn insert(&mut self, at: usize, value: AxisValue) {
        let at = at.min(self.values.len());
        self.values.insert(at, value);
    }

    pub fn delete(&mut self, id: AxisValueId) -> bool {
        if let Some(e) = self.values.iter_mut().find(|e| e.id == id) {
            let was = e.live;
            e.live = false;
            was
        } else {
            false
        }
    }

    pub fn restore(&mut self, id: AxisValueId) -> bool {
        if let Some(e) = self.values.iter_mut().find(|e| e.id == id) {
            let was = !e.live;
            e.live = true;
            was
        } else {
            false
        }
    }

    pub fn move_to(&mut self, id: AxisValueId, at: usize) -> Option<usize> {
        let old = self.values.iter().position(|e| e.id == id)?;
        let e = self.values.remove(old);
        self.values.insert(at.min(self.values.len()), e);
        Some(old)
    }

    /// Position of `id` in this axis, tombstones counted.
    pub fn position(&self, id: AxisValueId) -> Option<usize> {
        self.values.iter().position(|e| e.id == id)
    }

    pub fn live(&self) -> impl Iterator<Item = AxisValueId> + '_ {
        self.values.iter().filter(|e| e.live).map(|e| e.id)
    }

    pub fn all(&self) -> impl Iterator<Item = AxisValueId> + '_ {
        self.values.iter().map(|e| e.id)
    }

    pub fn path(&self, id: AxisValueId) -> &[NodeId] {
        self.values.iter().find(|e| e.id == id).map_or(&[], |e| &e.path)
    }

    pub fn scopes(&self, id: AxisValueId) -> &[NodeId] {
        self.values.iter().find(|e| e.id == id).map_or(&[], |e| &e.scopes)
    }
}

/// Discrete coordinate projection of a `Point` (`values` only, excluding
/// continuous `times`): keys persisted solve-layer poses (P17).
pub type DiscretePoint = BTreeMap<AxisId, AxisValueId>;

/// Per-view coordinate (`engine.md` §2.3, `EN-5a`/`EN-5b`): combines a virtual
/// view's derived placement path & enclosing scope chain (never saved as
/// authored `AxisValue`s) with its component-local discrete coordinates (e.g.
/// machine state or variant). Each scope entry `(scope, start)` records the
/// definition/variant scope and the index into `path` *after* the step that
/// entered `scope` (`0` for a scope enclosing the top-level authored node).
/// Structural layer precedence is derived from `scopes` (outermost scope wins
/// without globally renumbering `u32` layer integers).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ViewCoord {
    /// Placement steps from outermost placement to this view's owner.
    pub path: Vec<NodeId>,
    /// Enclosing scope chain as `(scope_id, path_start_offset)`, outermost first.
    pub scopes: Vec<(NodeId, usize)>,
    /// Component-local discrete coordinates (e.g. machine state, variant).
    pub local: DiscretePoint,
}

impl ViewCoord {
    pub fn new(path: Vec<NodeId>, scopes: Vec<(NodeId, usize)>) -> Self {
        debug_assert!(scopes.iter().all(|&(_, start)| start <= path.len()));
        ViewCoord { path, scopes, local: BTreeMap::new() }
    }

    pub fn has_scope(&self, scope: NodeId) -> bool {
        self.scopes.iter().any(|&(s, _)| s == scope)
    }

    pub fn with_local(mut self, axis: AxisId, value: AxisValueId) -> Self {
        self.local.insert(axis, value);
        self
    }

    /// Look up a discrete coordinate on `axis`, preferring component-local over
    /// the global document point.
    pub fn value_on(&self, global: &Point, axis: AxisId) -> Option<AxisValueId> {
        self.local.get(&axis).copied().or_else(|| global.get(axis))
    }
}

/// One coordinate per axis in play: discrete axis values plus continuous
/// clock times.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub values: BTreeMap<AxisId, AxisValueId>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub times: BTreeMap<AxisId, f64>,
}

impl Point {
    pub fn new() -> Self {
        Point::default()
    }

    pub fn discrete(&self) -> DiscretePoint {
        self.values.clone()
    }

    pub fn format_discrete(dp: &DiscretePoint) -> String {
        dp.iter().map(|(a, v)| format!("{a}={v}")).collect::<Vec<_>>().join(",")
    }

    pub fn parse_discrete(s: &str) -> Result<DiscretePoint, String> {
        let mut out = BTreeMap::new();
        if s.is_empty() {
            return Ok(out);
        }
        for part in s.split(',') {
            let (a_s, v_s) = part.split_once('=').ok_or_else(|| format!("invalid discrete point entry {part:?}"))?;
            let a: AxisId = a_s.parse()?;
            let v: AxisValueId = v_s.parse()?;
            if out.insert(a, v).is_some() {
                return Err(format!("duplicate axis {a} in discrete point"));
            }
        }
        Ok(out)
    }

    pub fn with_value(mut self, axis: AxisId, value: AxisValueId) -> Self {
        self.values.insert(axis, value);
        self
    }

    pub fn with_time(mut self, axis: AxisId, t: f64) -> Self {
        self.times.insert(axis, t);
        self
    }

    pub fn get(&self, axis: AxisId) -> Option<AxisValueId> {
        self.values.get(&axis).copied()
    }

    pub fn time(&self, axis: AxisId) -> Option<f64> {
        self.times.get(&axis).copied()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty() && self.times.is_empty()
    }

    /// Which axes changed between `self` and `other`.
    pub fn changed_axes(&self, other: &Point) -> BTreeSet<AxisId> {
        let mut out = BTreeSet::new();
        for (&a, &v) in &self.values {
            if other.values.get(&a) != Some(&v) {
                out.insert(a);
            }
        }
        for (&a, &v) in &other.values {
            if self.values.get(&a) != Some(&v) {
                out.insert(a);
            }
        }
        for (&a, &t) in &self.times {
            if other.times.get(&a).is_none_or(|&ot| ot.to_bits() != t.to_bits()) {
                out.insert(a);
            }
        }
        for (&a, &t) in &other.times {
            if self.times.get(&a).is_none_or(|&st| st.to_bits() != t.to_bits()) {
                out.insert(a);
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// Symbolic regions (patches.md §4)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "k", rename_all = "snake_case")]
pub enum Bound {
    Open,
    Incl { v: AxisValueId },
    Excl { v: AxisValueId },
    TimeIncl { t: f64 },
    TimeExcl { t: f64 },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "k", rename_all = "snake_case")]
pub enum Sel {
    All,
    Range { lo: Bound, hi: Bound },
    Set { values: BTreeSet<AxisValueId> },
    Not { values: BTreeSet<AxisValueId> },
    Suffix { tail: Vec<NodeId>, #[serde(default, skip_serializing_if = "Vec::is_empty")] not: Vec<Vec<NodeId>> },
}

static ALL_SEL: Sel = Sel::All;

fn pos(axis: &Axis, v: AxisValueId) -> i64 {
    axis.position(v).map_or(i64::MAX / 4, |p| p as i64)
}

fn lower_cut(axis: &Axis, b: &Bound) -> i64 {
    match b {
        Bound::Open => i64::MIN,
        Bound::Incl { v } => 2 * pos(axis, *v),
        Bound::Excl { v } => 2 * pos(axis, *v) + 1,
        Bound::TimeIncl { .. } | Bound::TimeExcl { .. } => i64::MIN,
    }
}

fn upper_cut(axis: &Axis, b: &Bound) -> i64 {
    match b {
        Bound::Open => i64::MAX,
        Bound::Incl { v } => 2 * pos(axis, *v),
        Bound::Excl { v } => 2 * pos(axis, *v) - 1,
        Bound::TimeIncl { .. } | Bound::TimeExcl { .. } => i64::MAX,
    }
}

fn canon_f64(t: f64) -> f64 {
    if t == 0.0 { 0.0 } else { t }
}

/// Strictly order-preserving map from finite `f64` (with `-0.0` canonicalized
/// to `+0.0`) to `i128`, avoiding microsecond truncation in time specificity.
fn f64_order_i128(t: f64) -> i128 {
    let bits = canon_f64(t).to_bits();
    let ordered = if (bits >> 63) == 0 {
        (bits ^ (1u64 << 63)) as i64
    } else {
        (!bits) as i64
    };
    ordered as i128
}

fn time_lower_ok(b: &Bound, t: f64) -> bool {
    let t = canon_f64(t);
    match b {
        Bound::Open => true,
        Bound::TimeIncl { t: lo } => t >= canon_f64(*lo),
        Bound::TimeExcl { t: lo } => t > canon_f64(*lo),
        _ => false,
    }
}

fn time_upper_ok(b: &Bound, t: f64) -> bool {
    let t = canon_f64(t);
    match b {
        Bound::Open => true,
        Bound::TimeIncl { t: hi } => t <= canon_f64(*hi),
        Bound::TimeExcl { t: hi } => t < canon_f64(*hi),
        _ => false,
    }
}

fn time_lo_key(b: &Bound) -> (f64, i8) {
    match b {
        Bound::Open => (f64::NEG_INFINITY, -1),
        Bound::TimeIncl { t } => (canon_f64(*t), 0),
        Bound::TimeExcl { t } => (canon_f64(*t), 1),
        _ => (f64::NEG_INFINITY, -1),
    }
}

fn time_hi_key(b: &Bound) -> (f64, i8) {
    match b {
        Bound::Open => (f64::INFINITY, 1),
        Bound::TimeIncl { t } => (canon_f64(*t), 0),
        Bound::TimeExcl { t } => (canon_f64(*t), -1),
        _ => (f64::INFINITY, 1),
    }
}

fn ends_with(path: &[NodeId], tail: &[NodeId]) -> bool {
    path.len() >= tail.len() && path[path.len() - tail.len()..] == *tail
}

impl Sel {
    pub fn contains(&self, axis: &Axis, v: AxisValueId) -> bool {
        match self {
            Sel::All => true,
            Sel::Range { lo, hi } => {
                let i = 2 * pos(axis, v);
                i >= lower_cut(axis, lo) && i <= upper_cut(axis, hi)
            }
            Sel::Set { values } => values.contains(&v),
            Sel::Not { values } => !values.contains(&v),
            Sel::Suffix { tail, not } => {
                let p = axis.path(v);
                ends_with(p, tail) && !not.iter().any(|n| ends_with(p, n))
            }
        }
    }

    pub fn contains_time(&self, t: f64) -> bool {
        match self {
            Sel::All => true,
            Sel::Range { lo, hi } => time_lower_ok(lo, t) && time_upper_ok(hi, t),
            _ => false,
        }
    }

    /// Direct path/scope matching for derived virtual-view instance coordinates
    /// (`engine.md` §2.3, `EN-5a`/`EN-5b`): requires no saved instance `AxisValue`.
    /// Each `(scope, start)` in `scopes` gives the slice `&path[start..]` inside `scope`.
    pub fn contains_path(&self, path: &[NodeId], scopes: &[(NodeId, usize)], anchor: Option<NodeId>) -> bool {
        match anchor {
            None => match self {
                Sel::All => true,
                Sel::Suffix { tail, not } => ends_with(path, tail) && !not.iter().any(|n| ends_with(path, n)),
                _ => false,
            },
            Some(scope) => match self {
                Sel::All => scopes.iter().any(|&(s, _)| s == scope),
                Sel::Suffix { tail, not } => scopes
                    .iter()
                    .filter(|&&(s, start)| s == scope && start <= path.len())
                    .any(|&(_, start)| {
                        let sub = &path[start..];
                        ends_with(sub, tail) && !not.iter().any(|n| ends_with(sub, n))
                    }),
                _ => false,
            },
        }
    }

    /// Whether this selector matches a specific slice `sub` inside an already-matched scope.
    pub fn contains_subpath(&self, sub: &[NodeId]) -> bool {
        match self {
            Sel::All => true,
            Sel::Suffix { tail, not } => ends_with(sub, tail) && !not.iter().any(|n| ends_with(sub, n)),
            _ => false,
        }
    }

    /// Containment with optional scope anchoring (patches.md §4, P18).
    pub fn contains_anchored(&self, axis: &Axis, v: AxisValueId, anchor: Option<NodeId>) -> bool {
        let Some(scope) = anchor else {
            return self.contains(axis, v);
        };
        if axis.kind != AxisKind::Instance {
            return self.contains(axis, v);
        }
        let path = axis.path(v);
        let scopes = axis.scopes(v);
        match self {
            Sel::All => scopes.contains(&scope),
            Sel::Suffix { tail, not } => scopes
                .iter()
                .enumerate()
                .filter(|&(k, &s)| s == scope && k < path.len())
                .any(|(k, _)| {
                    let sub = &path[k..];
                    ends_with(sub, tail) && !not.iter().any(|n| ends_with(sub, n))
                }),
            _ => scopes.contains(&scope) && self.contains(axis, v),
        }
    }

    pub fn members(&self, axis: &Axis) -> Vec<AxisValueId> {
        axis.live().filter(|&v| self.contains(axis, v)).collect()
    }

    pub fn is_empty(&self, axis: &Axis) -> bool {
        if axis.kind == AxisKind::Time {
            return self.void(axis);
        }
        if axis.kind == AxisKind::Instance && axis.values.is_empty() && matches!(self, Sel::All | Sel::Suffix { .. }) {
            return self.void(axis);
        }
        !axis.live().any(|v| self.contains(axis, v))
    }

    fn normal(&self, axis: &Axis) -> Sel {
        match (self, axis.kind) {
            (Sel::All, AxisKind::Ordered | AxisKind::Time) => Sel::Range { lo: Bound::Open, hi: Bound::Open },
            (Sel::All, AxisKind::Unordered) => Sel::Not { values: BTreeSet::new() },
            (Sel::All, AxisKind::Instance) => Sel::Suffix { tail: Vec::new(), not: Vec::new() },
            (s, _) => s.clone(),
        }
    }

    fn materialise(&self, axis: &Axis) -> BTreeSet<AxisValueId> {
        self.members(axis).into_iter().collect()
    }

    pub fn intersect(&self, other: &Sel, axis: &Axis) -> Sel {
        if *self == Sel::All {
            return other.clone();
        }
        if *other == Sel::All {
            return self.clone();
        }
        match (self.normal(axis), other.normal(axis)) {
            (Sel::Range { lo: a0, hi: a1 }, Sel::Range { lo: b0, hi: b1 }) if axis.kind == AxisKind::Time => {
                let lo = if time_lo_key(&a0).0.total_cmp(&time_lo_key(&b0).0).then(time_lo_key(&a0).1.cmp(&time_lo_key(&b0).1)) != Ordering::Less {
                    a0
                } else {
                    b0
                };
                let hi = if time_hi_key(&a1).0.total_cmp(&time_hi_key(&b1).0).then(time_hi_key(&a1).1.cmp(&time_hi_key(&b1).1)) != Ordering::Greater {
                    a1
                } else {
                    b1
                };
                Sel::Range { lo, hi }
            }
            (Sel::Range { lo: a0, hi: a1 }, Sel::Range { lo: b0, hi: b1 }) => {
                let lo = if lower_cut(axis, &a0) >= lower_cut(axis, &b0) { a0 } else { b0 };
                let hi = if upper_cut(axis, &a1) <= upper_cut(axis, &b1) { a1 } else { b1 };
                Sel::Range { lo, hi }
            }
            (Sel::Set { values: s }, Sel::Set { values: t }) => Sel::Set { values: &s & &t },
            (Sel::Set { values: s }, Sel::Not { values: t }) | (Sel::Not { values: t }, Sel::Set { values: s }) => Sel::Set { values: &s - &t },
            (Sel::Not { values: s }, Sel::Not { values: t }) => Sel::Not { values: &s | &t },
            (Sel::Set { values: s }, o) | (o, Sel::Set { values: s }) => Sel::Set {
                values: s.into_iter().filter(|v| o.contains(axis, *v)).collect(),
            },
            (Sel::Suffix { tail: a, not: an }, Sel::Suffix { tail: b, not: bn }) => {
                let tail = if ends_with(&a, &b) {
                    a
                } else if ends_with(&b, &a) {
                    b
                } else {
                    return Sel::Set { values: BTreeSet::new() };
                };
                let mut not = an;
                for n in bn {
                    if !not.contains(&n) {
                        not.push(n);
                    }
                }
                Sel::Suffix { tail, not }
            }
            (a, b) => Sel::Set { values: &a.materialise(axis) & &b.materialise(axis) },
        }
    }

    pub fn minus(&self, other: &Sel, axis: &Axis) -> Vec<Sel> {
        if other.is_empty(axis) {
            return if self.is_empty(axis) { Vec::new() } else { vec![self.clone()] };
        }
        let pieces: Vec<Sel> = match (self.normal(axis), other.normal(axis)) {
            (a @ Sel::Range { .. }, Sel::Range { lo: b0, hi: b1 }) if axis.kind == AxisKind::Time => {
                let below = match b0 {
                    Bound::Open => None,
                    Bound::TimeIncl { t } => Some(Sel::Range { lo: Bound::Open, hi: Bound::TimeExcl { t } }),
                    Bound::TimeExcl { t } => Some(Sel::Range { lo: Bound::Open, hi: Bound::TimeIncl { t } }),
                    _ => None,
                };
                let above = match b1 {
                    Bound::Open => None,
                    Bound::TimeIncl { t } => Some(Sel::Range { lo: Bound::TimeExcl { t }, hi: Bound::Open }),
                    Bound::TimeExcl { t } => Some(Sel::Range { lo: Bound::TimeIncl { t }, hi: Bound::Open }),
                    _ => None,
                };
                [below, above].into_iter().flatten().map(|c| a.intersect(&c, axis)).collect()
            }
            (a @ Sel::Range { .. }, Sel::Range { lo: b0, hi: b1 }) => {
                let below = match b0 {
                    Bound::Open => None,
                    Bound::Incl { v } => Some(Sel::Range { lo: Bound::Open, hi: Bound::Excl { v } }),
                    Bound::Excl { v } => Some(Sel::Range { lo: Bound::Open, hi: Bound::Incl { v } }),
                    _ => None,
                };
                let above = match b1 {
                    Bound::Open => None,
                    Bound::Incl { v } => Some(Sel::Range { lo: Bound::Excl { v }, hi: Bound::Open }),
                    Bound::Excl { v } => Some(Sel::Range { lo: Bound::Incl { v }, hi: Bound::Open }),
                    _ => None,
                };
                [below, above].into_iter().flatten().map(|c| a.intersect(&c, axis)).collect()
            }
            (a @ (Sel::Set { .. } | Sel::Not { .. }), Sel::Set { values: t }) => vec![a.intersect(&Sel::Not { values: t }, axis)],
            (a @ (Sel::Set { .. } | Sel::Not { .. }), Sel::Not { values: t }) => vec![a.intersect(&Sel::Set { values: t }, axis)],
            (a @ Sel::Suffix { .. }, Sel::Suffix { tail, not }) => {
                let Sel::Suffix { tail: at, not: an } = &a else { unreachable!() };
                let mut out = Vec::new();
                let mut outside = an.clone();
                outside.push(tail.clone());
                out.push(Sel::Suffix { tail: at.clone(), not: outside });
                let inside = a.intersect(&Sel::Suffix { tail: tail.clone(), not: Vec::new() }, axis);
                for (i, n) in not.iter().enumerate() {
                    out.push(inside.intersect(&Sel::Suffix { tail: n.clone(), not: not[..i].to_vec() }, axis));
                }
                out
            }
            (a, b) => vec![Sel::Set { values: &a.materialise(axis) - &b.materialise(axis) }],
        };
        pieces.into_iter().filter(|p| !p.is_empty(axis)).collect()
    }

    /// Whether it holds no value as written (P16).
    pub fn void(&self, axis: &Axis) -> bool {
        match self.normal(axis) {
            Sel::Range { lo, hi } if axis.kind == AxisKind::Time => {
                let (l, ls) = time_lo_key(&lo);
                let (h, hs) = time_hi_key(&hi);
                l > h || (l == h && (ls > 0 || hs < 0))
            }
            Sel::Range { lo, hi } => lower_cut(axis, &lo) > upper_cut(axis, &hi),
            Sel::Set { values } => values.is_empty(),
            Sel::Not { .. } | Sel::All => false,
            Sel::Suffix { tail, not } => not.iter().any(|m| ends_with(&tail, m)),
        }
    }

    /// Whether every value this holds as written, now or later, is in `outer` (P16).
    pub fn within(&self, outer: &Sel, axis: &Axis) -> bool {
        if *outer == Sel::All || self.void(axis) {
            return true;
        }
        match (self.normal(axis), outer.normal(axis)) {
            (Sel::Range { lo: a0, hi: a1 }, Sel::Range { lo: b0, hi: b1 }) if axis.kind == AxisKind::Time => {
                let lo_ok = time_lo_key(&a0).0.total_cmp(&time_lo_key(&b0).0).then(time_lo_key(&a0).1.cmp(&time_lo_key(&b0).1)) != Ordering::Less;
                let hi_ok = time_hi_key(&a1).0.total_cmp(&time_hi_key(&b1).0).then(time_hi_key(&a1).1.cmp(&time_hi_key(&b1).1)) != Ordering::Greater;
                lo_ok && hi_ok
            }
            (Sel::Range { lo: a0, hi: a1 }, Sel::Range { lo: b0, hi: b1 }) => {
                lower_cut(axis, &a0) >= lower_cut(axis, &b0) && upper_cut(axis, &a1) <= upper_cut(axis, &b1)
            }
            (Sel::Range { lo: a0, hi: a1 }, Sel::Set { values: t }) => {
                let (lo, hi) = (lower_cut(axis, &a0), upper_cut(axis, &a1));
                lo == hi && lo % 2 == 0 && axis.all().nth((lo / 2) as usize).is_some_and(|v| t.contains(&v))
            }
            (Sel::Set { values: s }, o @ Sel::Range { .. }) => s.iter().all(|v| o.contains(axis, *v)),
            (Sel::Set { values: s }, Sel::Set { values: t }) => s.is_subset(&t),
            (Sel::Set { values: s }, Sel::Not { values: t }) => s.is_disjoint(&t),
            (Sel::Not { values: s }, Sel::Not { values: t }) => t.is_subset(&s),
            (Sel::Suffix { tail: a, not: an }, Sel::Suffix { tail: b, not: bn }) => {
                ends_with(&a, &b)
                    && bn.iter().all(|n| {
                        if ends_with(&a, n) {
                            false
                        } else if ends_with(n, &a) {
                            an.iter().any(|m| ends_with(n, m))
                        } else {
                            true
                        }
                    })
            }
            (Sel::Set { values: s }, o @ Sel::Suffix { .. }) => s.iter().all(|v| o.contains(axis, *v)),
            _ => false,
        }
    }

    pub fn meets(&self, other: &Sel, axis: &Axis) -> bool {
        !self.intersect(other, axis).void(axis)
    }
}

/// A patch's region: one box (one selection per axis, P5).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Product {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sels: BTreeMap<AxisId, Sel>,
}

impl Product {
    pub fn all() -> Self {
        Product::default()
    }

    pub fn with(mut self, axis: AxisId, sel: Sel) -> Self {
        if sel == Sel::All {
            self.sels.remove(&axis);
        } else {
            self.sels.insert(axis, sel);
        }
        self
    }

    pub fn sel(&self, axis: AxisId) -> &Sel {
        self.sels.get(&axis).unwrap_or(&ALL_SEL)
    }

    pub fn contains(&self, axes: &BTreeMap<AxisId, Axis>, point: &Point) -> bool {
        self.sels.iter().all(|(a, s)| match axes.get(a) {
            Some(axis) if axis.kind == AxisKind::Time => point.time(*a).is_some_and(|t| s.contains_time(t)),
            Some(axis) => point.get(*a).is_some_and(|v| s.contains(axis, v)),
            None => false,
        })
    }

    pub fn contains_anchored(&self, axes: &BTreeMap<AxisId, Axis>, point: &Point, anchor: Option<NodeId>) -> bool {
        let Some(scope) = anchor else {
            return self.contains(axes, point);
        };
        let explicit_ok = self.sels.iter().all(|(a, s)| match axes.get(a) {
            Some(axis) if axis.kind == AxisKind::Time => point.time(*a).is_some_and(|t| s.contains_time(t)),
            Some(axis) => point.get(*a).is_some_and(|v| s.contains_anchored(axis, v, Some(scope))),
            None => false,
        });
        if !explicit_ok {
            return false;
        }
        axes.iter()
            .filter(|(a, ax)| ax.kind == AxisKind::Instance && !self.sels.contains_key(a))
            .all(|(a, ax)| point.get(*a).is_some_and(|v| Sel::All.contains_anchored(ax, v, Some(scope))))
    }

    /// Whether this box applies at a virtual view's derived coordinate
    /// (`global` document point + `view` placement path, enclosing scope chain,
    /// and component-local discrete coordinates; `engine.md` §2.3, `EN-5a`).
    pub fn contains_at_view(
        &self,
        axes: &BTreeMap<AxisId, Axis>,
        global: &Point,
        view: &ViewCoord,
        anchor: Option<NodeId>,
    ) -> bool {
        let mut checked_instance = false;
        for (a, sel) in &self.sels {
            let Some(ax) = axes.get(a) else { return false };
            let ok = match ax.kind {
                AxisKind::Time => global.time(*a).is_some_and(|t| sel.contains_time(t)),
                AxisKind::Instance => {
                    checked_instance = true;
                    if matches!(sel, Sel::All | Sel::Suffix { .. }) {
                        sel.contains_path(&view.path, &view.scopes, anchor)
                    } else {
                        view.value_on(global, *a).is_some_and(|v| sel.contains_anchored(ax, v, anchor))
                    }
                }
                AxisKind::Ordered | AxisKind::Unordered => view.value_on(global, *a).is_some_and(|v| sel.contains(ax, v)),
            };
            if !ok {
                return false;
            }
        }
        if let Some(scope) = anchor
            && !checked_instance
            && !view.has_scope(scope)
        {
            return false;
        }
        true
    }

    pub fn is_empty(&self, axes: &BTreeMap<AxisId, Axis>) -> bool {
        axes.iter().any(|(a, axis)| self.sel(*a).is_empty(axis)) || self.sels.keys().any(|a| !axes.contains_key(a))
    }

    pub fn intersect(&self, other: &Product, axes: &BTreeMap<AxisId, Axis>) -> Product {
        let mut out = self.clone();
        for (a, s) in &other.sels {
            let mine = out.sel(*a).clone();
            let joined = match axes.get(a) {
                Some(axis) => mine.intersect(s, axis),
                None => s.clone(),
            };
            out = out.with(*a, joined);
        }
        out
    }

    pub fn minus(&self, other: &Product, axes: &BTreeMap<AxisId, Axis>) -> Vec<Product> {
        if self.is_empty(axes) {
            return Vec::new();
        }
        if other.is_empty(axes) {
            return vec![self.clone()];
        }
        let mut out = Vec::new();
        let mut rest = self.clone();
        for (a, axis) in axes {
            let (mine, theirs) = (rest.sel(*a).clone(), other.sel(*a).clone());
            for piece in mine.minus(&theirs, axis) {
                out.push(rest.clone().with(*a, piece));
            }
            let inside = mine.intersect(&theirs, axis);
            if inside.is_empty(axis) {
                return out;
            }
            rest = rest.with(*a, inside);
        }
        out
    }

    pub fn void(&self, axes: &BTreeMap<AxisId, Axis>) -> bool {
        self.sels.iter().any(|(a, s)| axes.get(a).is_none_or(|axis| s.void(axis)))
    }

    pub fn within(&self, outer: &Product, axes: &BTreeMap<AxisId, Axis>) -> bool {
        self.void(axes) || axes.iter().all(|(a, axis)| self.sel(*a).within(outer.sel(*a), axis))
    }

    pub fn same(&self, other: &Product, axes: &BTreeMap<AxisId, Axis>) -> bool {
        self.within(other, axes) && other.within(self, axes)
    }

    pub fn meets(&self, other: &Product, axes: &BTreeMap<AxisId, Axis>) -> bool {
        !self.intersect(other, axes).void(axes)
    }

    pub fn points(&self, table: &PatchTable) -> Vec<Point> {
        table.points().into_iter().filter(|p| self.contains(&table.axes, p)).collect()
    }
}

/// Union of boxes: query answers (winning region, blast radius).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Region(pub Vec<Product>);

impl Region {
    pub fn contains(&self, axes: &BTreeMap<AxisId, Axis>, point: &Point) -> bool {
        self.0.iter().any(|p| p.contains(axes, point))
    }

    pub fn minus(&self, other: &Product, axes: &BTreeMap<AxisId, Axis>) -> Region {
        Region(self.0.iter().flat_map(|p| p.minus(other, axes)).collect())
    }

    pub fn is_empty(&self, axes: &BTreeMap<AxisId, Axis>) -> bool {
        self.0.iter().all(|p| p.is_empty(axes))
    }

    pub fn points(&self, table: &PatchTable) -> Vec<Point> {
        table.points().into_iter().filter(|p| self.contains(&table.axes, p)).collect()
    }
}

// ---------------------------------------------------------------------------
// Patches, entries, strength, and in-memory index
// ---------------------------------------------------------------------------

/// What aspect of a target node a patch entry sets (patches.md §5, P6, P21).
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PatchKey {
    Present,
    Hidden,
    FacetPresent(FacetKind),
    Leaf(LeafPath),
    Notation,
}

impl fmt::Display for PatchKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PatchKey::Present => f.write_str("present"),
            PatchKey::Hidden => f.write_str("hidden"),
            PatchKey::FacetPresent(k) => write!(f, "facet:{}", k.name()),
            PatchKey::Leaf(lp) => write!(f, "{lp}"),
            PatchKey::Notation => f.write_str("notation"),
        }
    }
}

impl FromStr for PatchKey {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "present" => Ok(PatchKey::Present),
            "hidden" => Ok(PatchKey::Hidden),
            "notation" => Ok(PatchKey::Notation),
            other => {
                if let Some(kname) = other.strip_prefix("facet:") {
                    let kind = FacetKind::from_name(kname).ok_or_else(|| format!("unknown facet {kname:?}"))?;
                    Ok(PatchKey::FacetPresent(kind))
                } else {
                    let lp: LeafPath = other.parse().map_err(|e: crate::facet::LeafError| e.to_string())?;
                    Ok(PatchKey::Leaf(lp))
                }
            }
        }
    }
}

impl Serialize for PatchKey {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for PatchKey {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// Value stored in a patch entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "k", rename_all = "snake_case")]
pub enum PatchValue {
    Bool { v: bool },
    Slot { slot: Slot },
    Notation { notation: String },
}

impl PatchValue {
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            PatchValue::Bool { v } => Some(*v),
            _ => None,
        }
    }
    pub fn as_slot(&self) -> Option<&Slot> {
        match self {
            PatchValue::Slot { slot } => Some(slot),
            _ => None,
        }
    }
    pub fn as_notation(&self) -> Option<&str> {
        match self {
            PatchValue::Notation { notation } => Some(notation),
            _ => None,
        }
    }
}

/// One entry in a patch's sorted entry map (canonical JSON serialises
/// `(NodeId, PatchKey)` as `"<node>|<key>"` so `BTreeMap` keys stay strings).
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PatchTarget {
    pub node: NodeId,
    pub key: PatchKey,
}

impl fmt::Display for PatchTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}|{}", self.node, self.key)
    }
}

impl FromStr for PatchTarget {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (n, k) = s.split_once('|').ok_or_else(|| format!("expected `<node>|<key>`, got {s:?}"))?;
        let node: NodeId = n.parse().map_err(|e: crate::id::IdParseError| e.to_string())?;
        let key: PatchKey = k.parse()?;
        Ok(PatchTarget { node, key })
    }
}

impl Serialize for PatchTarget {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for PatchTarget {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        text.parse().map_err(serde::de::Error::custom)
    }
}

/// A partial set of overrides with a region and stored `(layer, rank)` strength (P1, P5, P15).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Patch {
    pub id: PatchId,
    pub layer: u32,
    pub rank: OrderKey,
    #[serde(default)]
    pub region: Product,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub anchor: Option<NodeId>,
    #[serde(default = "default_true")]
    pub writable: bool,
    #[serde(default)]
    pub entries: BTreeMap<PatchTarget, PatchValue>,
}

/// One axis's specificity key, from the selection as written (P10).
pub type AxisKey = (i128, i128, Vec<i64>);

fn exclusions(tail: &[NodeId], not: &[Vec<NodeId>]) -> Vec<i64> {
    let mut ext: Vec<&Vec<NodeId>> = not.iter().filter(|n| n.len() > tail.len() && ends_with(n, tail)).collect();
    ext.sort();
    ext.dedup();
    let reduced: Vec<&Vec<NodeId>> = ext
        .iter()
        .copied()
        .filter(|n| !ext.iter().any(|m| m.len() < n.len() && ends_with(n, m)))
        .collect();
    let mut out = Vec::new();
    for n in reduced {
        let i = n.len() - tail.len() - 1;
        if out.len() <= i {
            out.resize(i + 1, 0);
        }
        out[i] += 1;
    }
    out
}

pub fn axis_key(axes: &BTreeMap<AxisId, Axis>, axis: AxisId, sel: &Sel) -> AxisKey {
    let Some(ax) = axes.get(&axis) else {
        return (i128::MIN, i128::MIN, vec![i64::MIN]);
    };
    if sel.void(ax) {
        return (i128::MAX, i128::MAX, vec![i64::MAX]);
    }
    let cut = |v: AxisValueId| (2 * pos(ax, v)) as i128;
    match (ax.kind, sel) {
        (AxisKind::Ordered, Sel::All) => (i128::MIN, -i128::MAX, vec![i64::MIN / 2]),
        (AxisKind::Ordered, Sel::Range { lo, hi }) => {
            let l = match lo {
                Bound::Open => i128::MIN,
                Bound::Incl { v } => cut(*v),
                Bound::Excl { v } => cut(*v) + 1,
                _ => i128::MIN,
            };
            let h = match hi {
                Bound::Open => i128::MAX,
                Bound::Incl { v } => cut(*v),
                Bound::Excl { v } => cut(*v) - 1,
                _ => i128::MAX,
            };
            (l, -h, vec![if l == h { -1 } else { i64::MIN / 2 }])
        }
        (AxisKind::Ordered, Sel::Set { values }) => {
            let cuts: Vec<i128> = values.iter().map(|v| cut(*v)).collect();
            (
                cuts.iter().copied().min().unwrap_or(0),
                -cuts.iter().copied().max().unwrap_or(0),
                vec![-(values.len() as i64)],
            )
        }
        (AxisKind::Time, Sel::All) => (i128::MIN, -i128::MAX, vec![i64::MIN / 2]),
        (AxisKind::Time, Sel::Range { lo, hi }) => {
            let enc = |t: f64| f64_order_i128(t) * 2;
            let l = match lo {
                Bound::Open => i128::MIN,
                Bound::TimeIncl { t } => enc(*t),
                Bound::TimeExcl { t } => enc(*t) + 1,
                _ => i128::MIN,
            };
            let h = match hi {
                Bound::Open => i128::MAX,
                Bound::TimeIncl { t } => enc(*t),
                Bound::TimeExcl { t } => enc(*t) - 1,
                _ => i128::MAX,
            };
            (l, -h, vec![if l == h { -1 } else { i64::MIN / 2 }])
        }
        (AxisKind::Unordered, Sel::Set { values }) => (1, -(values.len() as i128), Vec::new()),
        (AxisKind::Unordered, Sel::Not { values }) => (0, values.len() as i128, Vec::new()),
        (AxisKind::Unordered, Sel::All) => (0, 0, Vec::new()),
        (AxisKind::Instance, Sel::Suffix { tail, not }) => (tail.len() as i128, 0, exclusions(tail, not)),
        (AxisKind::Instance, Sel::All) => (0, 0, Vec::new()),
        _ => (i128::MIN, i128::MIN, vec![i64::MIN]),
    }
}

/// Whether a stronger patch's `anchor` covers every instance coordinate that
/// a weaker patch's `anchor` can match.
fn anchor_covers(strong: Option<NodeId>, weak: Option<NodeId>) -> bool {
    strong.is_none() || strong == weak
}

/// Stored authored patches, axes, dedicated solve layer (P17), and derived
/// in-memory indexes (`by_target`, `by_axis`, `anchored`, `solve_by_node`).
#[derive(Clone, Debug)]
pub struct PatchTable {
    pub axes: BTreeMap<AxisId, Axis>,
    pub axis_order: Vec<AxisId>,
    pub patches: BTreeMap<PatchId, Patch>,
    /// Dedicated system-owned solve layer keyed by `DiscretePoint` -> `Leaf` -> `Slot`
    /// (engine.md §2.4, patches.md P17). Nested by `DiscretePoint` so `Tree::solve_leaf`
    /// performs zero-allocation lookups and `Docs::set_point` touches only old/new points.
    pub solve: BTreeMap<DiscretePoint, BTreeMap<Leaf, Slot>>,
    pub next_axis: u64,
    pub next_axis_value: u64,
    pub next_patch: u64,
    // Derived in-memory indexes (rebuilt by `reindex`, outside `PartialEq`).
    by_target: HashMap<NodeId, BTreeSet<PatchId>>,
    by_axis: HashMap<AxisId, BTreeSet<PatchId>>,
    anchored: BTreeSet<PatchId>,
    solve_by_node: HashMap<NodeId, BTreeSet<(DiscretePoint, LeafPath)>>,
}

impl Default for PatchTable {
    fn default() -> Self {
        PatchTable {
            axes: BTreeMap::new(),
            axis_order: Vec::new(),
            patches: BTreeMap::new(),
            solve: BTreeMap::new(),
            next_axis: 1,
            next_axis_value: 1,
            next_patch: 1,
            by_target: HashMap::new(),
            by_axis: HashMap::new(),
            anchored: BTreeSet::new(),
            solve_by_node: HashMap::new(),
        }
    }
}

impl PartialEq for PatchTable {
    fn eq(&self, other: &Self) -> bool {
        self.axes == other.axes
            && self.axis_order == other.axis_order
            && self.patches == other.patches
            && self.solve == other.solve
            && self.next_axis == other.next_axis
            && self.next_axis_value == other.next_axis_value
            && self.next_patch == other.next_patch
    }
}

/// Lints reported over the patch table (patches.md §6.1, §10).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PatchLints {
    pub dead: Vec<(PatchId, PatchTarget)>,
    pub ties: Vec<(PatchId, PatchId)>,
    pub contradictions: Vec<(PatchId, PatchId)>,
    pub stale: Vec<(PatchId, PatchTarget)>,
}

impl PatchTable {
    pub fn is_empty(&self) -> bool {
        self.axes.is_empty() && self.patches.is_empty() && self.solve.is_empty()
    }

    pub fn next_axis_id(&self) -> AxisId {
        AxisId(self.next_axis)
    }

    pub fn next_axis_value_id(&self) -> AxisValueId {
        AxisValueId(self.next_axis_value)
    }

    pub fn next_patch_id(&self) -> PatchId {
        PatchId(self.next_patch)
    }

    /// First `Instance` axis in the table, if one exists (`EN-5b`).
    pub fn instance_axis_id(&self) -> Option<AxisId> {
        self.axes.values().find(|a| a.kind == AxisKind::Instance).map(|a| a.id)
    }

    /// Look up one solve-layer entry at discrete point `dp` without allocating.
    pub fn solve_get(&self, dp: &DiscretePoint, leaf: Leaf) -> Option<&Slot> {
        self.solve.get(dp)?.get(&leaf)
    }

    /// Insert or update a solve-layer entry (maintaining `solve_by_node`).
    pub(crate) fn solve_insert(&mut self, dp: DiscretePoint, leaf: Leaf, slot: Slot) -> Option<Slot> {
        self.index_solve(&dp, leaf, true);
        self.solve.entry(dp).or_default().insert(leaf, slot)
    }

    /// Remove a solve-layer entry (pruning empty inner maps and maintaining `solve_by_node`).
    pub(crate) fn solve_remove(&mut self, dp: &DiscretePoint, leaf: Leaf) -> Option<Slot> {
        self.index_solve(dp, leaf, false);
        let old = self.solve.get_mut(dp)?.remove(&leaf);
        if self.solve.get(dp).is_some_and(BTreeMap::is_empty) {
            self.solve.remove(dp);
        }
        old
    }

    /// Existing writable layer-0 patch matching `(anchor, Suffix { tail, not: [] })`
    /// on `inst_ax` (and no other axis restrictions), highest-ranked first (`EN-5b`).
    pub fn instance_override_patch(&self, inst_ax: AxisId, anchor: Option<NodeId>, tail: &[NodeId]) -> Option<&Patch> {
        let want = Product::default().with(inst_ax, Sel::Suffix { tail: tail.to_vec(), not: Vec::new() });
        self.patches
            .values()
            .filter(|p| p.writable && p.layer == 0 && p.anchor == anchor && p.region == want)
            .max_by(|a, b| self.compare(a, b))
    }

    /// Allocate a fresh `OrderKey` strictly above `floor` (when present) and strictly
    /// below the next occupied key in `used`, guaranteed never to collide (`EN-5b`).
    pub fn alloc_rank(used: &BTreeSet<OrderKey>, floor: Option<&OrderKey>) -> OrderKey {
        match floor {
            None => match used.iter().next() {
                Some(first) => OrderKey::before(first),
                None => OrderKey::first(),
            },
            Some(lo) => {
                let next = used.range((std::ops::Bound::Excluded(lo), std::ops::Bound::Unbounded)).next();
                match next {
                    Some(hi) => OrderKey::between(Some(lo), Some(hi)).expect("alloc_rank: hi > lo"),
                    None => OrderKey::after(lo),
                }
            }
        }
    }

    pub fn reindex(&mut self) {
        self.by_target.clear();
        self.by_axis.clear();
        self.anchored.clear();
        self.solve_by_node.clear();
        for p in self.patches.values() {
            for t in p.entries.keys() {
                self.by_target.entry(t.node).or_default().insert(p.id);
            }
            for &a in p.region.sels.keys() {
                self.by_axis.entry(a).or_default().insert(p.id);
            }
            if p.anchor.is_some() {
                self.anchored.insert(p.id);
            }
        }
        for (dp, leaves) in &self.solve {
            for leaf in leaves.keys() {
                self.solve_by_node.entry(leaf.node).or_default().insert((dp.clone(), leaf.path));
            }
        }
    }

    pub(crate) fn index_patch(&mut self, p: &Patch) {
        for t in p.entries.keys() {
            self.by_target.entry(t.node).or_default().insert(p.id);
        }
        for &a in p.region.sels.keys() {
            self.by_axis.entry(a).or_default().insert(p.id);
        }
        if p.anchor.is_some() {
            self.anchored.insert(p.id);
        }
    }

    pub(crate) fn unindex_patch(&mut self, p: &Patch) {
        for t in p.entries.keys() {
            if let Some(s) = self.by_target.get_mut(&t.node) {
                s.remove(&p.id);
                if s.is_empty() {
                    self.by_target.remove(&t.node);
                }
            }
        }
        for &a in p.region.sels.keys() {
            if let Some(s) = self.by_axis.get_mut(&a) {
                s.remove(&p.id);
                if s.is_empty() {
                    self.by_axis.remove(&a);
                }
            }
        }
        if p.anchor.is_some() {
            self.anchored.remove(&p.id);
        }
    }

    pub(crate) fn index_solve(&mut self, dp: &DiscretePoint, leaf: Leaf, present: bool) {
        if present {
            self.solve_by_node.entry(leaf.node).or_default().insert((dp.clone(), leaf.path));
        } else if let Some(s) = self.solve_by_node.get_mut(&leaf.node) {
            s.remove(&(dp.clone(), leaf.path));
            if s.is_empty() {
                self.solve_by_node.remove(&leaf.node);
            }
        }
    }

    /// O(1) lookup of authored patches targeting `node`.
    pub fn patches_for_target(&self, node: NodeId) -> impl Iterator<Item = &Patch> {
        self.by_target
            .get(&node)
            .into_iter()
            .flatten()
            .filter_map(|id| self.patches.get(id))
    }

    /// Every node targeted by any patch referencing `axis` (including anchored
    /// patches that implicitly depend on `Instance` axes, patches.md §4 P18).
    pub fn targets_for_axis(&self, axis: AxisId) -> BTreeSet<NodeId> {
        let mut out = BTreeSet::new();
        if let Some(ids) = self.by_axis.get(&axis) {
            for id in ids {
                if let Some(p) = self.patches.get(id) {
                    out.extend(p.entries.keys().map(|t| t.node));
                }
            }
        }
        if self.axes.get(&axis).is_some_and(|a| a.kind == AxisKind::Instance) {
            for id in &self.anchored {
                if let Some(p) = self.patches.get(id) {
                    out.extend(p.entries.keys().map(|t| t.node));
                }
            }
        }
        out
    }

    /// Whether any authored patch references `axis` (explicitly or via an
    /// anchor when `axis` is the last `Instance` axis).
    pub fn axis_referenced(&self, axis: AxisId) -> bool {
        if self.by_axis.get(&axis).is_some_and(|s| !s.is_empty()) {
            return true;
        }
        if self.axes.get(&axis).is_some_and(|a| a.kind == AxisKind::Instance)
            && !self.anchored.is_empty()
            && self.axes.values().filter(|a| a.kind == AxisKind::Instance).count() <= 1
        {
            return true;
        }
        false
    }

    /// Validate an axis definition (unique value ids, valid path/scope alignment).
    pub fn validate_axis(axis: &Axis) -> Result<(), String> {
        let mut seen = BTreeSet::new();
        for v in &axis.values {
            if !seen.insert(v.id) {
                return Err(format!("axis {} has duplicate value {}", axis.id, v.id));
            }
            if !v.scopes.is_empty() && v.scopes.len() != v.path.len() {
                return Err(format!("axis {} value {} scopes length {} != path length {}", axis.id, v.id, v.scopes.len(), v.path.len()));
            }
        }
        Ok(())
    }

    /// Validate a region & optional anchor against this table's axes.
    pub fn validate_region(&self, region: &Product, _anchor: Option<NodeId>) -> Result<(), String> {
        for (a, sel) in &region.sels {
            let ax = self.axes.get(a).ok_or_else(|| format!("region references unknown axis {a}"))?;
            match (ax.kind, sel) {
                (_, Sel::All) => {}
                (AxisKind::Ordered, Sel::Range { lo, hi }) => {
                    for b in [lo, hi] {
                        if matches!(b, Bound::TimeIncl { .. } | Bound::TimeExcl { .. }) {
                            return Err(format!("ordered axis {a} cannot use time bounds"));
                        }
                    }
                }
                (AxisKind::Ordered, Sel::Set { .. }) => {}
                (AxisKind::Time, Sel::Range { lo, hi }) => {
                    for b in [lo, hi] {
                        match b {
                            Bound::Open => {}
                            Bound::TimeIncl { t } | Bound::TimeExcl { t } => {
                                if !t.is_finite() {
                                    return Err(format!("time axis {a} bound must be finite"));
                                }
                            }
                            Bound::Incl { .. } | Bound::Excl { .. } => {
                                return Err(format!("time axis {a} cannot use discrete bounds"));
                            }
                        }
                    }
                }
                (AxisKind::Unordered, Sel::Set { .. } | Sel::Not { .. }) => {}
                (AxisKind::Instance, Sel::Suffix { .. }) => {}
                (AxisKind::Instance, Sel::Set { values }) => {
                    if ax.values.is_empty() && !values.is_empty() {
                        return Err(format!("derived-path instance axis {a} cannot use non-empty Set selector"));
                    }
                }
                (kind, _) => {
                    return Err(format!("selector not allowed on {kind:?} axis {a}"));
                }
            }
        }
        Ok(())
    }

    /// Validate one patch entry's key/value match, leaf type, and required-facet rules.
    pub fn validate_entry(target: PatchTarget, val: &PatchValue, node_ty: Option<crate::registry::TypeId>) -> Result<(), String> {
        match (target.key, val) {
            (PatchKey::Present | PatchKey::Hidden, PatchValue::Bool { .. }) => Ok(()),
            (PatchKey::FacetPresent(fk), PatchValue::Bool { v }) => {
                if let Some(ty) = node_ty {
                    if !*v && ty.spec().requires(fk) {
                        return Err(format!("{} ({}) requires facet {}", target.node, ty, fk.name()));
                    }
                    if *v && !ty.spec().allows(fk) {
                        return Err(format!("{} ({}) does not allow facet {}", target.node, ty, fk.name()));
                    }
                }
                Ok(())
            }
            (PatchKey::Leaf(lp), PatchValue::Slot { slot }) => {
                if let Some(ty) = node_ty
                    && !ty.spec().allows(lp.facet)
                {
                    return Err(format!("{} ({}) does not allow facet {}", target.node, ty, lp.facet.name()));
                }
                let mut probe = lp.facet.default_facet();
                probe.set(lp.field, slot.clone()).map_err(|e| format!("{target}: {e}"))?;
                Ok(())
            }
            (PatchKey::Notation, PatchValue::Notation { .. }) => Ok(()),
            _ => Err(format!("patch value kind does not match target {target}")),
        }
    }

    /// Validate an authored patch (region, layer, anchor, P19 time-axis rules, and all entries).
    pub fn validate_patch(&self, patch: &Patch, node_ty: &dyn Fn(NodeId) -> Option<crate::registry::TypeId>) -> Result<(), String> {
        if patch.layer == SESSION_LAYER {
            return Err("authored patch may not use SESSION_LAYER".into());
        }
        self.validate_region(&patch.region, patch.anchor)?;
        let constrains_time = patch.region.sels.iter().any(|(a, sel)| {
            !matches!(sel, Sel::All) && self.axes.get(a).is_some_and(|ax| ax.kind == AxisKind::Time)
        });
        for (&target, val) in &patch.entries {
            if constrains_time {
                if matches!(target.key, PatchKey::Present) {
                    return Err(format!("time-axis patch may not set Present on {}", target.node));
                }
                if let PatchValue::Slot { slot } = val
                    && slot.expr().is_some()
                {
                    return Err(format!("time-axis patch {target} must hold a constant, not a binding"));
                }
            }
            Self::validate_entry(target, val, node_ty(target.node))?;
        }
        Ok(())
    }

    /// Validate a session-layer patch (constant leaf entries only, P8).
    pub fn validate_session_patch(&self, patch: &Patch, node_ty: &dyn Fn(NodeId) -> Option<crate::registry::TypeId>) -> Result<(), String> {
        self.validate_region(&patch.region, patch.anchor)?;
        for (&target, val) in &patch.entries {
            match (target.key, val) {
                (PatchKey::Leaf(_), PatchValue::Slot { slot }) if slot.expr().is_none() => {
                    Self::validate_entry(target, val, node_ty(target.node))?;
                }
                (PatchKey::Leaf(_), PatchValue::Slot { .. }) => {
                    return Err(format!("session patch {target} must hold a constant slot"));
                }
                _ => {
                    return Err(format!("session patch supports leaf entries only, got {target}"));
                }
            }
        }
        Ok(())
    }

    /// Validate a solve-layer constant entry (P17).
    pub fn validate_solve_entry(leaf: Leaf, slot: &Slot, node_ty: Option<crate::registry::TypeId>) -> Result<(), String> {
        if slot.expr().is_some() {
            return Err(format!("solve layer entry {leaf} must be a constant, not a binding"));
        }
        Self::validate_entry(
            PatchTarget { node: leaf.node, key: PatchKey::Leaf(leaf.path) },
            &PatchValue::Slot { slot: slot.clone() },
            node_ty,
        )
    }

    /// Whole-table validation used by Schema 16 `serial::load`.
    pub fn validate_all(&self, node_ty: &dyn Fn(NodeId) -> Option<crate::registry::TypeId>) -> Result<(), String> {
        for (&k, ax) in &self.axes {
            if k != ax.id {
                return Err(format!("axis map key {k} disagrees with embedded id {}", ax.id));
            }
            Self::validate_axis(ax)?;
        }
        let mut ranks: BTreeSet<(Option<NodeId>, u32, OrderKey)> = BTreeSet::new();
        for (&k, p) in &self.patches {
            if k != p.id {
                return Err(format!("patch map key {k} disagrees with embedded id {}", p.id));
            }
            if !ranks.insert((p.anchor, p.layer, p.rank.clone())) {
                return Err(format!("duplicate rank {} in layer {}", p.rank, p.layer));
            }
            self.validate_patch(p, node_ty)?;
        }
        for leaves in self.solve.values() {
            for (leaf, slot) in leaves {
                Self::validate_solve_entry(*leaf, slot, node_ty(leaf.node))?;
            }
        }
        Ok(())
    }

    /// Every node that has at least one authored patch or solve entry.
    pub fn all_targets(&self) -> BTreeSet<NodeId> {
        self.by_target.keys().copied().chain(self.solve_by_node.keys().copied()).collect()
    }

    pub fn solve_leaves_of(&self, node: NodeId) -> impl Iterator<Item = LeafPath> + '_ {
        self.solve_by_node.get(&node).into_iter().flatten().map(|(_, lp)| *lp)
    }

    /// Axes in deciding order (P11): explicit `axis_order`, then any remaining
    /// by declared kind order (Instance, Unordered, Time, Ordered) and id.
    pub fn deciding(&self) -> Vec<AxisId> {
        let mut out: Vec<AxisId> = self.axis_order.iter().copied().filter(|a| self.axes.contains_key(a)).collect();
        let mut rest: Vec<(u8, AxisId)> = self
            .axes
            .iter()
            .filter(|(a, _)| !out.contains(a))
            .map(|(&id, ax)| {
                let rank = match ax.kind {
                    AxisKind::Instance => 0,
                    AxisKind::Unordered => 1,
                    AxisKind::Time => 2,
                    AxisKind::Ordered => 3,
                };
                (rank, id)
            })
            .collect();
        rest.sort();
        out.extend(rest.into_iter().map(|(_, id)| id));
        out
    }

    /// Specificity keys of `region` in deciding order (P10).
    pub fn keys(&self, region: &Product) -> Vec<AxisKey> {
        self.deciding().into_iter().map(|a| axis_key(&self.axes, a, region.sel(a))).collect()
    }

    /// Stored strength comparison: `(layer, rank, id)` (P1, P15).
    pub fn compare(&self, p: &Patch, q: &Patch) -> Ordering {
        if p.id == q.id {
            return Ordering::Equal;
        }
        p.layer.cmp(&q.layer).then_with(|| p.rank.cmp(&q.rank)).then_with(|| p.id.cmp(&q.id))
    }

    /// Default rank for a new patch on `region` in `(anchor, layer)` (patches.md §6.1):
    /// just above every patch of `(anchor, layer)` whose keys are `<= mine`, below the
    /// next rank above those. Never collides with an existing rank in `(anchor, layer)`.
    pub fn default_rank(&self, region: &Product, layer: u32) -> OrderKey {
        self.default_rank_in(region, None, layer)
    }

    pub fn default_rank_in(&self, region: &Product, anchor: Option<NodeId>, layer: u32) -> OrderKey {
        let mine = self.keys(region);
        let below = self
            .patches
            .values()
            .filter(|p| p.anchor == anchor && p.layer == layer && self.keys(&p.region) <= mine)
            .map(|p| &p.rank)
            .max();
        match below {
            None => match self.patches.values().filter(|p| p.anchor == anchor && p.layer == layer).map(|p| &p.rank).min() {
                Some(first) => OrderKey::before(first),
                None => OrderKey::first(),
            },
            Some(b) => self.rank_above_in(anchor, layer, b),
        }
    }

    /// A distinct fractional `OrderKey` in `(None, layer)` strictly between `r` and
    /// the next existing rank above `r`.
    pub fn rank_above(&self, layer: u32, r: &OrderKey) -> OrderKey {
        self.rank_above_in(None, layer, r)
    }

    /// A distinct fractional `OrderKey` in `(anchor, layer)` strictly between `r` and
    /// the next existing rank above `r`.
    pub fn rank_above_in(&self, anchor: Option<NodeId>, layer: u32, r: &OrderKey) -> OrderKey {
        let next = self.patches.values().filter(|p| p.anchor == anchor && p.layer == layer && &p.rank > r).map(|p| &p.rank).min();
        match next {
            Some(hi) => OrderKey::between(Some(r), Some(hi)).expect("rank_above: hi > r"),
            None => OrderKey::after(r),
        }
    }

    /// Every discrete point across all non-time axes (skipping derived-path
    /// `Instance` axes that hold no saved `AxisValue`s, `EN-5b`).
    pub fn points(&self) -> Vec<Point> {
        let mut out: Vec<Point> = vec![Point::new()];
        for (&a, axis) in &self.axes {
            if axis.kind == AxisKind::Time || (axis.kind == AxisKind::Instance && axis.values.is_empty()) {
                continue;
            }
            let vals: Vec<AxisValueId> = axis.live().collect();
            if vals.is_empty() {
                return Vec::new();
            }
            out = out
                .into_iter()
                .flat_map(|p| {
                    vals.iter().map(move |&v| {
                        let mut q = p.clone();
                        q.values.insert(a, v);
                        q
                    })
                })
                .collect();
        }
        out
    }

    /// Derive structural layer precedence for a patch anchored at `anchor`
    /// inside a view whose enclosing scope chain is `scopes` (outermost first;
    /// `engine.md` §2.3, `patches.md` §6.1, `EN-5a`/`EN-5b`).
    ///
    /// Outermost wins by structure without globally renumbering `u32` layers:
    /// - `anchor == None` (document-level) -> `Some(usize::MAX - 1)` (outermost authored)
    /// - `anchor == Some(scope)` at index `pos` in `scopes` -> `Some(scopes.len() - 1 - pos)`
    /// - `anchor == Some(scope)` not in `scopes` -> `None` (cannot apply in this view)
    pub fn derived_layer(anchor: Option<NodeId>, scopes: &[(NodeId, usize)]) -> Option<usize> {
        match anchor {
            None => Some(usize::MAX - 1),
            Some(scope) => scopes.iter().position(|&(s, _)| s == scope).map(|pos| scopes.len() - 1 - pos),
        }
    }

    /// Derived layer for patch `p` at `view`, handling recursive scope nesting
    /// (`[A, B, A]`) by choosing the outermost matching scope index whose
    /// subpath satisfies `p`'s instance selectors (`EN-5b`).
    pub fn derived_layer_for(&self, p: &Patch, view: &ViewCoord) -> Option<usize> {
        let Some(scope) = p.anchor else {
            return Some(usize::MAX - 1);
        };
        let has_inst_sel = p
            .region
            .sels
            .iter()
            .any(|(a, sel)| matches!(sel, Sel::Suffix { .. }) && self.axes.get(a).is_some_and(|ax| ax.kind == AxisKind::Instance));
        if !has_inst_sel {
            return Self::derived_layer(Some(scope), &view.scopes);
        }
        view.scopes
            .iter()
            .enumerate()
            .find(|&(_, &(s, start))| {
                s == scope
                    && start <= view.path.len()
                    && p.region.sels.iter().all(|(a, sel)| {
                        if matches!(sel, Sel::Suffix { .. }) && self.axes.get(a).is_some_and(|ax| ax.kind == AxisKind::Instance) {
                            sel.contains_subpath(&view.path[start..])
                        } else {
                            true
                        }
                    })
            })
            .map(|(pos, _)| view.scopes.len() - 1 - pos)
    }

    /// Compare two patches within a virtual view: session layer first, then
    /// derived enclosing scope-chain layer (outermost scope wins), then stored
    /// `(layer, rank, id)` within the same scope (`EN-5a`/`EN-5b`).
    pub fn compare_at_view(&self, p: &Patch, q: &Patch, view: &ViewCoord) -> Ordering {
        let eff_layer = |x: &Patch| -> usize {
            if x.layer == SESSION_LAYER {
                usize::MAX
            } else {
                self.derived_layer_for(x, view).unwrap_or(0)
            }
        };
        eff_layer(p)
            .cmp(&eff_layer(q))
            .then_with(|| p.layer.cmp(&q.layer))
            .then_with(|| p.rank.cmp(&q.rank))
            .then_with(|| p.id.cmp(&q.id))
    }

    /// Strongest authored patch setting `target` at a virtual view's derived
    /// coordinate `(global, view)` (`engine.md` §2.3, `EN-5a`).
    pub fn winner_authored_at_view(&self, global: &Point, view: &ViewCoord, target: PatchTarget) -> Option<&Patch> {
        let mut best: Option<&Patch> = None;
        for p in self.patches_for_target(target.node) {
            if p.layer < SESSION_LAYER
                && p.entries.contains_key(&target)
                && p.region.contains_at_view(&self.axes, global, view, p.anchor)
                && best.is_none_or(|b| self.compare_at_view(p, b, view) == Ordering::Greater)
            {
                best = Some(p);
            }
        }
        best
    }

    /// Strongest session patch setting `target` at a virtual view's derived
    /// coordinate `(global, view)` (`engine.md` §2.3, `EN-5a`).
    pub fn winner_session_at_view<'a>(
        &self,
        session: &'a BTreeMap<PatchId, Patch>,
        global: &Point,
        view: &ViewCoord,
        target: PatchTarget,
    ) -> Option<&'a Patch> {
        let mut best: Option<&Patch> = None;
        for p in session.values() {
            if p.layer == SESSION_LAYER
                && p.entries.contains_key(&target)
                && p.region.contains_at_view(&self.axes, global, view, p.anchor)
                && best.is_none_or(|b| self.compare_at_view(p, b, view) == Ordering::Greater)
            {
                best = Some(p);
            }
        }
        best
    }

    /// Strongest authored patch (`layer < SESSION_LAYER`) setting `target` at `point`.
    pub fn winner_authored(&self, point: &Point, target: PatchTarget) -> Option<&Patch> {
        let mut best: Option<&Patch> = None;
        for p in self.patches_for_target(target.node) {
            if p.layer < SESSION_LAYER
                && p.entries.contains_key(&target)
                && p.region.contains_anchored(&self.axes, point, p.anchor)
                && best.is_none_or(|b| self.compare(p, b) == Ordering::Greater)
            {
                best = Some(p);
            }
        }
        best
    }

    /// Strongest session patch (`layer == SESSION_LAYER`) setting `target` at `point`.
    pub fn winner_session<'a>(&self, session: &'a BTreeMap<PatchId, Patch>, point: &Point, target: PatchTarget) -> Option<&'a Patch> {
        let mut best: Option<&'a Patch> = None;
        for p in session.values() {
            if p.entries.contains_key(&target)
                && p.region.contains_anchored(&self.axes, point, p.anchor)
                && best.is_none_or(|b| self.compare(p, b) == Ordering::Greater)
            {
                best = Some(p);
            }
        }
        best
    }

    /// Provenance stack for `target` at `point`, strongest first (patches.md §10).
    pub fn patch_stack<'a>(&'a self, session: &'a BTreeMap<PatchId, Patch>, point: &Point, target: PatchTarget) -> Vec<(&'a Patch, &'a PatchValue)> {
        let mut list: Vec<(&'a Patch, &'a PatchValue)> = self
            .patches_for_target(target.node)
            .chain(session.values())
            .filter_map(|p| {
                let v = p.entries.get(&target)?;
                p.region.contains_anchored(&self.axes, point, p.anchor).then_some((p, v))
            })
            .collect();
        list.sort_by(|(a, _), (b, _)| self.compare(b, a));
        list
    }

    /// Symbolic winning region of patch `id`'s entry for `target` (law 3 blast radius).
    pub fn winning_region(&self, id: PatchId, target: PatchTarget) -> Region {
        let Some(p) = self.patches.get(&id).filter(|p| p.entries.contains_key(&target)) else {
            return Region::default();
        };
        let mut out = Region(vec![p.region.clone()]);
        for q in self.patches_for_target(target.node) {
            if q.id != p.id
                && q.entries.contains_key(&target)
                && anchor_covers(q.anchor, p.anchor)
                && self.compare(q, p) == Ordering::Greater
            {
                out = out.minus(&q.region, &self.axes);
            }
        }
        out
    }

    /// Dead authored entries: overridden everywhere they apply by stronger
    /// patches of the same layer (patches.md §6.1).
    pub fn dead(&self) -> Vec<(PatchId, PatchTarget)> {
        let mut out = Vec::new();
        for p in self.patches.values() {
            if p.region.is_empty(&self.axes) {
                continue;
            }
            for &target in p.entries.keys() {
                let mut left = Region(vec![p.region.clone()]);
                for q in self.patches_for_target(target.node) {
                    if q.id != p.id
                        && q.layer == p.layer
                        && q.anchor == p.anchor
                        && q.entries.contains_key(&target)
                        && self.compare(q, p) == Ordering::Greater
                    {
                        left = left.minus(&q.region, &self.axes);
                    }
                }
                if left.is_empty(&self.axes) {
                    out.push((p.id, target));
                }
            }
        }
        out
    }

    /// Specificity ties within a layer on distinct, meeting regions sharing a target (law 5).
    pub fn ties(&self) -> Vec<(PatchId, PatchId)> {
        let ps: Vec<&Patch> = self.patches.values().collect();
        let mut out = Vec::new();
        for (i, p) in ps.iter().enumerate() {
            for q in &ps[i + 1..] {
                if p.layer == q.layer
                    && p.anchor == q.anchor
                    && self.keys(&p.region) == self.keys(&q.region)
                    && !p.region.same(&q.region, &self.axes)
                    && p.region.meets(&q.region, &self.axes)
                    && p.entries.keys().any(|k| q.entries.contains_key(k))
                {
                    out.push((p.id.min(q.id), p.id.max(q.id)));
                }
            }
        }
        out
    }

    /// Inversions within a layer: region strictly within another (as written)
    /// that is weaker (patches.md §6.1).
    pub fn contradictions(&self) -> Vec<(PatchId, PatchId)> {
        let mut out = Vec::new();
        for p in self.patches.values() {
            for q in self.patches.values() {
                let inside = p.anchor == q.anchor
                    && p.region.within(&q.region, &self.axes)
                    && !q.region.within(&p.region, &self.axes);
                if p.id != q.id && p.layer == q.layer && inside && !p.region.void(&self.axes) && self.compare(p, q) == Ordering::Less {
                    out.push((p.id, q.id));
                }
            }
        }
        out
    }

    /// Stale entries (patches.md §5.3, P21): non-empty winning region where
    /// the target node or optional facet is absent at every point of it.
    pub fn stale(&self, is_present: &dyn Fn(&Point, NodeId) -> bool, has_facet: &dyn Fn(&Point, NodeId, FacetKind) -> bool) -> Vec<(PatchId, PatchTarget)> {
        let has_derived_instance = self.axes.values().any(|a| a.kind == AxisKind::Instance && a.values.is_empty());
        let mut out = Vec::new();
        for p in self.patches.values() {
            // Check whether a derived-path `Instance` suffix references a dead authored step.
            let dead_suffix_step = |pt: &Point| -> bool {
                for (a, sel) in &p.region.sels {
                    if self.axes.get(a).is_some_and(|ax| ax.kind == AxisKind::Instance && ax.values.is_empty())
                        && let Sel::Suffix { tail, .. } = sel
                    {
                        for &step in tail {
                            if !step.is_expanded() && !step.is_item_step() && !is_present(pt, step) {
                                return true;
                            }
                        }
                    }
                }
                if let Some(anc) = p.anchor
                    && !is_present(pt, anc)
                {
                    return true;
                }
                false
            };
            for (&target, val) in &p.entries {
                if matches!((target.key, val), (PatchKey::Present | PatchKey::FacetPresent(_), PatchValue::Bool { v: false })) {
                    continue;
                }
                let has_discrete_inst = self.axes.values().any(|ax| ax.kind == AxisKind::Instance && !ax.values.is_empty());
                let uses_derived_inst = (p.anchor.is_some() && !has_discrete_inst)
                    || (has_derived_instance
                        && p.region.sels.keys().any(|a| self.axes.get(a).is_some_and(|ax| ax.kind == AxisKind::Instance && ax.values.is_empty())));
                let points: Vec<Point> = if uses_derived_inst {
                    let wr = self.winning_region(p.id, target);
                    if wr.is_empty(&self.axes) {
                        Vec::new()
                    } else {
                        // Match discrete points on all non-derived-instance axes.
                        self.points()
                            .into_iter()
                            .filter(|x| {
                                wr.0.iter().any(|box_| {
                                    box_.sels.iter().all(|(a, sel)| {
                                        let Some(ax) = self.axes.get(a) else { return false };
                                        if ax.kind == AxisKind::Time || (ax.kind == AxisKind::Instance && ax.values.is_empty()) {
                                            return !sel.void(ax);
                                        }
                                        x.get(*a).is_some_and(|v| sel.contains(ax, v))
                                    })
                                })
                            })
                            .collect()
                    }
                } else if p.anchor.is_some() {
                    self.points()
                        .into_iter()
                        .filter(|x| self.winner_authored(x, target).is_some_and(|w| w.id == p.id))
                        .collect()
                } else {
                    self.winning_region(p.id, target).points(self)
                };
                let absent_at = |x: &Point| {
                    if uses_derived_inst && dead_suffix_step(x) {
                        return true;
                    }
                    if !is_present(x, target.node) {
                        return true;
                    }
                    if let PatchKey::Leaf(lp) = target.key {
                        let fp_target = PatchTarget { node: target.node, key: PatchKey::FacetPresent(lp.facet) };
                        let self_adds_facet = p.entries.get(&fp_target).and_then(|v| v.as_bool()) == Some(true);
                        if self_adds_facet || has_facet(x, target.node, lp.facet) {
                            return false;
                        }
                        // Also check whether another derived-instance patch covering `p`'s
                        // anchor/region enables `FacetPresent(lp.facet)` on the same target (M5).
                        if uses_derived_inst {
                            let peer_adds_facet = self.patches_for_target(target.node).any(|q| {
                                q.layer < SESSION_LAYER
                                    && anchor_covers(q.anchor, p.anchor)
                                    && q.entries.get(&fp_target).and_then(|v| v.as_bool()) == Some(true)
                                    && q.region.meets(&p.region, &self.axes)
                            });
                            if peer_adds_facet {
                                return false;
                            }
                        }
                        return true;
                    }
                    false
                };
                if !points.is_empty() && points.iter().all(absent_at) {
                    out.push((p.id, target));
                }
            }
        }
        out
    }

    /// All four patch lints bundled together (patches.md §7.5).
    pub fn lints(&self, is_present: &dyn Fn(&Point, NodeId) -> bool, has_facet: &dyn Fn(&Point, NodeId, FacetKind) -> bool) -> PatchLints {
        PatchLints {
            dead: self.dead(),
            ties: self.ties(),
            contradictions: self.contradictions(),
            stale: self.stale(is_present, has_facet),
        }
    }
}

// ---------------------------------------------------------------------------
// Structured transaction ops (AxisOp, PatchOp) and edit planners (patches.md §7)
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum AxisOp {
    Add { axis: Axis },
    Remove { id: AxisId },
    SetOrder { order: Vec<AxisId> },
    AddValue { axis: AxisId, at: usize, value: AxisValue },
    RemoveValue { axis: AxisId, id: AxisValueId },
    DeleteValue { axis: AxisId, id: AxisValueId },
    RestoreValue { axis: AxisId, id: AxisValueId },
    MoveValue { axis: AxisId, id: AxisValueId, at: usize },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum PatchOp {
    Add { patch: Patch },
    Remove { id: PatchId },
    SetEntry { patch: PatchId, target: PatchTarget, value: Option<PatchValue> },
    SetRegion { patch: PatchId, region: Product },
    SetRank { patch: PatchId, rank: OrderKey },
}

/// Plan a drag at `point` from a view in `layer` with edit region `edit`
/// (patches.md §7.2, P12, P17): clears any solve-layer entry on that leaf
/// first, respects outer layers and read-only patches, obeys PutGet/GetPut/PutPut.
#[allow(clippy::too_many_arguments)]
pub fn plan_drag(
    table: &PatchTable,
    point: &Point,
    edit: &Product,
    anchor: Option<NodeId>,
    target: PatchTarget,
    value: PatchValue,
    layer: u32,
    node_present: bool,
    facet_present: bool,
    base_value: Option<PatchValue>,
) -> Result<Vec<Op>, String> {
    if !node_present {
        return Err("nothing to drag here".into());
    }
    if !facet_present {
        return Err("facet is absent here".into());
    }
    if !edit.contains_anchored(&table.axes, point, anchor) {
        return Err("the point is outside the edit region".into());
    }
    let dp = point.discrete();
    let mut prefix: Vec<Op> = Vec::new();
    if let PatchKey::Leaf(lp) = target.key {
        let leaf = Leaf { node: target.node, path: lp };
        if layer < SESSION_LAYER && table.solve_get(&dp, leaf).is_some() {
            prefix.push(Op::Solve { point: dp, leaf, slot: None });
        }
    }
    let win = table.winner_authored(point, target);
    let current = win.and_then(|p| p.entries.get(&target).cloned()).or(base_value);
    if current.as_ref() == Some(&value) {
        return Ok(prefix);
    }
    let finish = |mut tail: Vec<Op>| {
        let mut out = prefix;
        out.append(&mut tail);
        out
    };
    let set_entry = |patch: PatchId| vec![Op::Patch(PatchOp::SetEntry { patch, target, value: Some(value.clone()) })];
    let add_patch = |region: Product, rank: Option<OrderKey>| {
        let rank = rank.unwrap_or_else(|| table.default_rank_in(&region, anchor, layer));
        let patch = Patch {
            id: table.next_patch_id(),
            layer,
            rank,
            region,
            anchor,
            writable: true,
            entries: BTreeMap::from([(target, value.clone())]),
        };
        vec![Op::Patch(PatchOp::Add { patch })]
    };
    let Some(win) = win else {
        let reuse = table
            .patches
            .values()
            .filter(|q| q.layer == layer && q.writable && q.anchor == anchor && q.region.same(edit, &table.axes))
            .min_by(|a, b| table.compare(a, b));
        return Ok(finish(match reuse {
            Some(q) => set_entry(q.id),
            None => add_patch(edit.clone(), None),
        }));
    };
    if win.layer > layer || (win.layer == layer && win.anchor.is_none() && anchor.is_some()) {
        return Err("an outer scope sets it here".into());
    }
    if win.layer == layer && win.writable && win.anchor == anchor && win.region.within(edit, &table.axes) {
        return Ok(finish(set_entry(win.id)));
    }
    let region = edit.intersect(&win.region, &table.axes);
    let clear = |q: &Patch| {
        !table.patches_for_target(target.node).any(|r| {
            r.id != q.id
                && r.id != win.id
                && r.entries.contains_key(&target)
                && table.compare(r, win) == Ordering::Greater
                && table.compare(q, r) == Ordering::Greater
                && r.region.meets(&region, &table.axes)
        })
    };
    let reuse = table
        .patches
        .values()
        .filter(|q| {
            q.layer == layer
                && q.writable
                && q.anchor == anchor
                && q.region.same(&region, &table.axes)
                && table.compare(q, win) == Ordering::Greater
                && clear(q)
        })
        .min_by(|a, b| table.compare(a, b));
    Ok(finish(match reuse {
        Some(q) => set_entry(q.id),
        None if win.layer == layer && win.anchor == anchor => add_patch(region, Some(table.rank_above_in(anchor, layer, &win.rank))),
        None => add_patch(region, None),
    }))
}

/// Plan splitting `targets` out of patch `id` into a new patch on `region`
/// (patches.md §7.1). On the same region, the new patch sits just above `id`.
pub fn plan_split(table: &PatchTable, id: PatchId, targets: &[PatchTarget], region: Product) -> Result<Vec<Op>, String> {
    let src = table.patches.get(&id).ok_or_else(|| "no such patch".to_owned())?;
    if !src.writable {
        return Err("a patch that may not be written".into());
    }
    let moving: BTreeMap<PatchTarget, PatchValue> = targets
        .iter()
        .filter_map(|t| src.entries.get(t).map(|v| (*t, v.clone())))
        .collect();
    let rank = if region.same(&src.region, &table.axes) {
        table.rank_above_in(src.anchor, src.layer, &src.rank)
    } else {
        table.default_rank_in(&region, src.anchor, src.layer)
    };
    let new = Patch {
        id: table.next_patch_id(),
        layer: src.layer,
        rank,
        region,
        anchor: src.anchor,
        writable: true,
        entries: moving.clone(),
    };
    let mut ops: Vec<Op> = moving
        .keys()
        .map(|&target| Op::Patch(PatchOp::SetEntry { patch: id, target, value: None }))
        .collect();
    ops.push(Op::Patch(PatchOp::Add { patch: new }));
    Ok(ops)
}

/// Plan merging two patches of the same layer and same region (as written)
/// into the stronger, refusing when an intervening patch conflicts (patches.md §7.1).
pub fn plan_merge(table: &PatchTable, a: PatchId, b: PatchId) -> Result<Vec<Op>, String> {
    if a == b {
        return Err("a patch with itself".into());
    }
    let (Some(pa), Some(pb)) = (table.patches.get(&a), table.patches.get(&b)) else {
        return Err("no such patch".into());
    };
    if !pa.writable || !pb.writable {
        return Err("a patch that may not be written".into());
    }
    if pa.layer != pb.layer || pa.anchor != pb.anchor || !pa.region.same(&pb.region, &table.axes) {
        return Err("their regions differ".into());
    }
    let (strong, weak) = if table.compare(pa, pb) == Ordering::Greater { (pa, pb) } else { (pb, pa) };
    let moving: Vec<(PatchTarget, PatchValue)> = weak
        .entries
        .iter()
        .filter(|(k, _)| !strong.entries.contains_key(*k))
        .map(|(k, v)| (*k, v.clone()))
        .collect();
    for r in table.patches.values() {
        let between = r.anchor == weak.anchor
            && table.compare(r, weak) == Ordering::Greater
            && table.compare(strong, r) == Ordering::Greater;
        if between && r.region.meets(&weak.region, &table.axes) && moving.iter().any(|(k, _)| r.entries.contains_key(k)) {
            return Err(format!("{} lies between them and sets what {} sets", r.id, weak.id));
        }
    }
    let mut ops: Vec<Op> = moving
        .into_iter()
        .map(|(target, val)| Op::Patch(PatchOp::SetEntry { patch: strong.id, target, value: Some(val) }))
        .collect();
    ops.push(Op::Patch(PatchOp::Remove { id: weak.id }));
    Ok(ops)
}

/// Plan reordering patch `id` to `rank` within its `(anchor, layer)` (distinct ranks enforced).
pub fn plan_reorder(table: &PatchTable, id: PatchId, rank: OrderKey) -> Result<Vec<Op>, String> {
    let src = table.patches.get(&id).ok_or_else(|| "no such patch".to_owned())?;
    if table.patches.values().any(|q| q.id != id && q.anchor == src.anchor && q.layer == src.layer && q.rank == rank) {
        return Err("another patch of the layer has that rank".into());
    }
    Ok(vec![Op::Patch(PatchOp::SetRank { patch: id, rank })])
}

/// Plan clearing solve-layer entries for `leaves` at `point` on constraint release (P17).
pub fn plan_release_solve(table: &PatchTable, point: &Point, leaves: &[Leaf]) -> Vec<Op> {
    let dp = point.discrete();
    leaves
        .iter()
        .filter(|l| table.solve_get(&dp, **l).is_some())
        .map(|&leaf| Op::Solve { point: dp.clone(), leaf, slot: None })
        .collect()
}

/// Plan clearing solve-layer entries for `leaves` across ALL discrete points
/// (e.g. when a constraint holding an instance part is deleted, P17).
pub fn plan_release_solve_all(table: &PatchTable, leaves: &[Leaf]) -> Vec<Op> {
    let want: BTreeSet<Leaf> = leaves.iter().copied().collect();
    let mut ops = Vec::new();
    for (dp, entries) in &table.solve {
        for &leaf in entries.keys() {
            if want.contains(&leaf) {
                ops.push(Op::Solve { point: dp.clone(), leaf, slot: None });
            }
        }
    }
    ops
}
