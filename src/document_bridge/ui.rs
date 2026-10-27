//! Change metadata for scoped UI reads. No UI snapshot or second document store.
use super::{DocumentOrigin, DocumentStore, GeometryRevision};
use bevy::prelude::*;
use iron_document::{NodeId, Store};
use std::collections::BTreeSet;

#[derive(Resource, Default)]
pub(crate) struct UiInterests {
    pub all: bool,
    pub compositions: bool,
    pub nodes: BTreeSet<NodeId>,
}

#[derive(Resource, Default)]
pub(crate) struct UiChanges {
    nodes: BTreeSet<NodeId>,
    structure: bool,
    compositions: bool,
    previous: Option<(u32, u64, u64, bool, bool, String)>,
}
impl UiChanges {
    pub(crate) fn reset_reader(&mut self) { self.previous = None; }
    pub(super) fn record(&mut self, store: &Store, nodes: &BTreeSet<NodeId>, structure: bool) {
        self.nodes.extend(nodes);
        self.structure |= structure;
        let composition = iron_document::registry::TypeId::lookup("composition");
        self.compositions |= structure || nodes.iter().any(|id| store.document().live(*id).is_some_and(|n| Some(n.ty) == composition));
    }
}
/// Once per frame, after both syncs and all producers. Resolved/session revisions
/// are independent of authored version and do not create transactions.
pub(super) fn publish(
    store: Res<DocumentStore>, geometry: Res<GeometryRevision>, origin: Res<DocumentOrigin>,
    mut changes: ResMut<UiChanges>, interests: Res<UiInterests>, events: Res<crate::host_events::HostEvents>,
) {
    let version = store.0.document().version().0 as u32;
    let session = store.0.session_version();
    let undo = store.0.history().can_undo();
    let redo = store.0.history().can_redo();
    let stamp = (version, session, geometry.0, undo, redo, origin.0.clone());
    if changes.previous.as_ref() == Some(&stamp) && changes.nodes.is_empty() { return; }
    let reset = changes.previous.as_ref().is_none_or(|p| p.5 != origin.0);
    // A live Stats composition must not wake the shell sixty times a second
    // unless a consumer actually reads it. Authored versions and history still
    // notify globally; resolved-only updates follow active read interests.
    let important = reset || changes.structure || changes.previous.as_ref().is_none_or(|p| p.0 != version || p.3 != undo || p.4 != redo)
        || (interests.compositions && changes.compositions)
        || changes.nodes.iter().any(|n| interests.nodes.contains(n) || (interests.all && !n.is_session()));
    if !important {
        changes.nodes.clear(); changes.structure = false; changes.compositions = false;
        changes.previous = Some(stamp);
        return;
    }
    let authored = changes.nodes.iter().any(|n| !n.is_session());
    events.push(crate::host_events::EngineEvent::StateChanged {
        version, session_version: session, resolved_version: geometry.0,
        nodes: std::mem::take(&mut changes.nodes).into_iter().map(|n| n.to_string()).collect(),
        authored,
        structure: std::mem::take(&mut changes.structure),
        compositions: std::mem::take(&mut changes.compositions), reset,
        can_undo: undo, can_redo: redo,
    });
    changes.previous = Some(stamp);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host_events::{EngineEvent, HostEvents};
    #[test]
    fn resolved_revision_and_history_state_do_not_create_transactions() {
        let mut app = App::new();
        app.insert_resource(DocumentStore(Store::new()))
            .init_resource::<GeometryRevision>()
            .init_resource::<DocumentOrigin>()
            .init_resource::<UiChanges>()
            .init_resource::<UiInterests>()
            .init_resource::<HostEvents>()
            .add_systems(Update, publish);
        app.update();
        let first = app.world().resource::<HostEvents>().drain();
        assert!(matches!(&first[..], [EngineEvent::StateChanged { reset: true, can_undo: false, can_redo: false, .. }]));
        let version = app.world().resource::<DocumentStore>().0.document().version();
        let node: NodeId = "#1".parse().unwrap();
        app.world_mut().resource_mut::<UiInterests>().nodes.insert(node);
        app.world_mut().resource_mut::<UiChanges>().nodes.insert(node);
        app.world_mut().resource_mut::<GeometryRevision>().0 += 1;
        app.update();
        let events = app.world().resource::<HostEvents>().drain();
        assert!(matches!(&events[..], [EngineEvent::StateChanged { resolved_version: 1, reset: false, .. }]));
        assert_eq!(app.world().resource::<DocumentStore>().0.document().version(), version);
        assert_eq!(app.world().resource::<DocumentStore>().0.history().log().count(), 0);
        app.update();
        assert!(app.world().resource::<HostEvents>().drain().is_empty());
        app.world_mut().resource_mut::<UiChanges>().reset_reader();
        app.update();
        assert!(matches!(&app.world().resource::<HostEvents>().drain()[..], [EngineEvent::StateChanged { reset: true, .. }]));
        // Live values outside every read interest (for example Stats) stay local.
        app.world_mut().resource_mut::<UiChanges>().nodes.insert("#2".parse().unwrap());
        app.world_mut().resource_mut::<GeometryRevision>().0 += 1;
        app.update();
        assert!(app.world().resource::<HostEvents>().drain().is_empty());
    }
}
