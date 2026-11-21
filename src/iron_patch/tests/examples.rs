//! Worked examples (plans/handoff-patches.md PT-3's three, and the note's
//! §11.5 additions), each with its expected values written down first. Each
//! runs under both strength rules that are total orders; where they differ,
//! the example says which gives the right answer.

use iron_patch::edit::{self, Op};
use iron_patch::*;
use std::collections::BTreeSet;

const BOTH: [Strength; 2] = [Strength::Stored, Strength::AxisKeys];

fn key(node: Node, field: Field) -> Key {
    Key { node, field }
}

/// Nodes 0..n, all children of 0 and present, with these base values.
fn doc(nodes: u32, base: &[(Key, i64)]) -> Doc {
    let mut d = Doc::default();
    for n in 0..nodes {
        d.parent.insert(n, if n == 0 { None } else { Some(0) });
        d.base.insert(key(n, PRESENT), 1);
    }
    for (k, v) in base {
        d.base.insert(*k, *v);
    }
    d
}

fn ordered(n: u32) -> Axis {
    let mut a = Axis::new(Kind::Ordered);
    for v in 0..n {
        a.push(ValueId(v), Vec::new());
    }
    a
}

fn add(w: &mut World, region: Product, entries: &[(Key, i64)]) -> PatchId {
    let p = w.make(region, entries.iter().copied().collect());
    let id = p.id;
    let at = w.patches.len();
    w.apply(&Op::Add { patch: p, at });
    id
}

fn at(pairs: &[(AxisId, u32)]) -> Point {
    pairs.iter().map(|(a, v)| (*a, ValueId(*v))).collect()
}

const SLIDE: AxisId = AxisId(0);
const A: u32 = 0;
const B: u32 = 1;
const C: u32 = 2;
const D: u32 = 3;
const E: u32 = 4;

fn from(v: u32) -> Product {
    Product::all().with(SLIDE, Sel::Range(Bound::Incl(ValueId(v)), Bound::Open))
}

fn only(v: u32) -> Product {
    Product::all().with(
        SLIDE,
        Sel::Range(Bound::Incl(ValueId(v)), Bound::Incl(ValueId(v))),
    )
}

fn span(a: u32, b: u32) -> Product {
    Product::all().with(
        SLIDE,
        Sel::Range(Bound::Incl(ValueId(a)), Bound::Incl(ValueId(b))),
    )
}

fn deck(strength: Strength, slides: u32) -> World {
    let mut s = Space::default();
    s.axes.insert(SLIDE, ordered(slides));
    World::new(
        s,
        doc(1, &[(key(0, 2), 0), (key(0, 3), 0), (key(0, 4), 0)]),
        strength,
    )
}

/// A deck whose slides B, C and D build on A: "from here on" patches, and a
/// one-slide exception that the next slide drops. Editing A from here on
/// reaches every slide that does not set the value itself.
#[test]
fn a_deck_that_builds_on_itself() {
    let (title, colour, note) = (key(0, 2), key(0, 3), key(0, 4));
    for strength in BOTH {
        let mut w = deck(strength, 4);
        add(&mut w, from(B), &[(title, 1)]);
        add(&mut w, from(C), &[(colour, 2)]);
        add(&mut w, only(C), &[(note, 9)]);
        let row =
            |w: &World, s| [title, colour, note].map(|k| w.value(&at(&[(SLIDE, s)]), k).unwrap());
        assert_eq!(
            [A, B, C, D].map(|s| row(&w, s)),
            [[0, 0, 0], [1, 0, 0], [1, 2, 9], [1, 2, 0]],
            "{strength:?}"
        );
        let ops = edit::drag(&w, &at(&[(SLIDE, A)]), &from(A), colour, 5).unwrap();
        w.apply_all(&ops);
        assert_eq!(
            [A, B, C, D].map(|s| row(&w, s)[1]),
            [5, 5, 2, 2],
            "{strength:?}: C and D keep their own colour"
        );
    }
}

/// `std:plot`'s `vis` machine: the base is the hidden pose, `{shown}` the
/// shown one; a field neither pose sets is shared. A transition between
/// them is the driver's (§11.4), not a point.
#[test]
fn the_plot_s_vis_machine() {
    const VIS: AxisId = AxisId(1);
    let (hidden, shown) = (10, 11);
    let (opacity, y, ink) = (key(1, 2), key(1, 3), key(1, 4));
    for strength in BOTH {
        let mut s = Space::default();
        let mut vis = Axis::new(Kind::Unordered);
        vis.push(ValueId(hidden), Vec::new());
        vis.push(ValueId(shown), Vec::new());
        s.axes.insert(VIS, vis);
        let mut w = World::new(s, doc(2, &[(opacity, 0), (y, 10), (ink, 7)]), strength);
        add(
            &mut w,
            Product::all().with(VIS, Sel::Set(BTreeSet::from([ValueId(shown)]))),
            &[(opacity, 1), (y, 0)],
        );
        let pose = |w: &World, v| [opacity, y, ink].map(|k| w.value(&at(&[(VIS, v)]), k).unwrap());
        assert_eq!(
            (pose(&w, hidden), pose(&w, shown)),
            ([0, 10, 7], [1, 0, 7]),
            "{strength:?}"
        );
    }
}

const INST: AxisId = AxisId(2);

fn instances(paths: &[(u32, Vec<u32>)]) -> Axis {
    let mut a = Axis::new(Kind::Path);
    for (v, p) in paths {
        a.push(ValueId(*v), p.clone());
    }
    a
}

fn suffix(tail: &[u32]) -> Product {
    Product::all().with(
        INST,
        Sel::Suffix {
            tail: tail.to_vec(),
            not: Vec::new(),
        },
    )
}

/// Two levels of nesting, outermost wins: a meter `j` in an oscilloscope
/// `o1` placed in a bench, and another oscilloscope `o2` placed alone. The
/// oscilloscope's definition sets its part `j` (`*/j`), the bench's sets its
/// `o1`'s (`*/o1/j`), the document sets the bench's (`bench/o1/j`). More
/// fixed steps are stronger, so the outermost opinion wins, and removing it
/// falls through to the next.
#[test]
fn two_levels_of_nesting_outermost_wins() {
    let (bench, o1, j, o2) = (1, 2, 3, 4);
    let colour = key(0, 2);
    for strength in BOTH {
        let mut s = Space::default();
        s.axes.insert(
            INST,
            instances(&[(20, vec![bench, o1, j]), (21, vec![o2, j])]),
        );
        let mut w = World::new(s, doc(1, &[(colour, 0)]), strength);
        add(&mut w, suffix(&[j]), &[(colour, 1)]);
        let bench_s = add(&mut w, suffix(&[o1, j]), &[(colour, 2)]);
        let doc_s = add(&mut w, suffix(&[bench, o1, j]), &[(colour, 3)]);
        let colours = |w: &World| [20, 21].map(|v| w.value(&at(&[(INST, v)]), colour).unwrap());
        assert_eq!(colours(&w), [3, 1], "{strength:?}");
        w.apply(&Op::Remove(doc_s));
        assert_eq!(colours(&w), [2, 1], "{strength:?}: the bench's opinion");
        w.apply(&Op::Remove(bench_s));
        assert_eq!(colours(&w), [1, 1], "{strength:?}: the definition's");
    }
}

/// `[B, D]` against `[C, ∞)`, an overlap on one axis: a later start wins
/// (the ordered axis's key), so C and D take `[C, ∞)`'s value.
#[test]
fn a_later_start_wins_on_one_axis() {
    let colour = key(0, 3);
    for strength in BOTH {
        let mut w = deck(strength, 5);
        add(&mut w, span(B, D), &[(colour, 1)]);
        add(&mut w, from(C), &[(colour, 2)]);
        assert_eq!(
            [A, B, C, D, E].map(|s| w.value(&at(&[(SLIDE, s)]), colour).unwrap()),
            [0, 1, 2, 2, 2],
            "{strength:?}"
        );
    }
}

/// An instance's override against its definition's hover pose: at
/// (this instance, hover) neither region contains the other, and the axis
/// order decides. With instances first, the instance wins, as today (its
/// constant replaces the channel binding) and as an id beats `:hover`.
#[test]
fn an_instance_override_beats_the_definition_s_pose_by_axis_order() {
    const STATE: AxisId = AxisId(1);
    let (rest, hover) = (10, 11);
    let fill = key(0, 2);
    for strength in BOTH {
        for (order, expect) in [
            (vec![INST, STATE], [3, 2, 3]),
            (vec![STATE, INST], [2, 2, 3]),
        ] {
            let mut s = Space::default();
            s.axes
                .insert(INST, instances(&[(20, vec![1]), (21, vec![2])]));
            let mut st = Axis::new(Kind::Unordered);
            st.push(ValueId(rest), Vec::new());
            st.push(ValueId(hover), Vec::new());
            s.axes.insert(STATE, st);
            s.order = order.clone();
            let mut w = World::new(s, doc(1, &[(fill, 0)]), strength);
            add(
                &mut w,
                Product::all().with(STATE, Sel::Set(BTreeSet::from([ValueId(hover)]))),
                &[(fill, 2)],
            );
            add(&mut w, suffix(&[1]), &[(fill, 3)]);
            let got = [(20, hover), (21, hover), (20, rest)]
                .map(|(i, s)| w.value(&at(&[(INST, i), (STATE, s)]), fill).unwrap());
            assert_eq!(got, expect, "{strength:?}, {order:?}");
        }
    }
}

/// A class (instances named one by one) against a nesting pattern on the
/// same axis. The path key counts trailing steps every selected instance
/// shares: a class across contexts shares none, so `*/j` beats it; a class
/// whose members all end in `j` ties with `*/j` on the key, and the rank
/// decides (the newer). PT-4a must say whether that tie is a lint.
#[test]
fn a_class_against_a_nesting_pattern() {
    let colour = key(0, 2);
    let (m1, m2, m3) = (20, 21, 22);
    for strength in BOTH {
        let mut s = Space::default();
        s.axes.insert(
            INST,
            instances(&[(m1, vec![5, 1]), (m2, vec![6, 1]), (m3, vec![7, 9])]),
        );
        let mut w = World::new(s.clone(), doc(1, &[(colour, 0)]), strength);
        add(&mut w, suffix(&[1]), &[(colour, 1)]);
        add(
            &mut w,
            Product::all().with(INST, Sel::Set([m1, m2, m3].map(ValueId).into())),
            &[(colour, 2)],
        );
        assert_eq!(
            [m1, m2, m3].map(|v| w.value(&at(&[(INST, v)]), colour).unwrap()),
            [1, 1, 2],
            "{strength:?}: across contexts"
        );
        let mut w = World::new(s, doc(1, &[(colour, 0)]), strength);
        add(&mut w, suffix(&[1]), &[(colour, 1)]);
        add(
            &mut w,
            Product::all().with(INST, Sel::Set([m1, m2].map(ValueId).into())),
            &[(colour, 2)],
        );
        assert_eq!(
            [m1, m2].map(|v| w.value(&at(&[(INST, v)]), colour).unwrap()),
            [2, 2],
            "{strength:?}: a tie, the newer"
        );
    }
}

/// The note's §5 flow, and its central flaw (§11.1). At C the user sets C's
/// own colour, then applies it from B on, where B's own patch sets it too:
/// both rules show the user's colour from B. But with a `[C, D]` patch
/// present, widening the `{C}` patch to `[C, ∞)` makes `[C, D]` the more
/// specific: with derived strength C changes, though it was in the patch
/// throughout. Stored strength keeps it.
#[test]
fn apply_from_b_on_and_the_widening_flaw() {
    let colour = key(0, 3);
    for strength in BOTH {
        let mut w = deck(strength, 4);
        add(&mut w, from(B), &[(colour, 7)]);
        let ops = edit::drag(&w, &at(&[(SLIDE, C)]), &only(C), colour, 5).unwrap();
        w.apply_all(&ops);
        let Some(Op::Add { patch, .. }) = ops.first() else {
            panic!("a new patch on {{C}}")
        };
        let mine = patch.id;
        assert_eq!(
            [A, B, C, D].map(|s| w.value(&at(&[(SLIDE, s)]), colour).unwrap()),
            [0, 7, 5, 7],
            "{strength:?}"
        );
        w.apply_all(&edit::set_region(mine, from(B)));
        assert_eq!(
            [A, B, C, D].map(|s| w.value(&at(&[(SLIDE, s)]), colour).unwrap()),
            [0, 5, 5, 5],
            "{strength:?}: applied from B on"
        );

        let mut w = deck(strength, 5);
        add(&mut w, span(C, D), &[(colour, 6)]);
        let ops = edit::drag(&w, &at(&[(SLIDE, C)]), &only(C), colour, 5).unwrap();
        w.apply_all(&ops);
        let Some(Op::Add { patch, .. }) = ops.first() else {
            panic!()
        };
        w.apply_all(&edit::set_region(patch.id, from(C)));
        let c = w.value(&at(&[(SLIDE, C)]), colour).unwrap();
        match strength {
            Strength::Stored => assert_eq!(c, 5, "stored: C keeps its colour"),
            _ => assert_eq!(c, 6, "derived: C changes, the flaw"),
        }
    }
}

/// Merge refused (§11.2): two patches on one region, a third between them
/// in strength. Each of the two wins a different field against the third;
/// one merged patch with one rank cannot keep both.
#[test]
fn a_merge_that_would_change_a_value_is_refused() {
    let (a, b) = (key(0, 2), key(0, 3));
    let mut w = deck(Strength::Stored, 2);
    let weak = add(&mut w, Product::all(), &[(a, 1)]);
    let middle = add(&mut w, only(A), &[(a, 5), (b, 5)]);
    let strong = add(&mut w, Product::all(), &[(b, 2)]);
    w.apply_all(&[
        Op::Rank {
            patch: weak,
            rank: 1.0,
        },
        Op::Rank {
            patch: middle,
            rank: 2.0,
        },
        Op::Rank {
            patch: strong,
            rank: 3.0,
        },
    ]);
    let x = at(&[(SLIDE, A)]);
    assert_eq!((w.value(&x, a), w.value(&x, b)), (Some(5), Some(2)));
    assert!(edit::merge(&w, weak, strong).is_err());
}

/// Axis values by stable id (§11.1): deleting C keeps `[C, ∞)` starting
/// where C was; a slide inserted after C's place is inside; a slide moved
/// before it is outside.
#[test]
fn an_interval_keeps_its_place_when_its_endpoint_is_deleted() {
    let colour = key(0, 3);
    for strength in BOTH {
        let mut w = deck(strength, 4);
        add(&mut w, from(C), &[(colour, 2)]);
        w.space.axes.get_mut(&SLIDE).unwrap().delete(ValueId(C));
        assert_eq!(
            [A, B, D].map(|s| w.value(&at(&[(SLIDE, s)]), colour).unwrap()),
            [0, 0, 2],
            "{strength:?}"
        );
        w.space.axes.get_mut(&SLIDE).unwrap().insert(3, ValueId(E));
        assert_eq!(
            w.value(&at(&[(SLIDE, E)]), colour),
            Some(2),
            "{strength:?}: inserted after C's place"
        );
        w.space.axes.get_mut(&SLIDE).unwrap().move_to(ValueId(D), 1);
        assert_eq!(
            w.value(&at(&[(SLIDE, D)]), colour),
            Some(0),
            "{strength:?}: moved before it"
        );
    }
}

/// Structure (§11.1): a weak patch's entry on a node a stronger patch adds
/// applies where the node is added; elsewhere it is inert, and since it
/// applies somewhere it is not listed as stale. One on a node that is never
/// added is.
#[test]
fn an_entry_on_an_added_node_applies_where_it_is_added() {
    const VIS: AxisId = AxisId(1);
    let added = 1;
    for strength in BOTH {
        let mut s = Space::default();
        let mut vis = Axis::new(Kind::Unordered);
        vis.push(ValueId(10), Vec::new());
        vis.push(ValueId(11), Vec::new());
        s.axes.insert(VIS, vis);
        let mut d = doc(1, &[]);
        d.parent.insert(added, Some(0));
        d.base.insert(key(added, PRESENT), 0);
        d.parent.insert(2, Some(0));
        let mut w = World::new(s, d, strength);
        let weak = add(
            &mut w,
            Product::all(),
            &[(key(added, 2), 7), (key(2, 2), 1)],
        );
        add(
            &mut w,
            Product::all().with(VIS, Sel::Set(BTreeSet::from([ValueId(11)]))),
            &[(key(added, PRESENT), 1)],
        );
        assert_eq!(
            (
                w.value(&at(&[(VIS, 10)]), key(added, 2)),
                w.value(&at(&[(VIS, 11)]), key(added, 2))
            ),
            (None, Some(7)),
            "{strength:?}"
        );
        assert_eq!(w.stale(), vec![(weak, key(2, 2))], "{strength:?}");
    }
}
