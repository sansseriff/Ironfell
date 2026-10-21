//! Object snapping (plans/specs/snapping.md): a moving box snaps its edges
//! and centre to other boxes' edges and centres, each axis on its own, as in
//! Figma. Pure geometry in one space; the caller chooses the boxes, freezes
//! them for a gesture, and draws the guides.
//!
//! Every box contributes three features per axis (both edges and the
//! centre), so edge to edge, edge to centre and centre to edge all come from
//! one nearest-feature query.

use kurbo::{Rect, Vec2};

/// One snappable line of a box: `at` along the axis it snaps on, and the
/// marks it shows along the guide (an edge's two corners, a centre's middle).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Feature {
    at: f64,
    lo: f64,
    hi: f64,
}

/// A box's features on one axis: vertical lines (x) or horizontal ones (y).
fn features(r: Rect, vertical: bool) -> [Feature; 3] {
    let (a0, a1, b0, b1) = if vertical { (r.x0, r.x1, r.y0, r.y1) } else { (r.y0, r.y1, r.x0, r.x1) };
    let (am, bm) = ((a0 + a1) * 0.5, (b0 + b1) * 0.5);
    [Feature { at: a0, lo: b0, hi: b1 }, Feature { at: am, lo: bm, hi: bm }, Feature { at: a1, lo: b0, hi: b1 }]
}

/// How near two features must be to count as aligned once snapped.
fn eps(at: f64) -> f64 {
    1e-6_f64.max(at.abs() * 1e-9)
}

fn finite(r: Rect) -> bool {
    [r.x0, r.y0, r.x1, r.y1].iter().all(|v| v.is_finite())
}

/// What a box can snap to, sorted per axis for nearest-feature queries.
#[derive(Clone, Debug, Default)]
pub struct Targets {
    x: Vec<Feature>,
    y: Vec<Feature>,
}

/// A guide to draw: a vertical line `x = at` (or a horizontal one `y = at`)
/// through `marks`, the aligned points along it. The line spans the marks.
#[derive(Clone, Debug, PartialEq)]
pub struct Guide {
    pub vertical: bool,
    pub at: f64,
    pub marks: Vec<f64>,
}

impl Guide {
    /// The line's extent along itself.
    pub fn span(&self) -> (f64, f64) {
        self.marks.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &m| (lo.min(m), hi.max(m)))
    }
}

/// A line of a box being resized: where it is now, and how far it moves
/// per unit of pointer travel along its axis (1 for a dragged edge, −1 for
/// its mirror when resizing about the centre).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    pub at: f64,
    pub rate: f64,
}

/// A snap: the offset to add to the moving box, and the guides it lines up
/// along once moved by it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snap {
    pub delta: Vec2,
    pub guides: Vec<Guide>,
}

impl Targets {
    pub fn new(boxes: impl IntoIterator<Item = Rect>) -> Self {
        let mut t = Self::default();
        for r in boxes.into_iter().filter(|&r| finite(r)) {
            let r = r.abs();
            t.x.extend(features(r, true));
            t.y.extend(features(r, false));
        }
        t.x.sort_by(|a, b| a.at.total_cmp(&b.at));
        t.y.sort_by(|a, b| a.at.total_cmp(&b.at));
        t
    }

    pub fn is_empty(&self) -> bool {
        self.x.is_empty()
    }

    /// Snap `moving` to the nearest features within `tol` on each axis
    /// `axes` allows (x, y). An axis with nothing near is left alone.
    pub fn snap(&self, moving: Rect, tol: f64, axes: [bool; 2]) -> Snap {
        if !finite(moving) {
            return Snap::default();
        }
        let moving = moving.abs();
        let mut out = Snap::default();
        for (axis, (on, targets)) in axes.into_iter().zip([&self.x, &self.y]).enumerate() {
            if !on {
                continue;
            }
            let vertical = axis == 0;
            let Some(d) = best(targets, features(moving, vertical), tol) else { continue };
            if vertical {
                out.delta.x = d;
            } else {
                out.delta.y = d;
            }
            out.guides.extend(guides(targets, &features(moving + if vertical { Vec2::new(d, 0.0) } else { Vec2::new(0.0, d) }, vertical), vertical));
        }
        out
    }

    /// The pointer correction on one axis (x when `vertical`) that puts the
    /// nearest of `lines` on a target within `tol`, measured at the line.
    pub fn snap_lines(&self, vertical: bool, lines: &[Line], tol: f64) -> Option<f64> {
        let targets = if vertical { &self.x } else { &self.y };
        let mut best: Option<(f64, f64)> = None;
        for l in lines.iter().filter(|l| l.at.is_finite() && l.rate.abs() > 1e-9) {
            if let Some(d) = nearest(targets, l.at)
                && d.abs() <= tol
                && best.is_none_or(|(b, _)| d.abs() < b.abs())
            {
                best = Some((d, d / l.rate));
            }
        }
        best.map(|(_, correction)| correction)
    }

    /// The guides where `lines` of `moving` (x positions when `vertical`)
    /// meet targets; its other lines are left out.
    pub fn guides_at(&self, moving: Rect, vertical: bool, lines: &[f64]) -> Vec<Guide> {
        if !finite(moving) {
            return Vec::new();
        }
        let only: Vec<Feature> = features(moving.abs(), vertical).into_iter().filter(|f| lines.iter().any(|l| (l - f.at).abs() <= eps(f.at))).collect();
        guides(if vertical { &self.x } else { &self.y }, &only, vertical)
    }
}

/// The signed offset from `at` to the nearest target feature.
fn nearest(targets: &[Feature], at: f64) -> Option<f64> {
    let i = targets.partition_point(|t| t.at < at);
    targets[i.saturating_sub(1)..(i + 1).min(targets.len())].iter().map(|t| t.at - at).min_by(|a, b| a.abs().total_cmp(&b.abs()))
}

/// The smallest offset within `tol` that puts one of `moving` on a target.
fn best(targets: &[Feature], moving: [Feature; 3], tol: f64) -> Option<f64> {
    let mut best: Option<f64> = None;
    for m in moving {
        if let Some(d) = nearest(targets, m.at)
            && d.abs() <= tol
            && best.is_none_or(|b| d.abs() < b.abs())
        {
            best = Some(d);
        }
    }
    best
}

/// One guide per line where a feature of `moving` meets targets, marked at
/// each aligned point. Two of `moving`'s features on one line (a box with no
/// width) make one guide.
fn guides(targets: &[Feature], moving: &[Feature], vertical: bool) -> Vec<Guide> {
    let mut out: Vec<Guide> = Vec::new();
    for &m in moving {
        let e = eps(m.at);
        let i = targets.partition_point(|t| t.at < m.at - e);
        let hit: Vec<&Feature> = targets[i..].iter().take_while(|t| t.at <= m.at + e).collect();
        if hit.is_empty() {
            continue;
        }
        let g = match out.iter_mut().find(|g| (g.at - m.at).abs() <= e) {
            Some(g) => g,
            None => {
                // At the target's line: the moved one carries float noise.
                out.push(Guide { vertical, at: hit[0].at, marks: Vec::new() });
                out.last_mut().expect("just pushed")
            }
        };
        for p in [m.lo, m.hi].into_iter().chain(hit.iter().flat_map(|t| [t.lo, t.hi])) {
            if !g.marks.iter().any(|&q| (q - p).abs() <= eps(p)) {
                g.marks.push(p);
            }
        }
    }
    out
}
