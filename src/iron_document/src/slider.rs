//! Shared slider geometry for drawing, export and pointer acquisition.
use kurbo::{Point, Rect, RoundedRect};

pub fn thumb_half(h: f64) -> f64 { (h.max(0.0) / 3.0).min(8.0) }

pub fn thumb(w: f64, h: f64, t: f64) -> RoundedRect {
    let r = thumb_half(h);
    RoundedRect::new(w * t - r, h * 0.5 - r, w * t + r, h * 0.5 + r, r * 0.25)
}

pub fn track(w: f64, h: f64) -> RoundedRect {
    let half = (h.max(0.0) / 9.0).min(2.5);
    RoundedRect::new(0.0, h * 0.5 - half, w.max(0.0), h * 0.5 + half, half)
}

/// Keep the full height easy to press, including the thumb beyond either end.
pub fn contains(w: f64, h: f64, t: f64, p: Point) -> bool {
    Rect::new(0.0, 0.0, w, h).contains(p) || thumb_hit(w, h, t, p)
}

pub fn thumb_hit(w: f64, h: f64, t: f64, p: Point) -> bool {
    thumb(w, h, t).rect().inflate(3.0, 3.0).contains(p)
}
