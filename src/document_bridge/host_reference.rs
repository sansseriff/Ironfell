//! The host reference (reactive-engine handoff §4.1, task 6; spec §16.1).
//! The store's differential harness shows whether the store is right; this
//! shows whether the host followed it. A headless Bevy `World` (no window,
//! no GPU) runs the document bridge's own systems, `sync_document` and
//! `prepare_spatial`, step by step through a script, keeping its derived
//! state incrementally as the app does. After every step the same state is
//! rebuilt from scratch from the store (`materialize::reconcile` over every
//! live node, the paint order, a new spatial scene), and the two must be
//! equal: the drawables, which nodes have entities and what those carry,
//! the paint order and the spatial index. Compared by document `NodeId`,
//! geometry and content, never by entity or asset handle.
//!
//!   cargo test -p ironfell --lib --features native --target aarch64-apple-darwin host_reference

use super::materialize::{self, Doc2d, Drawables, MeshAssets, MeshColor, Modes};
use super::*;
use bevy::ecs::system::{RunSystemOnce, SystemId};
use iron_document::{Actor, Value};

/// A headless host: the bridge's resources and systems, nothing drawn.
struct Host {
    world: World,
    sync: SystemId,
    spatial: SystemId,
    steps: usize,
}

fn mesh_world() -> World {
    let mut world = World::new();
    world.init_resource::<Assets<Mesh>>();
    world.init_resource::<Assets<Image>>();
    world.init_resource::<Assets<StandardMaterial>>();
    world.run_system_once(materialize::setup_mesh_assets).expect("mesh assets");
    world
}

/// What the host derives from the store, by node.
#[derive(Debug, PartialEq)]
struct Derived {
    drawables: BTreeMap<NodeId, Doc2d>,
    paint_index: BTreeMap<NodeId, u32>,
    /// Each node with an entity, and what the entity carries.
    entities: BTreeMap<NodeId, Carried>,
}

/// An entity's document-derived components. Render layers are left out:
/// a composition's layer number is assigned on first use, so a host built
/// in another order numbers them differently, and the composition is
/// compared instead (`InComposition`).
#[derive(Debug, PartialEq)]
struct Carried {
    name: Option<String>,
    composition: Option<Option<NodeId>>,
    doc2d: Option<Doc2d>,
    transform: Option<Transform>,
    global: Option<GlobalTransform>,
    color: Option<MeshColor>,
    visibility: Option<Visibility>,
    mesh_3d: bool,
}

fn derived(world: &World) -> Derived {
    let map = world.resource::<NodeMap>();
    let mut entities = BTreeMap::new();
    for id in map.ids() {
        let e = map.entity(id).expect("mapped");
        let Ok(r) = world.get_entity(e) else {
            entities.insert(id, Carried { name: Some("<despawned>".into()), composition: None, doc2d: None, transform: None, global: None, color: None, visibility: None, mesh_3d: false });
            continue;
        };
        assert_eq!(r.get::<Provenance>().map(|p| p.0), Some(id), "an entity names its node");
        entities.insert(
            id,
            Carried {
                name: r.get::<Name>().map(|n| n.as_str().to_owned()),
                composition: r.get::<InComposition>().map(|c| c.0),
                doc2d: r.get::<Doc2d>().cloned(),
                transform: r.get::<Transform>().copied(),
                global: r.get::<GlobalTransform>().copied(),
                color: r.get::<MeshColor>().copied(),
                visibility: r.get::<Visibility>().copied(),
                mesh_3d: r.contains::<Mesh3d>(),
            },
        );
    }
    Derived {
        drawables: world.resource::<Drawables>().0.iter().map(|(k, v)| (*k, v.clone())).collect(),
        paint_index: map.ids().filter_map(|id| Some((id, map.paint_index(id)?))).collect(),
        entities,
    }
}

/// The first difference between two `Derived`, named.
fn first_difference(a: &Derived, b: &Derived) -> Option<String> {
    fn map<V: PartialEq + std::fmt::Debug>(what: &str, a: &BTreeMap<NodeId, V>, b: &BTreeMap<NodeId, V>) -> Option<String> {
        for (k, x) in a {
            match b.get(k) {
                None => return Some(format!("{what} {k}: only the incremental host has {x:?}")),
                Some(y) if x != y => return Some(format!("{what} {k}:\n  incremental: {x:?}\n  rebuilt:     {y:?}")),
                _ => {}
            }
        }
        b.iter().find(|(k, _)| !a.contains_key(k)).map(|(k, y)| format!("{what} {k}: only the rebuilt host has {y:?}"))
    }
    map("drawable", &a.drawables, &b.drawables).or_else(|| map("paint index", &a.paint_index, &b.paint_index)).or_else(|| map("entity", &a.entities, &b.entities))
}

/// The host's state rebuilt from scratch from `store`, in a world of its own.
fn rebuild(store: Store, modes: &Modes) -> (Store, Derived, iron_spatial::Scene) {
    let mut world = mesh_world();
    world.insert_resource(DocumentStore(store));
    world.init_resource::<NodeMap>();
    world.init_resource::<CompositionLayers>();
    world.init_resource::<Drawables>();
    world.insert_resource(Modes { live: modes.live.clone() });
    world
        .run_system_once(|store: Res<DocumentStore>, mut map: ResMut<NodeMap>, mut layers: ResMut<CompositionLayers>, mut commands: Commands, assets: Res<MeshAssets>, mut drawables: ResMut<Drawables>, modes: Res<Modes>| {
            let all: BTreeSet<NodeId> = store.0.tree().live_nodes().map(|n| n.id).collect();
            materialize::reconcile(&store.0, &all, &mut map, &mut layers, &mut commands, &assets, &mut drawables, &modes);
            map.paint_index = paint_order(&store.0);
        })
        .expect("rebuild");
    world.flush();
    let derived = derived(&world);
    let store = world.remove_resource::<DocumentStore>().expect("store").0;
    // A new scene builds from the store alone.
    let mut scene = iron_spatial::Scene::default();
    scene.update(&store, Default::default());
    (store, derived, scene)
}

impl Host {
    fn new() -> Host {
        let mut world = mesh_world();
        world.insert_resource(DocumentStore(Store::new()));
        world.init_resource::<crate::host_events::HostEvents>();
        world.init_resource::<PendingTransactions>();
        world.init_resource::<HistoryRequests>();
        world.init_resource::<PendingLoad>();
        world.init_resource::<GestureRequests>();
        world.init_resource::<SyncedVersion>();
        world.init_resource::<NodeMap>();
        world.init_resource::<DocumentChanges>();
        world.init_resource::<GeometryRevision>();
        world.init_resource::<CompositionLayers>();
        world.init_resource::<Time>();
        world.init_resource::<DocumentOrigin>();
        world.init_resource::<Drawables>();
        world.init_resource::<Modes>();
        world.init_resource::<ui::UiChanges>();
        world.insert_resource(ClearColor(crate::bevy_app::CANVAS_LIGHT));
        world.init_resource::<LatencyRequested>();
        world.init_resource::<SpatialScene>();
        world.init_resource::<ChangeInbox>();
        let sync = world.register_system(sync_document);
        let spatial = world.register_system(spatial::prepare_spatial);
        let mut host = Host { world, sync, spatial, steps: 0 };
        host.frame("start");
        host
    }

    fn store(&mut self) -> Mut<'_, DocumentStore> {
        self.world.resource_mut::<DocumentStore>()
    }

    fn doc_named(&self, name: &str) -> NodeId {
        let doc = self.world.resource::<DocumentStore>().0.tree();
        doc.live_nodes()
            .find(|n| !n.id.is_expanded() && doc.get::<iron_document::facet::Name>(n.id).and_then(|x| x.text.constant()).and_then(|v| v.as_str()) == Some(name))
            .unwrap_or_else(|| panic!("no node named {name}"))
            .id
    }

    /// One frame of the bridge, then the comparison with a rebuilt host.
    fn frame(&mut self, step: &str) {
        self.world.run_system(self.sync).expect("sync_document");
        self.world.flush();
        self.world.run_system(self.spatial).expect("prepare_spatial");
        self.steps += 1;
        let incremental = derived(&self.world);
        let store = std::mem::replace(&mut self.store().0, Store::new());
        let modes = Modes { live: self.world.resource::<Modes>().live.clone() };
        let (store, rebuilt, scene) = rebuild(store, &modes);
        self.store().0 = store;
        if let Some(d) = first_difference(&incremental, &rebuilt) {
            panic!("host reference: after `{step}` the incremental host differs from a rebuilt one at {d}");
        }
        if let Some(d) = self.world.resource::<SpatialScene>().diff(&scene) {
            panic!("host reference: after `{step}` the spatial scene differs from a rebuilt one at {d}");
        }
    }

    fn apply(&mut self, step: &str, xml: &str) {
        let plan = iron_document::edit::plan(&self.store().0, &format!("<edit>{xml}</edit>"), None).unwrap_or_else(|f| panic!("{step}: {}", f.message()));
        self.world.resource_mut::<PendingTransactions>().push(step, Actor::Human, plan.ops);
        self.frame(step);
    }
}

fn path(s: &str) -> iron_document::LeafPath {
    s.parse().unwrap()
}

/// The fast smoke's document operations as store calls (smoke.txt): the
/// demo loaded, the square dragged in previews and committed, undo, redo,
/// an edit refused whole, the saved text loaded back.
#[test]
fn host_reference_follows_the_smoke() {
    let mut host = Host::new();
    let text = template_snapshot("demo").unwrap();
    host.world.resource_mut::<PendingLoad>().0 = Some(Store::load(&text).unwrap());
    host.frame("load the demo");
    let square = host.doc_named("square");
    let x0 = host.store().0.resolved(square, path("transform2d.x")).and_then(|v| v.as_f64()).unwrap();
    host.store().0.begin_gesture(&[square]);
    for k in 1..=6 {
        let x = x0 + 20.0 * k as f64;
        host.store().0.preview(square, path("transform2d.x"), Value::Number(x));
        host.store().0.preview(square, path("transform2d.y"), Value::Number(420.0 - 13.0 * k as f64));
        host.frame("drag the square");
    }
    host.world.resource_mut::<GestureRequests>().0.push(GestureRequest::Commit("move square".into()));
    host.frame("release");
    host.world.resource_mut::<HistoryRequests>().0.push(HistoryOp::Undo);
    host.frame("undo");
    host.world.resource_mut::<HistoryRequests>().0.push(HistoryOp::Redo);
    host.frame("redo");
    let before = host.store().0.save();
    let refused = iron_document::edit::plan(&host.store().0, r##"<edit><set id="#zz" x="1"/></edit>"##, None);
    if let Ok(plan) = refused {
        host.world.resource_mut::<PendingTransactions>().push("bad", Actor::Human, plan.ops);
    }
    host.frame("a refused edit");
    assert_eq!(host.store().0.save(), before, "nothing applied");
    host.world.resource_mut::<PendingLoad>().0 = Some(Store::load(&before).unwrap());
    host.frame("reload");
    assert_eq!(host.store().0.save(), before);
}

/// Structure that values drive, and structure that ops drive: repeats
/// growing, shrinking and re-keying, a conditional, instances placed and
/// deleted, a plot panned so its ticks come and go, a preview cancelled.
#[test]
fn host_reference_follows_derived_structure() {
    let mut host = Host::new();
    host.apply("sheet", r#"<model name="model"/><composition id="$c" name="sheet" dims="2d"/>"#);
    let c = host.doc_named("sheet");
    host.apply(
        "pieces",
        &format!(
            r#"<let in="{c}" id="$lo" name="lo" value="0"/>
            <group in="{c}" id="$t" name="ticks" repeat.each="{{= range($lo, $lo + 20, 4)}}" repeat.key="{{= t => t}}"><text name="tick" x="{{= $t.repeat.item * 3}}" y="0" content="{{= fmt($t.repeat.item)}}"/></group>
            <let in="{c}" id="$names" name="names" value="{{= list('a', 'b', 'c')}}"/>
            <group in="{c}" id="$rows" name="rows" y="40" repeat.each="{{= $names}}"><rect name="row" x="0" y="{{= $rows.repeat.index * 12}}" w="10" h="10"/></group>
            <let in="{c}" id="$open" name="open" value="false"/>
            <group in="{c}" name="body" when="{{= $open}}"><rect name="panel" w="10" h="10"/></group>
            <place def="std:plot" in="{c}" id="$plot" x="300" y="0" w="300" h="200" xs="0 1 2" ys="{{= $self.xs * 2}}" x_min="0" x_max="2"/>"#
        ),
    );
    let (lo, names, open) = (host.doc_named("lo"), host.doc_named("names"), host.doc_named("open"));
    let set = |host: &mut Host, step: &str, id: NodeId, leaf: &str, slot: iron_document::Slot| {
        host.world.resource_mut::<PendingTransactions>().push(step, Actor::Human, vec![Op::Set { id, path: path(leaf), slot }]);
        host.frame(step);
    };
    let bound = |e: &str| iron_document::Slot::Bound(iron_document::Expr::parse(e).unwrap());
    for v in [3.0, 7.5, 12.0, 0.0] {
        set(&mut host, "slide the ticks", lo, "let.value", iron_document::Slot::Const(Value::Number(v)));
    }
    for e in ["list('b', 'c', 'd')", "list('d')", "list(1, 2, 3)", "list()", "list('a', 'b')"] {
        set(&mut host, "change the rows", names, "let.value", bound(e));
    }
    // The same items in another order: copies move, none made or removed.
    for e in ["list('a', 'b', 'c')", "list('c', 'a', 'b')", "list('b', 'c', 'a')"] {
        set(&mut host, "reorder the rows", names, "let.value", bound(e));
    }
    for op in [HistoryOp::Undo, HistoryOp::Undo, HistoryOp::Redo] {
        host.world.resource_mut::<HistoryRequests>().0.push(op);
        host.frame("rows history");
    }
    set(&mut host, "open", open, "let.value", iron_document::Slot::Const(Value::Bool(true)));
    set(&mut host, "close", open, "let.value", iron_document::Slot::Const(Value::Bool(false)));
    // Pan the plot: its ticks come and go outside any transaction.
    let area = {
        let doc = host.world.resource::<DocumentStore>().0.tree();
        let plot = doc.roots().iter().flat_map(|&r| doc.descendants(r)).find(|&d| doc.live(d).is_some_and(|n| n.ty.name() == "use")).unwrap();
        doc.descendants(plot)
            .into_iter()
            .find(|&d| doc.is_live(d) && !iron_document::expand::in_repeat_template(doc, d) && doc.get::<iron_document::facet::Name>(d).and_then(|x| x.text.constant()).and_then(|v| v.as_str()) == Some("plot area"))
            .unwrap()
    };
    for k in 0..12 {
        host.store().0.pointer_action(area, "drag", [-35.0, 0.0, 100.0 - 35.0 * k as f64, 50.0, 0.0]).unwrap();
        host.frame("pan the plot");
    }
    host.world.resource_mut::<GestureRequests>().0.push(GestureRequest::Commit("pan".into()));
    host.frame("commit the pan");
    // A preview, cancelled.
    host.store().0.begin_gesture(&[lo]);
    host.store().0.preview(lo, path("let.value"), Value::Number(9.0));
    host.frame("preview");
    host.world.resource_mut::<GestureRequests>().0.push(GestureRequest::Cancel);
    host.frame("cancel");
    // Instances placed and deleted, and undone.
    host.apply("place", &format!(r#"<place def="std:meter" in="{c}" x="0" y="300"/><place def="std:laser" in="{c}" x="200" y="300"/>"#));
    let plot = host.world.resource::<DocumentStore>().0.tree().children(c).iter().copied().find(|&n| host.world.resource::<DocumentStore>().0.tree().live(n).is_some_and(|x| x.ty.name() == "use")).unwrap();
    // An instance moved to the front of its siblings, its copies with it.
    let front = {
        let doc = host.world.resource::<DocumentStore>().0.tree();
        let first = doc.children(c).iter().filter_map(|&k| doc.live(k)).map(|n| n.order.clone()).min().unwrap();
        iron_document::OrderKey::before(&first)
    };
    host.world.resource_mut::<PendingTransactions>().push("raise the plot", Actor::Human, vec![Op::Reorder { id: plot, order: front }]);
    host.frame("raise the plot");
    host.world.resource_mut::<HistoryRequests>().0.push(HistoryOp::Undo);
    host.frame("undo the raise");
    host.world.resource_mut::<PendingTransactions>().push("delete the plot", Actor::Human, vec![Op::Delete { id: plot }]);
    host.frame("delete the plot");
    for op in [HistoryOp::Undo, HistoryOp::Undo, HistoryOp::Redo] {
        host.world.resource_mut::<HistoryRequests>().0.push(op);
        host.frame("history");
    }
    assert!(host.steps > 30, "{} steps", host.steps);
}
