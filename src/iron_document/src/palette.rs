//! Colour tokens (plans/specs/themes.md): named colours with a light and a
//! dark value, used as `@name` in a formula (`fill.color="{= @series-2}"`,
//! or `fill.color="@series-2"` in an edit), the way CSS uses variables.
//!
//! A document's tokens are `token` nodes in a `palette` root, each with
//! `token.light` and `token.dark`; the store publishes `token.color`, the
//! value for the theme it is shown in (`Store::set_theme`), under
//! `ProducerId::THEME`. A formula's `@name` reads that leaf, so a theme
//! switch or an edited token redraws everything that uses it through the
//! ordinary reactive graph. The first edit that uses a built-in name in a
//! document without it adds the built-in palette (`ensure_ops`), so every
//! token in use is in the document, where people and the agent can see and
//! revise it.

use std::collections::BTreeMap;

use crate::document::Document;
use crate::expr::{Expr, Leaf};
use crate::facet::{FacetKind, LeafPath, LeafStruct, Name, Token};
use crate::id::NodeId;
use crate::op::Op;
use crate::order::OrderKey;
use crate::registry::TypeId;
use crate::value::{Slot, Value};

/// The built-in tokens: the dataviz reference palette, validated for
/// contrast and colour-vision separation on each theme's surface (light
/// `#fcfcfb`, dark `#1a1a19`). Name, light, dark.
pub const BUILT_IN: &[(&str, &str, &str)] = &[
    ("surface", "#fcfcfb", "#1a1a19"),
    ("ink", "#0b0b0b", "#f0efec"),
    ("ink-2", "#52514e", "#c3c2b7"),
    ("muted", "#898781", "#898781"),
    ("grid", "#e1e0d9", "#2c2c2a"),
    ("axis", "#c3c2b7", "#383835"),
    ("accent", "#2a78d6", "#3987e5"),
    ("series-1", "#2a78d6", "#3987e5"),
    ("series-2", "#eb6834", "#d95926"),
    ("series-3", "#1baf7a", "#199e70"),
    ("series-4", "#eda100", "#c98500"),
    ("series-5", "#e87ba4", "#d55181"),
    ("series-6", "#008300", "#008300"),
    ("series-7", "#6250d6", "#9085e9"),
    ("series-8", "#e34948", "#e66767"),
];

/// `#rrggbb` as a colour.
pub fn hex(h: &str) -> [f32; 4] {
    let h = h.trim_start_matches('#');
    let c = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).map_or(0.0, |b| b as f32 / 255.0);
    [c(0), c(2), c(4), 1.0]
}

fn ty(n: &str) -> TypeId {
    TypeId::lookup(n).expect("registered")
}

fn name_of(doc: &Document, id: NodeId) -> Option<String> {
    doc.get::<Name>(id)?.text.constant()?.as_str().map(str::to_owned)
}

/// The document's tokens, by name (the first of a name wins).
pub fn tokens(doc: &Document) -> BTreeMap<String, NodeId> {
    let mut out = BTreeMap::new();
    for t in doc.nodes_of_type(ty("token")) {
        if doc.is_live(t)
            && !t.is_expanded()
            && let Some(n) = name_of(doc, t)
        {
            out.entry(n).or_insert(t);
        }
    }
    out
}

/// The leaf `@name` reads.
pub fn color_leaf(token: NodeId) -> Leaf {
    Leaf { node: token, path: LeafPath::new(FacetKind::Token, "color").expect("declared") }
}

/// Every token's leaf, by name: what formulas' `@name` resolve to.
pub fn leaves(doc: &Document) -> BTreeMap<String, Leaf> {
    tokens(doc).into_iter().map(|(n, t)| (n, color_leaf(t))).collect()
}

/// `token.color` for each token in the theme (`dark` or light): what the
/// store publishes.
pub fn publish(doc: &Document, dark: bool) -> BTreeMap<Leaf, Value> {
    let mut out = BTreeMap::new();
    for t in doc.nodes_of_type(ty("token")).filter(|&t| doc.is_live(t)) {
        let Some(tok) = doc.get::<Token>(t) else { continue };
        let pick = if dark { &tok.dark } else { &tok.light };
        if let Some(v) = pick.constant() {
            out.insert(color_leaf(t), v.clone());
        }
    }
    out
}

/// Token names `ops` use in their formulas.
fn used(ops: &[Op]) -> Vec<String> {
    let mut names = Vec::new();
    let mut add = |e: &Expr| names.extend(e.tokens());
    for op in ops {
        let facets: Vec<&crate::facet::Facet> = match op {
            Op::Set { slot: Slot::Bound(e), .. } => {
                add(e);
                continue;
            }
            Op::Create { facets, .. } => facets.iter().collect(),
            Op::AddFacet { facet, .. } => vec![facet],
            _ => continue,
        };
        for f in facets {
            for field in f.field_names() {
                if let Some(e) = f.get(field).and_then(|s| s.expr()) {
                    add(e);
                }
            }
        }
    }
    names
}

/// Before `ops` apply: when they use a built-in token the document lacks,
/// the ops that add it (a `palette` named "Theme" if the document has none,
/// and every missing built-in token, so a document's theme is whole). Ids
/// start after any `ops` create.
pub fn ensure_ops(doc: &Document, ops: &[Op]) -> Vec<Op> {
    let have = tokens(doc);
    let names = used(ops);
    if !names.iter().any(|n| !have.contains_key(n) && BUILT_IN.iter().any(|(b, _, _)| b == n)) {
        return Vec::new();
    }
    let mut next = doc.next_id();
    for op in ops {
        if let Op::Create { id, .. } = op
            && !id.is_session()
            && *id >= next
        {
            next = id.offset(1);
        }
    }
    let mut out = Vec::new();
    let palette = match doc.nodes_of_type(ty("palette")).find(|&p| doc.is_live(p) && !p.is_session()) {
        Some(p) => p,
        None => {
            let id = next;
            next = next.offset(1);
            let order = doc.roots().iter().filter(|r| !r.is_session()).last().and_then(|&r| doc.live(r)).map_or_else(OrderKey::first, |n| OrderKey::after(&n.order));
            out.push(Op::Create { id, ty: ty("palette"), parent: None, order, facets: vec![named("Theme")] });
            id
        }
    };
    let mut order = doc.children(palette).iter().filter_map(|&c| doc.live(c)).map(|n| n.order.clone()).max().map_or_else(OrderKey::first, |o| OrderKey::after(&o));
    for (name, light, dark) in BUILT_IN {
        if have.contains_key(*name) {
            continue;
        }
        let mut t = Token::default();
        t.light = Slot::Const(Value::Color(hex(light)));
        t.dark = Slot::Const(Value::Color(hex(dark)));
        out.push(Op::Create { id: next, ty: ty("token"), parent: Some(palette), order: order.clone(), facets: vec![named(name), t.wrap()] });
        next = next.offset(1);
        order = OrderKey::after(&order);
    }
    out
}

fn named(t: &str) -> crate::facet::Facet {
    let mut n = Name::default();
    n.text = Slot::Const(Value::Str(t.into()));
    n.wrap()
}

/// A new token `name` with these colours, in the document's palette (made
/// if there is none, holding the built-in theme too, so the document's
/// theme is whole).
pub fn add_ops(doc: &Document, name: &str, light: [f32; 4], dark: [f32; 4]) -> Vec<Op> {
    // The built-in theme first, as a first use would add it.
    let probe = Op::Set { id: NodeId::from_raw(0), path: LeafPath::new(FacetKind::Fill, "color").expect("declared"), slot: Slot::Bound(Expr::Token("surface".into())) };
    let mut ops = if tokens(doc).is_empty() { ensure_ops(doc, &[probe]) } else { Vec::new() };
    let palette = doc.nodes_of_type(ty("palette")).find(|&p| doc.is_live(p) && !p.is_session()).or_else(|| ops.iter().find_map(|o| match o {
        Op::Create { id, ty: t, .. } if *t == ty("palette") => Some(*id),
        _ => None,
    }));
    let mut next = doc.next_id();
    for op in &ops {
        if let Op::Create { id, .. } = op
            && *id >= next
        {
            next = id.offset(1);
        }
    }
    let order = ops.iter().rev().find_map(|o| match o {
        Op::Create { order, parent: Some(_), .. } => Some(OrderKey::after(order)),
        _ => None,
    });
    let (parent, order) = match palette {
        Some(p) => (p, order.unwrap_or_else(|| doc.children(p).iter().filter_map(|&c| doc.live(c)).map(|n| n.order.clone()).max().map_or_else(OrderKey::first, |o| OrderKey::after(&o)))),
        None => {
            let id = next;
            next = next.offset(1);
            let o = doc.roots().iter().filter(|r| !r.is_session()).last().and_then(|&r| doc.live(r)).map_or_else(OrderKey::first, |n| OrderKey::after(&n.order));
            ops.push(Op::Create { id, ty: ty("palette"), parent: None, order: o, facets: vec![named("Theme")] });
            (id, OrderKey::first())
        }
    };
    let mut t = Token::default();
    t.light = Slot::Const(Value::Color(light));
    t.dark = Slot::Const(Value::Color(dark));
    ops.push(Op::Create { id: next, ty: ty("token"), parent: Some(parent), order, facets: vec![named(name), t.wrap()] });
    ops
}
