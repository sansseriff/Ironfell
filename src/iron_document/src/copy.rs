//! Copy, paste and duplicate (plans/specs/clipboard-and-assets.md).
//!
//! Copying takes a set of roots and works out its *closure*: the roots and
//! their descendants, plus the nodes that belong with them although nobody
//! selected them. The result is a [`Fragment`], a small document holding the
//! copied nodes under their original ids. Pasting turns a fragment into
//! ordinary `Create` and `Link` ops with fresh ids, so a paste is one
//! transaction and undoes like any other. There is no copy op.
//!
//! What happens to each reference that leaves the copied set:
//!
//! | Reference | Rule |
//! |---|---|
//! | `figures` (shape → entity) | the entity is copied too, with its ports and equations: a copied laser is a second laser |
//! | `connection` (two `connects`) | copied when both ports are; otherwise left behind, and the copy's port is unconnected |
//! | `constraint` (`constrains`) | copied when every operand is; otherwise left behind |
//! | clip (`animates`) | copied when every target is |
//! | `after`, `during`, `aligns_to`, `references` | copied when both ends are |
//! | binding reading an outside leaf | kept in the same document; frozen to the value it read when pasted elsewhere |
//! | `instance.def`, `embed.source`, `embed_camera.camera` | kept: definitions and embedded compositions are shared |
//! | `selects` (session) | never copied |
//!
//! A reference inside the set always follows the copy. `encapsulate` (ECM
//! §9) asks the same boundary question of a selection: what crosses it.
//!
//! The fragment's text form is the canonical file format wrapped with the
//! paste metadata, so a fragment survives the system clipboard and old
//! fragments migrate like old files.

use crate::component::{Component, ComponentKind, LeafPath};
use crate::composition;
use crate::document::{Document, Node, RelKind, Relation};
use crate::expr::{Expr, Leaf};
use crate::geom::Affine2;
use crate::id::{NodeId, RelationId};
use crate::op::Op;
use crate::order::OrderKey;
use crate::registry::TypeId;
use crate::value::{Slot, Value};
use crate::Store;
use std::collections::{BTreeMap, BTreeSet};

/// The format version of [`Fragment::to_text`].
pub const FRAGMENT_FORMAT: u64 = 1;

/// Types a copy starts from. Roots of the tree (compositions, the model, the
/// library, timelines), the session, and the parts of a definition are never
/// copied on their own; ports, equations and points come with their owner.
fn copyable(ty: &str) -> bool {
    !matches!(
        ty,
        "composition" | "model" | "library" | "timeline" | "session" | "selection" | "definition" | "param_spec" | "port_spec" | "template" | "port" | "equation" | "point"
    )
}

/// What was copied: the nodes under their original ids, and how to put
/// them back.
#[derive(Clone, Debug)]
pub struct Fragment {
    /// The copied nodes, components and inside relations. Roots and extras
    /// have no parent here.
    pub doc: Document,
    /// What was asked for, outermost only, in painter order.
    pub roots: Vec<NodeId>,
    /// Nodes the closure pulled in: entities, connections, constraints,
    /// clips. They go beside their originals, not where the roots go.
    pub extras: Vec<NodeId>,
    /// Each root's and extra's parent and sibling key where it was copied.
    pub homes: BTreeMap<NodeId, (Option<NodeId>, OrderKey)>,
    /// Each root's parent's world affine (document 2D space), to keep its
    /// place when pasted into another container.
    pub parent_world: BTreeMap<NodeId, Affine2>,
    /// The value every outside leaf read by a copied binding had, to freeze
    /// it with when the paste cannot keep the reference.
    pub externals: BTreeMap<Leaf, Value>,
    /// Who copied: the caller's token for the document and session. A
    /// paste keeps outside references only under the same token.
    pub origin: String,
    /// One line per reference that was left behind, for a person.
    pub dropped: Vec<String>,
}

/// Where a paste goes.
#[derive(Clone, Debug, Default)]
pub struct Placement {
    /// The container for the roots; `None` puts each back in its own.
    pub parent: Option<NodeId>,
    /// Each root right above its original (duplicate) rather than on top of
    /// its container (paste).
    pub beside: bool,
    /// Added to each root's position, document 2D space.
    pub offset: [f64; 2],
    /// Figures relink to the entities they picture instead of copying them,
    /// when those are live here: the first paste after a cut is a move.
    pub share_entities: bool,
}

/// A paste, ready to apply as one transaction.
#[derive(Clone, Debug, PartialEq)]
pub struct Pasted {
    pub ops: Vec<Op>,
    /// The new roots, in the fragment's order: what to select.
    pub roots: Vec<NodeId>,
    /// One line per binding frozen or reference that could not be kept.
    pub notes: Vec<String>,
}

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).expect("registered type")
}

fn type_name(doc: &Document, id: NodeId) -> &'static str {
    doc.node(id).map(|n| n.ty.name()).unwrap_or("")
}

/// Every live node in preorder: painter order.
fn preorder(doc: &Document) -> BTreeMap<NodeId, usize> {
    let mut out = BTreeMap::new();
    let mut stack: Vec<NodeId> = doc.roots().iter().rev().copied().collect();
    while let Some(id) = stack.pop() {
        out.insert(id, out.len());
        stack.extend(doc.children(id).iter().rev().copied());
    }
    out
}

/// Copy `roots` (and what belongs with them) out of `store`. Roots inside
/// another root, dead nodes and uncopyable types are skipped; `Err` when
/// nothing is left.
pub fn fragment(store: &Store, roots: &[NodeId], origin: &str) -> Result<Fragment, String> {
    let doc = store.document();
    let order = preorder(doc);
    let mut roots: Vec<NodeId> = roots
        .iter()
        .copied()
        .filter(|&r| doc.live(r).is_some_and(|n| copyable(n.ty.name())))
        .collect();
    roots.sort_by_key(|r| order.get(r).copied().unwrap_or(usize::MAX));
    roots.dedup();
    let all = roots.clone();
    roots.retain(|&r| !all.iter().any(|&o| o != r && doc.is_ancestor(o, r)));
    if roots.is_empty() {
        return Err("nothing to copy".into());
    }

    let mut set: BTreeSet<NodeId> = BTreeSet::new();
    let subtree = |id: NodeId, set: &mut BTreeSet<NodeId>| {
        set.insert(id);
        set.extend(doc.descendants(id));
    };
    for &r in &roots {
        subtree(r, &mut set);
    }
    let mut extras: Vec<NodeId> = Vec::new();
    let mut dropped: Vec<String> = Vec::new();

    // The entities the copied shapes picture.
    for n in set.clone() {
        if let Some(e) = composition::figured_by(doc, n)
            && !set.contains(&e)
        {
            extras.push(e);
            subtree(e, &mut set);
        }
    }
    // Connections, constraints and clips whose ends are all copied come
    // along; one with an end outside stays behind (and a constraint inside
    // the set that reaches out is left out of it).
    let ends_of = |id: NodeId, rel: RelKind| -> Vec<NodeId> { doc.relations_from(id).filter(|r| r.rel == rel).map(|r| r.to).collect() };
    for (kind, rel) in [("connection", RelKind::Connects), ("constraint", RelKind::Constrains), ("clip", RelKind::Animates)] {
        for c in doc.nodes_of_type(ty(kind)).collect::<Vec<_>>() {
            let ends = ends_of(c, rel);
            let inside = ends.iter().filter(|e| set.contains(e)).count();
            if ends.is_empty() || inside == 0 {
                continue;
            }
            if inside == ends.len() {
                if !set.contains(&c) {
                    extras.push(c);
                    set.insert(c);
                }
            } else {
                if set.remove(&c) {
                    roots.retain(|&r| r != c);
                }
                let outside: Vec<String> = ends.iter().filter(|e| !set.contains(e)).map(|e| e.to_string()).collect();
                dropped.push(format!("{kind} {c} not copied: {} outside the copy", outside.join(", ")));
            }
        }
    }

    // The fragment's tables.
    let mut nodes: BTreeMap<NodeId, Node> = BTreeMap::new();
    let mut components: BTreeMap<ComponentKind, BTreeMap<NodeId, Component>> = BTreeMap::new();
    let mut relations: BTreeMap<RelationId, Relation> = BTreeMap::new();
    let mut externals: BTreeMap<Leaf, Value> = BTreeMap::new();
    for &id in &set {
        let n = doc.live(id).expect("closure is live");
        let parent = n.parent.filter(|p| set.contains(p));
        nodes.insert(id, Node { id, ty: n.ty, parent, order: n.order.clone(), tombstoned: None });
        for c in doc.components_of(id) {
            for field in c.field_names() {
                if let Some(expr) = c.get(field).and_then(|s| s.expr()) {
                    for dep in expr.deps() {
                        if !set.contains(&dep.node)
                            && let Some(v) = store.resolved(dep.node, dep.path)
                        {
                            externals.insert(dep, v);
                        }
                    }
                }
            }
            components.entry(c.kind()).or_default().insert(id, c.clone());
        }
        for r in doc.relations_from(id) {
            if r.rel != RelKind::Selects && set.contains(&r.to) && !r.id.is_session() {
                relations.insert(r.id, r.clone());
            }
        }
    }
    let next_id = set.iter().map(|n| n.raw() + 1).max().unwrap_or(1);
    let next_rel = relations.keys().map(|r| r.raw() + 1).max().unwrap_or(1);
    let fragment_doc = Document::from_parts(nodes, components, relations, next_id, next_rel, doc.version());

    let mut homes = BTreeMap::new();
    let mut parent_world = BTreeMap::new();
    for &id in roots.iter().chain(&extras) {
        let n = doc.live(id).expect("live");
        homes.insert(id, (n.parent, n.order.clone()));
    }
    for &r in &roots {
        let pw = doc.live(r).and_then(|n| n.parent).map_or(Affine2::IDENTITY, |p| store.world_affine(p));
        parent_world.insert(r, pw);
    }
    Ok(Fragment { doc: fragment_doc, roots, extras, homes, parent_world, externals, origin: origin.to_owned(), dropped })
}

/// A sibling key just after `after` under `parent` in `doc`, before the
/// next sibling.
fn key_after(doc: &Document, parent: Option<NodeId>, after: &OrderKey) -> OrderKey {
    let siblings = match parent {
        Some(p) => doc.children(p),
        None => doc.roots(),
    };
    let next = siblings.iter().filter_map(|&s| doc.live(s)).map(|s| &s.order).find(|k| *k > after);
    OrderKey::between(Some(after), next).unwrap_or_else(|_| OrderKey::after(after))
}

/// A sibling key after every child of `parent`.
fn key_last(doc: &Document, parent: Option<NodeId>) -> OrderKey {
    let siblings = match parent {
        Some(p) => doc.children(p),
        None => doc.roots(),
    };
    siblings.last().and_then(|&s| doc.live(s)).map_or_else(OrderKey::first, |s| OrderKey::after(&s.order))
}

fn num(c: &Component, field: &str) -> Option<f64> {
    c.get(field)?.constant()?.as_f64()
}

/// Move a root's `transform2d` by `m` (its new parent space ← its old one).
/// A bound position stays where its binding puts it.
fn place_transform(c: &mut Component, m: Affine2, notes: &mut Vec<String>, id: NodeId) {
    if m.max_diff(&Affine2::IDENTITY) < 1e-12 {
        return;
    }
    let (Some(x), Some(y)) = (num(c, "x"), num(c, "y")) else {
        notes.push(format!("{id}: its position is bound, so the copy sits where the binding puts it"));
        return;
    };
    let [a, b, cc, d, _, _] = m.0;
    let linear = (a - 1.0).abs() < 1e-12 && b.abs() < 1e-12 && cc.abs() < 1e-12 && (d - 1.0).abs() < 1e-12;
    let set = |c: &mut Component, f: &str, v: f64| {
        let _ = c.set(f, Slot::Const(Value::Number(v)));
    };
    if linear {
        let p = m.apply([x, y]);
        set(c, "x", p[0]);
        set(c, "y", p[1]);
        return;
    }
    let (Some(rot), Some(sx), Some(sy)) = (num(c, "rot"), num(c, "sx"), num(c, "sy")) else {
        let p = m.apply([x, y]);
        set(c, "x", p[0]);
        set(c, "y", p[1]);
        return;
    };
    // The new local affine is m · L; read it back as T · R · S, shear dropped.
    let l = m.then(Affine2::from_trs(x, y, rot, sx, sy));
    let [a, b, cc, d, e, f] = l.0;
    let nsx = a.hypot(b).max(1e-12);
    set(c, "x", e);
    set(c, "y", f);
    set(c, "rot", b.atan2(a));
    set(c, "sx", nsx);
    set(c, "sy", (a * d - b * cc) / nsx);
}

/// The ops that put `frag` into `store`'s document. `here` is the caller's
/// token for this document and session (see [`Fragment::origin`]).
pub fn paste(store: &Store, frag: &Fragment, place: &Placement, here: &str) -> Result<Pasted, String> {
    let doc = store.document();
    let same = frag.origin == here;
    let mut notes = Vec::new();

    // Entities shared rather than copied (a move after a cut).
    let shared: BTreeSet<NodeId> = if place.share_entities && same {
        frag.extras.iter().copied().filter(|&e| type_name(&frag.doc, e) == "entity" && doc.is_live(e)).flat_map(|e| std::iter::once(e).chain(frag.doc.descendants(e))).collect()
    } else {
        BTreeSet::new()
    };

    // Creation order: each root's subtree, then each extra's, parents first.
    let mut create: Vec<NodeId> = Vec::new();
    for &r in frag.roots.iter().chain(&frag.extras) {
        if shared.contains(&r) {
            continue;
        }
        create.push(r);
        create.extend(frag.doc.descendants(r));
    }
    let ids = doc.next_ids(create.len());
    let map: BTreeMap<NodeId, NodeId> = create.iter().copied().zip(ids.iter().copied()).collect();

    // A reference into the fragment follows the copy; one out of it is kept
    // while it means the same thing here.
    let keep = |n: NodeId| same && doc.is_live(n);
    let leaf_for = |l: Leaf| -> Option<Leaf> {
        match map.get(&l.node) {
            Some(&n) => Some(Leaf { node: n, path: l.path }),
            None if keep(l.node) => Some(l),
            None => None,
        }
    };

    let offset = Affine2::translate(place.offset[0], place.offset[1]);
    let mut last_key: BTreeMap<Option<NodeId>, OrderKey> = BTreeMap::new();
    let mut ops = Vec::new();
    for &old in &create {
        let node = frag.doc.node(old).expect("fragment node");
        let is_root = frag.roots.contains(&old);
        let is_extra = frag.extras.contains(&old);
        let home = frag.homes.get(&old).cloned();
        let (parent, order) = if is_root {
            let parent = place.parent.or_else(|| home.as_ref().and_then(|h| h.0)).filter(|p| doc.is_live(*p));
            let parent = parent.or(place.parent);
            let beside = place.beside && same && home.as_ref().is_some_and(|h| h.0 == parent && doc.is_live(old));
            let order = if beside {
                key_after(doc, parent, &home.as_ref().expect("home").1)
            } else {
                let k = match last_key.get(&parent) {
                    Some(k) => OrderKey::after(k),
                    None => key_last(doc, parent),
                };
                last_key.insert(parent, k.clone());
                k
            };
            (parent, order)
        } else if is_extra {
            let (hp, hk) = home.clone().expect("extra has a home");
            let at_home = same && hp.is_none_or(|p| doc.is_live(p)) && doc.is_live(old);
            if at_home {
                (hp, key_after(doc, hp, &hk))
            } else {
                // Elsewhere: an entity or connection goes in the model, a
                // clip in the first timeline, a constraint with the roots.
                let parent = match node.ty.name() {
                    "entity" | "connection" => composition::model_root(doc),
                    "clip" => doc.nodes_of_type(ty("timeline")).next(),
                    _ => place.parent,
                };
                let k = match last_key.get(&parent) {
                    Some(k) => OrderKey::after(k),
                    None => key_last(doc, parent),
                };
                last_key.insert(parent, k.clone());
                (parent, k)
            }
        } else {
            (node.parent.map(|p| map[&p]), node.order.clone())
        };

        let mut comps: Vec<Component> = Vec::new();
        for c in frag.doc.components_of(old) {
            let mut c = c.clone();
            for field in c.field_names() {
                let Some(slot) = c.get(field).cloned() else { continue };
                let new = match slot {
                    Slot::Bound(expr) => {
                        let frozen = std::cell::RefCell::new(Vec::new());
                        let rewritten = expr.replace_refs(&|l| match leaf_for(l) {
                            Some(l) => Expr::Ref(l),
                            None => {
                                frozen.borrow_mut().push(l);
                                Expr::Lit(frag.externals.get(&l).cloned().unwrap_or(Value::Number(0.0)))
                            }
                        });
                        for l in frozen.into_inner() {
                            notes.push(format!("{}.{}.{field}: froze its read of {l}", map[&old], c.kind().name()));
                        }
                        Slot::Bound(rewritten)
                    }
                    Slot::Const(Value::Ref(r)) => Slot::Const(Value::Ref(map.get(&r).copied().unwrap_or(r))),
                    other => other,
                };
                let _ = c.set(field, new);
            }
            if is_root && c.kind() == ComponentKind::Transform2d {
                let src = frag.parent_world.get(&old).copied().unwrap_or(Affine2::IDENTITY);
                let dst = parent.map_or(Affine2::IDENTITY, |p| store.world_affine(p));
                let m = dst.inverse().unwrap_or(Affine2::IDENTITY).then(offset).then(src);
                place_transform(&mut c, m, &mut notes, map[&old]);
            }
            comps.push(c);
        }
        ops.push(Op::Create { id: map[&old], ty: node.ty, parent, order, components: comps });
    }

    // Relations inside the fragment, re-pointed; with shared entities, a
    // relation to one keeps its original end.
    let rels: Vec<&Relation> = frag.doc.relations().collect();
    let rel_ids = doc.next_relation_ids(rels.len());
    let end = |n: NodeId| map.get(&n).copied().or_else(|| shared.contains(&n).then_some(n));
    let mut used = 0;
    for r in rels {
        let (Some(from), Some(to)) = (end(r.from), end(r.to)) else { continue };
        if from == r.from && to == r.to {
            // Both ends shared: the original relation still stands.
            continue;
        }
        let mut data = r.data.clone();
        // A pin holds its operand at a world point, which moves with it.
        if r.rel == RelKind::Constrains
            && let Some(at) = data.get("at").and_then(|v| v.as_vec2())
        {
            data.insert("at".into(), Value::Vec2(offset.apply(at)));
        }
        ops.push(Op::Link { rel: Relation { id: rel_ids[used], from, to, rel: r.rel, data } });
        used += 1;
    }

    let roots = frag.roots.iter().filter_map(|r| map.get(r).copied()).collect();
    Ok(Pasted { ops, roots, notes })
}

/// Duplicate `roots` in place, each right above its original: Cmd+D.
pub fn duplicate(store: &Store, roots: &[NodeId]) -> Result<Pasted, String> {
    const HERE: &str = "duplicate";
    let frag = fragment(store, roots, HERE)?;
    let mut pasted = paste(store, &frag, &Placement { beside: true, ..Default::default() }, HERE)?;
    pasted.notes.extend(frag.dropped);
    Ok(pasted)
}

impl Fragment {
    /// The JSON text that goes on the clipboard.
    pub fn to_text(&self) -> String {
        use serde_json::json;
        let doc: serde_json::Value = serde_json::from_str(&crate::serial::save(&self.doc)).expect("saved document parses");
        let homes: serde_json::Map<String, serde_json::Value> = self
            .homes
            .iter()
            .map(|(id, (p, k))| (id.to_string(), json!({"parent": p.map(|p| p.to_string()), "order": k.as_str()})))
            .collect();
        let worlds: serde_json::Map<String, serde_json::Value> = self.parent_world.iter().map(|(id, a)| (id.to_string(), json!(a.0))).collect();
        let externals: Vec<serde_json::Value> = self.externals.iter().map(|(l, v)| json!({"leaf": l.to_string(), "value": v.to_json()})).collect();
        json!({
            "iron_fragment": FRAGMENT_FORMAT,
            "origin": self.origin,
            "roots": self.roots.iter().map(|r| r.to_string()).collect::<Vec<_>>(),
            "extras": self.extras.iter().map(|r| r.to_string()).collect::<Vec<_>>(),
            "homes": homes,
            "parent_world": worlds,
            "externals": externals,
            "dropped": self.dropped,
            "document": doc,
        })
        .to_string()
    }

    /// Whether `text` looks like a fragment, without parsing all of it.
    pub fn is_fragment_text(text: &str) -> bool {
        text.trim_start().starts_with('{') && text.contains("\"iron_fragment\":")
    }

    pub fn from_text(text: &str) -> Result<Fragment, String> {
        let j: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let format = j.get("iron_fragment").and_then(|v| v.as_u64()).ok_or("not an Iron fragment")?;
        if format > FRAGMENT_FORMAT {
            return Err(format!("fragment format {format} is newer than this build reads ({FRAGMENT_FORMAT})"));
        }
        let id = |v: &serde_json::Value| -> Result<NodeId, String> { v.as_str().ok_or("ids are strings")?.parse().map_err(|e: crate::id::IdParseError| e.to_string()) };
        let ids = |k: &str| -> Result<Vec<NodeId>, String> { j.get(k).and_then(|v| v.as_array()).ok_or(format!("missing {k}"))?.iter().map(id).collect() };
        let doc = crate::serial::load(&j.get("document").ok_or("missing document")?.to_string()).map_err(|e| e.0)?;
        let mut homes = BTreeMap::new();
        for (k, v) in j.get("homes").and_then(|v| v.as_object()).ok_or("missing homes")? {
            let parent = match v.get("parent") {
                Some(serde_json::Value::Null) | None => None,
                Some(p) => Some(id(p)?),
            };
            let order = OrderKey::parse(v.get("order").and_then(|o| o.as_str()).ok_or("home without order")?).map_err(|e| e.to_string())?;
            homes.insert(k.parse().map_err(|e: crate::id::IdParseError| e.to_string())?, (parent, order));
        }
        let mut parent_world = BTreeMap::new();
        for (k, v) in j.get("parent_world").and_then(|v| v.as_object()).ok_or("missing parent_world")? {
            let a: [f64; 6] = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
            parent_world.insert(k.parse().map_err(|e: crate::id::IdParseError| e.to_string())?, Affine2(a));
        }
        let mut externals = BTreeMap::new();
        for e in j.get("externals").and_then(|v| v.as_array()).ok_or("missing externals")? {
            let text = e.get("leaf").and_then(|v| v.as_str()).ok_or("external without leaf")?;
            let (node, path) = text.split_once('.').ok_or("bad leaf")?;
            let leaf = Leaf { node: node.parse().map_err(|e: crate::id::IdParseError| e.to_string())?, path: path.parse::<LeafPath>().map_err(|e| e.to_string())? };
            externals.insert(leaf, Value::from_json(e.get("value").ok_or("external without value")?)?);
        }
        let dropped = j.get("dropped").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_owned)).collect()).unwrap_or_default();
        let origin = j.get("origin").and_then(|v| v.as_str()).unwrap_or("").to_owned();
        let frag = Fragment { doc, roots: ids("roots")?, extras: ids("extras")?, homes, parent_world, externals, origin, dropped };
        for r in frag.roots.iter().chain(&frag.extras) {
            if frag.doc.live(*r).is_none() || !frag.homes.contains_key(r) {
                return Err(format!("fragment root {r} is missing"));
            }
        }
        Ok(frag)
    }
}
