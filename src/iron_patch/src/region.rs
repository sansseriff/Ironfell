//! Regions: what a patch applies to (the note's §2.1). A patch's region is
//! one `Product`, a selection per axis (note §11.1: one box per patch, since
//! a strength key that extends inclusion exists for boxes and not for
//! unions). Unions (`Region`) appear only as the answers to queries: a
//! winning region, a blast radius.
//!
//! The operations here are symbolic: they never enumerate the product of
//! the axes, only one axis's values at a time. The tests check each against
//! enumeration.

use crate::axis::{Axis, AxisId, Kind, Point, Space, ValueId};
use std::collections::{BTreeMap, BTreeSet};

/// One end of an interval on an ordered axis.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Bound {
    Open,
    Incl(ValueId),
    Excl(ValueId),
}

/// A selection of one axis's values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Sel {
    All,
    /// An interval on an ordered axis, by endpoint ids: `[C, ∞)` is
    /// `Range(Incl(C), Open)`. A value inserted inside it is inside it.
    Range(Bound, Bound),
    Set(BTreeSet<ValueId>),
    /// Every value but these (a cofinite set, which grows as values are
    /// added).
    Not(BTreeSet<ValueId>),
    /// The paths ending in `tail` and in none of `not`.
    Suffix {
        tail: Vec<u32>,
        not: Vec<Vec<u32>>,
    },
}

static ALL: Sel = Sel::All;

/// A threshold on doubled positions: a value at position `i` passes a lower
/// cut `c` when `2i >= c`, and an upper cut when `2i <= c`.
fn lower_cut(axis: &Axis, b: &Bound) -> i64 {
    match b {
        Bound::Open => i64::MIN,
        Bound::Incl(v) => 2 * pos(axis, *v),
        Bound::Excl(v) => 2 * pos(axis, *v) + 1,
    }
}

fn upper_cut(axis: &Axis, b: &Bound) -> i64 {
    match b {
        Bound::Open => i64::MAX,
        Bound::Incl(v) => 2 * pos(axis, *v),
        Bound::Excl(v) => 2 * pos(axis, *v) - 1,
    }
}

/// A value an interval names that is not on the axis lies past its end.
fn pos(axis: &Axis, v: ValueId) -> i64 {
    axis.position(v).map_or(i64::MAX / 4, |p| p as i64)
}

fn ends_with(path: &[u32], tail: &[u32]) -> bool {
    path.len() >= tail.len() && path[path.len() - tail.len()..] == *tail
}

impl Sel {
    pub fn contains(&self, axis: &Axis, v: ValueId) -> bool {
        match self {
            Sel::All => true,
            Sel::Range(lo, hi) => {
                let i = 2 * pos(axis, v);
                i >= lower_cut(axis, lo) && i <= upper_cut(axis, hi)
            }
            Sel::Set(s) => s.contains(&v),
            Sel::Not(s) => !s.contains(&v),
            Sel::Suffix { tail, not } => {
                let p = axis.path(v);
                ends_with(p, tail) && !not.iter().any(|n| ends_with(p, n))
            }
        }
    }

    /// The live values it selects, in axis order.
    pub fn members(&self, axis: &Axis) -> Vec<ValueId> {
        axis.live().filter(|&v| self.contains(axis, v)).collect()
    }

    pub fn is_empty(&self, axis: &Axis) -> bool {
        !axis.live().any(|v| self.contains(axis, v))
    }

    /// `All` in the form its axis's kind combines.
    fn normal(&self, axis: &Axis) -> Sel {
        match (self, axis.kind) {
            (Sel::All, Kind::Ordered) => Sel::Range(Bound::Open, Bound::Open),
            (Sel::All, Kind::Unordered) => Sel::Not(BTreeSet::new()),
            (Sel::All, Kind::Path) => Sel::Suffix {
                tail: Vec::new(),
                not: Vec::new(),
            },
            (s, _) => s.clone(),
        }
    }

    fn materialise(&self, axis: &Axis) -> BTreeSet<ValueId> {
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
            (Sel::Range(a0, a1), Sel::Range(b0, b1)) => {
                let lo = if lower_cut(axis, &a0) >= lower_cut(axis, &b0) {
                    a0
                } else {
                    b0
                };
                let hi = if upper_cut(axis, &a1) <= upper_cut(axis, &b1) {
                    a1
                } else {
                    b1
                };
                Sel::Range(lo, hi)
            }
            (Sel::Set(s), Sel::Set(t)) => Sel::Set(&s & &t),
            (Sel::Set(s), Sel::Not(t)) | (Sel::Not(t), Sel::Set(s)) => Sel::Set(&s - &t),
            (Sel::Not(s), Sel::Not(t)) => Sel::Not(&s | &t),
            (Sel::Suffix { tail: a, not: an }, Sel::Suffix { tail: b, not: bn }) => {
                let tail = if ends_with(&a, &b) {
                    a
                } else if ends_with(&b, &a) {
                    b
                } else {
                    return Sel::Set(BTreeSet::new());
                };
                let mut not = an;
                for n in bn {
                    if !not.contains(&n) {
                        not.push(n);
                    }
                }
                Sel::Suffix { tail, not }
            }
            (a, b) => Sel::Set(&a.materialise(axis) & &b.materialise(axis)),
        }
    }

    /// What of `self` is not in `other`, as disjoint non-empty pieces.
    pub fn minus(&self, other: &Sel, axis: &Axis) -> Vec<Sel> {
        if other.is_empty(axis) {
            return if self.is_empty(axis) {
                Vec::new()
            } else {
                vec![self.clone()]
            };
        }
        let pieces: Vec<Sel> = match (self.normal(axis), other.normal(axis)) {
            (a @ Sel::Range(..), Sel::Range(b0, b1)) => {
                let below = match b0 {
                    Bound::Open => None,
                    Bound::Incl(v) => Some(Sel::Range(Bound::Open, Bound::Excl(v))),
                    Bound::Excl(v) => Some(Sel::Range(Bound::Open, Bound::Incl(v))),
                };
                let above = match b1 {
                    Bound::Open => None,
                    Bound::Incl(v) => Some(Sel::Range(Bound::Excl(v), Bound::Open)),
                    Bound::Excl(v) => Some(Sel::Range(Bound::Incl(v), Bound::Open)),
                };
                [below, above]
                    .into_iter()
                    .flatten()
                    .map(|c| a.intersect(&c, axis))
                    .collect()
            }
            (a @ (Sel::Set(_) | Sel::Not(_)), Sel::Set(t)) => vec![a.intersect(&Sel::Not(t), axis)],
            (a @ (Sel::Set(_) | Sel::Not(_)), Sel::Not(t)) => vec![a.intersect(&Sel::Set(t), axis)],
            (a @ Sel::Suffix { .. }, Sel::Suffix { tail, not }) => {
                // Outside `tail`, then inside each excluded pattern but no
                // earlier one, so the pieces are disjoint.
                let Sel::Suffix { tail: at, not: an } = &a else {
                    unreachable!()
                };
                let mut out = Vec::new();
                let mut outside = an.clone();
                outside.push(tail.clone());
                out.push(Sel::Suffix {
                    tail: at.clone(),
                    not: outside,
                });
                let inside = a.intersect(
                    &Sel::Suffix {
                        tail: tail.clone(),
                        not: Vec::new(),
                    },
                    axis,
                );
                for (i, n) in not.iter().enumerate() {
                    out.push(inside.intersect(
                        &Sel::Suffix {
                            tail: n.clone(),
                            not: not[..i].to_vec(),
                        },
                        axis,
                    ));
                }
                out
            }
            (a, b) => vec![Sel::Set(&a.materialise(axis) - &b.materialise(axis))],
        };
        pieces.into_iter().filter(|p| !p.is_empty(axis)).collect()
    }
}

/// A product of one selection per axis; an axis not named is `All`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Product {
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
        self.sels.get(&axis).unwrap_or(&ALL)
    }

    pub fn contains(&self, space: &Space, point: &Point) -> bool {
        self.sels
            .iter()
            .all(|(a, s)| match (space.axes.get(a), point.get(a)) {
                (Some(axis), Some(&v)) => s.contains(axis, v),
                // An axis not in play selects nothing of it.
                _ => false,
            })
    }

    pub fn is_empty(&self, space: &Space) -> bool {
        space
            .axes
            .iter()
            .any(|(a, axis)| self.sel(*a).is_empty(axis))
            || self.sels.keys().any(|a| !space.axes.contains_key(a))
    }

    pub fn intersect(&self, other: &Product, space: &Space) -> Product {
        let mut out = self.clone();
        for (a, s) in &other.sels {
            let mine = out.sel(*a).clone();
            let joined = match space.axes.get(a) {
                Some(axis) => mine.intersect(s, axis),
                None => s.clone(),
            };
            out = out.with(*a, joined);
        }
        out
    }

    /// What of `self` is not in `other`, as disjoint non-empty products.
    pub fn minus(&self, other: &Product, space: &Space) -> Vec<Product> {
        if self.is_empty(space) {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut rest = self.clone();
        for (a, axis) in &space.axes {
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

    pub fn subset(&self, other: &Product, space: &Space) -> bool {
        self.minus(other, space).is_empty()
    }

    pub fn same(&self, other: &Product, space: &Space) -> bool {
        self.subset(other, space) && other.subset(self, space)
    }

    pub fn points(&self, space: &Space) -> Vec<Point> {
        space
            .points()
            .into_iter()
            .filter(|p| self.contains(space, p))
            .collect()
    }
}

/// A union of products: what a query answers.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Region(pub Vec<Product>);

impl Region {
    pub fn contains(&self, space: &Space, point: &Point) -> bool {
        self.0.iter().any(|p| p.contains(space, point))
    }

    pub fn minus(&self, other: &Product, space: &Space) -> Region {
        Region(self.0.iter().flat_map(|p| p.minus(other, space)).collect())
    }

    pub fn is_empty(&self, space: &Space) -> bool {
        self.0.iter().all(|p| p.is_empty(space))
    }

    pub fn points(&self, space: &Space) -> Vec<Point> {
        space
            .points()
            .into_iter()
            .filter(|p| self.contains(space, p))
            .collect()
    }
}
