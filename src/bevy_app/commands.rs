//! The engine's half of the command registry (iron_document `commands.rs`):
//! what a command does to the selection, as one transaction planned from the
//! store, and the right-click menu, resolved from the view's `menu.canvas`.
//!
//! A command that makes one transaction is a [`plan`]: run, it is applied
//! (`select::select_tool_system`, in key order); offered in a menu, the same
//! plan is made and dropped, so a disabled item says exactly why running it
//! would fail. Commands that are not one transaction (copy, select all, the
//! zoom keys, undo) are handled where their state lives, by id.

use bevy::prelude::*;
use iron_document::commands::{self, Clip, Runs};
use iron_document::{Actor, NodeId, Op, Store, TransactionInput};

use crate::document_bridge::{DocumentStore, PanelViews};
use crate::host_events::{EngineEvent, HostEvents, MenuItem};

/// A command's transaction, and what to select once it has applied.
pub struct Plan {
    pub input: TransactionInput,
    pub select: Vec<NodeId>,
}

/// What a command acts on: the selected nodes as the select tool reaches
/// them, and whether the view it is asked from is 2D.
pub struct Situation<'a> {
    pub store: &'a Store,
    pub selected: &'a [NodeId],
    pub two_d: bool,
}

fn transaction(label: impl Into<String>, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

/// The transaction command `id` would make now: `None` for a command that is
/// not planned (it is handled elsewhere), else the plan or why there is none.
pub fn plan(id: &str, s: &Situation) -> Option<Result<Plan, String>> {
    let needs = |ok: Result<Plan, String>| if s.selected.is_empty() { Err("Select something first".to_owned()) } else { ok };
    let doc = s.store.document();
    let extent = |n: NodeId| s.store.extent(n);
    Some(match id {
        // Add auto layout (layout-and-tables.md §2.4): a group gets a layout;
        // siblings are wrapped in a laid-out group.
        "layout" if !s.two_d => Err("Auto layout is for 2D views".into()),
        "layout" => needs(iron_document::layout::add_ops(doc, &extent, s.selected).map(|(ops, group)| Plan { input: transaction("add auto layout", ops), select: vec![group] })),
        // One linked instance of a new definition takes the selection's place
        // (component-model.md §5.5).
        "make-component" => needs(s.store.make_component(s.selected, "component", Actor::Human).map(|(made, input)| Plan { input, select: vec![made.picture] })),
        "front" => needs(commands::restack_ops(doc, s.selected, true).map(|ops| Plan { input: transaction("bring to front", ops), select: Vec::new() })),
        "back" => needs(commands::restack_ops(doc, s.selected, false).map(|ops| Plan { input: transaction("send to back", ops), select: Vec::new() })),
        "draw-on" => match s.selected {
            [one] => iron_document::timeline_view::draw_on_ops(s.store, *one, 1.0).map(|ops| Plan { input: transaction("draw on", ops), select: Vec::new() }),
            [] => Err("Select a shape first".into()),
            _ => Err("Select one shape".into()),
        },
        _ => return None,
    })
}

/// Whether command `id` can run now, or why not (a menu shows it disabled
/// with the reason).
pub fn availability(id: &str, s: &Situation) -> Result<(), String> {
    let Some(c) = commands::find(id) else { return Err(format!("unknown command {id}")) };
    if let Some(planned) = plan(id, s) {
        return planned.map(|_| ());
    }
    if c.selection && s.selected.is_empty() {
        return Err("Select something first".into());
    }
    match id {
        "create-control" if !s.two_d => Err("Controls need a 2D canvas".into()),
        "create-control" => iron_document::author::create_control(s.store, s.selected, None).map(|_| ()),
        "place-pivot" => match s.selected {
            [id] if s.two_d => iron_document::author::relocate_pivot(s.store, *id, s.store.local_affine(*id).translation()).map(|_| ()),
            _ => Err("Select one control in a 2D view".into()),
        },
        "undo" if !s.store.history().can_undo() => Err("Nothing to undo".into()),
        "redo" if !s.store.history().can_redo() => Err("Nothing to redo".into()),
        _ => Ok(()),
    }
}

/// A right-click asked for the menu of this view (`select` sets it once the
/// click has selected what it was on).
#[derive(Resource, Default)]
pub struct MenuRequest(pub Option<String>);

/// Answer a right-click: the view's `menu.canvas`, each command offered as
/// it can run now, with its chord from the view's keymap.
pub fn menu_system(
    mut request: ResMut<MenuRequest>,
    focus: Res<crate::bevy_app::view_focus::ViewFocus>,
    store: Res<DocumentStore>,
    sel: Res<crate::SelectionState>,
    tool: Res<crate::bevy_app::select::SelectTool>,
    views: Res<PanelViews>,
    events: Res<HostEvents>,
) {
    let Some(panel) = request.0.take() else { return };
    let Some(offer) = focus.offer(&panel) else {
        events.push(EngineEvent::Menu { panel, items: Vec::new() });
        return;
    };
    // What the select tool reaches in this view, as its keys act on it.
    let selected: Vec<NodeId> = sel.selected.iter().copied().filter(|id| tool.reach.get(id).is_some_and(|r| r.panel == panel)).collect();
    let s = Situation { store: &store.0, selected: &selected, two_d: views.get(&panel).is_some_and(|v| v.dims == iron_document::composition::Dims::Two) };
    let keymap = commands::keymap(&offer.commands);
    let items = offer
        .menu
        .split_whitespace()
        .map(|w| {
            let c = commands::find(w)?;
            let why = availability(c.id, &s).err().unwrap_or_default();
            Some(MenuItem {
                id: c.id.into(),
                label: c.label.into(),
                icon: c.icon.into(),
                chord: keymap.iter().find(|(_, id)| *id == c.id).map(|(k, _)| k.clone()).unwrap_or_default(),
                enabled: why.is_empty(),
                reason: why,
                clip: match c.clip { Clip::None => "none", Clip::Write => "write", Clip::Read => "read" }.into(),
                runs: match c.runs { Runs::Engine => "engine", Runs::Shell => "shell" }.into(),
            })
        })
        .collect();
    events.push(EngineEvent::Menu { panel, items });
}
