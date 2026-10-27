//! Each view's focus context (plans/specs/contexts.md §2.7): the world its
//! tools create into, the container its selection is open in, and its states
//! (`editing-text`, `editing-points`, `main`). What the view offers resolves
//! against it, as context data: its palette, which the shell draws, and its
//! keymap, through which plain letter keys reach tools. No tool or key is
//! named here.

use bevy::prelude::*;
use iron_document::composition::{self, Dims};
use iron_document::contexts::{self, Field, Focus, Resolver};
use iron_document::NodeId;

use crate::bevy_app::select::{PathEdit, SelectTool, ToolKeys};
use crate::bevy_app::text_tool::TextTool;
use crate::document_bridge::{DocumentStore, PanelViews, nest};
use crate::host_events::{EngineEvent, HostEvents};

/// What a view offers: its palette and keymap, as context values.
#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    pub palette: String,
    pub keys: String,
}

#[derive(Resource, Default)]
pub struct ViewFocus {
    /// Each view's focus and what it offers, in the views' order.
    pub views: Vec<(String, Focus, Offer)>,
    /// The view the pointer was last over: plain keys go to it when the
    /// pointer is over none.
    pub last_panel: Option<String>,
    /// What the focus was computed from, so it is recomputed only on change.
    stamp: Option<Stamp>,
}

#[derive(PartialEq)]
struct Stamp {
    version: u64,
    selected: Vec<NodeId>,
    text: Option<(NodeId, String)>,
    points: Option<String>,
    views: Vec<(String, Option<NodeId>)>,
}

/// Turn plain letter keys into tool picks (`tool:<id>`) through the keymap
/// of the view under the pointer (or the last one it was over; before any,
/// the first view that binds the letter). A letter that view does not bind
/// is dropped. Keys with modifiers, and named keys
/// (`escape`, `enter`, …), pass untouched.
pub fn route_tool_keys(hits: Res<crate::PointerHits>, mut focus: ResMut<ViewFocus>, mut keys: ResMut<ToolKeys>) {
    if let Some(p) = &hits.panel
        && focus.last_panel.as_deref() != Some(p.as_str())
    {
        focus.last_panel = Some(p.clone());
    }
    if keys.0.is_empty() {
        return;
    }
    let focus = &*focus;
    let tool_for = |key: &str| -> Option<String> {
        let bound = |o: &Offer| contexts::keymap(&o.keys).into_iter().find(|(k, _)| *k == key).map(|(_, t)| t.to_owned());
        match &focus.last_panel {
            Some(p) => focus.views.iter().find(|(v, ..)| v == p).and_then(|(_, _, o)| bound(o)),
            None => focus.views.iter().find_map(|(_, _, o)| bound(o)),
        }
    };
    keys.0.retain_mut(|(key, m)| {
        if *m != 0 || key.chars().count() != 1 {
            return true;
        }
        match tool_for(key) {
            Some(tool) => {
                *key = format!("tool:{tool}");
                true
            }
            None => false,
        }
    });
}

/// Recompute each view's focus when what it depends on changed, and tell
/// the shell what each view offers when that changed.
pub fn view_focus_system(
    views: Res<PanelViews>,
    store: Res<DocumentStore>,
    sel: Res<crate::SelectionState>,
    select: Res<SelectTool>,
    edit: Res<PathEdit>,
    text: Res<TextTool>,
    mut focus: ResMut<ViewFocus>,
    events: Res<HostEvents>,
) {
    let doc = store.0.document();
    let stamp = Stamp {
        version: doc.version().0,
        selected: sel.selected.clone(),
        text: text.edit.as_ref().map(|e| (e.id, e.panel.clone())),
        points: edit.reach.as_ref().map(|r| r.panel.clone()),
        views: views.0.iter().map(|v| (v.panel.clone(), v.composition)).collect(),
    };
    if focus.stamp.as_ref() == Some(&stamp) {
        return;
    }
    let r = Resolver::new(doc, store.0.contexts());
    let mut next = Vec::new();
    for view in &views.0 {
        let reach = select.reach.values().find(|r| r.panel == view.panel && sel.selected.contains(&r.node()));
        let mut world = view.composition;
        let mut scope: Vec<NodeId> = reach.map(|r| r.chain[..r.chain.len() - 1].to_vec()).unwrap_or_default();
        // Inside a top-level 2D embed, tools create into what it shows
        // (`pen::drop_target`): that is the world.
        if let Some(&first) = scope.first()
            && nest::is_embed(doc, first)
            && let Some(shown) = composition::embedded(doc, first).filter(|&s| composition::dims(doc, s) == Some(Dims::Two))
        {
            world = Some(shown);
            scope.remove(0);
        }
        let mut states = Vec::new();
        if text.edit.as_ref().is_some_and(|e| e.panel == view.panel) {
            states.push("editing-text");
        }
        if edit.reach.as_ref().is_some_and(|r| r.panel == view.panel) {
            states.push("editing-points");
        }
        if world.is_some_and(|w| doc.children(w).iter().any(|&c| iron_document::expand::main_definition(doc, c).is_some())) {
            states.push("main");
        }
        let f = Focus { world, scope, states };
        let v = r.view_values(&f);
        let offer = Offer { palette: v.get(Field::PALETTE).to_owned(), keys: v.get(Field::KEYS).to_owned() };
        if focus.views.iter().find(|(p, ..)| *p == view.panel).is_none_or(|(_, _, o)| *o != offer) {
            events.push(EngineEvent::Focus { panel: view.panel.clone(), palette: offer.palette.clone(), keys: offer.keys.clone() });
        }
        next.push((view.panel.clone(), f, offer));
    }
    focus.views = next;
    focus.stamp = Some(stamp);
}
