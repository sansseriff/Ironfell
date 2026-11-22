//! Composition clocks (handoff-patches.md PT-1.2; reactive-engine.md
//! §10.1). Each composition has a clock, and so does each entity a
//! composition pictures, since a placed instance's model state (its machines,
//! its traces) lives outside every composition. A clock is presentation time
//! less the time it has spent paused. The store's gate (`Store::gate`)
//! pauses the clocks of what no observer shows and nothing running reads,
//! once a frame. A paused clock stands still, so what steps on it (an
//! accumulator, a trace, a machine's transition, an item entering) holds
//! where it was and goes on from there when the clock runs again.
//!
//! A node outside every composition and every pictured entity (the timeline,
//! a loose root, a model node nothing pictures) reads presentation time.

use crate::composition;
use crate::id::NodeId;
use crate::tree::Tree;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default)]
pub(crate) struct Clocks {
    /// The entities some composition pictures: each has its own clock.
    pictured: BTreeSet<NodeId>,
    /// When each paused clock stopped, in presentation time.
    since: crate::work::Counted<BTreeMap<NodeId, f64>, { crate::work::CLOCKS }>,
    /// How long each clock spent paused before its current pause.
    behind: crate::work::Counted<BTreeMap<NodeId, f64>, { crate::work::CLOCKS }>,
    /// The gate's last decision, the clocks to pause and the pictured
    /// entities, and what it was decided from: the document's and the
    /// graph's stamps and the compositions observed. Deciding walks every
    /// hidden composition and its readers (8 ms a frame on the demo with one
    /// composition shown, 2026-11-21), and a frame that changes none of
    /// these, such as a drag's or the playhead moving, decides the same.
    pub(crate) gated: Option<(GateKey, BTreeSet<NodeId>, BTreeSet<NodeId>)>,
    /// How many times the gate decided rather than reused (`Store::gate_decisions`).
    pub(crate) decisions: u64,
}

/// What the gate decides from: `Docs::reshaped`, `Reactive::wired`, and the
/// compositions observed.
pub(crate) type GateKey = (u64, u64, Vec<NodeId>);

impl Clocks {
    /// Pause the clocks in `paused` at `now` and run the others, given the
    /// entities some composition pictures. The clocks of compositions and
    /// entities that are gone are forgotten.
    pub fn set(&mut self, doc: Tree<'_>, now: f64, paused: BTreeSet<NodeId>, pictured: BTreeSet<NodeId>) {
        let resumed: Vec<NodeId> = self.since.keys().filter(|k| !paused.contains(k)).copied().collect();
        for k in resumed {
            let stopped = self.since.remove(&k).unwrap_or(now);
            *self.behind.entry(k).or_default() += (now - stopped).max(0.0);
        }
        for k in paused {
            self.since.entry(k).or_insert(now);
        }
        self.behind.retain(|k, _| doc.is_live(*k));
        self.pictured = pictured;
    }

    /// The clock `node` steps on: its composition, else the pictured entity
    /// it belongs to (itself or its nearest such ancestor), else none.
    fn key(&self, doc: Tree<'_>, node: NodeId) -> Option<NodeId> {
        if self.since.is_empty() && self.behind.is_empty() {
            return None;
        }
        composition::composition_of(doc, node).or_else(|| {
            let mut cur = Some(node);
            while let Some(n) = cur.and_then(|c| doc.live(c)) {
                if self.pictured.contains(&n.id) {
                    return Some(n.id);
                }
                cur = n.parent;
            }
            None
        })
    }

    /// Whether `node`'s clock is paused.
    pub fn held(&self, doc: Tree<'_>, node: NodeId) -> bool {
        !self.since.is_empty() && self.key(doc, node).is_some_and(|k| self.since.contains_key(&k))
    }

    /// `node`'s time at presentation time `now`.
    pub fn time(&self, doc: Tree<'_>, node: NodeId, now: f64) -> f64 {
        let Some(k) = self.key(doc, node) else { return now };
        self.since.get(&k).copied().unwrap_or(now) - self.behind.get(&k).copied().unwrap_or(0.0)
    }
}
