//! Layout on groups (plans/specs/layout-and-tables.md §7, phase 1): a stack
//! reflows when content grows, its placements are published and never
//! stored, undo restores, and a laid-out position cannot be dragged.

use iron_document::facet::{Font, Layout, LayoutItem, Size, Text, Transform2d};
use iron_document::*;
use std::sync::OnceLock;

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn path(s: &str) -> LeafPath {
    s.parse().unwrap()
}

fn n(v: f64) -> Slot {
    Slot::Const(Value::Number(v))
}

fn e(s: &str) -> Slot {
    Slot::Const(Value::Enum(s.into()))
}

fn inter() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../public/fonts/Inter.ttf")).unwrap();
        iron_text::engine().register(bytes);
    });
}

fn at(x: f64, y: f64) -> Facet {
    let mut t = Transform2d::default();
    t.x = n(x);
    t.y = n(y);
    t.wrap()
}

fn words(s: &str) -> Facet {
    let mut t = Text::default();
    t.content = Slot::Const(Value::Str(s.into()));
    t.wrap()
}

fn num(store: &Store, id: NodeId, leaf: &str) -> f64 {
    store.resolved(id, path(leaf)).and_then(|v| v.as_f64()).unwrap()
}

/// A group at (100, 50) laid out by `layout`, holding `kids` (type, facets).
fn group(store: &mut Store, layout: Layout, kids: Vec<(&str, Vec<Facet>)>) -> (NodeId, Vec<NodeId>) {
    let ids = store.authored().next_ids(1 + kids.len());
    let mut ops = vec![Op::Create { id: ids[0], ty: ty("group"), parent: None, order: OrderKey::first(), facets: vec![at(100.0, 50.0), layout.wrap()] }];
    let mut order = OrderKey::first();
    for (i, (t, facets)) in kids.into_iter().enumerate() {
        ops.push(Op::Create { id: ids[i + 1], ty: ty(t), parent: Some(ids[0]), order: order.clone(), facets });
        order = OrderKey::after(&order);
    }
    store.apply(tx("group", ops)).unwrap();
    (ids[0], ids[1..].to_vec())
}

fn stack(gap: f64, pad: f64) -> Layout {
    Layout { mode: e("stack_y"), gap: n(gap), pad_t: n(pad), pad_r: n(pad), pad_b: n(pad), pad_l: n(pad), ..Layout::default() }
}

fn text(s: &str) -> (&'static str, Vec<Facet>) {
    ("text", vec![at(0.0, 0.0), words(s), Font::default().wrap()])
}

#[test]
fn a_stack_reflows_when_content_grows_and_stores_nothing() {
    inter();
    let mut store = Store::new();
    let (g, kids) = group(&mut store, stack(10.0, 20.0), vec![text("Title"), text("Body"), text("Footer")]);
    let line = store.local_bounds(kids[0]).unwrap().geometry.height();
    // Padding, then each line and the gap.
    assert_eq!(num(&store, kids[0], "transform2d.y"), 20.0);
    assert_eq!(num(&store, kids[1], "transform2d.y"), 20.0 + line + 10.0);
    let footer = num(&store, kids[2], "transform2d.y");
    assert!((footer - (20.0 + 2.0 * (line + 10.0))).abs() < 1e-6);
    assert_eq!(num(&store, kids[2], "transform2d.x"), 20.0);
    // The group hugs: padding on both sides around three lines and two gaps.
    assert!((num(&store, g, "size.h") - (40.0 + 3.0 * line + 20.0)).abs() < 1e-6);

    // The body grows to two lines: the footer moves down one line, with no
    // authored change to it.
    store.apply(tx("grow", vec![Op::Set { id: kids[1], path: path("text.content"), slot: Slot::Const(Value::Str("Body\nmore".into())) }])).unwrap();
    let grown = num(&store, kids[2], "transform2d.y");
    assert!((grown - footer - line).abs() < 0.5, "{footer} -> {grown}");
    assert_eq!(store.tree().leaf(kids[2], path("transform2d.y")).and_then(|s| s.constant()).cloned(), Some(Value::Number(0.0)));

    // Undo restores the footer's place.
    store.undo(0).unwrap().unwrap();
    assert!((num(&store, kids[2], "transform2d.y") - footer).abs() < 1e-6);

    // Saving writes the authored positions only, and no published size.
    let saved = store.save();
    let reloaded = Store::load(&saved).unwrap();
    assert_eq!(reloaded.tree().leaf(kids[2], path("transform2d.y")).and_then(|s| s.constant()).cloned(), Some(Value::Number(0.0)));
    assert!(reloaded.tree().facet(g, FacetKind::Size).is_none());
    // And the reloaded store lays out again.
    assert!((num(&reloaded, kids[2], "transform2d.y") - footer).abs() < 1e-6);
}

#[test]
fn a_laid_out_position_is_locked() {
    inter();
    let mut store = Store::new();
    let (_, kids) = group(&mut store, stack(0.0, 0.0), vec![text("A"), text("B")]);
    match store.handle(kids[1], path("transform2d.y")) {
        Handle::Locked(why) => assert!(why.contains("layout"), "{why}"),
        h => panic!("expected locked, got {h:?}"),
    }
    // An absolute child keeps its own position and leaves the flow.
    let mut abs = LayoutItem::default();
    abs.absolute = Slot::Const(Value::Bool(true));
    store.apply(tx("absolute", vec![Op::AddFacet { id: kids[0], facet: abs.wrap() }])).unwrap();
    assert_eq!(num(&store, kids[1], "transform2d.y"), 0.0);
    assert!(!store.handle(kids[0], path("transform2d.y")).is_locked());
}

#[test]
fn a_grid_places_rects_and_fill_takes_the_cell() {
    let mut store = Store::new();
    let rect = |w: f64, h: f64, extra: Option<Facet>| {
        let mut s = Size::default();
        s.w = n(w);
        s.h = n(h);
        let mut f = vec![at(0.0, 0.0), s.wrap()];
        f.extend(extra);
        ("rect", f)
    };
    let mut fill = LayoutItem::default();
    fill.w = e("fill");
    let grid = Layout { mode: e("grid"), columns: Slot::Const(Value::Str("auto 50".into())), gap: n(4.0), row_gap: n(6.0), ..Layout::default() };
    let (g, kids) = group(&mut store, grid, vec![rect(30.0, 10.0, None), rect(20.0, 20.0, None), rect(10.0, 10.0, Some(fill.wrap())), rect(10.0, 10.0, None)]);
    // Column 1 is as wide as its widest rect (30); column 2 is 50, after a gap of 4.
    assert_eq!(num(&store, kids[1], "transform2d.x"), 34.0);
    // Row 1 is 20 high (the second rect); row 2 starts after the row gap.
    assert_eq!(num(&store, kids[2], "transform2d.y"), 26.0);
    // The fill rect takes its column's width; the fixed one keeps its own.
    assert_eq!(num(&store, kids[2], "size.w"), 30.0);
    assert_eq!(num(&store, kids[3], "size.w"), 10.0);
    assert_eq!((num(&store, g, "size.w"), num(&store, g, "size.h")), (84.0, 36.0));
}

#[test]
fn text_in_a_fixed_width_stack_wraps() {
    inter();
    let mut store = Store::new();
    let mut layout = stack(0.0, 0.0);
    layout.sizing_w = e("fixed");
    let ids = store.authored().next_ids(2);
    let mut size = Size::default();
    size.w = n(80.0);
    let mut fill = LayoutItem::default();
    fill.w = e("fill");
    store
        .apply(tx(
            "card",
            vec![
                Op::Create { id: ids[0], ty: ty("group"), parent: None, order: OrderKey::first(), facets: vec![at(0.0, 0.0), layout.wrap(), size.wrap()] },
                Op::Create { id: ids[1], ty: ty("text"), parent: Some(ids[0]), order: OrderKey::first(), facets: vec![at(0.0, 0.0), words("several words that cannot fit on one line"), Font::default().wrap(), fill.wrap()] },
            ],
        ))
        .unwrap();
    assert_eq!(num(&store, ids[1], "size.w"), 80.0);
    assert_eq!(store.resolved(ids[1], path("text.sizing")), Some(Value::Enum("auto_height".into())));
    let b = store.local_bounds(ids[1]).unwrap().geometry;
    assert_eq!(b.width(), 80.0);
    assert!(b.height() > 60.0, "wrapped: {b:?}");
}

#[test]
fn add_auto_layout_keeps_the_arrangement_it_was_given() {
    let mut store = Store::new();
    let ids = store.authored().next_ids(4);
    let comp = ids[0];
    let mut ops = vec![Op::Create { id: comp, ty: ty("group"), parent: None, order: OrderKey::first(), facets: vec![] }];
    // Three 40-wide rects, 10 apart, top-aligned at y = 30, created out of
    // their left-to-right order.
    let mut order = OrderKey::first();
    for (i, x) in [(1, 150.0), (2, 50.0), (3, 100.0)] {
        let mut s = Size::default();
        s.w = n(40.0);
        s.h = n(20.0);
        ops.push(Op::Create { id: ids[i], ty: ty("rect"), parent: Some(comp), order: order.clone(), facets: vec![at(x, 30.0), s.wrap()] });
        order = OrderKey::after(&order);
    }
    store.apply(tx("rects", ops)).unwrap();
    let world = |store: &Store, id: NodeId| store.world_affine(id).translation();
    let before: Vec<[f64; 2]> = ids[1..].iter().map(|&r| world(&store, r)).collect();
    let extent = |n: NodeId| store.extent(n);
    let (ops, g) = iron_document::layout::add_ops(store.tree(), &extent, &ids[1..]).unwrap();
    store.apply(tx("add auto layout", ops)).unwrap();
    assert_eq!(store.resolved(g, path("layout.mode")), Some(Value::Enum("stack_x".into())));
    assert_eq!(num(&store, g, "layout.gap"), 10.0);
    // Ordered as they sit, and nothing moved.
    assert_eq!(*store.tree().children(g), [ids[2], ids[3], ids[1]]);
    for (r, b) in ids[1..].iter().zip(&before) {
        assert_eq!(&world(&store, *r), b);
    }
}

/// A repeat whose count reads a width that layout decides follows it at
/// once: at load, after a transaction and on each drag frame, not one step
/// behind (reactive-engine.md §17's phase 5 exit; handoff §4.10, RE-5.4).
#[test]
fn a_repeat_counting_a_laid_out_width_follows_it_at_once() {
    let mut store = Store::new();
    let src = r#"<edit><composition name="scene" dims="2d">
      <group id="$row" name="row" x="0" y="0" size.w="300" size.h="40" layout.mode="stack_x" layout.sizing_w="fixed" layout.sizing_h="fixed">
        <rect id="$slot" name="slot" w="10" h="10" layout_item.w="fill"/>
      </group>
      <group id="$many" name="many" y="100" repeat.count="{= floor($slot.size.w / 50)}"><rect x="{= $many.repeat.index * 20}" w="5" h="5"/></group>
    </composition></edit>"#;
    let plan = iron_document::edit::plan(&store, src, None).unwrap_or_else(|f| panic!("{}", f.message()));
    let (row, slot, many) = (plan.aliases["$row"], plan.aliases["$slot"], plan.aliases["$many"]);
    store.apply(tx("probe", plan.ops)).unwrap();
    let items = |s: &Store| s.tree().children(many).iter().filter(|c| c.is_item_scope() && s.tree().is_live(**c)).count();
    let want = |s: &Store| (num(s, slot, "size.w") / 50.0).floor() as usize;
    assert_eq!((num(&store, slot, "size.w"), items(&store)), (300.0, 6), "at load");
    store.advance(0.1);
    assert_eq!(items(&store), 6);
    store.apply(tx("wider", vec![Op::Set { id: row, path: path("size.w"), slot: n(500.0) }])).unwrap();
    assert_eq!(items(&store), want(&store), "after a transaction");
    for w in [200.0, 400.0, 250.0] {
        store.preview(row, path("size.w"), Value::Number(w));
        assert_eq!(items(&store), want(&store), "on a drag frame at {w}");
    }
    assert!(store.problems().iter().all(|p| p.kind != "feedback"), "{:?}", store.problems());
}

/// A count that reads its own items' layout never rests: it is a problem on
/// the repeat, and the store goes on (decision D6).
#[test]
fn a_count_that_reads_its_own_items_layout_is_a_problem() {
    let mut store = Store::new();
    let src = r#"<edit><composition name="scene" dims="2d">
      <group id="$many" name="many" x="0" y="0" size.w="10" size.h="10" layout.mode="stack_x" repeat.count="{= floor($many.size.w / 10) + 1}"><rect w="10" h="5"/></group>
    </composition></edit>"#;
    let plan = iron_document::edit::plan(&store, src, None).unwrap_or_else(|f| panic!("{}", f.message()));
    let many = plan.aliases["$many"];
    store.apply(tx("feedback", plan.ops)).unwrap();
    assert!(store.problems().iter().any(|p| p.kind == "feedback" && p.node == many), "{:?}", store.problems());
}

/// A container inside a plain group inside another: the outer run reads the
/// inner one's placements through the group's bounds, in the same call
/// (reactive-engine handoff §4.10, RE-5.4 step 2).
#[test]
fn an_outer_layout_reads_a_nested_one_through_a_plain_group_at_once() {
    let mut store = Store::new();
    let src = r#"<edit><composition name="scene" dims="2d">
      <group id="$outer" name="outer" x="0" y="0" size.w="10" size.h="10" layout.mode="stack_x">
        <rect name="first" w="10" h="10"/>
        <group id="$plain" name="plain">
          <group id="$inner" name="inner" size.w="10" size.h="10" layout.mode="stack_x">
            <rect id="$grow" name="grow" w="10" h="10"/><rect name="second" w="10" h="10"/>
          </group>
        </group>
        <rect id="$after" name="after" w="10" h="10"/>
      </group>
    </composition></edit>"#;
    let plan = iron_document::edit::plan(&store, src, None).unwrap_or_else(|f| panic!("{}", f.message()));
    let (grow, after) = (plan.aliases["$grow"], plan.aliases["$after"]);
    store.apply(tx("nested", plan.ops)).unwrap();
    assert_eq!(num(&store, after, "transform2d.x"), 30.0, "first 10, then the inner pair's 20");
    store.apply(tx("wider", vec![Op::Set { id: grow, path: path("size.w"), slot: n(50.0) }])).unwrap();
    assert_eq!(num(&store, after, "transform2d.x"), 70.0, "the inner pair is 60 wide now");
}

/// A container's gap bound to another's laid-out width follows it in the same
/// call, not the next (reactive-engine handoff §4.10, RE-5.4 step 2).
#[test]
fn a_layout_reading_a_binding_over_another_layout_follows_it_at_once() {
    let mut store = Store::new();
    let src = r#"<edit><composition name="scene" dims="2d">
      <group id="$a" name="a" x="0" y="0" size.w="10" size.h="10" layout.mode="stack_x">
        <rect id="$grow" name="grow" w="10" h="10"/>
      </group>
      <group id="$b" name="b" x="0" y="100" size.w="10" size.h="10" layout.mode="stack_x" layout.gap="{= $a.size.w}">
        <rect name="one" w="10" h="10"/><rect id="$two" name="two" w="10" h="10"/>
      </group>
    </composition></edit>"#;
    let plan = iron_document::edit::plan(&store, src, None).unwrap_or_else(|f| panic!("{}", f.message()));
    let (grow, two) = (plan.aliases["$grow"], plan.aliases["$two"]);
    store.apply(tx("chain", plan.ops)).unwrap();
    assert_eq!(num(&store, two, "transform2d.x"), 20.0, "one's 10, then a gap of a's 10");
    store.apply(tx("wider", vec![Op::Set { id: grow, path: path("size.w"), slot: n(40.0) }])).unwrap();
    assert_eq!(num(&store, two, "transform2d.x"), 50.0, "a gap of a's 40 now");
}

/// Each structural root is a layout native (reactive-engine handoff §4.10,
/// RE-5.4): an edit in one container lays out that one, and no other.
#[test]
fn an_edit_in_one_container_lays_out_no_other() {
    let mut store = Store::new();
    let src = r#"<edit><composition name="scene" dims="2d">
      <group id="$a" name="a" x="0" y="0" size.w="10" size.h="10" layout.mode="stack_x">
        <rect id="$grow" name="grow" w="10" h="10"/><rect id="$next" name="next" w="10" h="10"/>
      </group>
      <group id="$b" name="b" x="0" y="100" size.w="10" size.h="10" layout.mode="stack_x">
        <rect name="one" w="10" h="10"/><rect id="$two" name="two" w="10" h="10"/>
      </group>
    </composition></edit>"#;
    let plan = iron_document::edit::plan(&store, src, None).unwrap_or_else(|f| panic!("{}", f.message()));
    let (grow, next, two) = (plan.aliases["$grow"], plan.aliases["$next"], plan.aliases["$two"]);
    store.apply(tx("two rows", plan.ops)).unwrap();
    let runs = |s: &Store, n: NodeId| s.reactive().native_runs(Leaf { node: n, path: path("transform2d.x") }).expect("laid out by a native");
    let (a0, b0) = (runs(&store, next), runs(&store, two));
    store.apply(tx("wider", vec![Op::Set { id: grow, path: path("size.w"), slot: n(40.0) }])).unwrap();
    assert_eq!(num(&store, next, "transform2d.x"), 40.0);
    assert!(runs(&store, next) > a0, "the edited container laid out");
    assert_eq!(runs(&store, two), b0, "the other did not");
}

/// A contained box (`layout.contain = both`, reactive-engine.md §9.2) takes
/// no size from what is in it, so a repeat inside it may count by its
/// width: it follows the box as its row is resized, and is no feedback.
#[test]
fn a_repeat_inside_a_contained_box_counts_by_its_width() {
    let mut store = Store::new();
    let src = r#"<edit><composition name="scene" dims="2d">
      <group id="$row" name="row" x="0" y="0" size.w="300" size.h="40" layout.mode="stack_x" layout.sizing_w="fixed" layout.sizing_h="fixed">
        <group id="$box" name="box" size.w="10" size.h="40" layout.mode="stack_x" layout.contain="both" layout_item.w="fill">
          <group id="$many" name="many" repeat.count="{= floor($box.size.w / 50)}"><rect x="{= $many.repeat.index * 50}" w="40" h="10"/></group>
        </group>
      </group>
    </composition></edit>"#;
    let plan = iron_document::edit::plan(&store, src, None).unwrap_or_else(|f| panic!("{}", f.message()));
    let (row, boxed, many) = (plan.aliases["$row"], plan.aliases["$box"], plan.aliases["$many"]);
    store.apply(tx("contained", plan.ops)).unwrap();
    let items = |s: &Store| s.tree().children(many).iter().filter(|c| c.is_item_scope() && s.tree().is_live(**c)).count();
    assert_eq!((num(&store, boxed, "size.w"), items(&store)), (300.0, 6));
    for w in [500.0, 200.0, 420.0] {
        store.apply(tx("resize", vec![Op::Set { id: row, path: path("size.w"), slot: n(w) }])).unwrap();
        assert_eq!(num(&store, boxed, "size.w"), w, "the box takes the row's width, not its content's");
        assert_eq!(items(&store), (w / 50.0).floor() as usize);
    }
    assert!(store.problems().iter().all(|p| p.kind != "feedback"), "{:?}", store.problems());
}

/// A contained group that hugs is sized as if empty: its padding.
#[test]
fn a_contained_hugging_group_is_sized_as_if_empty() {
    let mut store = Store::new();
    let src = r#"<edit><composition name="scene" dims="2d">
      <group id="$row" name="row" x="0" y="0" size.w="10" size.h="10" layout.mode="stack_x">
        <group id="$box" name="box" size.w="10" size.h="10" layout.mode="stack_x" layout.contain="both" layout.pad_l="4" layout.pad_r="6">
          <rect w="100" h="20"/>
        </group>
        <rect id="$after" name="after" w="10" h="10"/>
      </group>
    </composition></edit>"#;
    let plan = iron_document::edit::plan(&store, src, None).unwrap_or_else(|f| panic!("{}", f.message()));
    let (boxed, after) = (plan.aliases["$box"], plan.aliases["$after"]);
    store.apply(tx("contained", plan.ops)).unwrap();
    assert_eq!(num(&store, boxed, "size.w"), 10.0);
    assert_eq!(num(&store, after, "transform2d.x"), 10.0);
}
