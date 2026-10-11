//! Cross-probe highlighting (entity-composition-model.md §7.2).
//!
//! Selection and hover hold the nodes the pointer picked. A picked node
//! that belongs to a figure stands for the figure's entity, so every figure
//! of that entity lights up, in every panel; a node that pictures nothing
//! lights up alone. A figure that is a group lights up as all of its
//! drawn descendants.

use super::DocumentStore;
use crate::SelectionState;
use bevy::prelude::*;
use iron_document::composition;
use iron_document::{Document, NodeId};
use std::collections::BTreeSet;

/// Every node drawn highlighted this frame: derived from [`SelectionState`]
/// and the document's `figures` relations, never stored.
#[derive(Resource, Default, Debug, PartialEq)]
pub struct Highlights {
    pub selected: BTreeSet<NodeId>,
    pub hovered: BTreeSet<NodeId>,
}

/// The nodes that light up for `picked`.
pub fn lit(doc: &Document, picked: impl IntoIterator<Item = NodeId>) -> BTreeSet<NodeId> {
    let mut out = BTreeSet::new();
    let mut add = |n: NodeId, out: &mut BTreeSet<NodeId>| {
        out.insert(n);
        out.extend(doc.descendants(n));
    };
    for node in picked {
        match composition::figure_of(doc, node) {
            Some((_, entity)) => {
                for f in composition::figures_of(doc, entity) {
                    add(f, &mut out);
                }
            }
            None => add(node, &mut out),
        }
    }
    out
}

/// Recompute when the selection changes or the document does (a link or an
/// unlink changes what lights up together). Written only on change, so the
/// 2D layer's "selection" redraw means something lit differently.
pub fn update_highlights(
    selection: Res<SelectionState>,
    store: Res<DocumentStore>,
    mut highlights: ResMut<Highlights>,
    mut seen: Local<Option<iron_document::Version>>,
) {
    let version = store.0.document().version();
    if !selection.is_changed() && *seen == Some(version) {
        return;
    }
    *seen = Some(version);
    let doc = store.0.document();
    let next = Highlights {
        selected: lit(doc, selection.selected.iter().copied().filter(|&n| doc.is_live(n))),
        hovered: lit(doc, selection.hovered.filter(|&n| doc.is_live(n))),
    };
    if *highlights != next {
        *highlights = next;
    }
}
