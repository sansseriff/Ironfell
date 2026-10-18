//! The hierarchical scene against a brute-force reference: every query must
//! agree with "transform every leaf by its world affine and test", and a
//! group's motion must touch a bounded number of entries.

use iron_document::facet::{Composition, Embed, Frame, LeafStruct, Name, Size, Transform2d};
use iron_document::*;
use iron_spatial::Scene;
use kurbo::{Affine, Point, Rect};
use std::collections::BTreeSet;

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn n(v: f64) -> Slot {
    Slot::Const(Value::Number(v))
}

fn at(x: f64, y: f64, rot: f64) -> Facet {
    let mut t = Transform2d::default();
    t.x = n(x);
    t.y = n(y);
    t.rot = n(rot);
    t.wrap()
}

fn size(w: f64, h: f64) -> Facet {
    let mut s = Size::default();
    s.w = n(w);
    s.h = n(h);
    s.wrap()
}

fn comp2d() -> Facet {
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    c.wrap()
}

fn named(text: &str) -> Facet {
    let mut c = Name::default();
    c.text = Slot::Const(Value::Str(text.into()));
    c.wrap()
}

fn set(id: NodeId, leaf: &str, v: f64) -> Op {
    Op::Set { id, path: leaf.parse().unwrap(), slot: n(v) }
}

/// A composition holding `field` (a group at (100, 100)) of `cells` cell
/// groups, each `per` 10×10 rects spaced 20 px apart, plus one loose rect.
struct Fixture {
    store: Store,
    composition: NodeId,
    field: NodeId,
    cells: Vec<NodeId>,
    rects: Vec<NodeId>,
    loose: NodeId,
}

fn fixture(cells: usize, per: usize) -> Fixture {
    let mut store = Store::new();
    let ids = store.document().next_ids(3 + cells + cells * per);
    let (composition, field, loose) = (ids[0], ids[1], ids[2]);
    let mut ops = vec![
        Op::Create { id: composition, ty: ty("composition"), parent: None, order: OrderKey::first(), facets: vec![comp2d(), named("scene")] },
        Op::Create { id: field, ty: ty("group"), parent: Some(composition), order: OrderKey::first(), facets: vec![at(100.0, 100.0, 0.0)] },
        Op::Create { id: loose, ty: ty("rect"), parent: None, order: OrderKey::after(&OrderKey::first()), facets: vec![at(-50.0, -50.0, 0.0), size(10.0, 10.0)] },
    ];
    let mut cell_ids = Vec::new();
    let mut rect_ids = Vec::new();
    let mut next = 3;
    let mut key = OrderKey::first();
    for c in 0..cells {
        let cell = ids[next];
        next += 1;
        cell_ids.push(cell);
        ops.push(Op::Create { id: cell, ty: ty("group"), parent: Some(field), order: key.clone(), facets: vec![at(c as f64 * 200.0, 0.0, 0.0)] });
        key = OrderKey::after(&key);
        let mut rkey = OrderKey::first();
        for r in 0..per {
            let rect = ids[next];
            next += 1;
            rect_ids.push(rect);
            ops.push(Op::Create { id: rect, ty: ty("rect"), parent: Some(cell), order: rkey.clone(), facets: vec![at(r as f64 * 20.0, (r % 3) as f64 * 20.0, 0.0), size(10.0, 10.0)] });
            rkey = OrderKey::after(&rkey);
        }
    }
    store.apply(tx("fixture", ops)).unwrap();
    Fixture { store, composition, field, cells: cell_ids, rects: rect_ids, loose }
}

/// Every drawable leaf under `roots` in painter order, with its world
/// affine and local bounds.
fn leaves(store: &Store, roots: &[NodeId]) -> Vec<(NodeId, Affine, iron_document::geom::LocalBounds)> {
    let doc = store.document();
    let mut out = Vec::new();
    let mut stack: Vec<NodeId> = roots.iter().rev().copied().collect();
    while let Some(id) = stack.pop() {
        if let Some(lb) = store.local_bounds(id) {
            out.push((id, Affine::new(store.world_affine(id).0), lb));
        }
        stack.extend(doc.children(id).iter().rev().copied());
    }
    out
}

/// Leaves whose world-space *axis-aligned* box meets the query: what a flat
/// index in composition space would answer.
fn reference_aabb(store: &Store, roots: &[NodeId], point: Option<Point>, rect: Rect) -> Vec<NodeId> {
    leaves(store, roots)
        .into_iter()
        .filter(|(_, world, lb)| {
            let b = iron_spatial::Bounds::transformed(lb.geometry, *world, lb.paint_pad, lb.hit_pad);
            match point {
                Some(p) => b.interaction.x0 <= p.x && p.x <= b.interaction.x1 && b.interaction.y0 <= p.y && p.y <= b.interaction.y1,
                None => b.painted.x0 <= rect.x1 && rect.x0 <= b.painted.x1 && b.painted.y0 <= rect.y1 && rect.y0 <= b.painted.y1,
            }
        })
        .map(|(id, ..)| id)
        .collect()
}

/// Two convex polygons overlap unless some edge normal separates them.
fn overlap(a: &[Point], b: &[Point]) -> bool {
    for poly in [a, b] {
        for i in 0..poly.len() {
            let e = poly[(i + 1) % poly.len()] - poly[i];
            let (nx, ny) = (-e.y, e.x);
            let proj = |q: &[Point]| q.iter().map(|p| p.x * nx + p.y * ny).fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| (lo.min(v), hi.max(v)));
            let (alo, ahi) = proj(a);
            let (blo, bhi) = proj(b);
            if ahi < blo || bhi < alo {
                return false;
            }
        }
    }
    true
}

/// Leaves whose transformed local box (a rotated rectangle) exactly meets
/// the query: what a host's own shape test can at most accept.
fn exact(store: &Store, roots: &[NodeId], point: Option<Point>, rect: Rect) -> Vec<NodeId> {
    leaves(store, roots)
        .into_iter()
        .filter(|(_, world, lb)| match point {
            Some(p) => {
                let local = lb.geometry.inflate(lb.hit_pad, lb.hit_pad);
                let q = world.inverse() * p;
                local.x0 <= q.x && q.x <= local.x1 && local.y0 <= q.y && q.y <= local.y1
            }
            None => {
                let local = lb.geometry.inflate(lb.paint_pad, lb.paint_pad);
                let quad: Vec<Point> = [(local.x0, local.y0), (local.x1, local.y0), (local.x1, local.y1), (local.x0, local.y1)].iter().map(|&(x, y)| *world * Point::new(x, y)).collect();
                let r: Vec<Point> = [(rect.x0, rect.y0), (rect.x1, rect.y0), (rect.x1, rect.y1), (rect.x0, rect.y1)].iter().map(|&(x, y)| Point::new(x, y)).collect();
                overlap(&quad, &r)
            }
        })
        .map(|(id, ..)| id)
        .collect()
}

const RECTS: [Rect; 4] = [Rect::new(0.0, 0.0, 400.0, 300.0), Rect::new(105.0, 105.0, 112.0, 112.0), Rect::new(-100.0, -100.0, 0.0, 0.0), Rect::new(90.0, 60.0, 1200.0, 200.0)];
const POINTS: [Point; 4] = [Point::new(105.0, 105.0), Point::new(-45.0, -45.0), Point::new(325.0, 125.0), Point::new(1000.0, 1000.0)];

/// With nothing rotated, the scene answers exactly what a flat
/// composition-space index would.
fn check_flat(scene: &Scene, store: &Store, roots: &[NodeId]) {
    for (i, rect) in RECTS.iter().enumerate() {
        assert_eq!(scene.candidates(roots, None, *rect), reference_aabb(store, roots, None, *rect), "rect query {i}");
    }
    for (i, p) in POINTS.iter().enumerate() {
        assert_eq!(scene.candidates(roots, Some(*p), Rect::ZERO), reference_aabb(store, roots, Some(*p), Rect::ZERO), "point query {i}");
    }
}

/// Candidates are a superset of the exact hits (never a miss), in painter
/// order, and for a point exactly the leaves whose own box holds it.
fn check_conservative(scene: &Scene, store: &Store, roots: &[NodeId]) {
    for (i, rect) in RECTS.iter().enumerate() {
        let got = scene.candidates(roots, None, *rect);
        let want: BTreeSet<NodeId> = exact(store, roots, None, *rect).into_iter().collect();
        let got_set: BTreeSet<NodeId> = got.iter().copied().collect();
        assert!(want.is_subset(&got_set), "rect query {i} missed {:?}", want.difference(&got_set).collect::<Vec<_>>());
        let order: Vec<NodeId> = leaves(store, roots).into_iter().map(|(id, ..)| id).filter(|id| got_set.contains(id)).collect();
        assert_eq!(got, order, "rect query {i} painter order");
    }
    for (i, p) in POINTS.iter().enumerate() {
        assert_eq!(scene.candidates(roots, Some(*p), Rect::ZERO), exact(store, roots, Some(*p), Rect::ZERO), "point query {i}");
    }
}

fn roots_of(store: &Store, composition: NodeId, loose: NodeId) -> Vec<NodeId> {
    let mut r = store.document().children(composition).to_vec();
    r.push(loose);
    r
}


#[test]
fn queries_match_the_reference_and_painter_order() {
    let mut f = fixture(3, 5);
    let mut scene = Scene::default();
    scene.update(&mut f.store);
    assert_eq!(scene.len(), 1 + 3 + 15 + 1, "field, cells, rects, loose");
    let roots = roots_of(&f.store, f.composition, f.loose);
    check_flat(&scene, &f.store, &roots);
    check_conservative(&scene, &f.store, &roots);
    // Painter order: the first cell's rects before the second's.
    let all = scene.candidates(&roots, None, Rect::new(-1000.0, -1000.0, 5000.0, 5000.0));
    assert_eq!(all.len(), 16);
    assert_eq!(all[0], f.rects[0]);
    assert_eq!(all[15], f.loose, "a loose root created after the composition draws last");
    assert_eq!(scene.chain(f.rects[7]), &[f.field, f.cells[1], f.rects[7]]);
    assert!(scene.painted(f.field).unwrap().contains(Point::new(105.0, 105.0)));
}

#[test]
fn rotating_the_field_touches_one_entry_not_its_descendants() {
    let mut f = fixture(4, 25);
    let mut scene = Scene::default();
    scene.update(&mut f.store);
    let roots = roots_of(&f.store, f.composition, f.loose);
    // A preview, as a slider scrub would.
    f.store.preview(f.field, "transform2d.rot".parse().unwrap(), Value::Number(0.7));
    scene.update(&mut f.store);
    assert_eq!(scene.updated, 1, "the field's own entry; its 104 descendants are untouched");
    check_conservative(&scene, &f.store, &roots);
    // And a transaction, which also re-solves nothing here.
    f.store.cancel_gesture();
    scene.update(&mut f.store);
    f.store.apply(tx("rotate", vec![set(f.field, "transform2d.rot", -1.1), set(f.field, "transform2d.x", 300.0)])).unwrap();
    scene.update(&mut f.store);
    assert_eq!(scene.updated, 1);
    check_conservative(&scene, &f.store, &roots);
    assert!(scene.damage.len() >= 2, "old and new painted bounds of the field: {:?}", scene.damage);
    assert_eq!(f.store.revisions().local(f.rects[0]), f.store.revisions().local(f.rects[1]));
}

#[test]
fn a_leaf_edit_updates_its_entry_and_the_unions_above_it() {
    let mut f = fixture(2, 3);
    let mut scene = Scene::default();
    scene.update(&mut f.store);
    let roots = roots_of(&f.store, f.composition, f.loose);
    let before = scene.painted(f.field).unwrap();
    f.store.apply(tx("grow", vec![set(f.rects[5], "size.w", 500.0)])).unwrap();
    scene.update(&mut f.store);
    // The rect, its cell, and the field: one entry per level.
    assert_eq!(scene.updated, 3, "rect, cell, field");
    assert!(scene.painted(f.field).unwrap().x1 > before.x1, "the field's box grew with its content");
    check_flat(&scene, &f.store, &roots);
    // Moving the rect is a placement of the rect, still three entries.
    f.store.preview(f.rects[5], "transform2d.y".parse().unwrap(), Value::Number(900.0));
    scene.update(&mut f.store);
    assert_eq!(scene.updated, 3);
    check_flat(&scene, &f.store, &roots);
}

#[test]
fn structure_rebuilds_and_deletion_drops_entries() {
    let mut f = fixture(2, 3);
    let mut scene = Scene::default();
    scene.update(&mut f.store);
    let roots = roots_of(&f.store, f.composition, f.loose);
    f.store.apply(tx("delete", vec![Op::Delete { id: f.cells[0] }])).unwrap();
    scene.update(&mut f.store);
    assert_eq!(scene.len(), 1 + 1 + 3 + 1);
    check_flat(&scene, &f.store, &roots);
    f.store.undo(0).unwrap().unwrap();
    scene.update(&mut f.store);
    assert_eq!(scene.len(), 1 + 2 + 6 + 1);
    check_flat(&scene, &f.store, &roots);
    let all: BTreeSet<NodeId> = scene.candidates(&roots, None, Rect::new(-1000.0, -1000.0, 5000.0, 5000.0)).into_iter().collect();
    assert!(all.contains(&f.rects[0]));
}

#[test]
fn an_embeds_region_follows_its_sources_content() {
    let mut f = fixture(1, 2);
    let ids = f.store.document().next_ids(2);
    let (other, embed) = (ids[0], ids[1]);
    let mut e = Embed::default();
    e.source = Slot::Const(Value::Ref(f.composition));
    f.store
        .apply(tx(
            "embed",
            vec![
                Op::Create { id: other, ty: ty("composition"), parent: None, order: OrderKey::after(&OrderKey::after(&OrderKey::first())), facets: vec![comp2d(), named("other")] },
                Op::Create { id: embed, ty: ty("embed"), parent: Some(other), order: OrderKey::first(), facets: vec![at(1000.0, 0.0, 0.0), e.wrap()] },
            ],
        ))
        .unwrap();
    let mut scene = Scene::default();
    scene.update(&mut f.store);
    assert_eq!(scene.embeds(), &[embed]);
    let content = scene.content_bounds(f.composition).unwrap();
    let region = scene.painted(embed).unwrap();
    assert!((region.x0 - (1000.0 + content.x0 - 2.0)).abs() < 1e-9, "{region:?} vs content {content:?}");
    // Grow the source's content: the embed's box follows, without structure.
    f.store.apply(tx("grow", vec![set(f.rects[1], "size.h", 800.0)])).unwrap();
    scene.update(&mut f.store);
    let region2 = scene.painted(embed).unwrap();
    assert!(region2.y1 > region.y1 + 700.0, "{region2:?}");
    assert_eq!(scene.candidates(&[embed], Some(Point::new(1000.0 + content.x0 + 5.0, content.y0 + 5.0)), Rect::ZERO), vec![embed]);
    // A frame pins it.
    let mut frame = Frame::default();
    frame.x1 = n(50.0);
    frame.y1 = n(50.0);
    f.store.apply(tx("frame", vec![Op::AddFacet { id: embed, facet: frame.wrap() }])).unwrap();
    scene.update(&mut f.store);
    let framed = scene.painted(embed).unwrap();
    assert!((framed.width() - 54.0).abs() < 1e-9, "{framed:?}");
}

/// The stress fixture's shape: 100 cells of 100. Rotating the field is one
/// entry; a leaf drag is three. Timings print for `--release` runs.
#[test]
fn ten_thousand_shapes_rotate_as_one_entry() {
    let mut f = fixture(100, 100);
    let mut scene = Scene::default();
    let t = std::time::Instant::now();
    scene.update(&mut f.store);
    let built = t.elapsed();
    assert_eq!(scene.len(), 1 + 100 + 10_000 + 1);
    let roots = roots_of(&f.store, f.composition, f.loose);
    let t = std::time::Instant::now();
    f.store.preview(f.field, "transform2d.rot".parse().unwrap(), Value::Number(0.3));
    let previewed = t.elapsed();
    let t = std::time::Instant::now();
    scene.update(&mut f.store);
    let rotated = t.elapsed();
    assert_eq!(scene.updated, 1);
    let t = std::time::Instant::now();
    let visible = scene.candidates(&roots, None, Rect::new(0.0, 0.0, 1600.0, 1000.0));
    let queried = t.elapsed();
    assert!(!visible.is_empty());
    f.store.preview(f.rects[5050], "transform2d.x".parse().unwrap(), Value::Number(1.0));
    let t = std::time::Instant::now();
    scene.update(&mut f.store);
    let dragged = t.elapsed();
    // The rect alone when the cell's union still covers it; the cell and
    // the field only when their unions grew.
    assert!((1..=3).contains(&scene.updated), "{}", scene.updated);
    eprintln!("10k scene: build {built:?}, preview {previewed:?}, rotate update {rotated:?}, visible query {queried:?} ({} hits), drag update {dragged:?}", visible.len());
}
