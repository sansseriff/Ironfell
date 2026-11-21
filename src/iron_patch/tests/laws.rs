//! The laws as properties over random scripts (plans/notes/patches-regions-
//! and-time.md §11.5), each checked against enumerating every point. A rule
//! that should hold runs over 10,000 scripts; a rule the council expected to
//! fail is searched for a counterexample, which the test prints and requires.

use iron_patch::edit::{self, Op};
use iron_patch::*;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

const SCRIPTS: u64 = 10_000;

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }
    fn pick<T: Clone>(&mut self, xs: &[T]) -> T {
        xs[self.below(xs.len())].clone()
    }
}

const SLIDES: AxisId = AxisId(0);
const STATES: AxisId = AxisId(1);
const INSTANCES: AxisId = AxisId(2);

/// Two or three axes: slides (ordered, maybe with a deleted or moved
/// value), states (unordered), instances (paths: `bench/o1/j`, `o2/j`,
/// `bench/o1/k`, `k`), in a random deciding order.
fn space(r: &mut Rng) -> Space {
    let mut s = Space::default();
    let mut slides = Axis::new(Kind::Ordered);
    for v in 0..5 {
        slides.push(ValueId(v), Vec::new());
    }
    if r.chance(25) {
        slides.delete(ValueId(r.below(5) as u32));
    }
    if r.chance(15) {
        slides.move_to(ValueId(r.below(5) as u32), r.below(5));
    }
    s.axes.insert(SLIDES, slides);
    if r.chance(75) {
        let mut states = Axis::new(Kind::Unordered);
        for v in 10..13 {
            states.push(ValueId(v), Vec::new());
        }
        s.axes.insert(STATES, states);
    }
    if r.chance(60) {
        let mut inst = Axis::new(Kind::Path);
        for (v, p) in [
            (20, vec![1, 2, 3]),
            (21, vec![4, 3]),
            (22, vec![1, 2, 5]),
            (23, vec![5]),
        ] {
            inst.push(ValueId(v), p);
        }
        s.axes.insert(INSTANCES, inst);
    }
    let mut order: Vec<AxisId> = s.axes.keys().copied().collect();
    for i in (1..order.len()).rev() {
        order.swap(i, r.below(i + 1));
    }
    s.order = order;
    s
}

fn subset(r: &mut Rng, ids: &[u32]) -> BTreeSet<ValueId> {
    ids.iter()
        .filter(|_| r.chance(50))
        .map(|&v| ValueId(v))
        .collect()
}

fn sel(r: &mut Rng, axis: AxisId) -> Sel {
    if r.chance(30) {
        return Sel::All;
    }
    match axis {
        SLIDES => {
            if r.chance(15) {
                return Sel::Set(subset(r, &[0, 1, 2, 3, 4]));
            }
            let bound = |r: &mut Rng| match r.below(4) {
                0 => Bound::Open,
                1 => Bound::Excl(ValueId(r.below(5) as u32)),
                _ => Bound::Incl(ValueId(r.below(5) as u32)),
            };
            let lo = bound(r);
            Sel::Range(lo, bound(r))
        }
        STATES => {
            if r.chance(50) {
                Sel::Set(subset(r, &[10, 11, 12]))
            } else {
                Sel::Not(subset(r, &[10, 11, 12]))
            }
        }
        _ => {
            if r.chance(20) {
                return Sel::Set(subset(r, &[20, 21, 22, 23]));
            }
            let tail = r.pick(&[
                vec![3],
                vec![2, 3],
                vec![1, 2, 3],
                vec![4, 3],
                vec![5],
                vec![2, 5],
                vec![],
            ]);
            let not = if r.chance(30) {
                vec![r.pick(&[vec![1, 2, 3], vec![4, 3], vec![2, 5]])]
            } else {
                Vec::new()
            };
            Sel::Suffix { tail, not }
        }
    }
}

fn product(r: &mut Rng, space: &Space) -> Product {
    let mut p = Product::all();
    for &a in space.axes.keys() {
        if r.chance(60) {
            p = p.with(a, sel(r, a));
        }
    }
    p
}

const FIELDS: [Field; 2] = [2, 3];

/// Nodes 0 to 4: 0 the root; 1 and 3 its children; 2 under 1, absent in
/// the base (a patch adds it); 4 under 2.
fn doc() -> Doc {
    let mut d = Doc::default();
    for (n, parent) in [
        (0, None),
        (1, Some(0)),
        (2, Some(1)),
        (3, Some(0)),
        (4, Some(2)),
    ] {
        d.parent.insert(n, parent);
        d.base.insert(
            Key {
                node: n,
                field: PRESENT,
            },
            if n == 2 { 0 } else { 1 },
        );
    }
    d.base.insert(Key { node: 1, field: 2 }, 100);
    d.base.insert(Key { node: 3, field: 3 }, 300);
    d
}

fn entries(r: &mut Rng) -> BTreeMap<Key, i64> {
    let mut e = BTreeMap::new();
    for _ in 0..1 + r.below(3) {
        let node = r.below(5) as Node;
        let field = if r.chance(15) {
            r.pick(&[PRESENT, HIDDEN])
        } else {
            r.pick(&FIELDS)
        };
        let value = if field == PRESENT || field == HIDDEN {
            r.below(2) as i64
        } else {
            r.below(4) as i64
        };
        e.insert(Key { node, field }, value);
    }
    e
}

/// The layer views in these scripts write in: the outermost, so nothing
/// they meet is set by an outer scope.
const VIEW: u32 = 2;

fn layer(r: &mut Rng) -> u32 {
    if r.chance(70) { 0 } else { r.below(3) as u32 }
}

/// Up to five patches, each made as a new patch is (`World::make`: a rank
/// from specificity), most in one layer, some read-only.
fn world(r: &mut Rng, strength: Strength) -> World {
    let mut w = World::new(space(r), doc(), strength);
    for _ in 0..r.below(6) {
        let l = layer(r);
        let mut p = w.make(product(r, &w.space), entries(r), l);
        p.writable = !r.chance(10);
        let at = w.patches.len();
        w.apply(&Op::Add { patch: p, at });
    }
    w
}

/// The (point, key) pairs whose value differs.
fn changed(a: &[(Point, State)], b: &[(Point, State)]) -> BTreeSet<(Point, Key)> {
    let mut out = BTreeSet::new();
    for ((p, sa), (_, sb)) in a.iter().zip(b) {
        for k in sa.keys().chain(sb.keys()) {
            if sa.get(k).copied().flatten() != sb.get(k).copied().flatten() {
                out.insert((p.clone(), *k));
            }
        }
    }
    out
}

fn overlap(w: &World, ps: &[&Patch]) -> bool {
    let mut x = Product::all();
    for p in ps {
        x = x.intersect(&p.region, &w.space);
    }
    !x.is_empty(&w.space)
}

/// Whether strength is a strict total order on patches that meet, and
/// extends inclusion; the first failure.
fn order_failure(w: &World, inclusion: bool) -> Option<String> {
    let ps: Vec<&Patch> = w.patches.iter().collect();
    for a in &ps {
        for b in &ps {
            if a.id != b.id && w.cmp(a, b) != w.cmp(b, a).reverse() {
                return Some(format!("not antisymmetric: {:?} {:?}", a.id, b.id));
            }
            let strictly_inside = a.layer == b.layer
                && a.region.within(&b.region, &w.space)
                && !b.region.within(&a.region, &w.space)
                && !a.region.void(&w.space);
            if inclusion && a.id != b.id && strictly_inside && w.cmp(a, b) != Ordering::Greater {
                return Some(format!("{:?} is inside {:?} and not stronger", a.id, b.id));
            }
            for c in &ps {
                if w.cmp(a, b) == Ordering::Greater
                    && w.cmp(b, c) == Ordering::Greater
                    && w.cmp(a, c) != Ordering::Greater
                    && overlap(w, &[a, b, c])
                {
                    return Some(format!(
                        "a cycle at a shared point: {:?} > {:?} > {:?} > {:?}\n{:#?}",
                        a.id,
                        b.id,
                        c.id,
                        a.id,
                        [a, b, c].map(|p| &p.region)
                    ));
                }
            }
        }
    }
    None
}

#[test]
fn axis_keys_strength_is_a_total_order_that_extends_inclusion() {
    for seed in 0..SCRIPTS {
        let w = world(&mut Rng::new(seed), Strength::AxisKeys);
        if let Some(f) = order_failure(&w, true) {
            panic!("seed {seed}: {f}");
        }
    }
}

/// Built only by making patches, stored ranks agree with `AxisKeys`, so
/// they extend inclusion too; only a region edit can make them disagree.
#[test]
fn stored_strength_is_a_total_order_and_starts_by_inclusion() {
    for seed in 0..SCRIPTS {
        let w = world(&mut Rng::new(seed), Strength::Stored);
        if let Some(f) = order_failure(&w, true) {
            panic!("seed {seed}: {f}");
        }
        assert!(w.contradictions().is_empty(), "seed {seed}");
    }
}

/// Laws 4 and 5 as written cycle (note §11.1): found, not assumed.
#[test]
fn the_laws_as_written_cycle() {
    let found = (0..SCRIPTS).find_map(|seed| {
        order_failure(&world(&mut Rng::new(seed), Strength::AsWritten), true)
            .filter(|f| f.contains("cycle"))
            .map(|f| (seed, f))
    });
    let (seed, f) = found.expect("no cycle under the laws as written");
    println!("seed {seed}: {f}");
}

#[test]
fn values_do_not_depend_on_storage_order() {
    for strength in [Strength::AxisKeys, Strength::Stored] {
        for seed in 0..SCRIPTS {
            let w = world(&mut Rng::new(seed), strength);
            let mut v = w.clone();
            v.patches.reverse();
            assert_eq!(w.snapshot(), v.snapshot(), "{strength:?}, seed {seed}");
        }
    }
}

#[test]
fn an_empty_patch_changes_nothing() {
    for strength in [Strength::AxisKeys, Strength::Stored] {
        for seed in 0..SCRIPTS {
            let mut r = Rng::new(seed);
            let mut w = world(&mut r, strength);
            let before = w.snapshot();
            let empty = w.make(product(&mut r, &w.space), BTreeMap::new(), 0);
            w.apply(&Op::Add {
                patch: empty,
                at: 0,
            });
            let nowhere = w.make(
                Product::all().with(SLIDES, Sel::Set(BTreeSet::new())),
                entries(&mut r),
                0,
            );
            w.apply(&Op::Add {
                patch: nowhere,
                at: 0,
            });
            assert_eq!(
                changed(&before, &w.snapshot()),
                BTreeSet::new(),
                "{strength:?}, seed {seed}"
            );
        }
    }
}

/// Law 2, restated (§11.1): exchanging two patches adjacent in strength that
/// set no key in common changes no value.
#[test]
fn reordering_patches_with_disjoint_keys_changes_nothing() {
    for seed in 0..SCRIPTS {
        let mut w = world(&mut Rng::new(seed), Strength::Stored);
        let mut by: Vec<&Patch> = w.patches.iter().collect();
        by.sort_by(|a, b| (a.layer, a.rank).partial_cmp(&(b.layer, b.rank)).unwrap());
        let Some((a, b)) = by.windows(2).map(|p| (p[0], p[1])).find(|(a, b)| {
            a.layer == b.layer && a.entries.keys().all(|k| !b.entries.contains_key(k))
        }) else {
            continue;
        };
        let (ida, idb, ra, rb) = (a.id, b.id, a.rank, b.rank);
        let before = w.snapshot();
        w.apply_all(&[
            Op::Rank {
                patch: ida,
                rank: rb,
            },
            Op::Rank {
                patch: idb,
                rank: ra,
            },
        ]);
        assert_eq!(before, w.snapshot(), "seed {seed}");
    }
}

/// An entry edit changes exactly the points of its winning region where the
/// value differs: law 3 with the winning region as its blast radius.
#[test]
fn an_entry_edit_changes_exactly_its_winning_region() {
    for strength in [Strength::AxisKeys, Strength::Stored] {
        for seed in 0..SCRIPTS {
            let mut r = Rng::new(seed);
            let mut w = world(&mut r, strength);
            let candidates: Vec<(PatchId, Key)> = w
                .patches
                .iter()
                .flat_map(|p| {
                    p.entries
                        .keys()
                        .filter(|k| k.field >= 2)
                        .map(move |k| (p.id, *k))
                })
                .collect();
            if candidates.is_empty() {
                continue;
            }
            let (p, key) = r.pick(&candidates);
            let v = r.below(4) as i64;
            let region = w.winning(p, key);
            let before = w.snapshot();
            let expected: BTreeSet<(Point, Key)> = before
                .iter()
                .filter(|(x, s)| {
                    region.contains(&w.space, x) && s[&key].is_some() && s[&key] != Some(v)
                })
                .map(|(x, _)| (x.clone(), key))
                .collect();
            w.apply_all(&edit::edit(p, key, v));
            assert_eq!(
                changed(&before, &w.snapshot()),
                expected,
                "{strength:?}, seed {seed}"
            );
        }
    }
}

/// A region edit changes only points it adds or removes. Holds for stored
/// strength; derived strength is searched for the central flaw (§11.1):
/// a point in both regions changes.
fn region_edit_failure(strength: Strength, seed: u64, by_move: bool) -> Option<String> {
    let mut r = Rng::new(seed);
    let mut w = world(&mut r, strength);
    if w.patches.is_empty() {
        return None;
    }
    let p = r.pick(&w.patches).clone();
    let new = product(&mut r, &w.space);
    let before = w.snapshot();
    let ops = if by_move {
        edit::widen_by_move(
            &w,
            p.id,
            &p.entries.keys().copied().collect::<Vec<_>>(),
            new.clone(),
        )
    } else {
        edit::set_region(p.id, new.clone())
    };
    w.apply_all(&ops);
    let after = w.snapshot();
    let bad: Vec<(Point, Key)> = changed(&before, &after)
        .into_iter()
        .filter(|(x, _)| p.region.contains(&w.space, x) == new.contains(&w.space, x))
        .collect();
    (!bad.is_empty()).then(|| {
        format!(
            "{:?} from {:?} to {:?} changed {:?}",
            p.id, p.region, new, bad[0]
        )
    })
}

#[test]
fn a_region_edit_with_stored_strength_changes_only_points_added_or_removed() {
    for seed in 0..SCRIPTS {
        if let Some(f) = region_edit_failure(Strength::Stored, seed, false) {
            panic!("seed {seed}: {f}");
        }
    }
}

#[test]
fn a_region_edit_with_derived_strength_changes_points_it_keeps() {
    for by_move in [false, true] {
        let found = (0..SCRIPTS).find_map(|seed| {
            region_edit_failure(Strength::AxisKeys, seed, by_move).map(|f| (seed, f))
        });
        let (seed, f) = found.expect("derived strength kept region edits local");
        println!(
            "{}seed {seed}: {f}",
            if by_move { "by moving entries, " } else { "" }
        );
    }
}

/// Moving entries to a patch on the new region (Opus's first repair) is
/// not local under stored strength either: the moved entries take the new
/// patch's rank where the regions overlap.
#[test]
fn widening_by_moving_entries_is_not_local() {
    let found = (0..SCRIPTS)
        .find_map(|seed| region_edit_failure(Strength::Stored, seed, true).map(|f| (seed, f)));
    let (seed, f) = found.expect("moving entries kept region edits local");
    println!("seed {seed}: {f}");
}

#[test]
fn symbolic_regions_agree_with_enumeration() {
    for seed in 0..SCRIPTS {
        let mut r = Rng::new(seed);
        let s = space(&mut r);
        let (a, b) = (product(&mut r, &s), product(&mut r, &s));
        let (pa, pb): (BTreeSet<Point>, BTreeSet<Point>) = (
            a.points(&s).into_iter().collect(),
            b.points(&s).into_iter().collect(),
        );
        let both: BTreeSet<Point> = a.intersect(&b, &s).points(&s).into_iter().collect();
        assert_eq!(both, &pa & &pb, "seed {seed}: ∩");
        let pieces = a.minus(&b, &s);
        let mut seen: BTreeSet<Point> = BTreeSet::new();
        for piece in &pieces {
            assert!(!piece.is_empty(&s), "seed {seed}: an empty piece");
            for x in piece.points(&s) {
                assert!(seen.insert(x), "seed {seed}: pieces overlap");
            }
        }
        assert_eq!(seen, &pa - &pb, "seed {seed}: \\");
        assert_eq!(a.subset(&b, &s), pa.is_subset(&pb), "seed {seed}: ⊆");
    }
}

#[test]
fn winning_regions_agree_with_enumeration() {
    for strength in [Strength::AxisKeys, Strength::Stored] {
        for seed in 0..SCRIPTS {
            let w = world(&mut Rng::new(seed), strength);
            for p in &w.patches {
                for &key in p.entries.keys() {
                    let symbolic: BTreeSet<Point> =
                        w.winning(p.id, key).points(&w.space).into_iter().collect();
                    let enumerated: BTreeSet<Point> = w
                        .space
                        .points()
                        .into_iter()
                        .filter(|x| w.winner(x, key).map(|q| q.id) == Some(p.id))
                        .collect();
                    assert_eq!(
                        symbolic, enumerated,
                        "{strength:?}, seed {seed}, {:?} {key:?}",
                        p.id
                    );
                }
            }
        }
    }
}

/// A point, and an edit region holding it.
fn edit_region(r: &mut Rng, w: &World, x: &Point) -> Product {
    for _ in 0..20 {
        let e = product(r, &w.space);
        if e.contains(&w.space, x) {
            return e;
        }
    }
    Product::all()
}

/// The drag's lens laws (§11.2): it shows (PutGet), dragging to what is
/// shown writes nothing (GetPut), a second drag adds no patch (PutPut), it
/// changes nothing outside the edit region, and it never writes into a
/// read-only patch.
#[test]
fn a_drag_obeys_the_lens_laws() {
    for strength in [Strength::AxisKeys, Strength::Stored] {
        for seed in 0..SCRIPTS {
            let mut r = Rng::new(seed);
            let mut w = world(&mut r, strength);
            let points = w.space.points();
            let x = r.pick(&points);
            let present: Vec<Node> = (0..5).filter(|&n| w.present(&x, n)).collect();
            if present.is_empty() {
                continue;
            }
            let key = Key {
                node: r.pick(&present),
                field: r.pick(&FIELDS),
            };
            let e = edit_region(&mut r, &w, &x);
            if let Some(now) = w.raw(&x, key) {
                assert_eq!(
                    edit::drag(&w, &x, &e, key, now, VIEW),
                    Ok(Vec::new()),
                    "{strength:?}, seed {seed}: GetPut"
                );
            }
            let v = 10 + r.below(5) as i64;
            let before = w.snapshot();
            let ops = edit::drag(&w, &x, &e, key, v, VIEW)
                .unwrap_or_else(|f| panic!("{strength:?}, seed {seed}: {f}"));
            let original = w.clone();
            for op in &ops {
                if let Op::Entry { patch, .. } = op {
                    assert!(
                        w.patch(*patch).is_none_or(|p| p.writable),
                        "{strength:?}, seed {seed}: wrote a read-only patch"
                    );
                }
            }
            w.apply_all(&ops);
            assert_eq!(
                w.value(&x, key),
                Some(v),
                "{strength:?}, seed {seed}: PutGet"
            );
            for (y, k) in changed(&before, &w.snapshot()) {
                assert!(
                    e.contains(&w.space, &y),
                    "{strength:?}, seed {seed}: {k:?} changed at {y:?}, outside the edit region"
                );
            }
            // The blast radius: exactly the written entry's winning region,
            // where the value was another.
            let target = match ops.first() {
                Some(Op::Entry { patch, .. }) => *patch,
                Some(Op::Add { patch, .. }) => patch.id,
                _ => unreachable!("a drag that writes"),
            };
            let radius = w.winning(target, key);
            let expected: BTreeSet<(Point, Key)> = before
                .iter()
                .filter(|(y, st)| {
                    let was = st.get(&key).copied().flatten();
                    radius.contains(&w.space, y) && w.present(y, key.node) && was != Some(v)
                })
                .map(|(y, _)| (y.clone(), key))
                .collect();
            assert_eq!(
                changed(&before, &w.snapshot()),
                expected,
                "{strength:?}, seed {seed}: blast radius"
            );
            // PutPut: a second drag writes what the first made, and the two
            // equal one drag to the second value.
            let again = edit::drag(&w, &x, &e, key, v + 1, VIEW).unwrap();
            assert!(
                !again.iter().any(|o| matches!(o, Op::Add { .. })),
                "{strength:?}, seed {seed}: PutPut added a patch"
            );
            w.apply_all(&again);
            let mut once = original.clone();
            let single = edit::drag(&once, &x, &e, key, v + 1, VIEW).unwrap();
            once.apply_all(&single);
            assert_eq!(
                once.snapshot(),
                w.snapshot(),
                "{strength:?}, seed {seed}: PutPut"
            );
            assert_eq!(
                once.patches.len(),
                w.patches.len(),
                "{strength:?}, seed {seed}: PutPut"
            );
        }
    }
}

#[test]
fn split_on_the_same_region_and_an_allowed_merge_keep_every_value() {
    for strength in [Strength::AxisKeys, Strength::Stored] {
        for seed in 0..SCRIPTS {
            let mut r = Rng::new(seed);
            let mut w = world(&mut r, strength);
            if w.patches.is_empty() {
                continue;
            }
            let before = w.snapshot();
            let p = r.pick(&w.patches).clone();
            let keys: Vec<Key> = p.entries.keys().copied().filter(|_| r.chance(50)).collect();
            let undo = w.apply_all(&edit::split(&w, p.id, &keys, p.region.clone()));
            assert_eq!(before, w.snapshot(), "{strength:?}, seed {seed}: split");
            // Merge it back, or any pair on equal regions.
            let pairs: Vec<(PatchId, PatchId)> = w
                .patches
                .iter()
                .flat_map(|a| {
                    w.patches
                        .iter()
                        .filter(move |b| a.id < b.id)
                        .map(move |b| (a.id, b.id))
                })
                .collect();
            for (a, b) in pairs {
                if let Ok(ops) = edit::merge(&w, a, b) {
                    let mut m = w.clone();
                    m.apply_all(&ops);
                    assert_eq!(
                        before,
                        m.snapshot(),
                        "{strength:?}, seed {seed}: merge {a:?} {b:?}"
                    );
                }
            }
            w.apply_all(&undo);
            assert_eq!(before, w.snapshot());
        }
    }
}

/// Any script of edits, undone in reverse, restores the patches exactly,
/// ranks included.
#[test]
fn undo_restores_everything() {
    for strength in [Strength::AxisKeys, Strength::Stored] {
        for seed in 0..SCRIPTS {
            let mut r = Rng::new(seed);
            let mut w = world(&mut r, strength);
            let start = w.clone();
            let mut undo: Vec<Vec<Op>> = Vec::new();
            for _ in 0..6 {
                let ops: Vec<Op> = match (r.below(8), w.patches.is_empty()) {
                    (_, true) | (0, _) => vec![Op::Add {
                        patch: {
                            let l = layer(&mut r);
                            w.make(product(&mut r, &w.space), entries(&mut r), l)
                        },
                        at: r.below(w.patches.len() + 1),
                    }],
                    (1, _) => vec![Op::Remove(r.pick(&w.patches).id)],
                    (2, _) => edit::set_region(r.pick(&w.patches).id, product(&mut r, &w.space)),
                    (3, _) => {
                        edit::reorder(r.pick(&w.patches).id, r.below(1000) as f64 / 7.0 - 70.0)
                    }
                    (4, _) => {
                        let p = r.pick(&w.patches);
                        let keys: Vec<Key> = p.entries.keys().copied().collect();
                        edit::split(&w, p.id, &keys[..keys.len() / 2], product(&mut r, &w.space))
                    }
                    (5, _) => edit::merge(&w, r.pick(&w.patches).id, r.pick(&w.patches).id)
                        .unwrap_or_default(),
                    (6, _) => {
                        let x = r.pick(&w.space.points());
                        let e = edit_region(&mut r, &w, &x);
                        edit::drag(
                            &w,
                            &x,
                            &e,
                            Key { node: 0, field: 2 },
                            r.below(9) as i64,
                            VIEW,
                        )
                        .unwrap_or_default()
                    }
                    _ => {
                        let p = r.pick(&w.patches);
                        edit::edit(
                            p.id,
                            Key {
                                node: r.below(5) as Node,
                                field: r.pick(&FIELDS),
                            },
                            r.below(4) as i64,
                        )
                    }
                };
                undo.push(w.apply_all(&ops));
            }
            for ops in undo.iter().rev() {
                w.apply_all(ops);
            }
            assert_eq!(w, start, "{strength:?}, seed {seed}");
        }
    }
}

/// Law 6 and its listing (§11.1): an entry on an absent node does nothing,
/// and is listed as stale only when it is absent wherever the entry wins.
#[test]
fn stale_entries_are_inert_and_listed_only_when_absent_wherever_they_win() {
    for strength in [Strength::AxisKeys, Strength::Stored] {
        for seed in 0..SCRIPTS {
            let w = world(&mut Rng::new(seed), strength);
            let mut expected = Vec::new();
            for p in &w.patches {
                for (&key, &v) in &p.entries {
                    if key.field == PRESENT && v == 0 {
                        continue;
                    }
                    let wins: Vec<Point> = w
                        .space
                        .points()
                        .into_iter()
                        .filter(|x| w.winner(x, key).map(|q| q.id) == Some(p.id))
                        .collect();
                    if !wins.is_empty() && wins.iter().all(|x| !w.present(x, key.node)) {
                        expected.push((p.id, key));
                    }
                    for x in &wins {
                        if !w.present(x, key.node) {
                            assert_eq!(w.value(x, key), None, "seed {seed}: inert");
                        }
                    }
                }
            }
            assert_eq!(w.stale(), expected, "{strength:?}, seed {seed}");
        }
    }
}

/// Structure (§11.1): presence is decided before fields, and hiding
/// cascades: a node is shown only where its parent is.
#[test]
fn structure_is_resolved_before_fields_and_hiding_cascades() {
    for seed in 0..SCRIPTS {
        let w = world(&mut Rng::new(seed), Strength::Stored);
        for x in w.space.points() {
            for (&n, &parent) in &w.doc.parent {
                if let Some(p) = parent {
                    assert!(
                        !w.shown(&x, n) || w.shown(&x, p),
                        "seed {seed}: {n} shown under hidden {p}"
                    );
                    assert!(!w.present(&x, n) || w.present(&x, p), "seed {seed}");
                }
                for f in FIELDS {
                    let k = Key { node: n, field: f };
                    let expect = if w.present(&x, n) {
                        w.winner(&x, k)
                            .map(|q| q.entries[&k])
                            .or(w.doc.base.get(&k).copied())
                    } else {
                        None
                    };
                    assert_eq!(w.value(&x, k), expect, "seed {seed}");
                }
            }
        }
    }
}

/// No decision depends on values an axis does not have yet (the PT-4a
/// council): appending a slide and inserting one leaves every value at the
/// old points, a drag's ops, a new patch's rank, every merge's outcome and
/// the lints as they were.
#[test]
fn growing_an_axis_changes_no_decision() {
    for strength in [Strength::AxisKeys, Strength::Stored] {
        for seed in 0..SCRIPTS {
            let mut r = Rng::new(seed);
            let w = world(&mut r, strength);
            let mut g = w.clone();
            let slides = g.space.axes.get_mut(&SLIDES).unwrap();
            slides.push(ValueId(9), Vec::new());
            let at = r.below(6);
            slides.insert(at, ValueId(8));
            let old: BTreeSet<Point> = w.space.points().into_iter().collect();
            for ((x, a), (_, b)) in w
                .snapshot()
                .iter()
                .zip(g.snapshot().iter().filter(|(x, _)| old.contains(x)))
            {
                assert_eq!(
                    changed(&[(x.clone(), a.clone())], &[(x.clone(), b.clone())]),
                    BTreeSet::new(),
                    "{strength:?}, seed {seed}: values"
                );
            }
            let x = r.pick(&w.space.points());
            let e = edit_region(&mut r, &w, &x);
            let key = Key {
                node: r.below(5) as Node,
                field: r.pick(&FIELDS),
            };
            let v = 10 + r.below(5) as i64;
            assert_eq!(
                edit::drag(&w, &x, &e, key, v, VIEW),
                edit::drag(&g, &x, &e, key, v, VIEW),
                "{strength:?}, seed {seed}: drag"
            );
            let region = product(&mut r, &w.space);
            assert_eq!(
                w.default_rank(&region, 0),
                g.default_rank(&region, 0),
                "{strength:?}, seed {seed}: default rank"
            );
            for a in &w.patches {
                for b in &w.patches {
                    assert_eq!(
                        edit::merge(&w, a.id, b.id),
                        edit::merge(&g, a.id, b.id),
                        "{strength:?}, seed {seed}: merge"
                    );
                }
            }
            assert_eq!(w.ties(), g.ties(), "{strength:?}, seed {seed}: ties");
            assert_eq!(
                w.contradictions(),
                g.contradictions(),
                "{strength:?}, seed {seed}: contradictions"
            );
        }
    }
}

/// An evaluator written apart from `World`: the patches holding the point
/// that set the field, strongest by (layer, rank, id), over the base, with
/// presence decided first up the tree.
fn reference(w: &World, x: &Point, key: Key) -> Option<i64> {
    let raw = |k: Key| {
        let mut setters: Vec<&Patch> = w
            .patches
            .iter()
            .filter(|p| {
                p.entries.contains_key(&k)
                    && p.region
                        .sels
                        .iter()
                        .all(|(a, s)| s.contains(&w.space.axes[a], x[a]))
            })
            .collect();
        setters.sort_by(|a, b| {
            (a.layer, a.rank, a.id)
                .partial_cmp(&(b.layer, b.rank, b.id))
                .unwrap()
        });
        setters
            .last()
            .map(|p| p.entries[&k])
            .or(w.doc.base.get(&k).copied())
    };
    let mut n = Some(key.node);
    while let Some(m) = n {
        if raw(Key {
            node: m,
            field: PRESENT,
        }) != Some(1)
        {
            return None;
        }
        n = w.doc.parent[&m];
    }
    match key.field {
        PRESENT => Some(1),
        HIDDEN => {
            let mut n = Some(key.node);
            while let Some(m) = n {
                if raw(Key {
                    node: m,
                    field: HIDDEN,
                }) == Some(1)
                {
                    return Some(1);
                }
                n = w.doc.parent[&m];
            }
            Some(0)
        }
        _ => raw(key),
    }
}

#[test]
fn values_agree_with_a_reference_written_apart() {
    for seed in 0..SCRIPTS {
        let w = world(&mut Rng::new(seed), Strength::Stored);
        for (x, state) in w.snapshot() {
            for (k, v) in state {
                assert_eq!(v, reference(&w, &x, k), "seed {seed}: {k:?} at {x:?}");
            }
        }
    }
}
