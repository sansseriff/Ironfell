use iron_document::appearance::{inside_clips, ClipFrame};
use iron_document::component::{Appearance, Fill, Gradient, GroupClip, Size, Transform2d};
use iron_document::*;

fn tx(ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: "appearance".into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}
fn num(v: f64) -> Slot { Slot::Const(Value::Number(v)) }
fn fixture() -> (Store, NodeId, NodeId) {
    let mut s = Store::new();
    let ids = s.document().next_ids(2);
    let [g, r] = [ids[0], ids[1]];
    let mut t = Transform2d::default();
    t.x = num(100.0); t.y = num(50.0); t.rot = num(std::f64::consts::FRAC_PI_2);
    let clip = GroupClip { w: num(120.0), h: num(80.0), radius: num(20.0), ..Default::default() };
    s.apply(tx(vec![
        Op::Create { id: g, ty: TypeId::lookup("group").unwrap(), parent: None, order: OrderKey::first(), components: vec![t.wrap(), clip.wrap(), Appearance::default().wrap()] },
        Op::Create { id: r, ty: TypeId::lookup("rect").unwrap(), parent: Some(g), order: OrderKey::first(), components: vec![Transform2d::default().wrap(), Size::default().wrap(), Fill::default().wrap(), Gradient::default().wrap()] },
    ])).unwrap();
    (s, g, r)
}

#[test]
fn clipped_picking_uses_resolved_local_geometry() {
    let (mut s, g, r) = fixture();
    let point = |xy| s.world_affine(g).apply(xy);
    let centre = point([60.0, 40.0]);
    let corner = point([1.0, 1.0]);
    let outside = point([140.0, 40.0]);
    assert!(inside_clips(&s, r, centre));
    assert!(!inside_clips(&s, r, corner));
    assert!(!inside_clips(&s, r, outside));
    let gesture = s.begin_gesture(&[g]);
    s.preview_target(g, "group_clip.w".parse().unwrap(), Value::Number(160.0)).unwrap();
    assert!(inside_clips(&s, r, outside));
    s.cancel_gesture();
    assert!(matches!(s.gesture_status(gesture), GestureStatus::Cancelled(_)));
    assert!(!inside_clips(&s, r, outside));
    s.apply(tx(vec![Op::Set { id: g, path: "group_clip.enabled".parse().unwrap(), slot: Slot::Const(Value::Bool(false)) }])).unwrap();
    assert!(inside_clips(&s, r, outside));
}

#[test]
fn ellipse_clip_and_degenerate_frames() {
    let ellipse = ClipFrame { w: 120.0, h: 80.0, radius: 0.0, ellipse: true };
    assert!(ellipse.contains([60.0, 40.0]));
    assert!(!ellipse.contains([5.0, 5.0]));
    assert!(!ClipFrame { w: 0.0, ..ellipse }.contains([0.0, 40.0]));
}

#[test]
fn a_scrub_previews_then_commits_once_and_round_trips() {
    let (mut s, g, _) = fixture();
    let before = s.save();
    let path = "appearance.opacity".parse().unwrap();
    s.begin_gesture(&[g]);
    for alpha in [0.9, 0.7, 0.5] { s.preview_target(g, path, Value::Number(alpha)).unwrap(); }
    assert_eq!(s.resolved(g, path), Some(Value::Number(0.5)));
    assert_eq!(s.save(), before, "previews are not authored");
    s.commit_gesture("scrub opacity", Actor::Human, 1).unwrap().unwrap();
    let saved = s.save();
    assert_eq!(Store::load(&saved).unwrap().save(), saved);
    s.undo(2).unwrap();
    assert_eq!(s.resolved(g, path), Some(Value::Number(1.0)));
    s.redo(3).unwrap();
    assert_eq!(s.resolved(g, path), Some(Value::Number(0.5)));
}
