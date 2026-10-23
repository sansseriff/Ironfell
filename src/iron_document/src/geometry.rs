//! Packed path geometry (plans/specs/packed-paths.md): a path's outlines as
//! one value, `path.d`, instead of `contour` and `point` nodes.
//!
//! A path is packed by default; its anchors become nodes only while
//! something addresses them one by one (`contour::expand_points`). The value
//! is immutable and shared (`Value::Geometry` holds an `Arc`), so cloning a
//! document, an op or an inverse never copies the anchors, and an edit
//! replaces the whole value.
//!
//! In files the anchors are numbers, six per anchor, not SVG path data:
//! path data re-derives a handle from absolute control points, which loses
//! bits, and has nowhere to put an open contour's end handles.

use crate::shape::Anchor;
use kurbo::{BezPath, Point, Rect, Shape as _, Vec2};
use std::fmt;
use std::sync::{Arc, OnceLock};

/// One outline of a packed path: its anchors in order, closed back to the
/// first or not.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct PackedContour {
    pub closed: bool,
    pub anchors: Vec<Anchor>,
}

impl PackedContour {
    /// Whether it is drawn closed: closed, with three anchors or more (two
    /// close onto themselves, which draws nothing more).
    pub fn draws_closed(&self) -> bool {
        self.closed && self.anchors.len() > 2
    }
}

/// A path's outlines. The drawn outlines and bounds are built on first read
/// and kept with the value.
#[derive(Default)]
pub struct Geometry {
    contours: Vec<PackedContour>,
    cache: OnceLock<Outlines>,
}

struct Outlines {
    outline: Arc<BezPath>,
    fill: Arc<BezPath>,
    bounds: Rect,
}

impl Geometry {
    pub fn new(contours: Vec<PackedContour>) -> Geometry {
        Geometry { contours, cache: OnceLock::new() }
    }

    /// The empty geometry, shared: `path.d`'s default.
    pub fn empty() -> Arc<Geometry> {
        static EMPTY: OnceLock<Arc<Geometry>> = OnceLock::new();
        EMPTY.get_or_init(|| Arc::new(Geometry::default())).clone()
    }

    pub fn contours(&self) -> &[PackedContour] {
        &self.contours
    }

    pub fn into_contours(self) -> Vec<PackedContour> {
        self.contours
    }

    pub fn is_empty(&self) -> bool {
        self.contours.is_empty()
    }

    pub fn anchor_count(&self) -> usize {
        self.contours.iter().map(|c| c.anchors.len()).sum()
    }

    /// Every anchor mapped through `f`, contours and closedness kept.
    pub fn map_anchors(&self, mut f: impl FnMut(Anchor) -> Anchor) -> Geometry {
        Geometry::new(self.contours.iter().map(|c| PackedContour { closed: c.closed, anchors: c.anchors.iter().map(|&a| f(a)).collect() }).collect())
    }

    fn outlines(&self) -> &Outlines {
        self.cache.get_or_init(|| {
            let mut outline = BezPath::new();
            let mut fill = BezPath::new();
            for c in &self.contours {
                append(&mut outline, &c.anchors, c.closed);
                if c.draws_closed() {
                    append(&mut fill, &c.anchors, true);
                }
            }
            let bounds = outline.bounding_box();
            Outlines { outline: Arc::new(outline), fill: Arc::new(fill), bounds }
        })
    }

    /// Every contour's outline: what is stroked.
    pub fn outline(&self) -> &Arc<BezPath> {
        &self.outlines().outline
    }

    /// The closed contours' outline: what is filled. An open contour has no
    /// inside.
    pub fn fill_outline(&self) -> &Arc<BezPath> {
        &self.outlines().fill
    }

    /// The outline's tight bounds, in the path's space.
    pub fn bounds(&self) -> Rect {
        self.outlines().bounds
    }

    /// The file form: `[[closed, [x, y, ix, iy, ox, oy, …]], …]`.
    pub fn to_json(&self) -> serde_json::Value {
        use serde_json::Value as J;
        J::Array(
            self.contours
                .iter()
                .map(|c| {
                    let nums = c.anchors.iter().flat_map(|a| [a.at.x, a.at.y, a.inn.x, a.inn.y, a.out.x, a.out.y]).map(|n| serde_json::json!(n)).collect();
                    J::Array(vec![J::Bool(c.closed), J::Array(nums)])
                })
                .collect(),
        )
    }

    pub fn from_json(v: &serde_json::Value) -> Result<Geometry, String> {
        use serde_json::Value as J;
        let items = v.as_array().ok_or("geo must be a list of contours")?;
        let mut contours = Vec::with_capacity(items.len());
        for (i, c) in items.iter().enumerate() {
            let bad = || format!("geo contour {i} must be [closed, [x, y, ix, iy, ox, oy, …]]");
            let [J::Bool(closed), J::Array(nums)] = c.as_array().map(Vec::as_slice).ok_or_else(bad)? else { return Err(bad()) };
            if nums.len() % 6 != 0 {
                return Err(format!("geo contour {i} has {} numbers; each anchor is six", nums.len()));
            }
            let nums: Vec<f64> = nums.iter().map(|n| n.as_f64()).collect::<Option<_>>().ok_or_else(bad)?;
            let anchors = nums.chunks_exact(6).map(|a| Anchor { at: Point::new(a[0], a[1]), inn: Vec2::new(a[2], a[3]), out: Vec2::new(a[4], a[5]) }).collect();
            contours.push(PackedContour { closed: *closed, anchors });
        }
        Ok(Geometry::new(contours))
    }
}

/// One contour's outline: a line between two anchors whose facing handles
/// are both zero, a cubic otherwise.
pub(crate) fn append(path: &mut BezPath, points: &[Anchor], closed: bool) {
    let Some(first) = points.first() else { return };
    path.move_to(first.at);
    let seg = |path: &mut BezPath, a: &Anchor, b: &Anchor| {
        if a.out == Vec2::ZERO && b.inn == Vec2::ZERO {
            path.line_to(b.at);
        } else {
            path.curve_to(a.at + a.out, b.at + b.inn, b.at);
        }
    };
    for w in points.windows(2) {
        seg(path, &w[0], &w[1]);
    }
    if closed && points.len() > 2 {
        seg(path, points.last().expect("non-empty"), first);
        path.close_path();
    }
}

impl Clone for Geometry {
    fn clone(&self) -> Geometry {
        Geometry::new(self.contours.clone())
    }
}

impl PartialEq for Geometry {
    fn eq(&self, other: &Geometry) -> bool {
        std::ptr::eq(self, other) || self.contours == other.contours
    }
}

impl fmt::Debug for Geometry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Geometry({self})")
    }
}

impl fmt::Display for Geometry {
    /// The model surface's summary: `38 contours, 2,140 anchors`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let plural = |n: usize, one: &str| format!("{} {one}{}", thousands(n), if n == 1 { "" } else { "s" });
        write!(f, "{}, {}", plural(self.contours.len(), "contour"), plural(self.anchor_count(), "anchor"))
    }
}

fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(x: f64, y: f64, ix: f64, iy: f64, ox: f64, oy: f64) -> Anchor {
        Anchor { at: Point::new(x, y), inn: Vec2::new(ix, iy), out: Vec2::new(ox, oy) }
    }

    #[test]
    fn the_file_form_is_exact() {
        // Numbers that do not survive `(at + inn) - at`, an open contour's
        // end handles, a one-anchor contour and an empty one.
        let g = Geometry::new(vec![
            PackedContour { closed: true, anchors: vec![a(0.1, 0.7, 0.2, 1e-17, 0.0, 0.0), a(1.0 / 3.0, 2.0, 0.0, 0.0, -5.5, 3.25), a(9.0, 0.30000000000000004, 1.0, 1.0, 0.0, 0.0)] },
            PackedContour { closed: false, anchors: vec![a(3.0, 4.0, -1.0, -2.0, 0.5, 0.25), a(7.0, 8.0, 0.1, 0.2, 0.3, 0.4)] },
            PackedContour { closed: false, anchors: vec![a(1.0, 1.0, 0.0, 0.0, 0.0, 0.0)] },
            PackedContour { closed: true, anchors: vec![] },
        ]);
        let text = serde_json::to_string(&g.to_json()).unwrap();
        let back = Geometry::from_json(&serde_json::from_str(&text).unwrap()).unwrap();
        assert_eq!(back, g);
        for (x, y) in back.contours.iter().zip(&g.contours) {
            for (p, q) in x.anchors.iter().zip(&y.anchors) {
                for (u, v) in [(p.at.x, q.at.x), (p.at.y, q.at.y), (p.inn.x, q.inn.x), (p.inn.y, q.inn.y), (p.out.x, q.out.x), (p.out.y, q.out.y)] {
                    assert_eq!(u.to_bits(), v.to_bits());
                }
            }
        }
        assert!(Geometry::from_json(&serde_json::json!([[true, [1, 2, 3]]])).is_err());
        assert!(Geometry::from_json(&serde_json::json!([["yes", []]])).is_err());
    }

    #[test]
    fn outlines_follow_the_contour_rules() {
        let square = |closed| PackedContour { closed, anchors: vec![a(0.0, 0.0, 0.0, 0.0, 0.0, 0.0), a(10.0, 0.0, 0.0, 0.0, 0.0, 0.0), a(10.0, 10.0, 0.0, 0.0, 0.0, 0.0)] };
        let g = Geometry::new(vec![square(true), PackedContour { closed: false, anchors: vec![a(20.0, 0.0, 0.0, 0.0, 0.0, 0.0), a(30.0, 5.0, 0.0, 0.0, 0.0, 0.0)] }]);
        assert_eq!(g.bounds(), Rect::new(0.0, 0.0, 30.0, 10.0));
        // Only the closed contour fills.
        assert_eq!(g.fill_outline().bounding_box(), Rect::new(0.0, 0.0, 10.0, 10.0));
        assert_eq!(g.to_string(), "2 contours, 5 anchors");
        assert_eq!(thousands(603023), "603,023");
        // A clone starts with no cache but compares equal.
        assert_eq!(g.clone(), g);
    }
}
