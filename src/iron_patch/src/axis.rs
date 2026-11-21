//! Axes, their values and the points they span (the note's §2.1).

use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AxisId(pub u16);

/// A value on an axis, by a stable id: a region names values by id, so
/// inserting, deleting or moving a value never renames one (note §11.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ValueId(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Slides, iterations: regions are intervals by endpoint ids.
    Ordered,
    /// Machine states, variants: regions are sets.
    Unordered,
    /// Instances: each value is a path of placement steps, outermost first,
    /// and regions are suffix patterns ("part `j` of any `o1`").
    Path,
}

#[derive(Clone, Debug, PartialEq)]
struct Entry {
    id: ValueId,
    live: bool,
    path: Vec<u32>,
}

/// One axis. A deleted value stays as a tombstone in its place, so an
/// interval that names it as an endpoint keeps its meaning: `[C, ∞)` with C
/// deleted still starts where C was (the deleted-endpoint rule, §11.1).
#[derive(Clone, Debug, PartialEq)]
pub struct Axis {
    pub kind: Kind,
    values: Vec<Entry>,
}

impl Axis {
    pub fn new(kind: Kind) -> Self {
        Axis {
            kind,
            values: Vec::new(),
        }
    }

    /// Append a value; `path` only for a `Path` axis.
    pub fn push(&mut self, id: ValueId, path: Vec<u32>) {
        self.values.push(Entry {
            id,
            live: true,
            path,
        });
    }

    pub fn insert(&mut self, at: usize, id: ValueId) {
        let at = at.min(self.values.len());
        self.values.insert(
            at,
            Entry {
                id,
                live: true,
                path: Vec::new(),
            },
        );
    }

    /// Delete a value: no point takes it any more, and it keeps its place.
    pub fn delete(&mut self, id: ValueId) {
        if let Some(e) = self.values.iter_mut().find(|e| e.id == id) {
            e.live = false;
        }
    }

    /// Move a value to position `at` (reordering slides).
    pub fn move_to(&mut self, id: ValueId, at: usize) {
        if let Some(i) = self.values.iter().position(|e| e.id == id) {
            let e = self.values.remove(i);
            self.values.insert(at.min(self.values.len()), e);
        }
    }

    /// Where `id` stands, tombstones counted.
    pub fn position(&self, id: ValueId) -> Option<usize> {
        self.values.iter().position(|e| e.id == id)
    }

    /// The values a point can take, in order.
    pub fn live(&self) -> impl Iterator<Item = ValueId> + '_ {
        self.values.iter().filter(|e| e.live).map(|e| e.id)
    }

    /// Every value, tombstones too, in order.
    pub fn all(&self) -> impl Iterator<Item = ValueId> + '_ {
        self.values.iter().map(|e| e.id)
    }

    pub fn path(&self, id: ValueId) -> &[u32] {
        self.values
            .iter()
            .find(|e| e.id == id)
            .map_or(&[], |e| &e.path)
    }
}

/// One value on every axis in play.
pub type Point = BTreeMap<AxisId, ValueId>;

/// The axes in play, and the order in which they decide strength (law 5).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Space {
    pub axes: BTreeMap<AxisId, Axis>,
    /// Axes in the order they decide, first first. Axes not listed decide
    /// after these, by id.
    pub order: Vec<AxisId>,
}

impl Space {
    /// The axes in deciding order.
    pub fn deciding(&self) -> Vec<AxisId> {
        let mut out: Vec<AxisId> = self
            .order
            .iter()
            .copied()
            .filter(|a| self.axes.contains_key(a))
            .collect();
        let rest: Vec<AxisId> = self
            .axes
            .keys()
            .copied()
            .filter(|a| !out.contains(a))
            .collect();
        out.extend(rest);
        out
    }

    /// Every point: the product of each axis's live values.
    pub fn points(&self) -> Vec<Point> {
        let mut out: Vec<Point> = vec![Point::new()];
        for (&a, axis) in &self.axes {
            let vals: Vec<ValueId> = axis.live().collect();
            out = out
                .into_iter()
                .flat_map(|p| {
                    vals.iter().map(move |&v| {
                        let mut q = p.clone();
                        q.insert(a, v);
                        q
                    })
                })
                .collect();
        }
        out
    }
}
