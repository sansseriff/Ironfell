//! Text nodes: their style and box from resolved leaves, measured through
//! `iron_text` (plans/specs/text.md §2–3). One function, [`measure`], so
//! bounds, picking, the renderer and the caret all agree on where a text's
//! glyphs are.

use crate::{NodeId, Store, Value};
use iron_text::{Align, StyleRun, TextLayout, TextStyle, VAlign};
use kurbo::Rect;
use std::sync::Arc;

/// How a text node's box sizes (the `text.sizing` leaf).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sizing {
    AutoWidth,
    AutoHeight,
    Fixed,
}

impl Sizing {
    pub fn parse(s: &str) -> Sizing {
        match s {
            "auto_height" => Sizing::AutoHeight,
            "fixed" => Sizing::Fixed,
            _ => Sizing::AutoWidth,
        }
    }
}

/// A measured text node.
#[derive(Clone, Debug)]
pub struct Measured {
    pub layout: Arc<TextLayout>,
    /// The node's box, from its origin: what bounds, picking and the
    /// selection outline use.
    pub frame: Rect,
    /// Where the layout's origin sits in the box (vertical alignment).
    pub offset: kurbo::Vec2,
    pub sizing: Sizing,
}

/// What a value reads as on a text node. `content` is a string leaf, but
/// a runtime signal or a binding may hand it a number or a quantity; those
/// show as `fmt` would show them, never as an error.
pub fn display(v: &Value) -> String {
    match v {
        Value::Str(s) | Value::Enum(s) => s.clone(),
        Value::Number(n) => crate::expr::format_number(*n, None),
        Value::Bool(b) => b.to_string(),
        Value::Quantity { si, dim } => crate::units::format(*si, *dim),
        other => format!("{other:?}"),
    }
}

/// The node's `font` leaves as a style.
pub fn style(store: &Store, id: NodeId) -> TextStyle {
    let read = |leaf: &str| store.resolved(id, leaf.parse().expect("declared leaf"));
    let num = |leaf: &str, d: f64| read(leaf).and_then(|v| v.as_f64()).filter(|v| v.is_finite()).unwrap_or(d);
    let word = |leaf: &str| read(leaf).and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default();
    let family = word("font.family");
    TextStyle {
        family: if family.is_empty() { iron_text::DEFAULT_FAMILY.to_owned() } else { family },
        size: num("font.size", 24.0).clamp(0.5, 4096.0) as f32,
        weight: num("font.weight", 400.0).clamp(1.0, 1000.0) as f32,
        italic: read("font.italic") == Some(Value::Bool(true)),
        line_height: num("font.line_height", 0.0) as f32,
        letter_spacing: num("font.letter_spacing", 0.0) as f32,
        align: Align::parse(&word("font.align")),
        tabular: read("font.tabular") == Some(Value::Bool(true)),
        underline: read("font.underline") == Some(Value::Bool(true)),
        strike: read("font.strike") == Some(Value::Bool(true)),
    }
}

/// The node's text as it should read now.
pub fn content(store: &Store, id: NodeId) -> String {
    store.resolved(id, "text.content".parse().expect("declared leaf")).map(|v| display(&v)).unwrap_or_default()
}

/// The node's style runs (`text.runs`), valid for its current content.
pub fn runs(store: &Store, id: NodeId) -> Vec<StyleRun> {
    let json = store.resolved(id, "text.runs".parse().expect("declared leaf")).and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default();
    iron_text::runs::normalize(&content(store, id), iron_text::runs::parse(&json))
}

/// Lay out a text node with its current content and style.
pub fn measure(store: &Store, id: NodeId) -> Measured {
    measure_with(store, id, &content(store, id), &runs(store, id))
}

/// Lay out a text node as if its content were `text` styled by `runs` (the
/// editor's draft).
pub fn measure_with(store: &Store, id: NodeId, text: &str, runs: &[StyleRun]) -> Measured {
    let read = |leaf: &str| store.resolved(id, leaf.parse().expect("declared leaf"));
    let num = |leaf: &str, d: f64| read(leaf).and_then(|v| v.as_f64()).filter(|v| v.is_finite()).unwrap_or(d);
    let sizing = Sizing::parse(read("text.sizing").and_then(|v| v.as_str().map(str::to_owned)).as_deref().unwrap_or("auto_width"));
    let style = style(store, id);
    let (w, h) = (num("size.w", 100.0).max(1.0), num("size.h", 100.0).max(1.0));
    // A table's cell: its box is the whole cell, its words inset by the
    // table's padding (layout-and-tables.md §3).
    let [px, py] = cell_pad(store, id);
    let layout = iron_text::engine().layout(text, &style, runs, (sizing != Sizing::AutoWidth).then_some((w - 2.0 * px).max(1.0)));
    let (fw, fh) = match sizing {
        Sizing::AutoWidth => (layout.width + 2.0 * px, layout.height + 2.0 * py),
        Sizing::AutoHeight => (w, layout.height + 2.0 * py),
        Sizing::Fixed => (w, h),
    };
    let valign = VAlign::parse(read("font.valign").and_then(|v| v.as_str().map(str::to_owned)).as_deref().unwrap_or("top"));
    let dy = if sizing == Sizing::Fixed { valign.offset(layout.height, fh - 2.0 * py) } else { 0.0 };
    Measured { layout, frame: Rect::new(0.0, 0.0, fw, fh), offset: kurbo::Vec2::new(px, py + dy), sizing }
}

/// How far a text's words sit inside its box: a table's cell padding, or
/// nothing for any other text.
pub fn cell_pad(store: &Store, id: NodeId) -> [f64; 2] {
    let doc = store.document();
    let row = doc.live(id).and_then(|n| n.parent).filter(|&p| doc.live(p).is_some_and(|n| n.ty.name() == "row"));
    match row.and_then(|r| crate::table::table_of(doc, r)) {
        Some(t) => {
            let num = |leaf: &str, d: f64| store.resolved(t, leaf.parse().expect("declared leaf")).and_then(|v| v.as_f64()).filter(|v| v.is_finite()).unwrap_or(d).max(0.0);
            [num("table.pad_x", 12.0), num("table.pad_y", 8.0)]
        }
        None => [0.0, 0.0],
    }
}

/// Plain text pasted onto the canvas (plans/specs/text.md §7): a fragment of
/// one `auto_width` text node in the default style and black, its box
/// centred on `centre`, for the paste path to place like any other.
pub fn fragment(text: &str, centre: [f64; 2]) -> Result<crate::copy::Fragment, String> {
    use crate::facet::{Fill, Font, Name, Text, Transform2d};
    use crate::{Actor, Affine2, LeafStruct, Op, OrderKey, Slot, TransactionInput, TypeId};
    use std::collections::BTreeMap;
    // Line endings as the editor keeps them; trailing ones dropped.
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let text = text.trim_end_matches('\n');
    let l = iron_text::engine().layout(text, &TextStyle::default(), &[], None);
    let mut store = Store::new();
    let id = store.document().next_ids(1)[0];
    let mut t = Transform2d::default();
    t.x = Slot::Const(Value::Number((centre[0] - l.width * 0.5).round()));
    t.y = Slot::Const(Value::Number((centre[1] - l.height * 0.5).round()));
    let mut body = Text::default();
    body.content = Slot::Const(Value::Str(text.to_owned()));
    let mut fill = Fill::default();
    fill.color = Slot::Const(Value::Color([0.0, 0.0, 0.0, 1.0]));
    let mut name = Name::default();
    let first_line: String = text.lines().next().unwrap_or("").chars().take(40).collect();
    name.text = Slot::Const(Value::Str(if first_line.is_empty() { "Text".into() } else { first_line }));
    let ops = vec![Op::Create { id, ty: TypeId::lookup("text").expect("registered"), parent: None, order: OrderKey::first(), facets: vec![t.wrap(), body.wrap(), Font::default().wrap(), fill.wrap(), name.wrap()] }];
    store
        .apply(TransactionInput { label: "paste text".into(), actor: Actor::System, ts: 0, idempotency_key: None, ops, exact: false })
        .map_err(|e| format!("the text did not apply: {e:?}"))?;
    Ok(crate::copy::Fragment {
        doc: store.document().clone(),
        roots: vec![id],
        extras: Vec::new(),
        homes: BTreeMap::from([(id, (None, OrderKey::first()))]),
        parent_world: BTreeMap::from([(id, Affine2::IDENTITY)]),
        externals: BTreeMap::new(),
        origin: "text".into(),
        dropped: Vec::new(),
    })
}
