use iron_document::{facet::{LeafStruct, Slider, Transform2d, Size}, *};
use kurbo::Point;

#[test]
fn endpoint_thumbs_and_their_grab_margin_are_inside_the_spatial_hit_bounds() {
    for h in [12.0, 24.0, 60.0] {
        for t in [0.0, 0.5, 1.0] {
            let rect = slider::thumb(200.0, h, t).rect();
            for p in [Point::new(rect.x0 + 0.1, rect.y0 + 0.1), Point::new(rect.x1 - 0.1, rect.y1 - 0.1)] {
                assert!(slider::contains(200.0, h, t, p));
                assert!(slider::thumb_hit(200.0, h, t, p));
            }
        }
    }
    assert!(!slider::contains(200.0, 24.0, 0.5, Point::new(-5.0, 12.0)));
}

#[test]
fn slider_defaults_follow_tokens_and_custom_colours_round_trip() {
    let mut store = Store::new();
    let id = store.authored().next_id();
    let apply = |store: &mut Store, ops| store.apply(TransactionInput { label: "slider".into(), actor: Actor::Human, ts: 0, idempotency_key: None, exact: false, ops }).unwrap();
    apply(&mut store, vec![Op::Create { id, ty: TypeId::lookup("slider").unwrap(), parent: None, order: OrderKey::first(), facets: vec![Slider::default().wrap(), Transform2d::default().wrap(), Size::default().wrap()] }]);
    let color = "slider.color".parse().unwrap();
    assert_eq!(store.resolved(id, color), Some(Value::Color(palette::hex("#ae743e"))));
    store.set_theme(true);
    assert_eq!(store.resolved(id, color), Some(Value::Color(palette::hex("#d5a06a"))));
    let bounds = geom::local_bounds(&store, id).unwrap();
    assert_eq!(bounds.hit_pad, 11.0);
    apply(&mut store, vec![Op::Set { id, path: color, slot: Value::Color(palette::hex("#123456")).into() }]);
    let saved = store.save();
    let loaded = Store::load(&saved).unwrap();
    assert_eq!(loaded.resolved(id, color), Some(Value::Color(palette::hex("#123456"))));
}

fn slider_bench() -> (Store, NodeId) {
    let mut store = Store::new();
    let id = store.authored().next_id();
    let mut slider = Slider::default();
    slider.value = Value::Number(0.0).into();
    store.apply(TransactionInput { label: "slider".into(), actor: Actor::Human, ts: 0, idempotency_key: None, exact: false, ops: vec![Op::Create { id, ty: TypeId::lookup("slider").unwrap(), parent: None, order: OrderKey::first(), facets: vec![slider.wrap(), Transform2d::default().wrap()] }] }).unwrap();
    (store, id)
}

#[test]
fn track_easing_is_presentation_only_interruptible_and_not_saved() {
    let (mut store, id) = slider_bench();
    let value = Leaf { node: id, path: "slider.value".parse().unwrap() };
    store.set_motion(true);
    store.advance_motion(10.0);
    store.begin_gesture(&[id]);
    store.preview_target(id, value.path, Value::Number(1.0)).unwrap();
    store.animate_presentation(value, 0.0);
    assert_eq!(store.resolved(id, value.path), Some(Value::Number(1.0)));
    assert_eq!(store.presented_number(value), Some(0.0));
    store.commit_gesture("track click".to_owned(), Actor::Human, 0).unwrap().unwrap();
    let saved = store.save();
    assert_eq!(Store::load(&saved).unwrap().presented_number(value), Some(1.0));
    assert!(store.advance_motion(10.06).contains(&id));
    let middle = store.presented_number(value).unwrap();
    assert!(middle > 0.0 && middle < 1.0, "{middle}");
    assert_eq!(store.save(), saved);
    store.advance_motion(10.13);
    assert_eq!(store.presented_number(value), Some(1.0));
    assert!(store.advance_motion(10.14).is_empty());
    // A new click retargets from the current drawn position.
    store.begin_gesture(&[id]);
    store.preview_target(id, value.path, Value::Number(0.0)).unwrap();
    store.animate_presentation(value, 1.0);
    store.commit_gesture("track click".to_owned(), Actor::Human, 0).unwrap().unwrap();
    store.advance_motion(10.18);
    let from = store.presented_number(value).unwrap();
    assert!(from > 0.0 && from < 1.0);
    store.undo(0).unwrap().unwrap();
    assert_eq!(store.presented_number(value), Some(1.0), "undo bypasses stale presentation");
    store.advance_motion(10.19);
    assert!(store.advance_motion(10.20).is_empty());
    store.begin_gesture(&[id]);
    store.preview_target(id, value.path, Value::Number(0.0)).unwrap();
    store.animate_presentation(value, 1.0);
    store.cancel_presentation(value);
    assert_eq!(store.presented_number(value), Some(0.0), "direct drag bypasses easing");
    store.cancel_gesture();
}

#[test]
fn hover_lighting_and_selection_policy_are_separate_and_template_driven() {
    use contexts::Field;
    let (mut store, id) = slider_bench();
    assert_eq!(store.resolver().lighting(&[id], 0, false), "control");
    assert_eq!(store.resolver().lighting(&[id], 0, true), "shape");
    assert_eq!(store.resolver().get(&[id], 0, Field::SELECTION), "modifier");
    let comp = store.authored().next_id();
    store.apply(TransactionInput { label: "composition".into(), actor: Actor::Human, ts: 0, idempotency_key: None, exact: false, ops: vec![Op::Create { id: comp, ty: TypeId::lookup("composition").unwrap(), parent: None, order: OrderKey::first(), facets: vec![facet::Composition::default().wrap(), facet::Name::default().wrap()] }] }).unwrap();
    let slider = store.place_standard("slider", comp, [0.0, 0.0], Actor::Human).unwrap();
    assert_eq!(store.resolver().get(&[slider], 0, Field::SELECTION), "modifier");
    assert_eq!(store.resolver().lighting(&[slider], 0, false), "none");
    assert_eq!(store.resolver().lighting(&[slider], 0, true), "frame");
}
