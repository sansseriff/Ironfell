//! The tests plans/document-spine.md §14 asks for: inverses restore, replay
//! equals snapshot, atomicity, structural validation, and view elision.

use iron_document::component::{Fill, Name, Size, Timing, Transform2d, Transform3d};
use iron_document::*;
use std::collections::BTreeMap;

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput {
        label: label.into(),
        actor: Actor::Human,
        ts: 0,
        idempotency_key: None,
        ops,
    }
}

fn num(n: f64) -> Slot {
    Slot::Const(Value::Number(n))
}

fn path(s: &str) -> LeafPath {
    s.parse().unwrap()
}

/// Canonical JSON with the version removed, for comparing content across an
/// apply/undo pair (undo advances the version).
fn content(store: &Store) -> String {
    let mut v: serde_json::Value = serde_json::from_str(&store.save()).unwrap();
    v.as_object_mut().unwrap().remove("version");
    serde_json::to_string_pretty(&v).unwrap()
}

/// Only what a reader can see: live nodes in tree order with their
/// components, plus relations. Tombstones are excluded, which is what makes
/// this the right comparison for undoing a create.
fn live_content(store: &Store) -> String {
    let doc = store.document();
    let mut out = String::new();
    let mut stack: Vec<NodeId> = doc.roots().iter().rev().copied().collect();
    while let Some(id) = stack.pop() {
        let n = doc.live(id).unwrap();
        out.push_str(&format!("{id} {} {:?} {}", n.ty, n.parent, n.order));
        for c in doc.components_of(id) {
            out.push_str(&format!(" {}", serde_json::to_string(c).unwrap()));
        }
        out.push('\n');
        stack.extend(doc.children(id).iter().rev().copied());
    }
    for r in doc.relations() {
        out.push_str(&format!("{}\n", serde_json::to_string(r).unwrap()));
    }
    out
}

fn check(store: &Store) {
    store.document().check_invariants().unwrap();
}

struct Scene {
    store: Store,
    group: NodeId,
    rect: NodeId,
    circle: NodeId,
    inner: NodeId,
    mesh: NodeId,
}

/// group { rect, circle, inner_group { mesh } }
fn scene() -> Scene {
    let mut store = Store::new();
    let ids = store.document().next_ids(5);
    let (group, rect, circle, inner, mesh) = (ids[0], ids[1], ids[2], ids[3], ids[4]);
    let k0 = OrderKey::first();
    let k1 = OrderKey::after(&k0);
    let k2 = OrderKey::after(&k1);
    let mut name = Name::default();
    name.text = Slot::Const(Value::Str("scene".into()));
    store
        .apply(tx(
            "build scene",
            vec![
                Op::Create {
                    id: group,
                    ty: ty("group"),
                    parent: None,
                    order: k0.clone(),
                    components: vec![name.wrap()],
                },
                Op::Create {
                    id: rect,
                    ty: ty("rect"),
                    parent: Some(group),
                    order: k0.clone(),
                    components: vec![
                        Transform2d::default().wrap(),
                        Size::default().wrap(),
                        Fill::default().wrap(),
                    ],
                },
                Op::Create {
                    id: circle,
                    ty: ty("circle"),
                    parent: Some(group),
                    order: k1.clone(),
                    components: vec![
                        Transform2d::default().wrap(),
                        component::Radius::default().wrap(),
                    ],
                },
                Op::Create {
                    id: inner,
                    ty: ty("group"),
                    parent: Some(group),
                    order: k2,
                    components: vec![],
                },
                Op::Create {
                    id: mesh,
                    ty: ty("mesh"),
                    parent: Some(inner),
                    order: k0,
                    components: vec![
                        Transform3d::default().wrap(),
                        component::Mesh::default().wrap(),
                    ],
                },
            ],
        ))
        .unwrap();
    check(&store);
    Scene {
        store,
        group,
        rect,
        circle,
        inner,
        mesh,
    }
}

fn relation(store: &Store, from: NodeId, to: NodeId) -> Relation {
    let mut data = BTreeMap::new();
    data.insert("prop".to_owned(), Value::Str("opacity".into()));
    Relation {
        id: store.document().next_relation_id(),
        from,
        to,
        rel: RelKind::Animates,
        data,
    }
}

#[test]
fn every_op_kind_inverts_exactly() {
    let s = scene();
    let Scene {
        group,
        rect,
        circle,
        inner,
        mesh,
        ..
    } = s;
    let mut store = s.store;
    // A clip and a relation so link/unlink have something to work on.
    let clip = store.document().next_id();
    let mut timing = Timing::default();
    timing.dur = num(2.0);
    store
        .apply(tx(
            "add clip",
            vec![Op::Create {
                id: clip,
                ty: ty("clip"),
                parent: None,
                order: OrderKey::first(),
                components: vec![timing.wrap(), component::Clip::default().wrap()],
            }],
        ))
        .unwrap();
    let rel = relation(&store, clip, mesh);
    store
        .apply(tx("link", vec![Op::Link { rel: rel.clone() }]))
        .unwrap();
    check(&store);

    let fresh = store.document().next_id();
    let cases: Vec<(&str, Vec<Op>)> = vec![
        (
            "create",
            vec![Op::Create {
                id: fresh,
                ty: ty("rect"),
                parent: Some(group),
                order: OrderKey::first(),
                components: vec![Transform2d::default().wrap(), Size::default().wrap()],
            }],
        ),
        ("delete leaf", vec![Op::Delete { id: rect }]),
        ("delete subtree", vec![Op::Delete { id: group }]),
        ("delete relation target", vec![Op::Delete { id: mesh }]),
        (
            "reparent",
            vec![Op::Reparent {
                id: circle,
                parent: Some(inner),
                order: OrderKey::first(),
            }],
        ),
        (
            "reparent to root",
            vec![Op::Reparent {
                id: circle,
                parent: None,
                order: OrderKey::first(),
            }],
        ),
        (
            "reorder",
            vec![Op::Reorder {
                id: rect,
                order: OrderKey::after(&OrderKey::after(&OrderKey::first())),
            }],
        ),
        (
            "set",
            vec![Op::Set {
                id: rect,
                path: path("transform2d.x"),
                slot: num(42.0),
            }],
        ),
        (
            "add comp",
            vec![Op::AddComp {
                id: circle,
                comp: Fill::default().wrap(),
            }],
        ),
        (
            "remove comp",
            vec![Op::RemoveComp {
                id: rect,
                kind: ComponentKind::Fill,
            }],
        ),
        ("unlink", vec![Op::Unlink { id: rel.id }]),
        (
            "compound",
            vec![
                Op::Set {
                    id: rect,
                    path: path("size.w"),
                    slot: num(7.0),
                },
                Op::Delete { id: inner },
                Op::Reparent {
                    id: rect,
                    parent: None,
                    order: OrderKey::first(),
                },
            ],
        ),
    ];
    for (label, ops) in cases {
        let before = content(&store);
        let before_live = live_content(&store);
        let v0 = store.document().version();
        store
            .apply(tx(label, ops))
            .unwrap_or_else(|e| panic!("{label}: {e:?}"));
        check(&store);
        assert_ne!(
            live_content(&store),
            before_live,
            "{label}: op changed nothing"
        );
        store
            .undo(0)
            .unwrap()
            .unwrap_or_else(|e| panic!("undo {label}: {e:?}"));
        check(&store);
        assert_eq!(
            live_content(&store),
            before_live,
            "{label}: undo did not restore"
        );
        if label == "create" {
            // The inverse of a create is a delete, and ids are never reused:
            // the undone node stays as a tombstone.
            assert!(store.document().node(fresh).is_some_and(|n| !n.is_live()));
        } else {
            assert_eq!(content(&store), before, "{label}: undo did not restore");
        }
        assert_eq!(store.document().version(), Version(v0.0 + 2));
        // And redo re-applies cleanly.
        store
            .redo(0)
            .unwrap()
            .unwrap_or_else(|e| panic!("redo {label}: {e:?}"));
        check(&store);
        store.undo(0).unwrap().unwrap();
        check(&store);
        assert_eq!(
            live_content(&store),
            before_live,
            "{label}: undo after redo did not restore"
        );
    }
}

#[test]
fn replay_of_log_equals_snapshot_and_load_roundtrips() {
    let s = scene();
    let Scene {
        group, rect, mesh, ..
    } = s;
    let mut store = s.store;
    store
        .apply(tx(
            "move",
            vec![Op::Set {
                id: rect,
                path: path("transform2d.x"),
                slot: num(10.0),
            }],
        ))
        .unwrap();
    store
        .apply(tx("delete", vec![Op::Delete { id: mesh }]))
        .unwrap();
    store.undo(0).unwrap().unwrap();
    let extra = store.document().next_id();
    store
        .apply(tx(
            "add",
            vec![Op::Create {
                id: extra,
                ty: ty("circle"),
                parent: Some(group),
                order: OrderKey::first(),
                components: vec![
                    Transform2d::default().wrap(),
                    component::Radius::default().wrap(),
                ],
            }],
        ))
        .unwrap();
    let rel = relation(&store, extra, rect);
    store.apply(tx("link", vec![Op::Link { rel }])).unwrap();
    check(&store);

    // Replay every logged transaction (including the undo) into a fresh store.
    let mut replay = Store::new();
    for t in store.history().log() {
        replay
            .apply(TransactionInput {
                label: t.label.clone(),
                actor: t.actor,
                ts: t.ts,
                idempotency_key: None,
                ops: t.ops.clone(),
            })
            .unwrap();
    }
    assert_eq!(replay.save(), store.save());

    // Save, load, save again.
    let text = store.save();
    let loaded = Store::load(&text).unwrap();
    check(&loaded);
    assert_eq!(loaded.save(), text);
    assert!(text.starts_with("{\"schema\": 1,"));
    // The loaded store keeps working: the counter is right, so a new create
    // does not collide.
    let mut loaded = loaded;
    let id = loaded.document().next_id();
    loaded
        .apply(tx(
            "after load",
            vec![Op::Create {
                id,
                ty: ty("group"),
                parent: None,
                order: OrderKey::first(),
                components: vec![],
            }],
        ))
        .unwrap();
    check(&loaded);
}

#[test]
fn a_bad_op_rejects_the_whole_batch_and_reports_every_error() {
    let s = scene();
    let Scene {
        rect,
        circle,
        group,
        ..
    } = s;
    let mut store = s.store;
    let before = store.save();
    let err = store
        .apply(tx(
            "mixed",
            vec![
                Op::Set {
                    id: rect,
                    path: path("transform2d.x"),
                    slot: num(1.0),
                }, // fine
                Op::Set {
                    id: rect,
                    path: path("transform2d.y"),
                    slot: Slot::Const(Value::Bool(true)),
                }, // type
                Op::Reparent {
                    id: rect,
                    parent: Some(circle),
                    order: OrderKey::first(),
                }, // not admitted
                Op::Set {
                    id: circle,
                    path: path("size.w"),
                    slot: num(1.0),
                }, // no such component
            ],
        ))
        .unwrap_err();
    let kinds: Vec<(usize, ErrorKind)> = err.iter().map(|e| (e.op, e.kind)).collect();
    assert_eq!(
        kinds,
        vec![
            (1, ErrorKind::TypeMismatch),
            (2, ErrorKind::ChildNotAdmitted),
            (3, ErrorKind::UnknownPath)
        ]
    );
    assert!(
        err[0].message.contains("expects number"),
        "{}",
        err[0].message
    );
    assert_eq!(store.save(), before, "store must be untouched");
    assert!(store.history().past().len() == 1);
}

#[test]
fn structural_rules() {
    let s = scene();
    let Scene {
        group,
        inner,
        rect,
        mesh,
        ..
    } = s;
    let mut store = s.store;

    // Reparent into a descendant is a cycle.
    let e = store
        .apply(tx(
            "cycle",
            vec![Op::Reparent {
                id: group,
                parent: Some(inner),
                order: OrderKey::first(),
            }],
        ))
        .unwrap_err();
    assert_eq!(e[0].kind, ErrorKind::Cycle);

    // Tombstoned targets are rejected loudly, and the message says when.
    store
        .apply(tx("delete", vec![Op::Delete { id: mesh }]))
        .unwrap();
    let e = store
        .apply(tx(
            "touch dead",
            vec![Op::Set {
                id: mesh,
                path: path("mesh.asset"),
                slot: Slot::Const(Value::Str("cube".into())),
            }],
        ))
        .unwrap_err();
    assert_eq!(e[0].kind, ErrorKind::Tombstoned);
    assert!(e[0].message.contains("version"));

    // Ids are never reused: a create below the counter is rejected even for
    // an id that was never allocated (there is none, but a stale one is).
    let e = store
        .apply(tx(
            "reuse",
            vec![Op::Create {
                id: rect,
                ty: ty("group"),
                parent: None,
                order: OrderKey::first(),
                components: vec![],
            }],
        ))
        .unwrap_err();
    assert_eq!(e[0].kind, ErrorKind::BadId);

    // Required components are enforced on create and on remove.
    let id = store.document().next_id();
    let e = store
        .apply(tx(
            "bare rect",
            vec![Op::Create {
                id,
                ty: ty("rect"),
                parent: Some(group),
                order: OrderKey::first(),
                components: vec![],
            }],
        ))
        .unwrap_err();
    assert_eq!(e[0].kind, ErrorKind::ComponentRequired);
    let e = store
        .apply(tx(
            "strip",
            vec![Op::RemoveComp {
                id: rect,
                kind: ComponentKind::Size,
            }],
        ))
        .unwrap_err();
    assert_eq!(e[0].kind, ErrorKind::ComponentRequired);

    // Unknown paths do not parse, and a known path on the wrong node is an error.
    assert!("transform2d.zz".parse::<LeafPath>().is_err());
    let e = store
        .apply(tx(
            "wrong node",
            vec![Op::Set {
                id: group,
                path: path("size.w"),
                slot: num(1.0),
            }],
        ))
        .unwrap_err();
    assert_eq!(e[0].kind, ErrorKind::UnknownPath);

    // Deleting a subtree tombstones every descendant, with one stamp.
    store
        .apply(tx("delete group", vec![Op::Delete { id: group }]))
        .unwrap();
    let v = store.document().version();
    for n in [group, inner, rect] {
        assert_eq!(store.document().node(n).unwrap().tombstoned, Some(v), "{n}");
    }
    assert!(store.document().roots().is_empty());
    check(&store);

    // Undo brings the subtree back, parents before children.
    store.undo(0).unwrap().unwrap();
    check(&store);
    assert_eq!(store.document().children(group), &[rect, s.circle, inner]);
}

#[test]
fn sibling_order_survives_many_inserts_between_the_same_pair() {
    let s = scene();
    let Scene {
        group,
        rect,
        circle,
        ..
    } = s;
    let mut store = s.store;
    let mut prev = store.document().live(rect).unwrap().order.clone();
    let hi = store.document().live(circle).unwrap().order.clone();
    let mut inserted = Vec::new();
    for _ in 0..1000 {
        let key = OrderKey::between(Some(&prev), Some(&hi)).unwrap();
        let id = store.document().next_id();
        store
            .apply(tx(
                "insert",
                vec![Op::Create {
                    id,
                    ty: ty("rect"),
                    parent: Some(group),
                    order: key.clone(),
                    components: vec![Transform2d::default().wrap(), Size::default().wrap()],
                }],
            ))
            .unwrap();
        inserted.push(id);
        prev = key;
    }
    check(&store);
    let kids = store.document().children(group);
    assert_eq!(kids[0], rect);
    assert_eq!(&kids[1..1001], &inserted[..]);
    assert_eq!(kids[1001], circle);
}

#[test]
fn views_mark_elision_and_summary_is_stable_under_hidden_edits() {
    let s = scene();
    let Scene {
        group, inner, rect, ..
    } = s;
    let mut store = s.store;

    let full = store
        .view(&ViewQuery {
            scope: None,
            fidelity: Fidelity::Full,
            depth: None,
        })
        .unwrap();
    assert!(
        full.contains("<group id=\"#1\" name=\"scene\" count=\"3\">"),
        "{full}"
    );
    assert!(
        full.contains(
            "<rect id=\"#2\" x=\"0\" y=\"0\" rot=\"0\" sx=\"1\" sy=\"1\" w=\"100\" h=\"100\""
        ),
        "{full}"
    );
    assert!(full.contains("<mesh id=\"#5\" pos=\"0,0,0\""), "{full}");
    assert!(!full.contains("elided"));

    // Depth limit: the inner group is shown but its child is not, and that is marked.
    let shallow = store
        .view(&ViewQuery {
            scope: None,
            fidelity: Fidelity::Full,
            depth: Some(1),
        })
        .unwrap();
    assert!(
        shallow.contains("<group id=\"#4\" count=\"1\" elided/>"),
        "{shallow}"
    );
    assert!(!shallow.contains("<mesh"));

    // Skeleton has no leaves.
    let skel = store
        .view(&ViewQuery {
            scope: Some(group),
            fidelity: Fidelity::Skeleton,
            depth: None,
        })
        .unwrap();
    assert!(skel.contains("<rect id=\"#2\"/>"), "{skel}");
    assert!(!skel.contains("x="));

    // Summary elides a big child list to two samples, and edits to hidden
    // children do not change the rendered text.
    let mut ops = Vec::new();
    for id in store.document().next_ids(20) {
        ops.push(Op::Create {
            id,
            ty: ty("rect"),
            parent: Some(inner),
            order: OrderKey::first(),
            components: vec![Transform2d::default().wrap(), Size::default().wrap()],
        });
    }
    store.apply(tx("fill inner", ops)).unwrap();
    let q = ViewQuery {
        scope: Some(inner),
        fidelity: Fidelity::Summary,
        depth: None,
    };
    let summary = store.view(&q).unwrap();
    assert!(summary.contains("count=\"21\" elided>"), "{summary}");
    // Two samples: the mesh that was already there, then the first rect.
    assert_eq!(summary.matches("<mesh").count(), 1, "{summary}");
    assert_eq!(summary.matches("<rect").count(), 1, "{summary}");
    let hidden = *store.document().children(inner).last().unwrap();
    store
        .apply(tx(
            "edit hidden",
            vec![Op::Set {
                id: hidden,
                path: path("transform2d.x"),
                slot: num(99.0),
            }],
        ))
        .unwrap();
    let after: String = store.view(&q).unwrap();
    // Only the version attribute may differ.
    let strip = |s: &str| {
        s.replacen(
            &format!("version=\"{}\"", store.document().version().0),
            "",
            1,
        )
    };
    assert_eq!(
        strip(&after).lines().skip(1).collect::<Vec<_>>(),
        summary.lines().skip(1).collect::<Vec<_>>()
    );

    // Relations render as child elements on their source.
    let clip = store.document().next_id();
    store
        .apply(tx(
            "clip",
            vec![Op::Create {
                id: clip,
                ty: ty("clip"),
                parent: None,
                order: OrderKey::after(&OrderKey::first()),
                components: vec![Timing::default().wrap(), component::Clip::default().wrap()],
            }],
        ))
        .unwrap();
    let rel = relation(&store, clip, rect);
    store.apply(tx("link", vec![Op::Link { rel }])).unwrap();
    let v = store
        .view(&ViewQuery {
            scope: Some(clip),
            fidelity: Fidelity::Full,
            depth: None,
        })
        .unwrap();
    assert!(v.contains("<animates to=\"#2\" prop=\"opacity\"/>"), "{v}");
    assert_eq!(store.document().relations_to(rect).count(), 1);

    // Views of tombstoned scopes are errors, not empty documents.
    store
        .apply(tx("delete", vec![Op::Delete { id: clip }]))
        .unwrap();
    assert!(
        store
            .view(&ViewQuery {
                scope: Some(clip),
                fidelity: Fidelity::Full,
                depth: None
            })
            .is_err()
    );
}

#[test]
fn idempotency_keys_replay_instead_of_reapplying() {
    let s = scene();
    let rect = s.rect;
    let mut store = s.store;
    let input = TransactionInput {
        label: "nudge".into(),
        actor: Actor::Model,
        ts: 1,
        idempotency_key: Some("k1".into()),
        ops: vec![Op::Set {
            id: rect,
            path: path("transform2d.x"),
            slot: num(5.0),
        }],
    };
    let a = store.apply(input.clone()).unwrap();
    let b = store.apply(input).unwrap();
    assert!(!a.replayed && b.replayed);
    assert_eq!(a.version, b.version);
    assert_eq!(store.history().past().len(), 2);
}

#[test]
fn ops_and_transactions_roundtrip_through_json() {
    let s = scene();
    let store = s.store;
    let t = &store.history().past()[0];
    let text = serde_json::to_string(t).unwrap();
    let back: Transaction = serde_json::from_str(&text).unwrap();
    assert_eq!(&back, t);
    assert!(text.contains(r#""t":"create""#));
    assert!(text.contains(r#""ty":"rect""#));
    let op: Op =
        serde_json::from_str(r##"{"t":"set","id":"#2","path":"transform2d.x","slot":12}"##)
            .unwrap();
    assert_eq!(
        op,
        Op::Set {
            id: s.rect,
            path: path("transform2d.x"),
            slot: num(12.0)
        }
    );
}
