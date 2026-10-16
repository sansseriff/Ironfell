//! SVG export and import for 2D compositions (plans/specs/svg-import-export.md).
//!
//! Iron's 2D space is SVG's: y-down, origin top-left, and a `transform2d`
//! is `translate(x y) rotate(deg) scale(sx sy)`. Export writes resolved
//! values (bindings flattened to what they evaluate to now) as SVG text;
//! import turns SVG into an [`iron_document::copy::Fragment`], so placement,
//! ids and the transaction go through the paste path.
//!
//! Neither direction drops anything silently: what cannot be carried over
//! is reported as a [`Cut`], one line for a person.
//!
//! **Iron's own data.** Export marks every element it writes for a node with
//! `data-iron-type`, plus the parameters SVG cannot hold (`data-iron-w`,
//! `data-iron-corners`, `data-iron-arc`, `data-iron-shadow`, …). Import
//! reads them back from the raw XML, keyed by element `id`, so rects and
//! ellipses come back as rects and ellipses, and Iron's numbers come back
//! exactly: export → import → export gives the same text.

mod export;
mod import;

pub use export::{export, Export, Scope};
pub use import::{import, is_svg_text, Imported};

/// One human-readable line about a feature that was dropped or approximated.
pub type Cut = String;

/// Cuts in the order first seen, each counted, so a file with a hundred
/// dashed strokes says so once.
#[derive(Default)]
pub(crate) struct Cuts(Vec<(String, usize)>);

impl Cuts {
    pub(crate) fn add(&mut self, line: impl Into<String>) {
        let line = line.into();
        match self.0.iter_mut().find(|(l, _)| *l == line) {
            Some((_, n)) => *n += 1,
            None => self.0.push((line, 1)),
        }
    }

    pub(crate) fn into_lines(self) -> Vec<Cut> {
        self.0.into_iter().map(|(l, n)| if n == 1 { l } else { format!("{l} (×{n})") }).collect()
    }
}

/// A number as export writes it: at most four decimals, no trailing zeros,
/// no `-0`. Import reads these back exactly, which is what makes the round
/// trip stable.
pub(crate) fn num(v: f64) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let s = format!("{:.4}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    match s {
        "-0" | "" => "0".into(),
        s => s.to_owned(),
    }
}

/// A shape parameter in Iron's data attributes: the shortest text that
/// reads back as the same f64. The outline written beside it is computed
/// from it, so a rounded parameter would draw a different outline on the
/// next export (a sweep of 4.7124 rather than 3π/2 takes another cubic).
pub(crate) fn exact(v: f64) -> String {
    if !v.is_finite() || v == 0.0 { "0".into() } else { format!("{v}") }
}

/// A stroke's dash lengths from `stroke.dash` (SVG's dasharray syntax:
/// numbers separated by spaces or commas). Empty for a solid line, or when
/// every length is zero or any is negative, as SVG says.
pub(crate) fn dashes(text: &str) -> Vec<f64> {
    let v: Vec<f64> = text.split(|c: char| c == ',' || c.is_whitespace()).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect();
    if v.iter().any(|&d| d < 0.0 || !d.is_finite()) || v.iter().all(|&d| d == 0.0) { Vec::new() } else { v }
}

/// `v` rounded as [`num`] writes it.
pub(crate) fn round4(v: f64) -> f64 {
    num(v).parse().unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_short_and_stable() {
        assert_eq!(num(0.0), "0");
        assert_eq!(num(-0.00001), "0");
        assert_eq!(num(1.5), "1.5");
        assert_eq!(num(100.0), "100");
        assert_eq!(num(1.0 / 3.0), "0.3333");
        assert_eq!(num(-2.25), "-2.25");
        assert_eq!(num(round4(12.345678)), num(12.345678));
    }

    #[test]
    fn cuts_are_counted() {
        let mut c = Cuts::default();
        c.add("a");
        c.add("b");
        c.add("a");
        assert_eq!(c.into_lines(), vec!["a (×2)".to_owned(), "b".to_owned()]);
    }
}
