//! Embedded compositions (ECM §7.4): one composition shown inside another,
//! by live reference; loops refused however they would form.

use iron_document::component::{Camera, Composition, Embed, EmbedCamera, Name, Size, Transform2d, Transform3d};
use iron_document::composition;
use iron_document::*;

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn named(text: &str) -> Component {
    let mut n = Name::default();
    n.text = Slot::Const(Value::Str(text.into()));
    n.wrap()
}

fn comp(name: &str, dims: &str) -> Vec<Component> {
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum(dims.into()));
    vec![named(name), c.wrap()]
}

fn embed_of(c: NodeId) -> Component {
    let mut e = Embed::default();
    e.source = Slot::Const(Value::Ref(c));
    e.wrap()
}

struct Three {
    store: Store,
    main: NodeId,
    scene: NodeId,
    assembly: NodeId,
    camera: NodeId,
}

/// main (2d), scene (2d) { rect }, assembly (3d) { camera }.
fn three() -> Three {
    let mut store = Store::new();
    let ids = store.document().next_ids(5);
    let (main, scene, rect, assembly, camera) = (ids[0], ids[1], ids[2], ids[3], ids[4]);
    let k = OrderKey::first();
    let k1 = OrderKey::after(&k);
    let k2 = OrderKey::after(&k1);
    store
        .apply(tx(
            "setup",
            vec![
                Op::Create { id: main, ty: ty("composition"), parent: None, order: k.clone(), components: comp("main", "2d") },
                Op::Create { id: scene, ty: ty("composition"), parent: None, order: k1.clone(), components: comp("scene", "2d") },
                Op::Create { id: rect, ty: ty("rect"), parent: Some(scene), order: k.clone(), components: vec![Transform2d::default().wrap(), Size::default().wrap()] },
                Op::Create { id: assembly, ty: ty("composition"), parent: None, order: k2, components: comp("assembly", "3d") },
                Op::Create { id: camera, ty: ty("camera"), parent: Some(assembly), order: k, components: vec![Transform3d::default().wrap(), Camera::default().wrap()] },
            ],
        ))
        .unwrap();
    Three { store, main, scene, assembly, camera }
}

fn add_embed(store: &mut Store, parent: NodeId, of: NodeId, extra: Vec<Component>) -> Result<NodeId, Vec<ApplyError>> {
    let id = store.document().next_id();
    let order = store.document().children(parent).last().map(|&c| OrderKey::after(&store.document().live(c).unwrap().order)).unwrap_or_else(OrderKey::first);
    let mut comps = vec![embed_of(of), Transform2d::default().wrap()];
    comps.extend(extra);
    store.apply(tx("embed", vec![Op::Create { id, ty: ty("embed"), parent: Some(parent), order, components: comps }]))?;
    Ok(id)
}

#[test]
fn a_composition_embeds_others_by_reference() {
    let mut t = three();
    let e1 = add_embed(&mut t.store, t.main, t.scene, vec![]).unwrap();
    let mut cam = EmbedCamera::default();
    cam.camera = Slot::Const(Value::Ref(t.camera));
    let e2 = add_embed(&mut t.store, t.main, t.assembly, vec![cam.wrap()]).unwrap();
    let doc = t.store.document();
    assert_eq!(composition::embedded(doc, e1), Some(t.scene));
    assert_eq!(composition::embedded(doc, e2), Some(t.assembly));
    assert_eq!(composition::embed_camera(doc, e2), Some(t.camera));
    assert_eq!(composition::embed_camera(doc, e1), None);
    assert_eq!(composition::embeds_in(doc, t.main), vec![e1, e2]);
    // Embedding is not owning: the scene is still a root, and one
    // composition may be embedded twice.
    assert!(doc.roots().contains(&t.scene));
    add_embed(&mut t.store, t.main, t.scene, vec![]).unwrap();
    // The view shows the reference and the camera.
    let v = t.store.view(&ViewQuery { relation: ViewRelation::Composition(t.main), scope: None, fidelity: Fidelity::Full, depth: None }).unwrap();
    assert!(v.contains(&format!(r#"source="{}" camera="{}"/>"#, t.assembly, t.camera)), "{v}");
    // Round trip.
    let text = t.store.save();
    assert_eq!(Store::load(&text).unwrap().save(), text);
}

#[test]
fn loops_are_refused_however_they_would_form() {
    let mut t = three();
    // Directly: main inside main.
    assert_eq!(add_embed(&mut t.store, t.main, t.main, vec![]).unwrap_err()[0].kind, ErrorKind::Cycle);
    // Transitively: main shows scene; scene showing main would loop.
    add_embed(&mut t.store, t.main, t.scene, vec![]).unwrap();
    let err = add_embed(&mut t.store, t.scene, t.main, vec![]).unwrap_err();
    assert_eq!(err[0].kind, ErrorKind::Cycle);
    assert!(err[0].message.contains("loop"), "{err:?}");
    // By a set: an embed in scene of assembly, then retargeted to main.
    let e = add_embed(&mut t.store, t.scene, t.assembly, vec![]).unwrap();
    let path: LeafPath = "embed.source".parse().unwrap();
    let err = t.store.apply(tx("retarget", vec![Op::Set { id: e, path, slot: Slot::Const(Value::Ref(t.main)) }])).unwrap_err();
    assert_eq!(err[0].kind, ErrorKind::Cycle);
    // By a move: an embed of main, built in a composition nothing shows,
    // then moved into scene (which main shows).
    let spare = t.store.document().next_id();
    t.store.apply(tx("spare", vec![Op::Create { id: spare, ty: ty("composition"), parent: None, order: OrderKey::after(&t.store.document().live(t.assembly).unwrap().order), components: comp("spare", "2d") }])).unwrap();
    let em = add_embed(&mut t.store, spare, t.main, vec![]).unwrap();
    let err = t.store.apply(tx("move", vec![Op::Reparent { id: em, parent: Some(t.scene), order: OrderKey::first() }])).unwrap_err();
    assert_eq!(err[0].kind, ErrorKind::Cycle);
    // Not a composition, and a camera from elsewhere.
    assert!(add_embed(&mut t.store, t.main, t.camera, vec![]).is_err());
    let mut cam = EmbedCamera::default();
    cam.camera = Slot::Const(Value::Ref(t.camera));
    assert!(add_embed(&mut t.store, t.main, t.scene, vec![cam.wrap()]).is_err());
}
