//! Free-running compositions (plans/specs/free-running.md §2).
//!
//! Producers that change something every frame mark the composition their
//! nodes are in (`LiveSources`). Each frame, every panel's reach through its
//! embeds decides which views render regardless of input (`Liveness`): a
//! panel's 3D camera, a 3D embed's camera, and the window as a whole.
//! `composition::reach` is the rule; this module applies it to the panels
//! and embed views on screen.

use super::{DocumentStore, EmbedViews, PanelViews};
use bevy::prelude::*;
use iron_document::composition;
use iron_document::NodeId;
use std::collections::BTreeSet;

/// Compositions a running producer changes every frame. Producers insert
/// their composition while they run and remove it when they stop.
#[derive(Resource, Default, Debug)]
pub struct LiveSources(pub BTreeSet<NodeId>);

/// What renders every frame, decided once a frame after the embed views.
#[derive(Resource, Default, Debug, PartialEq)]
pub struct Liveness {
    /// Panels whose view is live.
    pub panels: BTreeSet<String>,
    /// `EmbedView::key`s whose camera is live.
    pub embeds: BTreeSet<String>,
    /// Sources some view reaches on an open path: the producers that run.
    pub running: BTreeSet<NodeId>,
}

impl Liveness {
    /// Whether anything on screen renders every frame.
    pub fn any(&self) -> bool {
        !self.panels.is_empty()
    }
}

/// The compositions a panel shows directly: its composition, and for the
/// viewer the compositions its loose embeds show.
fn panel_compositions(doc: &iron_document::Document, view: &super::PanelView) -> Vec<NodeId> {
    let mut out: Vec<NodeId> = view.composition.into_iter().collect();
    if view.loose {
        let loose: Vec<NodeId> = super::nest::panel_roots(doc, view).into_iter().filter(|&r| composition::composition_of(doc, r).is_none()).collect();
        out.extend(super::nest::preorder(doc, &loose).into_iter().filter_map(|n| composition::embedded(doc, n)));
    }
    out
}

pub fn compute_liveness(store: Res<DocumentStore>, views: Res<PanelViews>, embeds: Res<EmbedViews>, sources: Res<LiveSources>, mut out: ResMut<Liveness>) {
    let doc = store.0.document();
    let mut next = Liveness::default();
    for view in &views.0 {
        let reach = composition::reach(doc, &panel_compositions(doc, view), &sources.0);
        if reach.live {
            next.panels.insert(view.panel.clone());
        }
        next.running.extend(reach.running);
    }
    for e in embeds.0.iter().filter(|e| e.open) {
        let reach = composition::reach(doc, &[e.source], &sources.0);
        if reach.live {
            next.embeds.insert(e.key.clone());
        }
    }
    if *out != next {
        *out = next;
    }
}
