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
    add_in(w, 0, region, entries)
}

/// A patch written in `layer`: 0 a definition's own, higher further out.
fn add_in(w: &mut World, layer: u32, region: Product, entries: &[(Key, i64)]) -> PatchId {
    let p = w.make(region, entries.iter().copied().collect(), layer);
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
        let ops = edit::drag(&w, &at(&[(SLIDE, A)]), &from(A), colour, 5, 0).unwrap();
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
        // Written in either order: where each is written decides (its
        // layer), not when (the council's probe made the document's first).
        for outer_first in [false, true] {
            let mut w = World::new(s.clone(), doc(1, &[(colour, 0)]), strength);
            let (bench_s, doc_s);
            if outer_first {
                doc_s = add_in(&mut w, 2, suffix(&[bench, o1, j]), &[(colour, 3)]);
                bench_s = add_in(&mut w, 1, suffix(&[o1, j]), &[(colour, 2)]);
                add_in(&mut w, 0, suffix(&[j]), &[(colour, 1)]);
            } else {
                add_in(&mut w, 0, suffix(&[j]), &[(colour, 1)]);
                bench_s = add_in(&mut w, 1, suffix(&[o1, j]), &[(colour, 2)]);
                doc_s = add_in(&mut w, 2, suffix(&[bench, o1, j]), &[(colour, 3)]);
            }
            let colours = |w: &World| [20, 21].map(|v| w.value(&at(&[(INST, v)]), colour).unwrap());
            assert_eq!(colours(&w), [3, 1], "{strength:?}");
            w.apply(&Op::Remove(doc_s));
            assert_eq!(colours(&w), [2, 1], "{strength:?}: the bench's opinion");
            w.apply(&Op::Remove(bench_s));
            assert_eq!(colours(&w), [1, 1], "{strength:?}: the definition's");
        }
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

/// An instance's override against its definition's hover pose: the
/// override is written outside the definition (the document), the pose
/// inside it, so the override is the stronger layer and wins at every
/// state, whatever the axis order and whichever was written first. So does
/// a slide's patch written in the document, which is why a deck that sets a
/// part's leaf loses that part's hover feedback on that slide unless its
/// region leaves `hover` out (round 2).
#[test]
fn an_outer_layer_beats_the_definition_s_pose_at_every_state() {
    const STATE: AxisId = AxisId(1);
    let (rest, hover) = (10, 11);
    let fill = key(0, 2);
    let hovered = || Product::all().with(STATE, Sel::Set(BTreeSet::from([ValueId(hover)])));
    for strength in BOTH {
        for order in [vec![INST, STATE, SLIDE], vec![SLIDE, STATE, INST]] {
            for pose_first in [true, false] {
                let mut s = Space::default();
                s.axes
                    .insert(INST, instances(&[(20, vec![1]), (21, vec![2])]));
                let mut st = Axis::new(Kind::Unordered);
                st.push(ValueId(rest), Vec::new());
                st.push(ValueId(hover), Vec::new());
                s.axes.insert(STATE, st);
                s.axes.insert(SLIDE, ordered(3));
                s.order = order.clone();
                let mut w = World::new(s, doc(1, &[(fill, 0)]), strength);
                if pose_first {
                    add_in(&mut w, 0, hovered(), &[(fill, 2)]);
                    add_in(&mut w, 1, suffix(&[1]), &[(fill, 3)]);
                } else {
                    add_in(&mut w, 1, suffix(&[1]), &[(fill, 3)]);
                    add_in(&mut w, 0, hovered(), &[(fill, 2)]);
                }
                add_in(&mut w, 1, only(C), &[(fill, 4)]);
                let got = [(20, hover, A), (21, hover, A), (20, rest, A), (21, hover, C)]
                    .map(|(i, st, sl)| {
                        w.value(&at(&[(INST, i), (STATE, st), (SLIDE, sl)]), fill)
                            .unwrap()
                    });
                assert_eq!(got[..3], [3, 2, 3], "{strength:?}, {order:?}");
                assert_eq!(got[3], 4, "{strength:?}, {order:?}: the slide's patch");
            }
        }
    }
}

/// Within one layer the axis order decides where neither region contains
/// the other: the document's per-instance patch against the document's own
/// pose for hover. With instances first, the instance wins, as an id beats
/// `:hover`.
#[test]
fn in_one_layer_the_axis_order_decides() {
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

/// A variant is a layer between its base definition and what places it
/// (patches.md §6.1), and its patches are anchored in it: they apply in
/// each placement of the variant and nowhere else, so this world is the
/// variant's placements (`o1`, `o5`). The base's patch, the variant's and
/// the document's override on `o1` are three layers: the override beats the
/// variant on its own instance, and the variant beats the base. No class is
/// needed, and placing the variant again changes no region.
#[test]
fn an_override_on_a_variant_s_instance_beats_the_variant() {
    let colour = key(0, 2);
    let (o1, o5) = (20, 21);
    for strength in BOTH {
        let mut s = Space::default();
        s.axes
            .insert(INST, instances(&[(o1, vec![1]), (o5, vec![5])]));
        for override_first in [true, false] {
            let mut w = World::new(s.clone(), doc(1, &[(colour, 0)]), strength);
            if override_first {
                add_in(&mut w, 2, suffix(&[1]), &[(colour, 3)]);
            }
            add_in(&mut w, 0, Product::all(), &[(colour, 1)]);
            add_in(&mut w, 1, Product::all(), &[(colour, 2)]);
            if !override_first {
                add_in(&mut w, 2, suffix(&[1]), &[(colour, 3)]);
            }
            assert_eq!(
                [o1, o5].map(|v| w.value(&at(&[(INST, v)]), colour).unwrap()),
                [3, 2],
                "{strength:?}"
            );
            let mut g = w.clone();
            g.space
                .axes
                .get_mut(&INST)
                .unwrap()
                .push(ValueId(22), vec![9]);
            assert_eq!(
                g.value(&at(&[(INST, 22)]), colour),
                Some(2),
                "{strength:?}: a new placement of the variant"
            );
        }
    }
}

/// Deleting C leaves `{C}` and `[C, C]` the same region as written, and
/// neither the empty one: tombstones are values as written (round 2: the
/// three relations were not transitive).
#[test]
fn a_deleted_value_stays_a_value_as_written() {
    let mut s = Space::default();
    s.axes.insert(SLIDE, ordered(4));
    s.axes.get_mut(&SLIDE).unwrap().delete(ValueId(C));
    let set = |v: &[u32]| Product::all().with(SLIDE, Sel::Set(v.iter().map(|x| ValueId(*x)).collect()));
    assert!(set(&[C]).same(&only(C), &s));
    assert!(!set(&[C]).same(&set(&[]), &s));
    assert!(!only(C).same(&set(&[]), &s));
    assert!(set(&[C]).is_empty(&s), "no point takes it");
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
        let ops = edit::drag(&w, &at(&[(SLIDE, C)]), &only(C), colour, 5, 0).unwrap();
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
        let ops = edit::drag(&w, &at(&[(SLIDE, C)]), &only(C), colour, 5, 0).unwrap();
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

/// With one placement, "this placement" and "every placement" select the
/// same instance today, yet "this placement" is the more specific as
/// written (Sol's case): the instance's override still beats the hover
/// pose.
#[test]
fn one_placement_still_overrides_the_hover_pose() {
    const STATE: AxisId = AxisId(1);
    let fill = key(0, 2);
    for strength in BOTH {
        let mut s = Space::default();
        s.axes.insert(INST, instances(&[(20, vec![1])]));
        let mut st = Axis::new(Kind::Unordered);
        st.push(ValueId(10), Vec::new());
        st.push(ValueId(11), Vec::new());
        s.axes.insert(STATE, st);
        s.order = vec![INST, STATE];
        let mut w = World::new(s, doc(1, &[(fill, 0)]), strength);
        add(&mut w, suffix(&[1]), &[(fill, 3)]);
        add(
            &mut w,
            Product::all().with(STATE, Sel::Set(BTreeSet::from([ValueId(11)]))),
            &[(fill, 2)],
        );
        assert_eq!(
            w.value(&at(&[(INST, 20), (STATE, 11)]), fill),
            Some(3),
            "{strength:?}"
        );
    }
}

/// Law 5's flag: `{a, b}` and `{b, c}` on one axis tie on specificity and
/// meet at b (the note's own cycle example), so the rank alone decides, and
/// the pair is listed. Two patches on one region are a deliberate order,
/// and are not.
#[test]
fn a_tie_of_specificity_is_flagged_and_one_region_is_not() {
    const STATE: AxisId = AxisId(1);
    let colour = key(0, 2);
    let set =
        |vs: &[u32]| Product::all().with(STATE, Sel::Set(vs.iter().map(|v| ValueId(*v)).collect()));
    let mut s = Space::default();
    let mut st = Axis::new(Kind::Unordered);
    for v in [10, 11, 12] {
        st.push(ValueId(v), Vec::new());
    }
    s.axes.insert(STATE, st);
    let mut w = World::new(s, doc(1, &[(colour, 0)]), Strength::Stored);
    let ab = add(&mut w, set(&[10, 11]), &[(colour, 1)]);
    add(&mut w, set(&[10, 11]), &[(colour, 2)]);
    assert_eq!(w.ties(), Vec::new(), "one region");
    let bc = add(&mut w, set(&[11, 12]), &[(colour, 3)]);
    assert!(w.ties().contains(&(ab, bc)), "{:?}", w.ties());
}

/// A removal is never stale: its node is absent because it worked. An
/// entry on an absent node is inert, and applies again when the node
/// returns.
#[test]
fn a_removal_is_not_stale_and_an_entry_returns_with_its_node() {
    let mut d = doc(2, &[]);
    d.parent.insert(2, Some(0));
    d.base.insert(key(2, PRESENT), 0);
    let mut s = Space::default();
    s.axes.insert(SLIDE, ordered(2));
    let mut w = World::new(s, d, Strength::Stored);
    let removal = add(&mut w, Product::all(), &[(key(1, PRESENT), 0)]);
    let waiting = add(&mut w, Product::all(), &[(key(2, 2), 7)]);
    assert_eq!(
        w.stale(),
        vec![(waiting, key(2, 2))],
        "{removal:?} is not listed"
    );
    assert_eq!(w.value(&at(&[(SLIDE, A)]), key(2, 2)), None);
    w.doc.base.insert(key(2, PRESENT), 1);
    assert_eq!(w.value(&at(&[(SLIDE, A)]), key(2, 2)), Some(7));
    assert!(w.stale().is_empty());
}

/// `*/j not */b/j` is strictly within `*/j not {*/a/b/j, */c/b/j}`: one
/// shorter exclusion removes more than two longer ones. Its key must be the
/// larger (Sol's round 2 counterexample to counting exclusions).
#[test]
fn one_short_exclusion_is_narrower_than_two_long_ones() {
    let (a, b, c, j) = (1, 2, 3, 4);
    let mut s = Space::default();
    s.axes.insert(
        INST,
        instances(&[(20, vec![a, b, j]), (21, vec![c, b, j]), (22, vec![9, b, j]), (23, vec![j])]),
    );
    let pat = |not: Vec<Vec<u32>>| Product::all().with(INST, Sel::Suffix { tail: vec![j], not });
    let narrow = pat(vec![vec![b, j]]);
    let wide = pat(vec![vec![a, b, j], vec![c, b, j]]);
    assert!(narrow.within(&wide, &s) && !wide.within(&narrow, &s));
    assert!(patch::keys(&s, &narrow) > patch::keys(&s, &wide));
}

/// Scope-anchored patterns (patches.md §4, P18): a patch written in `OscA`
/// with suffix `*/j` matches placements inside `OscA` (`bench/o1/j` and
/// `o2/j`), never a placement of another definition `OscB` that happens to
/// share step id `j`, and cannot match a step above its anchor scope.
#[test]
fn scope_anchored_patterns_match_only_inside_their_definition() {
    const DOC_SCOPE: ScopeId = 0;
    const BENCH_DEF: ScopeId = 1;
    const OSC_A: ScopeId = 2;
    const OSC_B: ScopeId = 3;
    let (bench, o1, o2, j) = (1, 2, 3, 4);
    let fill = key(0, 2);

    let mut inst = Axis::new(Kind::Path);
    // bench/o1/j: bench in DOC, o1 in BENCH_DEF, j in OSC_A
    inst.push_scoped(ValueId(20), vec![bench, o1, j], vec![DOC_SCOPE, BENCH_DEF, OSC_A]);
    // o2/j: o2 in DOC, j in OSC_A
    inst.push_scoped(ValueId(21), vec![o2, j], vec![DOC_SCOPE, OSC_A]);
    // bench/o1/j inside OSC_B (same local step id `j`, different definition)
    inst.push_scoped(ValueId(22), vec![bench, o1, j], vec![DOC_SCOPE, BENCH_DEF, OSC_B]);

    let mut s = Space::default();
    s.axes.insert(INST, inst);
    let mut w = World::new(s, doc(1, &[(fill, 0)]), Strength::Stored);

    // Patch anchored in OSC_A matching `*/j`: hits 20 and 21, never 22.
    let p_a = w.make_anchored(suffix(&[j]), [(fill, 7)].into(), 0, Some(OSC_A));
    w.apply(&Op::Add { patch: p_a, at: 0 });
    // Pattern `*/o1/j` anchored in OSC_A cannot reach above OSC_A (sub-path is `[j]` only).
    let p_reach = w.make_anchored(suffix(&[o1, j]), [(fill, 99)].into(), 0, Some(OSC_A));
    w.apply(&Op::Add { patch: p_reach, at: 1 });

    assert_eq!(w.value(&at(&[(INST, 20)]), fill), Some(7));
    assert_eq!(w.value(&at(&[(INST, 21)]), fill), Some(7));
    assert_eq!(w.value(&at(&[(INST, 22)]), fill), Some(0));
}

/// Enclosing-view instance coordinate (patches.md §4, P18): a `use` node's
/// presence is evaluated at its enclosing view's coordinate, not the view it
/// places, so removing the `use` hides its entire placed view with no
/// self-reference.
#[test]
fn enclosing_view_presence_hides_nested_placed_view_without_self_reference() {
    let (bench_use, inner_use, child_rect) = (0, 1, 2);
    let fill = key(child_rect, 2);
    let mut d = doc(3, &[(fill, 42)]);
    d.parent.insert(bench_use, None);
    d.parent.insert(inner_use, Some(bench_use));
    d.parent.insert(child_rect, None); // root inside placed view `o1`
    // View 21 (`bench/o1`) is placed by `inner_use` in enclosing view 20 (`bench`).
    d.enclosing_use.insert((INST, ValueId(21)), (inner_use, ValueId(20)));

    let mut s = Space::default();
    s.axes.insert(INST, instances(&[(20, vec![1]), (21, vec![1, 2])]));
    let mut w = World::new(s, d, Strength::Stored);

    assert_eq!(w.value(&at(&[(INST, 21)]), fill), Some(42));
    // Remove `inner_use` at its enclosing view `bench` (20): view 21 vanishes.
    let rm = add(&mut w, suffix(&[1]), &[(key(inner_use, PRESENT), 0)]);
    assert!(!w.present(&at(&[(INST, 20)]), inner_use));
    assert!(!w.present(&at(&[(INST, 21)]), child_rect));
    assert_eq!(w.value(&at(&[(INST, 21)]), fill), None);
    // Undoing restores the placed view.
    w.apply(&Op::Remove(rm));
    assert_eq!(w.value(&at(&[(INST, 21)]), fill), Some(42));
}

/// Optional facet presence (patches.md §5.2–5.3, P21): fields belonging to an
/// optional facet apply only where both the node and that facet are present;
/// entries on an absent facet are inert and reported stale, while removing a
/// facet is never stale.
#[test]
fn optional_facet_presence_controls_fields_and_staleness() {
    const STROKE: FacetId = 1;
    let stroke_w = key(1, 10);
    let stroke_present = key(1, facet_present(STROKE));
    let mut d = doc(2, &[(stroke_w, 2)]);
    d.field_facet.insert(10, STROKE);
    // Facet is absent in the base.
    d.base.insert(stroke_present, 0);

    let mut s = Space::default();
    s.axes.insert(SLIDE, ordered(3));
    let mut w = World::new(s, d, Strength::Stored);

    // Setting `stroke.width` on all slides while stroke facet is absent: inert and stale.
    let set_w = add(&mut w, Product::all(), &[(stroke_w, 5)]);
    assert_eq!(w.value(&at(&[(SLIDE, A)]), stroke_w), None);
    assert_eq!(w.stale(), vec![(set_w, stroke_w)]);
    assert!(edit::drag(&w, &at(&[(SLIDE, A)]), &Product::all(), stroke_w, 8, 0).is_err());

    // Add the optional facet from slide B on: `set_w` now applies on B and C (no longer stale).
    add(&mut w, from(B), &[(stroke_present, 1)]);
    assert_eq!(w.value(&at(&[(SLIDE, A)]), stroke_w), None);
    assert_eq!(w.value(&at(&[(SLIDE, B)]), stroke_w), Some(5));
    assert_eq!(w.value(&at(&[(SLIDE, C)]), stroke_w), Some(5));
    assert!(w.stale().is_empty());

    // Explicitly removing the facet on C is never stale.
    let rm_f = add(&mut w, only(C), &[(stroke_present, 0)]);
    assert_eq!(w.value(&at(&[(SLIDE, C)]), stroke_w), None);
    assert!(!w.stale().iter().any(|(id, _)| *id == rm_f));
}

/// Dedicated system-owned solve layer (engine.md §2.4, patches.md P17):
/// 6-tier precedence (session > binding > producer > solve > authored > base),
/// authored edit clearing the solve entry first, constraint release revealing
/// underlying value, and solver-free undo/redo via recorded op inverses.
#[test]
fn solve_layer_precedence_authored_clear_release_and_solver_free_undo() {
    let x_pos = key(1, 2);
    let mut s = Space::default();
    s.axes.insert(INST, instances(&[(20, vec![1, 2])]));
    let mut w = World::new(s, doc(2, &[(x_pos, 10)]), Strength::Stored);
    let pt = at(&[(INST, 20)]);

    // Authored instance patch sets x = 20.
    add_in(&mut w, 1, suffix(&[1, 2]), &[(x_pos, 20)]);
    assert_eq!(w.effective(&pt, x_pos, None, None), Some(20));

    // Stage 4 constraint solve writes x = 55 to the dedicated solve layer.
    let solve_inv = w.apply_all(&[edit::set_solve(pt.clone(), x_pos, 55)]);
    // Precedence check: solve beats authored (20) and base (10),
    // but loses to producer (70), binding (80), and session preview (90).
    assert_eq!(w.effective(&pt, x_pos, None, None), Some(55));
    assert_eq!(w.effective(&pt, x_pos, None, Some(70)), Some(70));
    assert_eq!(w.effective(&pt, x_pos, Some(80), Some(70)), Some(80));
    let sess = w.make(Product::all(), [(x_pos, 90)].into(), SESSION_LAYER);
    let sess_id = sess.id;
    w.apply(&Op::Add { patch: sess, at: w.patches.len() });
    assert_eq!(w.effective(&pt, x_pos, Some(80), Some(70)), Some(90));
    w.apply(&Op::Remove(sess_id));

    // Authored edit on the copy clears the solve entry first and writes x = 35.
    let before_edit = w.clone();
    let drag_ops = edit::drag(&w, &pt, &suffix(&[1, 2]), x_pos, 35, 1).unwrap();
    let drag_inv = w.apply_all(&drag_ops);
    assert!(!w.solve.contains_key(&(pt.clone(), x_pos)));
    assert_eq!(w.effective(&pt, x_pos, None, None), Some(35));

    // Solver-free undo restores the solved pose (55) directly from recorded ops.
    let redo_ops = w.apply_all(&drag_inv);
    assert_eq!(w, before_edit);
    assert_eq!(w.effective(&pt, x_pos, None, None), Some(55));

    // Constraint deletion releases the copy's solve entry, revealing authored 20.
    let rel_ops = edit::release_solve(&w, &pt, &[x_pos]);
    let rel_inv = w.apply_all(&rel_ops);
    assert_eq!(w.effective(&pt, x_pos, None, None), Some(20));
    // Undoing constraint deletion restores 55 without re-solving.
    w.apply_all(&rel_inv);
    assert_eq!(w.effective(&pt, x_pos, None, None), Some(55));
    // Redo of the authored edit + undo of original solve both stay exact.
    w.apply_all(&redo_ops);
    assert_eq!(w.effective(&pt, x_pos, None, None), Some(35));
    w.apply_all(&drag_inv);
    w.apply_all(&solve_inv);
    assert_eq!(w.effective(&pt, x_pos, None, None), Some(20));
}
