//! Object snapping: edges and centres to edges and centres, per axis, the
//! nearest within reach, and the guides that show what lined up.

use iron_spatial::snap::{Guide, Line, Targets};
use kurbo::{Rect, Vec2};

fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect::new(x, y, x + w, y + h)
}

#[test]
fn an_edge_snaps_to_an_edge() {
    let t = Targets::new([rect(100.0, 0.0, 50.0, 50.0)]);
    // The moving box's right edge (97) is 3 from the target's left (100).
    let s = t.snap(rect(47.0, 200.0, 50.0, 50.0), 5.0, [true, true]);
    assert_eq!(s.delta, Vec2::new(3.0, 0.0));
    assert_eq!(s.guides, vec![Guide { vertical: true, at: 100.0, marks: vec![200.0, 250.0, 0.0, 50.0] }]);
}

#[test]
fn centres_snap_to_edges_and_edges_to_centres() {
    let t = Targets::new([rect(0.0, 0.0, 100.0, 100.0)]);
    // Moving centre x 98 → the target's right edge, 100.
    let s = t.snap(rect(88.0, 300.0, 20.0, 20.0), 4.0, [true, false]);
    assert_eq!(s.delta.x, 2.0);
    // Moving left edge 52 → the target's centre, 50.
    let s = t.snap(rect(52.0, 300.0, 10.0, 10.0), 4.0, [true, false]);
    assert_eq!(s.delta.x, -2.0);
    assert_eq!(s.guides[0].at, 50.0);
}

#[test]
fn the_nearest_feature_wins_and_axes_are_independent() {
    let t = Targets::new([rect(0.0, 0.0, 100.0, 100.0), rect(203.0, 0.0, 10.0, 10.0)]);
    // x: the left edge (199) is 4 from 203, the right (209) 4 from 213, but
    // the centre (204) is 1 from 203. y: the top (101) is 1 from 100.
    let s = t.snap(rect(199.0, 101.0, 10.0, 10.0), 5.0, [true, true]);
    assert_eq!(s.delta, Vec2::new(-1.0, -1.0));
}

#[test]
fn nothing_within_reach_leaves_the_box_alone() {
    let t = Targets::new([rect(0.0, 0.0, 10.0, 10.0)]);
    let s = t.snap(rect(30.0, 30.0, 10.0, 10.0), 5.0, [true, true]);
    assert_eq!(s.delta, Vec2::ZERO);
    assert!(s.guides.is_empty());
}

#[test]
fn a_held_axis_does_not_snap() {
    let t = Targets::new([rect(0.0, 0.0, 10.0, 10.0)]);
    let s = t.snap(rect(12.0, 12.0, 10.0, 10.0), 5.0, [false, true]);
    assert_eq!(s.delta, Vec2::new(0.0, -2.0));
    assert!(s.guides.iter().all(|g| !g.vertical));
}

#[test]
fn every_line_that_lines_up_gets_one_guide() {
    // Two targets share their left edge at x = 0; the moving box, the same
    // width as both, lines up all three features with both.
    let t = Targets::new([rect(0.0, 0.0, 40.0, 10.0), rect(0.0, 100.0, 40.0, 10.0)]);
    let s = t.snap(rect(2.0, 50.0, 40.0, 10.0), 5.0, [true, false]);
    assert_eq!(s.delta.x, -2.0);
    let ats: Vec<f64> = s.guides.iter().map(|g| g.at).collect();
    assert_eq!(ats, vec![0.0, 20.0, 40.0]);
    // The left edge's guide runs from the first target's top to the second's bottom.
    assert_eq!(s.guides[0].span(), (0.0, 110.0));
}

#[test]
fn reversed_and_non_finite_boxes_are_safe() {
    let t = Targets::new([Rect::new(10.0, 10.0, 0.0, 0.0), Rect::new(f64::NAN, 0.0, 1.0, 1.0)]);
    let s = t.snap(rect(11.0, 40.0, 5.0, 5.0), 2.0, [true, true]);
    assert_eq!(s.delta.x, -1.0);
    assert_eq!(t.snap(Rect::new(f64::NAN, 0.0, 1.0, 1.0), 2.0, [true, true]).delta, Vec2::ZERO);
}

#[test]
fn a_resized_edge_snaps_by_the_pointer() {
    let t = Targets::new([rect(0.0, 0.0, 100.0, 100.0)]);
    // The top edge, dragged (rate 1), is at -3: 3 short of the target's top.
    assert_eq!(t.snap_lines(false, &[Line { at: -3.0, rate: 1.0 }], 5.0), Some(3.0));
    // Resizing about the centre, the mirrored bottom (rate -1) is at 98:
    // the pointer moves -2 to bring it to 100.
    assert_eq!(t.snap_lines(false, &[Line { at: 10.0, rate: 1.0 }, Line { at: 98.0, rate: -1.0 }], 5.0), Some(-2.0));
    assert_eq!(t.snap_lines(false, &[Line { at: 20.0, rate: 1.0 }], 5.0), None);
}

#[test]
fn guides_at_marks_only_the_lines_asked_for() {
    let t = Targets::new([rect(0.0, 0.0, 100.0, 100.0)]);
    // A box from y 0 to 100 shares its top, middle and bottom with the
    // target; asked about its top alone, only that line has a guide.
    let g = t.guides_at(rect(200.0, 0.0, 50.0, 100.0), false, &[0.0]);
    assert_eq!(g, vec![Guide { vertical: false, at: 0.0, marks: vec![200.0, 250.0, 0.0, 100.0] }]);
}
