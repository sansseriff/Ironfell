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

use crate::Tree;
use std::collections::BTreeMap;

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
    ("slider-accent", "#ae743e", "#d5a06a"),
    ("slider-track", "#d5d2cc", "#5b5853"),
    ("slider-thumb", "#ffffff", "#ffffff"),
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

fn name_of(doc: Tree<'_>, id: NodeId) -> Option<String> {
    doc.get::<Name>(id)?.text.constant()?.as_str().map(str::to_owned)
}

/// The document's tokens, by name (first in document root order, then id order,
/// matching `tokens_and_publish`).
pub fn tokens(doc: Tree<'_>) -> BTreeMap<String, NodeId> {
    let token_ty = ty("token");
    let mut out = BTreeMap::new();
    for &root in doc.roots() {
        for t in doc.nodes_of_type_in(root, token_ty) {
            if doc.is_live(t)
                && !t.is_expanded()
                && let Some(n) = name_of(doc, t)
            {
                out.entry(n).or_insert(t);
            }
        }
    }
    out
}

/// The leaf `@name` reads.
pub fn color_leaf(token: NodeId) -> Leaf {
    Leaf { node: token, path: LeafPath::new(FacetKind::Token, "color").expect("declared") }
}

/// Every token's leaf, by name: what formulas' `@name` resolve to.
pub fn leaves(doc: Tree<'_>) -> BTreeMap<String, Leaf> {
    tokens(doc).into_iter().map(|(n, t)| (n, color_leaf(t))).collect()
}

/// Every token's leaf and both light & dark theme color tables, collected
/// through keyed per-root reads so `ThemeMaker` records exact structural
/// dependencies and switches themes in O(|tokens|) without rescanning roots.
pub fn tokens_and_themes(doc: Tree<'_>) -> (BTreeMap<String, Leaf>, BTreeMap<Leaf, Value>, BTreeMap<Leaf, Value>) {
    let token_ty = ty("token");
    let mut leaves = BTreeMap::new();
    let mut light = BTreeMap::new();
    let mut dark = BTreeMap::new();
    for &root in doc.roots() {
        for t in doc.nodes_of_type_in(root, token_ty) {
            if !doc.is_live(t) {
                continue;
            }
            if !t.is_expanded()
                && let Some(n) = name_of(doc, t)
            {
                leaves.entry(n).or_insert_with(|| color_leaf(t));
            }
            if let Some(tok) = doc.get::<Token>(t) {
                if let Some(v) = tok.light.constant() {
                    light.insert(color_leaf(t), v.clone());
                }
                if let Some(v) = tok.dark.constant() {
                    dark.insert(color_leaf(t), v.clone());
                }
            }
        }
    }
    (leaves, light, dark)
}

/// The leaf `@name` reads from `reader` (reactivity.md §6): the nearest
/// `let` of that name with `let.provide="yes"` among the children of the
/// reader's ancestors, then the document's colour token, then the nearest
/// `provide="default"` let. A let never reads itself, so a provider may
/// refine the value it shadows.
///
/// Queries only direct `let` children of `reader`'s ancestors via
/// `doc.lets_in(parent)` (`Key::LetsIn(parent)`), touching neither non-`let`
/// siblings nor `let` nodes in independent sibling groups (`O(depth)`).
pub fn provided(doc: Tree<'_>, reader: NodeId, name: &str, tokens: &BTreeMap<String, Leaf>) -> Option<Leaf> {
    let value = |n: NodeId| Leaf { node: n, path: LeafPath { facet: FacetKind::Let, field: "value" } };
    let mut fallback: Option<NodeId> = None;
    let mut at = doc.parent_of(reader);
    loop {
        let mut best_yes: Option<(crate::order::OrderKey, NodeId)> = None;
        let mut best_default: Option<(crate::order::OrderKey, NodeId)> = None;
        for c in doc.lets_in(at) {
            if c == reader || !doc.is_live(c) || name_of(doc, c).as_deref() != Some(name) {
                continue;
            }
            let Some(l) = doc.get::<crate::facet::Let>(c) else { continue };
            let Some(order) = doc.order_of(c).cloned() else { continue };
            match l.provide.constant().and_then(Value::as_str) {
                Some("yes") => {
                    let cand = (order, c);
                    if best_yes.as_ref().is_none_or(|b| cand < *b) {
                        best_yes = Some(cand);
                    }
                }
                Some("default") if fallback.is_none() => {
                    let cand = (order, c);
                    if best_default.as_ref().is_none_or(|b| cand < *b) {
                        best_default = Some(cand);
                    }
                }
                _ => {}
            }
        }
        if let Some((_, c)) = best_yes {
            return Some(value(c));
        }
        if fallback.is_none()
            && let Some((_, c)) = best_default
        {
            fallback = Some(c);
        }
        match at {
            Some(p) => at = doc.parent_of(p),
            None => break,
        }
    }
    tokens.get(name).copied().or_else(|| fallback.map(value))
}

/// Whether `name` is served to `reader` from inside `within` (a
/// definition): a providing or default `let` of that name among the
/// children of the reader's ancestors, up to `within`. This is the
/// declaration a definition's context read needs (reactivity.md §6).
pub fn declared(doc: Tree<'_>, reader: NodeId, name: &str, within: NodeId) -> bool {
    let mut at = doc.node(reader).and_then(|n| n.parent);
    while let Some(p) = at {
        let found = doc.children(p).iter().any(|&c| {
            c != reader
                && doc.is_live(c)
                && name_of(doc, c).as_deref() == Some(name)
                && doc.get::<crate::facet::Let>(c).is_some_and(|l| matches!(l.provide.constant().and_then(Value::as_str), Some("yes" | "default")))
        });
        if found {
            return true;
        }
        if p == within {
            break;
        }
        at = doc.node(p).and_then(|n| n.parent);
    }
    false
}

/// Whether `name` is a colour token: a built-in, or one the document has.
/// Tokens are the theme, read anywhere without a declaration.
pub fn is_token(doc: Tree<'_>, name: &str) -> bool {
    BUILT_IN.iter().any(|(n, _, _)| *n == name) || tokens(doc).contains_key(name)
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
pub fn ensure_ops(doc: Tree<'_>, ops: &[Op]) -> Vec<Op> {
    // The ops first: most name no built-in token, and then no token is
    // looked up (plans/handoff-scaling.md DS-3a).
    let names = used(ops);
    if !names.iter().any(|n| BUILT_IN.iter().any(|(b, _, _)| b == n)) {
        return Vec::new();
    }
    let have = tokens(doc);
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
pub fn add_ops(doc: Tree<'_>, name: &str, light: [f32; 4], dark: [f32; 4]) -> Vec<Op> {
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
