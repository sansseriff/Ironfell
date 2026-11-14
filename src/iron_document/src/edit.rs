//! The edit language (plans/specs/markup.md): one `<edit>` is one
//! transaction. Its elements are node markup (markup.rs), each a new node
//! and its children, and verbs on what exists:
//!
//! ```xml
//! <edit expect-version="7">
//!   <group in="#C" id="$g" name="readout" x="40" y="300">
//!     <text id="$t" content="{= fmt($v, 2)}"/>
//!     <place def="std:plot" id="$plot" x="0" y="40" w="300" h="200" ys="{= $self.xs * 2}" marks_x="{= $v}"/>
//!     <text x="{= $plot.transform2d.x}" y="250" content="{= fmt($plot.x_hi, 1)}"/>
//!   </group>
//!   <let in="#C" id="$v" name="v" value="0.5"/>
//!   <set id="#P" x="12" size.h="{= #O.slider.value * 200}"/>
//!   <add id="#N" stroke.width="2"/>
//!   <delete id="#3"/>
//!   <reparent id="#4" in="#1" first="true"/>
//!   <link from="$clip" to="#Q" rel="animates" offset="0.5"/>
//!   <unlink id="r1"/>
//!   <circuit sheet="circuit"><place part="resistor" x="200" y="100"/>…</circuit>
//!   <table in="#4" x="80" y="400" preset="light">a | b
//!   1 | 2</table>
//!   <context name="…">…</context>
//!   <import-svg in="#4" x="100" y="50"><svg …>…</svg></import-svg>
//!   <link-figures a="#l" b="#Q" keep="#v"/>
//!   <write narrative="#N" after="#B">markdown</write>
//!   <rewrite block="#B">markdown</rewrite>
//! </edit>
//! ```
//!
//! A top-level node goes `in` a node that exists (or one this edit makes,
//! written earlier, or a composition by name; none: a root), after its
//! last child unless `after` or `first` says where. `id="$x"` names a new
//! node for the whole edit, `id="$p"` on a `<place>` or `<make-component>`
//! what it made. The nodes and the component verbs (wherever written) are
//! made first, so every other verb may name them; those then apply in the
//! order written, each seeing what came before it. Inside a node every
//! element is a node, except `<place>`, `<make-component>` and `<link>`;
//! at the top level `set`, `table` and `context` are verbs.
//!
//! A place's `$p.facet.field` is its picture's leaf and `$p.port` its
//! instance's port (`#use.port` for an instance the document has), so a
//! mark can sit on a plot: leaves that read a place are set once it exists.

use crate::compose::{self, ComposeError, Named};
use crate::document::{RelKind, Relation};
use crate::facet::LeafPath;
use crate::id::{NodeId, RelationId};
use crate::markup::{self, Names};
use crate::op::Op;
use crate::order::OrderKey;
use crate::parts::El;
use crate::registry::TypeId;
use crate::value::{Slot, Value, ValueKind};
use crate::{Actor, Store, TransactionInput};
use serde::Serialize;
use std::collections::BTreeMap;

/// The verbs; any other top-level element is a node.
pub const VERBS: &[&str] = &[
    "set", "add", "delete", "reparent", "link", "unlink", "place", "make-component", "circuit", "table", "context", "import-svg", "link-figures", "write", "rewrite",
];

/// The verbs that only write ops, which may be planned together.
const PLAIN: &[&str] = &["set", "add", "delete", "reparent", "link", "unlink"];

/// An edit, planned: its ops as one transaction, and how to talk about
/// them in the edit's own terms.
#[derive(Debug, Serialize)]
pub struct Plan {
    pub ops: Vec<Op>,
    /// Each `$name` the edit gave, and the node it named.
    pub aliases: BTreeMap<String, NodeId>,
    /// For each op, the verb (top-level element, from 0) that wrote it.
    pub op_verb: Vec<usize>,
    /// Each verb, briefly: `create $sinv`, `set #P`.
    pub verbs: Vec<String>,
    /// Lines for the `<applied>` answer: what was created, placed, built.
    pub summary: Vec<String>,
    pub expect_version: u64,
    pub exact: bool,
}

/// What went wrong, at a verb (`op`, counting the edit's elements from 0).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct VerbError {
    pub op: Option<usize>,
    pub kind: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verb: Option<String>,
}

#[derive(Debug)]
pub enum Failure {
    Errors(Vec<VerbError>),
    /// An answer already in markup: `<link-figures>` asking which entity to keep.
    Markup(String),
}

impl Failure {
    /// One line, for callers that only report it.
    pub fn message(&self) -> String {
        match self {
            Failure::Errors(es) => es.iter().map(|e| match e.op {
                Some(op) => format!("op {op} ({}): {}", e.verb.as_deref().unwrap_or("?"), e.message),
                None => e.message.clone(),
            }).collect::<Vec<_>>().join("; "),
            Failure::Markup(m) => m.clone(),
        }
    }
}

/// SVG text to a fragment and what it could not carry (iron_svg, which
/// this crate does not depend on).
pub type SvgImport<'a> = &'a dyn Fn(&str) -> Result<(crate::copy::Fragment, Vec<String>), String>;

/// Plan `xml` (one `<edit>`) against `store`: nothing is applied.
pub fn plan(store: &Store, xml: &str, svg: Option<SvgImport>) -> Result<Plan, Failure> {
    let parse = |m: String| Failure::Errors(vec![VerbError { op: None, kind: "parse".into(), message: m, verb: None }]);
    let els = crate::parts::elements(xml).map_err(|e| parse(format!("not well-formed: {e}")))?;
    let root = match els.as_slice() {
        [r] if r.name == "edit" => r,
        _ => return Err(parse("expected one <edit> element".into())),
    };
    let expect_version = root.attr("expect-version").and_then(|v| v.trim().parse().ok()).unwrap_or(0);
    let exact = root.attr("exact").is_some_and(|v| matches!(v.trim().to_ascii_lowercase().as_str(), "true" | "yes" | "1"));
    let verbs: Vec<String> = root.children.iter().map(describe).collect();
    let mut p = Planner { store, svg, verbs: &verbs, scratch: Store::with_document(store.document().clone()), ops: Vec::new(), op_verb: Vec::new(), summary: Vec::new(), nodes: BTreeMap::new(), places: BTreeMap::new(), pending: Vec::new() };
    for (i, el) in root.children.iter().enumerate() {
        if !VERBS.contains(&el.name.as_str()) && TypeId::lookup(&el.name).is_none() {
            return Err(Failure::Errors(vec![p.error(i, "parse", format!("<{}> is not a verb or a node type; verbs: {}", el.name, VERBS.join(", ")))]));
        }
    }
    let links = p.build(&root.children).map_err(Failure::Errors)?;
    // A plain verb's mistake is collected and planning goes on, so the
    // answer lists every one; a verb that reads the document stops it.
    let mut errors: Vec<VerbError> = Vec::new();
    let mut run = |p: &mut Planner, i: usize, el: &El, errors: &mut Vec<VerbError>| -> Result<(), Failure> {
        match p.verb(i, el) {
            Err(Failure::Errors(es)) if PLAIN.contains(&el.name.as_str()) => {
                errors.extend(es);
                Ok(())
            }
            Err(Failure::Errors(es)) => {
                errors.extend(es);
                Err(Failure::Errors(std::mem::take(errors)))
            }
            other => other,
        }
    };
    for (i, el) in root.children.iter().enumerate() {
        if TypeId::lookup(&el.name).is_some() && !VERBS.contains(&el.name.as_str()) {
            for link in links.iter().filter(|(v, _)| *v == i) {
                run(&mut p, i, &link.1, &mut errors)?;
            }
        } else if !matches!(el.name.as_str(), "place" | "make-component") {
            run(&mut p, i, el, &mut errors)?;
        }
    }
    if let Err(es) = p.flush() {
        errors.extend(es);
    }
    if !errors.is_empty() {
        return Err(Failure::Errors(errors));
    }
    let mut aliases = BTreeMap::new();
    for (name, (id, _)) in &p.nodes {
        if !name.starts_with("__") {
            aliases.insert(format!("${name}"), *id);
        }
    }
    for (name, placed) in &p.places {
        if !name.starts_with("__") {
            aliases.insert(format!("${name}"), placed.picture);
        }
    }
    let mut summary: Vec<String> = aliases.iter().map(|(a, id)| format!("  <created alias=\"{a}\" id=\"{id}\"/>")).collect();
    summary.extend(p.summary);
    Ok(Plan { ops: p.ops, aliases, op_verb: p.op_verb, verbs, summary, expect_version, exact })
}

/// [`plan`] as a transaction, for callers that apply an edit as the system
/// (the demo's gallery): its errors as one message.
pub fn transaction(store: &Store, xml: &str, label: &str, actor: Actor) -> Result<(TransactionInput, Plan), String> {
    let plan = plan(store, xml, None).map_err(|f| f.message())?;
    Ok((TransactionInput { label: label.into(), actor, ts: 0, idempotency_key: None, ops: plan.ops.clone(), exact: plan.exact }, plan))
}

/// A verb in the edit's own terms, for errors: what it names, or what it
/// makes and where.
fn describe(el: &El) -> String {
    let a = |k: &str| el.attr(k).unwrap_or("");
    match el.name.as_str() {
        "link" => format!("link {} {} {}", a("from"), el.attr("rel").unwrap_or("references"), a("to")),
        "place" | "make-component" => match el.attr("id") {
            Some(id) => format!("{} {id}", el.name),
            None => el.name.clone(),
        },
        n if VERBS.contains(&n) => {
            let what = ["id", "block", "narrative", "sheet", "name", "in", "a"].iter().find_map(|k| el.attr(k)).unwrap_or("");
            format!("{n} {what}").trim().to_owned()
        }
        n => match el.attr("id") {
            Some(id) => format!("create {id}"),
            None => format!("create {n}{}", el.attr("in").map(|p| format!(" in {p}")).unwrap_or_default()),
        },
    }
}

struct Planner<'a, 'v> {
    store: &'a Store,
    svg: Option<SvgImport<'a>>,
    verbs: &'v [String],
    /// The document with every op so far applied: what each verb reads.
    scratch: Store,
    ops: Vec<Op>,
    op_verb: Vec<usize>,
    summary: Vec<String>,
    nodes: BTreeMap<String, (NodeId, TypeId)>,
    places: BTreeMap<String, Named>,
    /// Ops of plain verbs (set, add, delete, reparent, link, unlink) not yet
    /// applied to the scratch document, with their verbs: the store checks a
    /// transaction whole (a connection's two links), so they apply together,
    /// before the next verb that reads the document, and at the end.
    pending: Vec<(Op, usize)>,
}

/// A component verb lifted out of a node (or written at the top level):
/// the verb it belongs to, the local name of the node it sits in.
struct Lifted {
    verb: usize,
    owner: Option<String>,
    el: El,
}

impl Planner<'_, '_> {
    fn error(&self, verb: usize, kind: &str, message: String) -> VerbError {
        // An id that names nothing reads as the store says it.
        let kind = if message.starts_with("no node #") { "unknown_node" } else { kind };
        VerbError { op: Some(verb), kind: kind.into(), message, verb: self.verbs.get(verb).cloned() }
    }

    fn names(&self) -> Names<'_> {
        Names { nodes: self.nodes.clone(), places: self.places.clone(), doc: Some(self.scratch.document()) }
    }

    /// Apply `ops`, written by verb `verb`, to the scratch document and
    /// keep them.
    fn push(&mut self, verb: usize, ops: Vec<Op>) -> Result<(), Vec<VerbError>> {
        if ops.is_empty() {
            return Ok(());
        }
        let tx = TransactionInput { label: "plan".into(), actor: Actor::Model, ts: 0, idempotency_key: None, ops: ops.clone(), exact: false };
        self.scratch.apply(tx).map_err(|es| es.iter().map(|e| self.error(verb, &kind_name(e), e.message.clone())).collect::<Vec<_>>())?;
        self.op_verb.extend(std::iter::repeat_n(verb, ops.len()));
        self.ops.extend(ops);
        Ok(())
    }

    /// A node as written: `#id`, or a `$name` this edit gave (a place's
    /// is its picture).
    fn node(&self, raw: &str) -> Result<NodeId, String> {
        let raw = raw.trim();
        if let Some(name) = raw.strip_prefix('$') {
            return self.nodes.get(name).map(|(id, _)| *id).or_else(|| self.places.get(name).map(|p| p.picture)).ok_or_else(|| format!("{raw} names no node made in this edit"));
        }
        let id: NodeId = raw.parse().map_err(|_| format!("{raw:?} is not a node id (#…) or a $name"))?;
        self.scratch.document().live(id).map(|_| id).ok_or_else(|| format!("no node {raw}"))
    }

    /// `$name`s in an attribute this edit gave, as ids (bare, a value
    /// node's value); others (`$self`, a component verb's own) as written.
    fn substitute(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(i) = rest.find('$') {
            out.push_str(&rest[..i]);
            let after = &rest[i + 1..];
            let end = after.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(after.len());
            let name = &after[..end];
            match self.nodes.get(name) {
                Some(_) => {
                    let one = Names { nodes: self.nodes.iter().filter(|(k, _)| k.as_str() == name).map(|(k, v)| (k.clone(), *v)).collect(), ..Default::default() };
                    let next = &after[end..after.len().min(end + 1)];
                    let r = markup::resolve(&format!("${name}{next}"), &one).expect("named");
                    out.push_str(r.strip_suffix(next).unwrap_or(&r));
                }
                None => {
                    out.push('$');
                    out.push_str(name);
                }
            }
            rest = &after[end..];
        }
        out.push_str(rest);
        out
    }

    /// Order keys for `count` new children of `parent`: after its last
    /// child, first (`first`), or right after `after`; among what exists
    /// and `planned` (children this edit has put there, not yet applied).
    fn keys(&self, parent: Option<NodeId>, after: Option<&str>, first: bool, count: usize, planned: &[(NodeId, OrderKey)]) -> Result<Vec<OrderKey>, String> {
        let doc = self.scratch.document();
        let siblings: Vec<NodeId> = match parent {
            Some(p) if doc.live(p).is_some() => doc.children(p).to_vec(),
            Some(_) => Vec::new(),
            None => doc.roots().to_vec(),
        };
        let mut keys: Vec<(NodeId, OrderKey)> = siblings.iter().filter(|c| !c.is_expanded()).filter_map(|&c| doc.live(c).map(|n| (c, n.order.clone()))).collect();
        keys.extend(planned.iter().cloned());
        keys.sort_by(|a, b| a.1.cmp(&b.1));
        let (mut lo, hi) = if first {
            (None, keys.first().map(|k| k.1.clone()))
        } else if let Some(a) = after {
            let a = self.node(a).or_else(|e| a.strip_prefix('$').and_then(|n| self.nodes.get(n)).map(|(id, _)| *id).ok_or(e))?;
            let at = keys.iter().position(|k| k.0 == a).ok_or_else(|| format!("{a} is not a child of {}", parent.map_or("the document".to_owned(), |p| p.to_string())))?;
            (Some(keys[at].1.clone()), keys.get(at + 1).map(|k| k.1.clone()))
        } else {
            (keys.last().map(|k| k.1.clone()), None)
        };
        let mut out = Vec::new();
        for _ in 0..count {
            let k = OrderKey::between(lo.as_ref(), hi.as_ref()).map_err(|e| format!("{e:?}"))?;
            lo = Some(k.clone());
            out.push(k);
        }
        Ok(out)
    }

    /// The nodes and component verbs, as markup builds them: every node,
    /// then the places, then the leaves that read a place. Returns the
    /// `<link>`s found inside nodes, each with its verb.
    fn build(&mut self, children: &[El]) -> Result<Vec<(usize, El)>, Vec<VerbError>> {
        let one = |p: &Self, verb: usize, m: String| vec![p.error(verb, "parse", m)];
        // The component verbs' names: `id="$p"` becomes compose.rs's `as`.
        let mut trees: Vec<(usize, El)> = Vec::new();
        let mut top: Vec<Lifted> = Vec::new();
        let mut place_count = 0;
        let mut place_names: Vec<String> = Vec::new();
        fn name_places(el: &mut El, count: &mut usize, names: &mut Vec<String>) -> Result<(), String> {
            if matches!(el.name.as_str(), "place" | "make-component") {
                if el.attr("as").is_some() {
                    return Err(format!("<{}> is named by id=\"$p\", not as=", el.name));
                }
                let alias = match el.attrs.iter().position(|(k, _)| k == "id") {
                    Some(pos) => {
                        let (_, v) = el.attrs.remove(pos);
                        let local = v.strip_prefix('$').filter(|l| !l.is_empty()).ok_or_else(|| format!("<{} id=\"{v}\">: a name is a $ then letters, digits or _", el.name))?;
                        if names.iter().any(|n| n == local) {
                            return Err(format!("id=\"{v}\" names two places"));
                        }
                        names.push(local.to_owned());
                        v
                    }
                    None => format!("$__place_{count}"),
                };
                *count += 1;
                el.attrs.push(("as".into(), alias));
                return Ok(());
            }
            for c in el.children.iter_mut() {
                name_places(c, count, names)?;
            }
            Ok(())
        }
        for (i, el) in children.iter().enumerate() {
            let is_node = TypeId::lookup(&el.name).is_some() && !VERBS.contains(&el.name.as_str());
            if !is_node && !matches!(el.name.as_str(), "place" | "make-component") {
                continue;
            }
            let mut el = el.clone();
            name_places(&mut el, &mut place_count, &mut place_names).map_err(|e| one(self, i, e))?;
            if is_node {
                trees.push((i, el));
            } else {
                top.push(Lifted { verb: i, owner: None, el });
            }
        }

        // Lift the component verbs and links out of the nodes, each with
        // the node it sits in (one is named if it has no name), and set
        // aside the leaves that read a place: they are written after it.
        struct Lift<'p> {
            places: Vec<Lifted>,
            links: Vec<(usize, El)>,
            deferred: Vec<(usize, String, String, String)>,
            sequences: Vec<(usize, Option<String>, Vec<Sibling>)>,
            synthetic: usize,
            place_names: &'p [String],
        }
        enum Sibling {
            Node(String),
            Place(String),
        }
        fn lift(els: &mut Vec<El>, owner: Option<&str>, verb: usize, top: bool, l: &mut Lift) {
            let has_place = !top && els.iter().any(|e| matches!(e.name.as_str(), "place" | "make-component"));
            let mut sequence = Vec::new();
            let mut kept = Vec::with_capacity(els.len());
            for mut el in els.drain(..) {
                if matches!(el.name.as_str(), "place" | "make-component") {
                    sequence.push(Sibling::Place(el.attr("as").expect("named above").to_owned()));
                    l.places.push(Lifted { verb, owner: owner.map(str::to_owned), el });
                    continue;
                }
                if el.name == "link" {
                    l.links.push((verb, el));
                    continue;
                }
                let defers = el.attrs.iter().any(|(_, v)| markup::reads_place(v, l.place_names));
                let holds = el.children.iter().any(|c| matches!(c.name.as_str(), "place" | "make-component" | "link"));
                if (defers || has_place || holds) && el.attr("id").is_none() {
                    el.attrs.push(("id".into(), format!("$__node_{}", l.synthetic)));
                    l.synthetic += 1;
                }
                let local = el.attr("id").and_then(|i| i.strip_prefix('$')).map(str::to_owned);
                if defers {
                    let local = local.clone().expect("named above");
                    for (k, v) in el.attrs.iter_mut() {
                        if markup::reads_place(v, l.place_names) {
                            l.deferred.push((verb, local.clone(), k.clone(), std::mem::take(v)));
                            *k = format!("__defer:{k}");
                        }
                    }
                }
                if has_place {
                    sequence.push(Sibling::Node(local.clone().expect("named above")));
                }
                lift(&mut el.children, local.as_deref(), verb, false, l);
                kept.push(el);
            }
            if has_place {
                l.sequences.push((verb, owner.map(str::to_owned), sequence));
            }
            *els = kept;
        }
        let mut l = Lift { places: Vec::new(), links: Vec::new(), deferred: Vec::new(), sequences: Vec::new(), synthetic: 0, place_names: &place_names };
        for (verb, el) in trees.iter_mut() {
            let mut one_tree = vec![std::mem::replace(el, El { name: String::new(), attrs: Vec::new(), children: Vec::new(), text: String::new(), src: String::new(), inner: String::new() })];
            lift(&mut one_tree, None, *verb, true, &mut l);
            *el = one_tree.pop().expect("one");
        }
        let Lift { places: mut lifted, links, deferred, sequences, .. } = l;
        // Top-level component verbs keep their place in the edit's order.
        lifted.extend(top);
        lifted.sort_by_key(|p| p.verb);

        // Ids for every node first, so any may name another.
        let mut next = self.scratch.document().next_id().raw();
        let mut ids = Vec::new();
        for (verb, el) in &trees {
            markup::number(std::slice::from_ref(el), &mut next, &mut self.nodes, &mut ids).map_err(|e| one(self, *verb, e))?;
        }
        for n in &place_names {
            if self.nodes.contains_key(n) {
                return Err(vec![self.error(lifted.iter().find(|p| p.el.attr("as") == Some(&format!("${n}"))).map_or(0, |p| p.verb), "parse", format!("${n} names a node and a place"))]);
            }
        }
        let mut ids = ids.into_iter();
        let mut made: Vec<(Op, usize)> = Vec::new();
        for (verb, el) in &trees {
            let verb = *verb;
            let parent = match el.attr("in") {
                None => None,
                Some(raw) if raw.starts_with('#') || raw.starts_with('$') => {
                    let p = self.node(raw).map_err(|e| one(self, verb, format!("in=\"{raw}\": {e}")))?;
                    if raw.starts_with('$') && !made.iter().any(|(op, _)| matches!(op, Op::Create { id, .. } if *id == p)) {
                        return Err(one(self, verb, format!("in=\"{raw}\": {raw} is made later in this edit; write it first")));
                    }
                    Some(p)
                }
                Some(name) => Some(crate::parts::sheet_named(self.scratch.document(), name).map_err(|e| one(self, verb, format!("in=\"{name}\": {e}")))?),
            };
            // Keys among what exists and what this edit already put there.
            let order = {
                let planned: Vec<(NodeId, OrderKey)> = made.iter().filter_map(|(op, _)| match op {
                    Op::Create { id, parent: p, order, .. } if *p == parent => Some((*id, order.clone())),
                    _ => None,
                }).collect();
                self.keys(parent, el.attr("after"), el.attr("first").is_some_and(|f| f != "false"), 1, &planned).map_err(|e| one(self, verb, e))?.remove(0)
            };
            let names = Names { nodes: self.nodes.clone(), places: BTreeMap::new(), doc: Some(self.scratch.document()) };
            let mut ops = Vec::new();
            markup::emit(el, parent, order, &mut ids, &names, &mut ops).map_err(|e| one(self, verb, e))?;
            made.extend(ops.into_iter().map(|op| (op, verb)));
        }
        let (before, before_verb): (Vec<Op>, Vec<usize>) = made.into_iter().unzip();

        // The component verbs, each `in` the node it sat in.
        let mut verbs = Vec::new();
        for p in &lifted {
            let mut el = p.el.clone();
            for (k, v) in el.attrs.iter_mut() {
                if k != "as" {
                    *v = self.substitute(v);
                }
            }
            if el.name == "place" && el.attr("in").is_none() {
                let target = match &p.owner {
                    Some(local) => self.nodes.get(local).map(|(id, _)| *id).ok_or_else(|| one(self, p.verb, format!("no node ${local}")))?,
                    None => return Err(one(self, p.verb, "a <place> at the top level needs in=".into())),
                };
                el.attrs.push(("in".into(), target.to_string()));
            }
            verbs.push(el);
        }
        if verbs.is_empty() {
            self.push_each(before, before_verb)?;
        } else {
            let composed = compose::compose_els(self.store, before, verbs).map_err(|e| match e {
                ComposeError::Ops(errors) => errors.iter().map(|e| self.error(before_verb.get(e.op).copied().unwrap_or(0), &kind_name(e), e.message.clone())).collect::<Vec<_>>(),
                ComposeError::Verb(m) => {
                    let n: usize = m.strip_prefix('<').and_then(|r| r.split_once("> ")).and_then(|(_, r)| r.split_once(':')).and_then(|(n, _)| n.trim().parse().ok()).unwrap_or(1);
                    let verb = lifted.get(n.saturating_sub(1)).map_or(0, |p| p.verb);
                    vec![self.error(verb, "compose", m)]
                }
            })?;
            let mut op_verb = before_verb.clone();
            let mut start = before_verb.len();
            for (k, &end) in composed.ends.iter().enumerate() {
                op_verb.extend(std::iter::repeat_n(lifted[k].verb, end - start));
                start = end;
            }
            self.push_each(composed.ops, op_verb)?;
            self.summary.extend(composed.lines);
            for (alias, named) in composed.named {
                self.places.insert(alias.trim_start_matches('$').to_owned(), named);
            }
        }

        // Siblings back in markup order: a mark written after a plot paints
        // over it.
        let mut later = Vec::new();
        for (verb, owner, sequence) in &sequences {
            let doc = self.scratch.document();
            let Some(parent) = owner.as_ref().and_then(|o| self.nodes.get(o)).map(|(id, _)| *id) else { continue };
            let ids: Vec<NodeId> = sequence.iter().filter_map(|s| match s {
                Sibling::Node(local) => self.nodes.get(local).map(|(id, _)| *id),
                Sibling::Place(alias) => self.places.get(alias.trim_start_matches('$')).map(|p| p.picture),
            }).collect();
            let Some(mut key) = ids.first().and_then(|&id| doc.live(id)).map(|n| n.order.clone()) else { continue };
            for &id in &ids[1..] {
                key = OrderKey::after(&key);
                later.push((Op::Reparent { id, parent: Some(parent), order: key.clone() }, *verb));
            }
        }
        // The leaves that read a place, now that it exists.
        for (verb, local, key, raw) in deferred {
            let (id, ty) = self.nodes[&local];
            let text = raw.trim().strip_prefix("{=").and_then(|r| r.strip_suffix('}')).ok_or_else(|| one(self, verb, format!("{key}=\"{raw}\": a leaf that reads a place is a binding, {{= …}}")))?.trim().to_owned();
            let names = self.names();
            let text = markup::resolve(&text, &names).map_err(|e| one(self, verb, format!("{key}=\"{raw}\": {e}")))?;
            let (kind, field) = markup::leaf(ty, &key).map_err(|e| one(self, verb, e))?;
            let path = LeafPath::new(kind, &field).map_err(|e| one(self, verb, format!("{key}: {e:?}")))?;
            let expr = crate::expr::Expr::parse(&text).map_err(|e| one(self, verb, format!("{key}=\"{raw}\": {e}")))?;
            later.push((Op::Set { id, path, slot: Slot::Bound(expr) }, verb));
        }
        let (ops, op_verb): (Vec<Op>, Vec<usize>) = later.into_iter().unzip();
        self.push_each(ops, op_verb)?;
        Ok(links)
    }

    /// Apply the plain verbs' ops held back so far.
    fn flush(&mut self) -> Result<(), Vec<VerbError>> {
        let (ops, op_verb): (Vec<Op>, Vec<usize>) = std::mem::take(&mut self.pending).into_iter().unzip();
        self.push_each(ops, op_verb)
    }

    /// Hold a plain verb's ops for the next [`Planner::flush`].
    fn hold(&mut self, verb: usize, ops: Vec<Op>) {
        self.pending.extend(ops.into_iter().map(|op| (op, verb)));
    }

    /// [`Planner::push`] for ops written by several verbs, in one go.
    fn push_each(&mut self, ops: Vec<Op>, op_verb: Vec<usize>) -> Result<(), Vec<VerbError>> {
        if ops.is_empty() {
            return Ok(());
        }
        let tx = TransactionInput { label: "plan".into(), actor: Actor::Model, ts: 0, idempotency_key: None, ops: ops.clone(), exact: false };
        self.scratch.apply(tx).map_err(|es| es.iter().map(|e| self.error(op_verb.get(e.op).copied().unwrap_or(0), &kind_name(e), e.message.clone())).collect::<Vec<_>>())?;
        self.op_verb.extend(op_verb);
        self.ops.extend(ops);
        Ok(())
    }

    /// One verb, planned and applied to the scratch document.
    fn verb(&mut self, i: usize, el: &El) -> Result<(), Failure> {
        let fail = |p: &Self, kind: &str, m: String| Failure::Errors(vec![p.error(i, kind, m)]);
        // A verb that reads the document sees the plain verbs before it.
        if !PLAIN.contains(&el.name.as_str()) {
            self.flush().map_err(Failure::Errors)?;
        }
        match el.name.as_str() {
            "set" | "add" => {
                let id = self.node(el.need("id").map_err(|e| fail(self, "parse", e))?).map_err(|e| fail(self, "parse", e))?;
                let ty = self.scratch.document().live(id).map(|n| n.ty).expect("live");
                let mut pairs: Vec<(String, String)> = Vec::new();
                let skip: &[&str] = if el.attr("path").is_some() { &["id", "type", "path", "value", "bind"] } else { &["id", "type"] };
                if let Some(path) = el.attr("path") {
                    pairs.push((path.to_owned(), match el.attr("bind") {
                        Some(b) => format!("{{= {b}}}"),
                        None => el.attr("value").unwrap_or("").to_owned(),
                    }));
                }
                pairs.extend(markup::leaf_attrs(el, skip).into_iter().map(|(k, v)| (k, v.into_owned())));
                if pairs.is_empty() {
                    return Err(fail(self, "parse", "nothing to set; give leaf attributes such as x=\"10\", or path=\"size.h\" value=\"10\"".into()));
                }
                let names = self.names();
                let mut ops = Vec::new();
                let mut added: Vec<crate::facet::Facet> = Vec::new();
                for (k, raw) in &pairs {
                    let (kind, field) = markup::leaf(ty, k).map_err(|e| fail(self, "parse", e))?;
                    let slot = markup::slot(markup::kind_of(kind, &field), raw, &names).map_err(|e| fail(self, "parse", format!("{k}=\"{raw}\": {e}")))?;
                    if el.name == "set" {
                        ops.push(Op::Set { id, path: LeafPath::new(kind, &field).map_err(|e| fail(self, "parse", format!("{k}: {e:?}")))?, slot });
                    } else {
                        if !added.iter().any(|f| f.kind() == kind) {
                            added.push(kind.default_facet());
                        }
                        match added.iter_mut().find(|f| f.kind() == kind).expect("added") {
                            crate::facet::Facet::Args(a) => a.insert(&field, slot),
                            f => {
                                f.set(&field, slot).map_err(|e| fail(self, "parse", format!("{k}: {e}")))?;
                            }
                        }
                    }
                }
                drop(names);
                ops.extend(added.into_iter().map(|facet| Op::AddFacet { id, facet }));
                self.hold(i, ops);
                Ok(())
            }
            "delete" => {
                let id = self.node(el.need("id").map_err(|e| fail(self, "parse", e))?).map_err(|e| fail(self, "parse", e))?;
                self.hold(i, vec![Op::Delete { id }]);
                Ok(())
            }
            "reparent" => {
                let id = self.node(el.need("id").map_err(|e| fail(self, "parse", e))?).map_err(|e| fail(self, "parse", e))?;
                let parent = match el.attr("in") {
                    Some(raw) => Some(self.node(raw).map_err(|e| fail(self, "parse", e))?),
                    None => None,
                };
                // Among what exists and what earlier held verbs put there.
                let planned: Vec<(NodeId, OrderKey)> = self.pending.iter().filter_map(|(op, _)| match op {
                    Op::Reparent { id, parent: p, order } if *p == parent => Some((*id, order.clone())),
                    _ => None,
                }).collect();
                let order = self.keys(parent, el.attr("after"), el.attr("first").is_some_and(|f| f != "false"), 1, &planned).map_err(|e| fail(self, "parse", e))?.remove(0);
                self.hold(i, vec![Op::Reparent { id, parent, order }]);
                Ok(())
            }
            "link" => {
                let from = self.node(el.need("from").map_err(|e| fail(self, "parse", e))?).map_err(|e| fail(self, "parse", e))?;
                let to = self.node(el.need("to").map_err(|e| fail(self, "parse", e))?).map_err(|e| fail(self, "parse", e))?;
                let rel_name = el.attr("rel").unwrap_or("references");
                let rel = RelKind::from_name(rel_name).ok_or_else(|| fail(self, "parse", format!("rel=\"{rel_name}\": one of {}", RelKind::ALL.map(|k| k.name()).join(", "))))?;
                let mut data = BTreeMap::new();
                for (k, raw) in el.attrs.iter().filter(|(k, _)| !matches!(k.as_str(), "from" | "to" | "rel" | "id")) {
                    match markup::slot(ValueKind::Any, raw, &Names::default()).map_err(|e| fail(self, "parse", format!("{k}: {e}")))? {
                        Slot::Const(v) => {
                            data.insert(k.clone(), v);
                        }
                        _ => return Err(fail(self, "parse", format!("{k}: a relation's data is a value, not a binding"))),
                    }
                }
                // After the links held back, which have not taken theirs yet.
                let held = self.pending.iter().filter(|(op, _)| matches!(op, Op::Link { .. })).count() as u64;
                let id = self.scratch.document().next_relation_id().offset(held);
                self.hold(i, vec![Op::Link { rel: Relation { id, from, to, rel, data } }]);
                Ok(())
            }
            "unlink" => {
                let raw = el.need("id").map_err(|e| fail(self, "parse", e))?;
                let id: RelationId = raw.parse().map_err(|_| fail(self, "parse", format!("{raw:?} is not a relation id (r…)")))?;
                self.hold(i, vec![Op::Unlink { id }]);
                Ok(())
            }
            "circuit" => {
                let markup = self.substitute(&el.src);
                let (ops, summary) = crate::parts::circuit_ops(&self.scratch, &markup).map_err(|e| fail(self, "circuit", e))?;
                self.push(i, ops).map_err(Failure::Errors)?;
                let sheet = self.substitute(el.attr("sheet").unwrap_or(""));
                self.summary.push(format!("  <circuit>{summary}</circuit>"));
                if let Ok(id) = crate::parts::sheet_named(self.scratch.document(), &sheet) {
                    self.summary.push(format!("  <netlist sheet=\"{sheet}\">\n{}\n  </netlist>", crate::parts::netlist(&self.scratch, id)));
                }
                Ok(())
            }
            "table" => self.table(i, el).map_err(|e| fail(self, "table", e)),
            "context" => {
                // A context printed by a view carries its node's id; the name decides.
                let markup = match el.src.strip_prefix("<context") {
                    Some(rest) => {
                        let mut e = crate::parts::elements(&el.src).map_err(|e| fail(self, "context", e))?.remove(0);
                        e.attrs.retain(|(k, _)| k != "id");
                        let _ = rest;
                        let attrs: String = e.attrs.iter().map(|(k, v)| format!(" {k}=\"{}\"", v.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;"))).collect();
                        format!("<context{attrs}>{}</context>", el.inner)
                    }
                    None => el.src.clone(),
                };
                let (ops, node) = crate::contexts::put_ops(self.scratch.document(), &markup).map_err(|e| fail(self, "context", e))?;
                self.push(i, ops).map_err(Failure::Errors)?;
                self.summary.push(format!("  <context id=\"{node}\"/>"));
                Ok(())
            }
            "import-svg" => {
                let svg = self.svg.ok_or_else(|| fail(self, "import_svg", "SVG import is not available here".into()))?;
                let parent = self.node(el.need("in").map_err(|e| fail(self, "parse", format!("{e} (a composition or group)")))?).map_err(|e| fail(self, "parse", e))?;
                let (fragment, cuts) = svg(el.text.trim()).map_err(|e| fail(self, "import_svg", e))?;
                let at = [el.num("x").map_err(|e| fail(self, "parse", e))?, el.num("y").map_err(|e| fail(self, "parse", e))?];
                let place = crate::copy::Placement { parent: Some(parent), beside: false, offset: at, share_entities: false };
                let pasted = crate::copy::paste(&self.scratch, &fragment, &place, "import").map_err(|e| fail(self, "import_svg", e))?;
                self.push(i, pasted.ops).map_err(Failure::Errors)?;
                self.summary.push(format!("  <imported roots=\"{}\"/>", pasted.roots.iter().map(|r| r.to_string()).collect::<Vec<_>>().join(" ")));
                self.summary.extend(cuts.iter().map(|c| format!("  <cut>{}</cut>", escape(c))));
                Ok(())
            }
            "link-figures" => {
                let a = self.node(el.need("a").map_err(|e| fail(self, "parse", e))?).map_err(|e| fail(self, "parse", e))?;
                let b = self.node(el.need("b").map_err(|e| fail(self, "parse", e))?).map_err(|e| fail(self, "parse", e))?;
                let keep = match el.attr("keep").filter(|k| !k.is_empty()) {
                    Some(k) => Some(self.node(k).map_err(|e| fail(self, "parse", e))?),
                    None => None,
                };
                match self.scratch.link(a, b, keep, Actor::Model) {
                    Ok(tx) => self.push(i, tx.ops).map_err(Failure::Errors),
                    Err(crate::composition::LinkError::Question(q)) => Err(Failure::Markup(question(self.store.document().version().0, a, b, &q))),
                    Err(e) => Err(fail(self, "link", e.to_string())),
                }
            }
            "write" | "rewrite" => self.narrative(i, el).map_err(|e| fail(self, &el.name, e)),
            other => Err(fail(self, "parse", format!("<{other}> is not a verb"))),
        }
    }

    /// `<table>` (plans/specs/layout-and-tables.md §5.2): rows of cells, one
    /// per line, split by `|`; `<` merges a position into the cell on its
    /// left, `^` into the one above; `{= expr}` binds a cell. With `in`
    /// (and `x`, `y`, `preset`, `header`, `label-col`, `emphasis` (1-based),
    /// `align`, `caption`) it makes a table; with `id` it replaces that
    /// table's words, keeping each cell's id when the shape holds. The
    /// `<cells>` and `<style>` lines of a table's view are not rows.
    fn table(&mut self, i: usize, el: &El) -> Result<(), String> {
        use crate::facet::LeafStruct;
        use crate::table;
        let rows = table::parse_rows(&el.text.lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join("\n"));
        let num = |k: &str| -> Result<Option<f64>, String> { el.attr(k).filter(|v| !v.trim().is_empty()).map(|v| v.trim().parse::<f64>().map_err(|_| format!("{k}=\"{v}\": expected a number"))).transpose() };
        let flatten = |es: Vec<VerbError>| es.iter().map(|e| e.message.clone()).collect::<Vec<_>>().join("; ");
        if let Some(raw) = el.attr("id") {
            let t = self.node(raw)?;
            let mut ops = table::replace_ops(&self.scratch, t, &rows)?;
            if let Some(h) = num("header")? {
                ops.push(Op::Set { id: t, path: "table.header".parse().expect("declared leaf"), slot: Slot::Const(Value::Number(h)) });
            }
            self.push(i, ops).map_err(flatten)?;
            let now = self.scratch.resolved(t, "table.preset".parse().expect("declared leaf")).and_then(|p| p.as_str().map(str::to_owned));
            if let Some(p) = el.attr("preset").filter(|p| Some(*p) != now.as_deref()) {
                let (ops, _) = table::edit_ops(&self.scratch, t, &format!("preset:{p}"), &[])?;
                self.push(i, ops).map_err(flatten)?;
            }
            self.summary.push(format!("  <table id=\"{t}\"/>"));
            return Ok(());
        }
        let parent = self.node(el.attr("in").ok_or("<table> needs in=\"#id\" (a composition or group) to make a table, or id=\"#table\" to replace one")?)?;
        let spec = table::Spec {
            rows,
            preset: table::Preset::parse(el.attr("preset").unwrap_or("light")),
            header: num("header")?.unwrap_or(1.0) as usize,
            label_col: el.attr("label-col") == Some("true"),
            emphasis: num("emphasis")?.filter(|e| *e >= 1.0).map(|e| e as usize - 1),
            align: el.attr("align").map(|a| a.split_whitespace().map(str::to_owned).collect()).unwrap_or_default(),
        };
        let at = [num("x")?.unwrap_or(0.0), num("y")?.unwrap_or(0.0)];
        let order = self.keys(Some(parent), None, false, 1, &[])?.remove(0);
        let doc = self.scratch.document();
        let (ops, made) = match el.attr("caption").filter(|c| !c.trim().is_empty()) {
            None => table::create_ops(doc, Some(parent), order, at, &spec)?,
            Some(words) => {
                // A stack: the caption above the table (layout-and-tables.md
                // §3.3), small, spaced capitals in the muted ink.
                let ids = doc.next_ids(2);
                let mut stack = crate::facet::Layout::default();
                stack.mode = Slot::Const(Value::Enum("stack_y".into()));
                stack.gap = Slot::Const(Value::Number(10.0));
                let mut place = crate::facet::Transform2d::default();
                place.x = Slot::Const(Value::Number(at[0]));
                place.y = Slot::Const(Value::Number(at[1]));
                let mut name = crate::facet::Name::default();
                name.text = Slot::Const(Value::Str("captioned table".into()));
                let mut text = crate::facet::Text::default();
                text.content = Slot::Const(Value::Str(words.to_uppercase()));
                let mut font = crate::facet::Font::default();
                font.size = Slot::Const(Value::Number(12.0));
                font.weight = Slot::Const(Value::Number(500.0));
                font.letter_spacing = Slot::Const(Value::Number(12.0));
                let mut fill = crate::facet::Fill::default();
                fill.color = Slot::Bound(crate::expr::Expr::parse("@muted").expect("token"));
                let mut ops = vec![
                    Op::Create { id: ids[0], ty: TypeId::lookup("group").expect("registered"), parent: Some(parent), order, facets: vec![place.wrap(), stack.wrap(), name.wrap()] },
                    Op::Create { id: ids[1], ty: TypeId::lookup("text").expect("registered"), parent: Some(ids[0]), order: OrderKey::first(), facets: vec![crate::facet::Transform2d::default().wrap(), text.wrap(), font.wrap(), fill.wrap()] },
                ];
                // The table's ids follow the two just taken.
                let (more, t) = table::create_ops_after(doc, 2, Some(ids[0]), OrderKey::after(&OrderKey::first()), [0.0, 0.0], &spec)?;
                ops.extend(more);
                (ops, t)
            }
        };
        self.push(i, ops).map_err(flatten)?;
        self.summary.push(format!("  <table id=\"{made}\"/>"));
        Ok(())
    }

    /// `<write narrative after>` and `<rewrite block>` (plans/specs/
    /// narrative-panel.md §4): markdown with directives into a narrative.
    fn narrative(&mut self, i: usize, el: &El) -> Result<(), String> {
        use crate::narrative::{self, Edit};
        let body = el.text.strip_prefix('\n').unwrap_or(&el.text).trim_end().to_owned();
        let doc = self.scratch.document();
        let (n, edits, keys) = if el.name == "write" {
            let source = el.attr("narrative").ok_or("<write> needs narrative=\"#N\" (and after=\"#B\", or after=\"end\")")?;
            let n = narrative::find(doc, source).ok_or_else(|| format!("no narrative {source:?}"))?;
            let mut prev = match el.attr("after").unwrap_or("end") {
                "" => None,
                "end" => doc.children(n).last().map(|b| b.to_string()),
                a => Some(a.to_owned()),
            };
            let mut edits = Vec::new();
            let mut keys = Vec::new();
            for (k, block) in narrative::parse_markdown(&body).into_iter().enumerate() {
                let key = format!("w{k}");
                edits.push(Edit::Insert { key: key.clone(), after: prev.clone(), block });
                keys.push(key.clone());
                prev = Some(key);
            }
            if edits.is_empty() {
                return Err("nothing to write: the markdown has no blocks".into());
            }
            (n, edits, keys)
        } else {
            let block = el.attr("block").ok_or("<rewrite> needs block=\"#B\"")?;
            let id = block.parse::<NodeId>().map_err(|e| e.to_string())?;
            let n = doc.live(id).and_then(|b| b.parent).filter(|p| narrative::narratives(doc).contains(p)).ok_or_else(|| format!("{block} is not a narrative block"))?;
            let mut edits = Vec::new();
            let mut keys = Vec::new();
            let mut prev = block.to_owned();
            for (k, b) in narrative::parse_markdown(&body).into_iter().enumerate() {
                if k == 0 {
                    edits.push(Edit::Set { id: block.to_owned(), block: b });
                    keys.push(block.to_owned());
                } else {
                    let key = format!("r{k}");
                    edits.push(Edit::Insert { key: key.clone(), after: Some(prev.clone()), block: b });
                    prev = key.clone();
                    keys.push(key);
                }
            }
            if edits.is_empty() {
                edits.push(Edit::Delete { id: block.to_owned() });
            }
            (n, edits, keys)
        };
        let planned = narrative::plan(doc, n, &edits)?;
        let blocks: Vec<String> = keys.into_iter().map(|k| planned.minted.get(&k).map_or(k, |v| v.to_string())).collect();
        self.push(i, planned.ops).map_err(|es| es.iter().map(|e| e.message.clone()).collect::<Vec<_>>().join("; "))?;
        self.summary.push(format!("  <{} blocks=\"{}\"/>", el.name, blocks.join(" ")));
        Ok(())
    }
}

/// A store error's kind as the store names it (`cycle`, `unknown_node`).
fn kind_name(e: &crate::ApplyError) -> String {
    serde_json::to_value(&e.kind).ok().and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_else(|| "store".into())
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// `<link-figures>` asking which entity to keep (ECM I5), as markup.
fn question(version: u64, a: NodeId, b: NodeId, q: &crate::composition::LinkQuestion) -> String {
    let entity = |e: &crate::composition::EntitySummary| {
        let attr = |s: &str| escape(s).replace('"', "&quot;");
        format!(
            "    <entity id=\"{}\"{}{}{} figures=\"{}\"{}/>",
            e.entity,
            e.name.as_ref().map(|n| format!(" name=\"{}\"", attr(n))).unwrap_or_default(),
            e.def.as_ref().map(|d| format!(" definition=\"{}\"", attr(d))).unwrap_or_default(),
            e.args.iter().map(|(k, v)| format!(" {k}=\"{}\"", attr(v))).collect::<String>(),
            e.figures.iter().map(|f| f.to_string()).collect::<Vec<_>>().join(" "),
            if e.ports.is_empty() { String::new() } else { format!(" ports=\"{}\"", e.ports.join(" ")) },
        )
    };
    let collisions = if q.port_collisions.is_empty() { String::new() } else { format!("\n    <port-collision names=\"{}\"/>", q.port_collisions.join(" ")) };
    format!(
        "<edit-failed applied=\"none\" version=\"{version}\">\n  <question kind=\"merge\" a=\"{a}\" b=\"{b}\">\n{}\n{}{collisions}\n    Both already picture an entity. Resend <link-figures> with keep=\"{}\" or keep=\"{}\"; the other entity is deleted and its figures move to the one kept.\n  </question>\n</edit-failed>",
        entity(&q.a),
        entity(&q.b),
        q.a.entity,
        q.b.entity
    )
}
