//! The port and empty tools (component-authoring.md §2): a click places a
//! port on the component main under the pointer, or an empty where it
//! lands (inside a main, in its definition). One transaction each; the new
//! node is selected and the select tool is back, as after drawing a shape.

use bevy::prelude::*;
use iron_document::composition::Dims;
use iron_document::{Actor, LeafStruct, NodeId, Op, Slot, TransactionInput, TypeId, Value};
use kurbo::Point;

use crate::bevy_app::pen::{after_last, drop_target};
use crate::bevy_app::select::{ActiveTool, Geo, top_hit};
use crate::document_bridge::{DocumentStore, PanelViews};

/// The outermost main on a hit's chain, if any.
fn main_on(doc: &iron_document::Document, chain: &[NodeId]) -> Option<NodeId> {
    chain.iter().copied().find(|&n| iron_document::expand::main_definition(doc, n).is_some())
}

#[allow(clippy::too_many_arguments)]
pub fn author_tool_system(
    pointer: Res<crate::PointerState>,
    hits: Res<crate::PointerHits>,
    views: Res<PanelViews>,
    mut active: ResMut<ActiveTool>,
    mut sel: ResMut<crate::SelectionState>,
    mut store: ResMut<DocumentStore>,
    spatial: Res<crate::document_bridge::SpatialScene>,
    drawables: Res<crate::document_bridge::Drawables>,
    mut placed: Local<bool>,
) {
    // The select tool takes over on release, not on the press that placed
    // the node: it would read that press as a click and select under it.
    if *placed {
        if pointer.just_released_left || !pointer.buttons.left {
            *placed = false;
            *active = ActiveTool::Select;
        }
        return;
    }
    if !matches!(*active, ActiveTool::Port | ActiveTool::Empty) || !pointer.just_pressed_left {
        return;
    }
    let Some(view) = hits.panel.as_deref().and_then(|p| views.get(p)).filter(|v| v.dims == Dims::Two).cloned() else { return };
    let screen = Point::new(pointer.screen.x as f64, pointer.screen.y as f64);
    let at = view.affine().inverse() * screen;
    let (ops, made, label) = {
        let geo = Geo { store: &store.0, drawables: &drawables, spatial: &spatial };
        let doc = store.0.document();
        let main = top_hit(&hits).and_then(|(chain, _)| main_on(doc, &chain));
        match *active {
            ActiveTool::Port => {
                let Some(main) = main else {
                    info!("port tool: click on a component's main to place a port on it");
                    return;
                };
                let local = geo.world(main).inverse() * at;
                match iron_document::author::add_port(doc, main, [local.x, local.y], "in", false) {
                    Ok((ops, _, anchor)) => (ops, NodeId::expanded(main, anchor), "add port"),
                    Err(e) => {
                        warn!("port tool: {e}");
                        return;
                    }
                }
            }
            _ => {
                let (parent, local) = match main {
                    Some(m) => (m, geo.world(m).inverse() * at),
                    None => match drop_target(&geo, &hits, &view) {
                        Some((c, to_comp)) => (c, to_comp * at),
                        None => return,
                    },
                };
                let id = doc.next_id();
                let mut t = iron_document::facet::Transform2d::default();
                t.x = Slot::Const(Value::Number(local.x));
                t.y = Slot::Const(Value::Number(local.y));
                let mut name = iron_document::facet::Name::default();
                name.text = Slot::Const(Value::Str("empty".into()));
                let order = match main {
                    Some(_) => iron_document::expand::main_template(doc, parent).map_or_else(iron_document::OrderKey::first, |t| after_last(doc, Some(t))),
                    None => after_last(doc, Some(parent)),
                };
                let shown = if main.is_some() { NodeId::expanded(parent, id) } else { Some(id) };
                (vec![Op::Create { id, ty: TypeId::lookup("empty").expect("registered"), parent: Some(parent), order, facets: vec![t.wrap(), name.wrap()] }], shown, "add empty")
            }
        }
    };
    match store.0.apply(TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }) {
        Ok(_) => {
            if let Some(m) = made.filter(|&m| store.0.document().is_live(m)) {
                sel.selected = vec![m];
            }
        }
        Err(errors) => {
            for e in errors {
                warn!("{label} rejected: {e}");
            }
        }
    }
    *placed = true;
}
