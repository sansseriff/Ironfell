//! The layers panel's query (plans/specs/layers-panel.md §7.1): visible rows
//! of one composition, keyed by chain, opened by key, through embeds, with
//! chips for an entity's pictures elsewhere.

use iron_document::document::{RelKind, Relation};
use iron_document::facet::{Args, Composition, Embed, Font, Instance, Mesh, Name, PathShape, Size, Text, Transform2d, Transform3d};
use iron_document::layers::{self, Row};
use iron_document::*;
use std::collections::{BTreeMap, BTreeSet};

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn named(text: &str) -> Facet {
    let mut n = Name::default();
    n.text = Slot::Const(Value::Str(text.into()));
    n.wrap()
}

fn comp(name: &str, dims: &str) -> Vec<Facet> {
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum(dims.into()));
    vec![named(name), c.wrap()]
}

fn rect(name: &str) -> Vec<Facet> {
    vec![Transform2d::default().wrap(), Size::default().wrap(), named(name)]
}

/// Ids by name, created in order under a running key per parent.
struct Doc {
    store: Store,
    ops: Vec<Op>,
    ids: BTreeMap<&'static str, NodeId>,
    last: BTreeMap<Option<NodeId>, OrderKey>,
}

impl Doc {
    fn new() -> Doc {
        Doc { store: Store::new(), ops: Vec::new(), ids: BTreeMap::new(), last: BTreeMap::new() }
    }
    fn add(&mut self, name: &'static str, ty_name: &str, parent: Option<&str>, facets: Vec<Facet>) -> NodeId {
        let pending = self.ops.iter().filter(|o| matches!(o, Op::Create { .. })).count();
        let id = self.store.document().next_ids(pending + 1)[pending];
        let parent = parent.map(|p| self[p]);
        let order = self.last.get(&parent).map(OrderKey::after).unwrap_or_else(OrderKey::first);
        self.last.insert(parent, order.clone());
        self.ops.push(Op::Create { id, ty: ty(ty_name), parent, order, facets });
        self.ids.insert(name, id);
        id
    }
    fn apply(&mut self) {
        let ops = std::mem::take(&mut self.ops);
        self.store.apply(tx("setup", ops)).unwrap();
    }
}

impl std::ops::Index<&str> for Doc {
    type Output = NodeId;
    fn index(&self, name: &str) -> &NodeId {
        &self.ids[name]
    }
}

fn embed_of(c: NodeId) -> Vec<Facet> {
    let mut e = Embed::default();
    e.source = Slot::Const(Value::Ref(c));
    vec![e.wrap(), Transform2d::default().wrap()]
}

fn rows(d: &Doc, c: &str, open: &[&str]) -> Vec<Row> {
    let open: BTreeSet<String> = open.iter().map(|s| s.to_string()).collect();
    layers::rows(d.store.document(), d[c], &open)
}

fn names(rows: &[Row]) -> Vec<String> {
    rows.iter().map(|r| format!("{}{}", "  ".repeat(r.depth as usize), r.name.clone().unwrap_or_else(|| format!("{} {}", r.kind, r.id)))).collect()
}

/// sheet (2d) { back, group { a, b }, label (text) }, pendulum (2d) { bob },
/// explainer (2d) { left → pendulum, right → pendulum }.
fn sheet() -> Doc {
    let mut d = Doc::new();
    d.add("sheet", "composition", None, comp("sheet", "2d"));
    d.add("back", "rect", Some("sheet"), rect("back"));
    d.add("group", "group", Some("sheet"), vec![named("group")]);
    d.add("a", "rect", Some("group"), rect("a"));
    d.add("b", "rect", Some("group"), rect("b"));
    let mut t = Text::default();
    t.content = Slot::Const(Value::Str("\nHello there\nsecond line".into()));
    d.add("label", "text", Some("sheet"), vec![Transform2d::default().wrap(), t.wrap(), Font::default().wrap()]);
    d.add("pendulum", "composition", None, comp("pendulum", "2d"));
    d.add("bob", "rect", Some("pendulum"), rect("bob"));
    d.add("explainer", "composition", None, comp("explainer", "2d"));
    d.apply();
    let p = d["pendulum"];
    d.add("left", "embed", Some("explainer"), [embed_of(p), vec![named("left")]].concat());
    d.add("right", "embed", Some("explainer"), [embed_of(p), vec![named("right")]].concat());
    d.apply();
    d
}

#[test]
fn rows_run_topmost_first_and_open_by_key() {
    let d = sheet();
    let closed = rows(&d, "sheet", &[]);
    // The last child draws on top, so it is listed first; a text row with no
    // name takes its first line.
    assert_eq!(names(&closed), ["Hello there", "group", "back"]);
    let group = &closed[1];
    assert_eq!((group.key.clone(), group.count, group.depth), (d["group"].to_string(), 2, 0));
    assert_eq!(closed[2].count, 0);

    let open = rows(&d, "sheet", &[&group.key]);
    assert_eq!(names(&open), ["Hello there", "group", "  b", "  a", "back"]);
    // A child's key is its chain from the composition's top level.
    assert_eq!(open[2].key, format!("{} {}", d["group"], d["b"]));
    assert_eq!(open[2].key, layers::key(&layers::lineage(d.store.document(), d["b"])));
}

#[test]
fn an_embed_shows_its_composition_and_each_copy_has_its_own_keys() {
    let d = sheet();
    let closed = rows(&d, "explainer", &[]);
    assert_eq!(names(&closed), ["right", "left"]);
    assert_eq!(closed[0].embed, Some((d["pendulum"].to_string(), Some("pendulum".into()))));
    assert_eq!(closed[0].count, 1);

    let left = d["left"].to_string();
    let open = rows(&d, "explainer", &[&left]);
    assert_eq!(names(&open), ["right", "left", "  bob"]);
    // The select tool's chain through an embed: the embed, then the node in
    // the embedded composition.
    assert_eq!(open[2].key, format!("{} {}", d["left"], d["bob"]));

    let both = rows(&d, "explainer", &[&left, &d["right"].to_string()]);
    let bobs: Vec<&Row> = both.iter().filter(|r| r.id == d["bob"].to_string()).collect();
    assert_eq!(bobs.len(), 2);
    assert_ne!(bobs[0].key, bobs[1].key);
}

#[test]
fn a_figure_carries_one_chip_per_other_composition() {
    let mut d = Doc::new();
    d.add("cartoon", "composition", None, comp("cartoon", "2d"));
    d.add("box", "rect", Some("cartoon"), rect("box"));
    d.add("schematic", "composition", None, comp("schematic", "2d"));
    d.add("frame", "group", Some("schematic"), vec![named("frame")]);
    d.add("symbol", "rect", Some("frame"), rect("symbol"));
    d.add("symbol2", "rect", Some("schematic"), rect("symbol2"));
    d.add("assembly", "composition", None, comp("assembly", "3d"));
    d.add("body", "mesh", Some("assembly"), vec![Transform3d::default().wrap(), Mesh::default().wrap(), named("body")]);
    d.add("plain", "rect", Some("cartoon"), rect("plain"));
    d.add("model", "model", None, vec![]);
    d.add("library", "library", None, vec![]);
    d.add("thing", "definition", Some("library"), vec![named("thing")]);
    d.apply();
    let mut instance = Instance::default();
    instance.def = Slot::Const(Value::Ref(d["thing"]));
    let laser = d.add("laser", "entity", Some("model"), vec![instance.wrap(), Args::default().wrap()]);
    let rels = d.store.document().next_relation_ids(4);
    for (rel, from) in rels.iter().zip(["box", "symbol", "symbol2", "body"]) {
        let from = d[from];
        d.ops.push(Op::Link { rel: Relation { id: *rel, from, to: laser, rel: RelKind::Figures, data: BTreeMap::new() } });
    }
    d.apply();

    let cartoon = rows(&d, "cartoon", &[]);
    let boxed = cartoon.iter().find(|r| r.name.as_deref() == Some("box")).unwrap();
    assert_eq!(boxed.entity, Some(laser.to_string()));
    // One chip per other composition, naming the first figure there with
    // its chain, so Show in can select it as the select tool would.
    let chips: Vec<(&str, &str, &str)> = boxed.chips.iter().map(|c| (c.name.as_deref().unwrap(), c.dims, c.node.as_str())).collect();
    let symbol = d["symbol"].to_string();
    let body = d["body"].to_string();
    assert_eq!(chips, [("schematic", "2d", symbol.as_str()), ("assembly", "3d", body.as_str())]);
    assert_eq!(boxed.chips[0].chain, format!("{} {}", d["frame"], d["symbol"]));
    // Local geometry has no entity and no chips.
    let plain = cartoon.iter().find(|r| r.name.as_deref() == Some("plain")).unwrap();
    assert!(plain.entity.is_none() && plain.chips.is_empty());

    // A 3D composition keeps its stored order and lists the cartoon.
    let assembly = rows(&d, "assembly", &[]);
    assert_eq!(assembly[0].dims, "3d");
    assert_eq!(assembly[0].chips.iter().map(|c| c.name.clone().unwrap()).collect::<Vec<_>>(), ["cartoon", "schematic"]);
}

#[test]
fn a_paths_contours_and_points_are_not_rows() {
    let mut d = Doc::new();
    d.add("c", "composition", None, comp("c", "2d"));
    d.add("p", "path", Some("c"), vec![Transform2d::default().wrap(), PathShape::default().wrap(), named("p")]);
    d.add("ring", "contour", Some("p"), vec![iron_document::facet::Contour::default().wrap()]);
    d.add("pt", "point", Some("ring"), vec![Transform2d::default().wrap()]);
    d.apply();
    let r = rows(&d, "c", &[]);
    assert_eq!(names(&r), ["p"]);
    assert_eq!(r[0].count, 0);
}
