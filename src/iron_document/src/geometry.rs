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

    /// SVG path data (`M 0 0 L 10 5 C …`, every command, absolute or
    /// relative; arcs become cubics) as packed contours: how a `path.d`
    /// written or bound as text becomes geometry (plans/specs/plot.md §2).
    pub fn from_svg_path(data: &str) -> Result<Geometry, String> {
        use kurbo::PathEl;
        let path = BezPath::from_svg(data).map_err(|e| format!("not SVG path data ({e}): {}", preview(data)))?;
        let mut contours: Vec<PackedContour> = Vec::new();
        let mut cur: Option<PackedContour> = None;
        let finish = |c: Option<PackedContour>, out: &mut Vec<PackedContour>| {
            if let Some(mut c) = c {
                // A closed contour whose last anchor sits on its first: one
                // anchor, the last's in-handle moved onto the first.
                if c.closed && c.anchors.len() > 1 {
                    let (first, last) = (c.anchors[0], c.anchors[c.anchors.len() - 1]);
                    if (first.at - last.at).hypot() < 1e-9 {
                        c.anchors[0].inn = last.inn;
                        c.anchors.pop();
                    }
                }
                if !c.anchors.is_empty() {
                    out.push(c);
                }
            }
        };
        let corner = |p: Point| Anchor { at: p, inn: Vec2::ZERO, out: Vec2::ZERO };
        // `cur` is only the pen left by a Z, with nothing drawn from it yet:
        // not a contour unless a segment follows.
        let mut pen_only = false;
        for el in path.elements() {
            if !matches!(el, PathEl::MoveTo(_) | PathEl::ClosePath) {
                pen_only = false;
            }
            match *el {
                PathEl::MoveTo(p) => {
                    if std::mem::take(&mut pen_only) {
                        cur = None;
                    }
                    finish(cur.take(), &mut contours);
                    cur = Some(PackedContour { closed: false, anchors: vec![corner(p)] });
                }
                PathEl::LineTo(p) => cur.get_or_insert_with(PackedContour::default).anchors.push(corner(p)),
                PathEl::QuadTo(c, p) => {
                    let c0 = cur.get_or_insert_with(PackedContour::default);
                    let from = c0.anchors.last().map_or(p, |a| a.at);
                    let (c1, c2) = (from + (c - from) * (2.0 / 3.0), p + (c - p) * (2.0 / 3.0));
                    if let Some(last) = c0.anchors.last_mut() {
                        last.out = c1 - last.at;
                    }
                    c0.anchors.push(Anchor { at: p, inn: c2 - p, out: Vec2::ZERO });
                }
                PathEl::CurveTo(c1, c2, p) => {
                    let c0 = cur.get_or_insert_with(PackedContour::default);
                    if let Some(last) = c0.anchors.last_mut() {
                        last.out = c1 - last.at;
                    }
                    c0.anchors.push(Anchor { at: p, inn: c2 - p, out: Vec2::ZERO });
                }
                PathEl::ClosePath => {
                    if let Some(c) = cur.as_mut() {
                        c.closed = true;
                    }
                    let start = cur.as_ref().and_then(|c| c.anchors.first()).map(|a| a.at);
                    finish(cur.take(), &mut contours);
                    // SVG: after Z the pen is back at the subpath's start.
                    if let Some(at) = start {
                        cur = Some(PackedContour { closed: false, anchors: vec![corner(at)] });
                        pen_only = true;
                    }
                }
            }
        }
        if pen_only {
            cur = None;
        }
        finish(cur, &mut contours);
        Ok(Geometry::new(contours))
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

/// One drawn segment of a path, with which subpath it belongs to.
#[derive(Clone, Copy)]
struct Piece {
    seg: kurbo::PathSeg,
    len: f64,
    sub: usize,
}

/// A path's segments by subpath (a closing `Z` is its line back to the
/// start), and whether each subpath is closed.
fn pieces(path: &BezPath) -> (Vec<Piece>, Vec<bool>) {
    use kurbo::{ParamCurveArclen, PathEl, PathSeg};
    const ACC: f64 = 1e-3;
    let (mut out, mut closed) = (Vec::new(), Vec::new());
    let (mut start, mut at) = (Point::ZERO, Point::ZERO);
    let mut push = |seg: PathSeg, out: &mut Vec<Piece>, sub: usize| out.push(Piece { len: seg.arclen(ACC), seg, sub });
    for el in path.elements() {
        let sub = closed.len().saturating_sub(1);
        match *el {
            PathEl::MoveTo(p) => {
                closed.push(false);
                (start, at) = (p, p);
            }
            PathEl::LineTo(p) => {
                push(PathSeg::Line(kurbo::Line::new(at, p)), &mut out, sub);
                at = p;
            }
            PathEl::QuadTo(c, p) => {
                push(PathSeg::Quad(kurbo::QuadBez::new(at, c, p)), &mut out, sub);
                at = p;
            }
            PathEl::CurveTo(c1, c2, p) => {
                push(PathSeg::Cubic(kurbo::CubicBez::new(at, c1, c2, p)), &mut out, sub);
                at = p;
            }
            PathEl::ClosePath => {
                if (at - start).hypot() > 1e-9 {
                    push(PathSeg::Line(kurbo::Line::new(at, start)), &mut out, sub);
                }
                if let Some(c) = closed.last_mut() {
                    *c = true;
                }
                at = start;
            }
        }
    }
    (out, closed)
}

/// How Trim Paths treats a path of several subpaths (After Effects' "Trim
/// Multiple Shapes").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrimMode {
    /// Each subpath trimmed by the same fractions at once (tick marks all
    /// growing together). After Effects' default.
    Simultaneously,
    /// The subpaths as one path, one after another (a curve with breaks
    /// drawn along its length).
    Individually,
}

impl TrimMode {
    pub fn named(name: &str) -> TrimMode {
        if name == "individually" { TrimMode::Individually } else { TrimMode::Simultaneously }
    }
}

/// After Effects' Trim Paths: the part of `path` from `start` to `end`
/// (fractions of its length, clamped to 0..1), shifted by `offset`, which
/// wraps around the end. A span over a closed subpath's start joins there;
/// a closed subpath kept whole stays closed.
pub fn trim(path: &BezPath, start: f64, end: f64, offset: f64, mode: TrimMode) -> BezPath {
    let (start, end) = (start.clamp(0.0, 1.0), end.clamp(0.0, 1.0));
    if end <= start {
        return BezPath::new();
    }
    if end - start >= 1.0 {
        return path.clone();
    }
    let (all, closed) = pieces(path);
    let mut out = BezPath::new();
    match mode {
        TrimMode::Individually => cut(&all, &closed, start, end, offset, &mut out),
        TrimMode::Simultaneously => {
            for sub in 0..closed.len() {
                let one: Vec<Piece> = all.iter().copied().filter(|p| p.sub == sub).collect();
                cut(&one, &closed, start, end, offset, &mut out);
            }
        }
    }
    out
}

/// Append the kept part of `segs` (one stretch of 0..1 by arc length).
fn cut(segs: &[Piece], closed: &[bool], start: f64, end: f64, offset: f64, out: &mut BezPath) {
    use kurbo::{ParamCurve, ParamCurveArclen};
    const ACC: f64 = 1e-3;
    let total: f64 = segs.iter().map(|p| p.len).sum();
    if !(total > 0.0) {
        return;
    }
    let a = (start + offset).rem_euclid(1.0);
    let b = a + (end - start);
    // The kept spans in 0..1; a span past 1 wraps to the start.
    let spans: Vec<(f64, f64)> = if b <= 1.0 { vec![(a, b)] } else { vec![(a, 1.0), (0.0, b - 1.0)] };
    // The pen carries across the wrap, so a closed subpath's start joins.
    let mut pen: Option<Point> = None;
    for (lo, hi) in spans {
        let (lo, hi) = (lo * total, hi * total);
        let mut at = 0.0;
        // The subpath being drawn and whether every segment of it so far
        // was kept whole from its first.
        let mut whole: Option<(usize, bool)> = None;
        for (i, p) in segs.iter().enumerate() {
            let (s0, s1) = (at, at + p.len);
            at = s1;
            if s1 <= lo || s0 >= hi || p.len <= 0.0 {
                whole = None;
                continue;
            }
            let t0 = if lo > s0 { p.seg.inv_arclen(lo - s0, ACC) } else { 0.0 };
            let t1 = if hi < s1 { p.seg.inv_arclen(hi - s0, ACC) } else { 1.0 };
            let piece = p.seg.subsegment(t0..t1);
            let from = piece.start();
            let first_of_sub = i == 0 || segs[i - 1].sub != p.sub;
            if pen.is_none_or(|q| (q - from).hypot() > 1e-9) {
                out.move_to(from);
            }
            whole = match whole {
                Some((sub, w)) if sub == p.sub => Some((sub, w && t0 == 0.0 && t1 == 1.0)),
                _ => Some((p.sub, first_of_sub && t0 == 0.0 && t1 == 1.0)),
            };
            out.push(piece.as_path_el());
            pen = Some(piece.end());
            let last_of_sub = segs.get(i + 1).is_none_or(|n| n.sub != p.sub);
            if last_of_sub && closed.get(p.sub) == Some(&true) && whole == Some((p.sub, true)) {
                out.close_path();
                pen = None;
            }
        }
    }
}

fn preview(data: &str) -> String {
    let short: String = data.chars().take(60).collect();
    if short.len() < data.len() { format!("{short}…") } else { short }
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
