//! Style runs: styling part of a text (plans/specs/text.md §2.1).
//!
//! A text node has one style (its `font` component) and, over it, a list of
//! runs, each a byte range of the content with the properties it overrides.
//! The list is stored in one leaf (`text.runs`) as JSON, so an edit to the
//! words and their styling is one `Set` each, and a whole-node style change
//! clears the property it set from every run, as Figma's does.
//!
//! Ranges are byte offsets into the content, on char boundaries, sorted and
//! disjoint; [`normalize`] restores that after any change.

use serde::{Deserialize, Serialize};

/// The properties a run may override; `None` inherits the node's.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RunStyle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub weight: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    /// Linear RGBA; overrides the node's fill for these glyphs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<[f32; 4]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub underline: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strike: Option<bool>,
}

impl RunStyle {
    pub fn is_empty(&self) -> bool {
        *self == RunStyle::default()
    }

    /// `self` with every property `patch` sets taken from it.
    pub fn merged(&self, patch: &RunStyle) -> RunStyle {
        RunStyle {
            family: patch.family.clone().or_else(|| self.family.clone()),
            size: patch.size.or(self.size),
            weight: patch.weight.or(self.weight),
            italic: patch.italic.or(self.italic),
            color: patch.color.or(self.color),
            underline: patch.underline.or(self.underline),
            strike: patch.strike.or(self.strike),
        }
    }

    /// `self` without the properties `which` sets.
    pub fn without(&self, which: &RunStyle) -> RunStyle {
        RunStyle {
            family: if which.family.is_some() { None } else { self.family.clone() },
            size: if which.size.is_some() { None } else { self.size },
            weight: if which.weight.is_some() { None } else { self.weight },
            italic: if which.italic.is_some() { None } else { self.italic },
            color: if which.color.is_some() { None } else { self.color },
            underline: if which.underline.is_some() { None } else { self.underline },
            strike: if which.strike.is_some() { None } else { self.strike },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StyleRun {
    /// Byte range of the content: `start..end`.
    pub start: usize,
    pub end: usize,
    #[serde(flatten)]
    pub style: RunStyle,
}

/// Runs from the leaf's JSON; empty or malformed text is no runs.
pub fn parse(json: &str) -> Vec<StyleRun> {
    if json.trim().is_empty() {
        return Vec::new();
    }
    serde_json::from_str(json).unwrap_or_default()
}

/// The leaf's JSON for `runs`; empty for none.
pub fn to_json(runs: &[StyleRun]) -> String {
    if runs.is_empty() { String::new() } else { serde_json::to_string(runs).expect("plain data") }
}

fn floor_char(text: &str, i: usize) -> usize {
    let mut i = i.min(text.len());
    while !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Sorted, disjoint, on char boundaries, within the text, without empty
/// ranges or empty styles, adjacent equal styles merged.
pub fn normalize(text: &str, runs: Vec<StyleRun>) -> Vec<StyleRun> {
    let mut runs: Vec<StyleRun> = runs
        .into_iter()
        .map(|r| StyleRun { start: floor_char(text, r.start), end: floor_char(text, r.end), style: r.style })
        .filter(|r| r.end > r.start && !r.style.is_empty())
        .collect();
    runs.sort_by_key(|r| r.start);
    let mut out: Vec<StyleRun> = Vec::with_capacity(runs.len());
    for mut r in runs {
        if let Some(last) = out.last_mut() {
            if r.start < last.end {
                r.start = last.end;
                if r.end <= r.start {
                    continue;
                }
            }
            if last.end == r.start && last.style == r.style {
                last.end = r.end;
                continue;
            }
        }
        out.push(r);
    }
    out
}

/// Apply `patch` to `start..end`: covered runs take its properties, gaps
/// in the range become runs of it alone.
pub fn apply(text: &str, runs: &[StyleRun], start: usize, end: usize, patch: &RunStyle) -> Vec<StyleRun> {
    let (start, end) = (floor_char(text, start.min(end)), floor_char(text, start.max(end)));
    if start == end {
        return runs.to_vec();
    }
    let mut out = Vec::new();
    let mut at = start;
    for r in runs {
        // Outside the range, unchanged; overlapping, split in three.
        if r.end <= start || r.start >= end {
            out.push(r.clone());
            continue;
        }
        if r.start < start {
            out.push(StyleRun { start: r.start, end: start, style: r.style.clone() });
        }
        let (s, e) = (r.start.max(start), r.end.min(end));
        if s > at {
            out.push(StyleRun { start: at, end: s, style: patch.clone() });
        }
        out.push(StyleRun { start: s, end: e, style: r.style.merged(patch) });
        at = e;
        if r.end > end {
            out.push(StyleRun { start: end, end: r.end, style: r.style.clone() });
        }
    }
    if at < end {
        out.push(StyleRun { start: at, end, style: patch.clone() });
    }
    normalize(text, out)
}

/// Every run without the properties `which` sets: the node now sets them.
pub fn clear(text: &str, runs: &[StyleRun], which: &RunStyle) -> Vec<StyleRun> {
    normalize(text, runs.iter().map(|r| StyleRun { start: r.start, end: r.end, style: r.style.without(which) }).collect())
}

/// The style of the character at byte offset `i`: the run covering it.
pub fn of_char(runs: &[StyleRun], i: usize) -> RunStyle {
    runs.iter().find(|r| r.start <= i && i < r.end).map(|r| r.style.clone()).unwrap_or_default()
}

/// The style in effect at a byte offset (the run covering it, or the one
/// ending there), for the inspector and for typing.
pub fn at(runs: &[StyleRun], i: usize) -> RunStyle {
    runs.iter().find(|r| r.start <= i && i < r.end).or_else(|| runs.iter().find(|r| r.end == i && i > r.start)).map(|r| r.style.clone()).unwrap_or_default()
}

/// Carry runs from `old` text to `new` across one edit: the common prefix
/// and suffix keep their styling, typed characters take the style of the
/// one before them (Figma's rule), deleted ones take theirs away.
pub fn remap(old: &str, new: &str, runs: &[StyleRun]) -> Vec<StyleRun> {
    if runs.is_empty() || old == new {
        return runs.to_vec();
    }
    let prefix = old.chars().zip(new.chars()).take_while(|(a, b)| a == b).map(|(a, _)| a.len_utf8()).sum::<usize>();
    // The common suffix, not overlapping the prefix in either text.
    let room = (old.len() - prefix).min(new.len() - prefix);
    let mut suffix = 0;
    for (a, b) in old[prefix..].chars().rev().zip(new[prefix..].chars().rev()) {
        if a != b || suffix + a.len_utf8() > room {
            break;
        }
        suffix += a.len_utf8();
    }
    let old_end = old.len() - suffix;
    let inserted = new.len() - suffix - prefix;
    let shift = |o: usize| o + new.len() - old.len();
    let map = |o: usize, is_end: bool, starts_before: bool| -> usize {
        if o < prefix {
            o
        } else if o == prefix && is_end && starts_before {
            // A run ending where typing began takes the typed text.
            prefix + inserted
        } else if o >= old_end {
            shift(o)
        } else if o == prefix {
            prefix
        } else {
            prefix + inserted
        }
    };
    let out = runs
        .iter()
        .map(|r| {
            let starts_before = r.start < prefix;
            StyleRun { start: map(r.start, false, starts_before), end: map(r.end, true, starts_before), style: r.style.clone() }
        })
        .collect();
    normalize(new, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bold() -> RunStyle {
        RunStyle { weight: Some(700.0), ..Default::default() }
    }

    #[test]
    fn apply_splits_and_merges() {
        let t = "hello world";
        let r = apply(t, &[], 0, 5, &bold());
        assert_eq!(r, vec![StyleRun { start: 0, end: 5, style: bold() }]);
        let red = RunStyle { color: Some([1.0, 0.0, 0.0, 1.0]), ..Default::default() };
        let r = apply(t, &r, 3, 8, &red);
        assert_eq!(r.len(), 3);
        assert_eq!((r[0].start, r[0].end), (0, 3));
        assert_eq!((r[1].start, r[1].end, r[1].style.weight, r[1].style.color.is_some()), (3, 5, Some(700.0), true));
        assert_eq!((r[2].start, r[2].end, r[2].style.weight), (5, 8, None));
        // Clearing weight leaves only the colour.
        let c = clear(t, &r, &bold());
        assert_eq!(c.len(), 1);
        assert_eq!((c[0].start, c[0].end), (3, 8));
    }

    #[test]
    fn typing_extends_the_run_before_it() {
        let runs = vec![StyleRun { start: 0, end: 5, style: bold() }];
        let r = remap("hello world", "hello!! world", &runs);
        assert_eq!((r[0].start, r[0].end), (0, 7));
        // Typing before a run moves it.
        let r = remap("ab hello", "abX hello", &[StyleRun { start: 3, end: 8, style: bold() }]);
        assert_eq!((r[0].start, r[0].end), (4, 9));
        // Deleting inside shrinks it.
        let r = remap("hello world", "helo world", &runs);
        assert_eq!((r[0].start, r[0].end), (0, 4));
        // Deleting all of it removes it.
        assert!(remap("hello world", " world", &runs).is_empty());
    }

    #[test]
    fn json_round_trip() {
        let runs = vec![StyleRun { start: 0, end: 2, style: RunStyle { underline: Some(true), ..Default::default() } }];
        assert_eq!(parse(&to_json(&runs)), runs);
        assert_eq!(to_json(&runs), r#"[{"start":0,"end":2,"underline":true}]"#);
    }
}
