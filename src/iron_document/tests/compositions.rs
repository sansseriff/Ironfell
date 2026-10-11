//! Compositions and the model tree (roadmap stage C1, ECM §5 and phase 2):
//! composition roots in 2D and 3D, the `model` root holding entities, and
//! figures lifted to the entity they picture.

use iron_document::component::{Composition, Instance, Mesh, Name, Params, Size, Transform2d, Transform3d};
use iron_document::composition::{self, Dims};
use iron_document::*;
use std::collections::BTreeMap;

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn named(text: &str) -> Name {
    let mut n = Name::default();
    n.text = Slot::Const(Value::Str(text.into()));
    n
}

fn comp(notation: &str, dims: &str) -> Composition {
    let mut c = Composition::default();
    c.notation = Slot::Const(Value::Str(notation.into()));
    c.dims = Slot::Const(Value::Enum(dims.into()));
    c
}

pub struct Two {
    pub store: Store,
    pub cartoon: NodeId,
    pub assembly: NodeId,
    pub model: NodeId,
    pub thing: NodeId,
    pub group: NodeId,
    pub boxed: NodeId,
    pub torus: NodeId,
}

/// cartoon (2d) { group { rect "box" } }, assembly (3d) { mesh "torus" },
/// model, library { definition "thing" }.
fn two_compositions() -> Two {
    let mut store = Store::new();
    let ids = store.document().next_ids(8);
    let (cartoon, group, boxed, assembly, torus, model, library, thing) = (ids[0], ids[1], ids[2], ids[3], ids[4], ids[5], ids[6], ids[7]);
    let k = OrderKey::first();
    let k1 = OrderKey::after(&k);
    let k2 = OrderKey::after(&k1);
    let k3 = OrderKey::after(&k2);
    store
        .apply(tx(
            "setup",
            vec![
                Op::Create { id: cartoon, ty: ty("composition"), parent: None, order: k.clone(), components: vec![named("cartoon").wrap(), comp("cartoon", "2d").wrap()] },
                Op::Create { id: group, ty: ty("group"), parent: Some(cartoon), order: k.clone(), components: vec![] },
                Op::Create { id: boxed, ty: ty("rect"), parent: Some(group), order: k.clone(), components: vec![Transform2d::default().wrap(), Size::default().wrap(), named("box").wrap()] },
                Op::Create { id: assembly, ty: ty("composition"), parent: None, order: k1.clone(), components: vec![named("assembly").wrap(), comp("cad", "3d").wrap()] },
                Op::Create { id: torus, ty: ty("mesh"), parent: Some(assembly), order: k.clone(), components: vec![Transform3d::default().wrap(), Mesh::default().wrap(), named("torus").wrap()] },
                Op::Create { id: model, ty: ty("model"), parent: None, order: k2, components: vec![] },
                Op::Create { id: library, ty: ty("library"), parent: None, order: k3, components: vec![] },
                Op::Create { id: thing, ty: ty("definition"), parent: Some(library), order: k, components: vec![named("thing").wrap()] },
            ],
        ))
        .unwrap();
    Two { store, cartoon, assembly, model, thing, group, boxed, torus }
}

fn entity_ops(t: &Two, id: NodeId, figures: &[(RelationId, NodeId)]) -> Vec<Op> {
    let mut instance = Instance::default();
    instance.def = Slot::Const(Value::Ref(t.thing));
    let mut ops = vec![Op::Create { id, ty: ty("entity"), parent: Some(t.model), order: OrderKey::first(), components: vec![instance.wrap(), Params::default().wrap()] }];
    for &(rel, from) in figures {
        ops.push(Op::Link { rel: Relation { id: rel, from, to: id, rel: RelKind::Figures, data: BTreeMap::new() } });
    }
    ops
}

#[test]
fn compositions_are_roots_with_dimensions() {
    let t = two_compositions();
    let doc = t.store.document();
    assert_eq!(composition::compositions(doc), vec![t.cartoon, t.assembly]);
    assert_eq!(composition::dims(doc, t.cartoon), Some(Dims::Two));
    assert_eq!(composition::dims(doc, t.assembly), Some(Dims::Three));
    assert_eq!(composition::notation(doc, t.assembly).as_deref(), Some("cad"));
    assert_eq!(composition::composition_of(doc, t.boxed), Some(t.cartoon));
    assert_eq!(composition::composition_of(doc, t.torus), Some(t.assembly));
    assert_eq!(composition::composition_of(doc, t.model), None);
    assert_eq!(composition::model_root(doc), Some(t.model));
}

#[test]
fn compositions_and_the_model_only_live_at_the_root() {
    let mut t = two_compositions();
    let id = t.store.document().next_id();
    // A composition inside a group, a model inside a composition, and a
    // rect inside the model are all refused.
    for (ty_name, parent, comps) in [
        ("composition", t.group, vec![named("nested").wrap(), Composition::default().wrap()]),
        ("model", t.cartoon, vec![]),
        ("rect", t.model, vec![Transform2d::default().wrap(), Size::default().wrap()]),
    ] {
        let err = t.store.apply(tx("bad", vec![Op::Create { id, ty: ty(ty_name), parent: Some(parent), order: OrderKey::first(), components: comps }])).unwrap_err();
        assert_eq!(err[0].kind, ErrorKind::ChildNotAdmitted, "{ty_name} under {parent}: {err:?}");
    }
    // Moving a composition under another node is refused the same way.
    let err = t.store.apply(tx("bad move", vec![Op::Reparent { id: t.assembly, parent: Some(t.cartoon), order: OrderKey::first() }])).unwrap_err();
    assert_eq!(err[0].kind, ErrorKind::ChildNotAdmitted);
    // The file round-trips, and one that nests a composition does not load.
    let text = t.store.save();
    assert_eq!(Store::load(&text).unwrap().save(), text);
    let line = text.lines().find(|l| l.trim_start().starts_with(&format!("\"{}\":", t.assembly))).unwrap();
    // The parent is the line's only null.
    let nested = line.replace("null", &format!("\"{}\"", t.cartoon));
    assert_ne!(line, nested, "{line}");
    let Err(err) = Store::load(&text.replace(line, &nested)) else { panic!("nested composition loaded") };
    assert!(err.to_string().contains("does not admit"), "{err}");
}

#[test]
fn a_figure_lifts_to_its_entity_through_its_ancestors() {
    let mut t = two_compositions();
    let entity = t.store.document().next_id();
    let rels = t.store.document().next_relation_ids(2);
    // The group (not the rect) is the cartoon's figure; the torus is the
    // assembly's.
    let ops = entity_ops(&t, entity, &[(rels[0], t.group), (rels[1], t.torus)]);
    t.store.apply(tx("entity", ops)).unwrap();
    let doc = t.store.document();
    assert_eq!(composition::figures_of(doc, entity), vec![t.group, t.torus]);
    assert_eq!(composition::figure_of(doc, t.boxed), Some((t.group, entity)));
    assert_eq!(composition::figure_of(doc, t.torus), Some((t.torus, entity)));
    assert_eq!(composition::figure_of(doc, t.cartoon), None);
    // Deleting the entity leaves the figures pointing at a tombstone: no
    // longer a figure of anything until the delete is undone.
    t.store.apply(tx("delete", vec![Op::Delete { id: entity }])).unwrap();
    assert_eq!(composition::figure_of(t.store.document(), t.boxed), None);
    t.store.undo(1).unwrap().unwrap();
    assert_eq!(composition::figure_of(t.store.document(), t.boxed), Some((t.group, entity)));
}
