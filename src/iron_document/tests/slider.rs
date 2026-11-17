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
