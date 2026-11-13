//! Cross-probe highlighting (entity-composition-model.md §7.2).
//!
//! Selection and hover hold the nodes the pointer picked. A picked node
//! that belongs to a figure stands for the figure's entity, so every figure
//! of that entity lights up, in every panel; a node that pictures nothing
//! lights up alone.
//!
//! The sets hold only the lit nodes themselves, never their descendants. In
//! a 2D panel the editor overlay (`bevy_app::select`) draws a lit leaf's
//! outline and a lit group's or embed's frame, as Figma does, so feedback
//! costs the same whatever a group holds. A 2D composition on a plane and a
//! 3D mesh light up under a lit ancestor too ([`Highlights::covers`]).

use super::DocumentStore;
use crate::SelectionState;
use bevy::prelude::*;
use iron_document::composition;
use iron_document::{Document, NodeId};
use std::collections::BTreeSet;

/// Every node highlighted this frame: derived from [`SelectionState`] and
/// the document's `figures` relations, never stored.
#[derive(Resource, Default, Debug, PartialEq)]
pub struct Highlights {
    pub selected: BTreeSet<NodeId>,
    pub hovered: BTreeSet<NodeId>,
}

/// The nodes that light up for `picked`.
pub fn lit(doc: &Document, picked: impl IntoIterator<Item = NodeId>) -> BTreeSet<NodeId> {
    let mut out = BTreeSet::new();
    for node in picked {
        match composition::figure_of(doc, node) {
            Some((_, entity)) => out.extend(composition::figures_of(doc, entity)),
            None => {
                out.insert(node);
            }
        }
        // The timeline (timeline.md §6): a segment stands for its clip, so
        // what the clip animates lights up; a node a clip animates lights
        // its segments, so its rows show in the timeline.
        use iron_document::RelKind;
        for p in doc.relations_from(node).filter(|r| r.rel == RelKind::Projects) {
            out.extend(doc.relations_from(p.to).filter(|r| r.rel == RelKind::Animates && doc.is_live(r.to)).map(|r| r.to));
        }
        for a in doc.relations_to(node).filter(|r| r.rel == RelKind::Animates) {
            out.extend(doc.relations_to(a.from).filter(|r| r.rel == RelKind::Projects && doc.is_live(r.from)).map(|r| r.from));
        }
    }
    out
}

impl Highlights {
    /// `(selected, hovered)` for `id` drawn whole: lit itself or under a lit
    /// ancestor. O(depth); for the few nodes drawn that way (planes, meshes).
    pub fn covers(&self, doc: &Document, id: NodeId) -> (bool, bool) {
        let (mut selected, mut hovered) = (false, false);
        let mut cur = Some(id);
        while let Some(n) = cur {
            selected |= self.selected.contains(&n);
            hovered |= self.hovered.contains(&n);
            cur = doc.live(n).and_then(|n| n.parent);
        }
        (selected, hovered)
    }
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
        // Marked nodes are outlined as the selection is (they are what the
        // selected slices move), without its box or handles.
        selected: lit(doc, selection.selected.iter().chain(selection.marked.iter()).copied().filter(|&n| doc.is_live(n))),
        hovered: lit(doc, selection.hovered.filter(|&n| doc.is_live(n))),
    };
    if *highlights != next {
        *highlights = next;
    }
}
