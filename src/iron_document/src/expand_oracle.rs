//! Legacy whole-document expansion oracle (`IRON_OWNER_CHECK=1`,
//! `cfg(feature = "differential")`): computes from scratch what every owner
//! wants and compares it directly against `Tree`'s virtual views (`verify_tree`)
//! — nodes, parents, types, orders, facets, derived relations, and problems.

use super::*;
use crate::document::Relation;
use crate::facet::Facet;
use crate::id::RelationId;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, PartialEq)]
struct Want {
    id: NodeId,
    ty: TypeId,
    parent: NodeId,
    order: OrderKey,
    facets: Vec<Facet>,
    scoped: bool,
}

#[derive(Debug, PartialEq)]
struct RelWant {
    rel: Relation,
}

struct Wanted {
    nodes: Vec<Want>,
    index: BTreeMap<NodeId, usize>,
    rels: Vec<RelWant>,
    problems: Vec<Problem>,
    full: bool,
    cap: usize,
}

impl Wanted {
    fn new(cap: usize) -> Wanted {
        Wanted { nodes: Vec::new(), index: BTreeMap::new(), rels: Vec::new(), problems: Vec::new(), full: false, cap }
    }

    fn push(&mut self, w: Want) -> bool {
        if self.nodes.len() >= self.cap {
            if !self.full {
                self.full = true;
                self.problems.push(Problem {
                    node: w.parent,
                    kind: "budget",
                    message: format!("the document derives more than {NODE_BUDGET} nodes; the rest are left out, from {} on", w.id),
                });
            }
            return false;
        }
        self.index.insert(w.id, self.nodes.len());
        self.nodes.push(w);
        true
    }

    fn get(&self, id: NodeId) -> Option<&Want> {
        self.index.get(&id).map(|&i| &self.nodes[i])
    }

    fn enter(&mut self, owner: NodeId) -> bool {
        if self.full {
            return false;
        }
        let depth = owner.nesting() + 1;
        if depth > DEPTH_MAX {
            self.problems.push(Problem {
                node: owner,
                kind: "depth_blocked",
                message: format!("nested more than {DEPTH_MAX} deep: not expanded (does something contain itself?)"),
            });
            return false;
        }
        if depth == DEPTH_REPORTED + 1 {
            self.problems.push(Problem {
                node: owner,
                kind: "depth",
                message: format!("nested more than {DEPTH_REPORTED} deep"),
            });
        }
        true
    }
}

struct Whole {
    nodes: Vec<Want>,
    rels: Vec<RelWant>,
    problems: Vec<Problem>,
}

fn top_owners(doc: Tree<'_>) -> BTreeSet<NodeId> {
    let mut out = BTreeSet::new();
    for ty in ["entity", "use"] {
        let ty = TypeId::lookup(ty).expect("registered");
        out.extend(doc.nodes_of_type(ty).filter(|&n| doc.is_live(n) && !n.is_expanded()));
    }
    out.extend(drawn_repeats(doc).into_iter().filter(|g| !g.is_expanded()));
    out.extend(doc.expanded_ids().filter(|&d| doc.is_live(d)).filter_map(|d| d.expanded_root()));
    out.extend(doc.relations().filter_map(|r| r.id.root()));
    out
}

fn draws_repeat(doc: Tree<'_>, id: NodeId, facets: &[Facet]) -> bool {
    facets.iter().any(|f| owns_items(f.kind())) && !id.path().first().is_some_and(|&root| in_library(doc, root))
}

fn linked_def_of(doc: Tree<'_>, facets: &[Facet]) -> Option<NodeId> {
    facets
        .iter()
        .find_map(|f| match f {
            Facet::Instance(inst) if inst.mode.constant().and_then(|v| v.as_str()) == Some("linked") => inst.def.constant()?.as_ref(),
            _ => None,
        })
        .filter(|&d| doc.live(d).is_some_and(|n| n.ty.name() == "definition"))
}

fn in_template(doc: Tree<'_>, out: &Wanted, id: NodeId) -> bool {
    let parent_of = |n: NodeId| out.get(n).map(|w| w.parent).or_else(|| doc.parent_of(n));
    let is_repeat = |n: NodeId| match out.get(n) {
        Some(w) => w.facets.iter().any(|f| owns_items(f.kind())),
        None => is_item_owner(doc, n),
    };
    let mut at = parent_of(id);
    while let Some(p) = at {
        if is_repeat(p) && id.item_of(p).is_none() {
            return true;
        }
        at = parent_of(p);
    }
    false
}

/// Compare every virtual node, facet, relation, and expansion problem in `doc`
/// against from-scratch legacy expansion (`wanted`).
pub(crate) fn verify_tree(doc: Tree<'_>, counts: &Items, problems: &[Problem]) -> Result<(), String> {
    let Whole { nodes, rels, problems: expected_problems } = wanted(doc, counts);
    let expected_ids: BTreeSet<NodeId> = nodes.iter().map(|w| w.id).collect();
    let actual_ids: BTreeSet<NodeId> = doc.expanded_ids().collect();
    if actual_ids != expected_ids {
        let missing: Vec<_> = expected_ids.difference(&actual_ids).take(5).collect();
        let extra: Vec<_> = actual_ids.difference(&expected_ids).take(5).collect();
        return Err(format!("virtual node set differs: missing={missing:?}, extra={extra:?}"));
    }
    for w in &nodes {
        let Some(actual) = doc.live(w.id) else {
            return Err(format!("virtual node {} missing", w.id));
        };
        if actual.ty != w.ty || actual.parent != Some(w.parent) || actual.order != w.order {
            return Err(format!(
                "virtual node {} metadata differs: got ({:?}, {:?}, {:?}) vs expected ({:?}, {:?}, {:?})",
                w.id, actual.ty, actual.parent, actual.order, w.ty, Some(w.parent), w.order
            ));
        }
        let mut expected_facets = w.facets.clone();
        expected_facets.sort_by_key(Facet::kind);
        let actual_facets: Vec<Facet> = doc.facets_of(w.id).cloned().collect();
        if actual_facets != expected_facets {
            return Err(format!("virtual node {} facets differ:\n  got:      {actual_facets:?}\n  expected: {expected_facets:?}", w.id));
        }
    }
    let expected_rels: BTreeMap<RelationId, Relation> = rels
        .into_iter()
        .map(|rw| rw.rel)
        .filter(|r| {
            let from_ok = expected_ids.contains(&r.from) || (!r.from.is_expanded() && doc.is_live(r.from));
            let to_ok = match r.to.expanded_root() {
                Some(root) => doc.node(root).is_some(),
                None => doc.is_live(r.to),
            };
            from_ok && to_ok
        })
        .map(|r| (r.id, r))
        .collect();
    let actual_rels: BTreeMap<RelationId, Relation> = doc.relations().filter(|r| r.id.is_expanded()).map(|r| (r.id, r.clone())).collect();
    if actual_rels != expected_rels {
        return Err(format!("derived relations differ:\n  got:      {:?}\n  expected: {:?}", actual_rels.keys().collect::<Vec<_>>(), expected_rels.keys().collect::<Vec<_>>()));
    }
    if problems != expected_problems.as_slice() {
        return Err(format!("expansion problems differ:\n  got:      {problems:?}\n  expected: {expected_problems:?}"));
    }
    Ok(())
}

fn wanted(doc: Tree<'_>, counts: &Items) -> Whole {
    let mut out = Whole { nodes: Vec::new(), rels: Vec::new(), problems: Vec::new() };
    let use_ty = TypeId::lookup("use").expect("registered");
    let mut by_root: BTreeMap<NodeId, Vec<(bool, NodeId)>> = BTreeMap::new();
    for owner in top_owners(doc) {
        let mut root = owner;
        while let Some(p) = doc.parent_of(root) {
            root = p;
        }
        by_root.entry(root).or_default().push((doc.ty_of(owner) == Some(use_ty), owner));
    }
    for (_, mut owners) in by_root {
        owners.sort_unstable();
        for (_, owner) in owners {
            let w = wanted_for(doc, owner, counts, NODE_BUDGET);
            out.nodes.extend(w.nodes);
            out.rels.extend(w.rels);
            out.problems.extend(w.problems);
        }
    }
    sort_problems(&mut out.problems);
    out
}

fn wanted_for(doc: Tree<'_>, owner: NodeId, counts: &Items, cap: usize) -> Wanted {
    let mut out = Wanted::new(cap);
    let Some(node) = doc.live(owner).filter(|_| !owner.is_expanded()) else { return out };
    match node.ty.name() {
        "entity" => {
            if let Some(def) = linked_definition(doc, owner) {
                want_entity(doc, owner, def, &mut out);
            }
        }
        "use" => {
            let notation = doc.get::<Use>(owner).and_then(|x| x.notation.constant()).and_then(|v| v.as_str()).map(str::to_owned);
            let sized = doc.has(owner, FacetKind::Size);
            if let Some(def) = main_definition(doc, owner) {
                if let Some(notation) = notation {
                    want_use(doc, owner, sized, None, def, &notation, &mut out);
                }
            } else if let Some(entity) = use_entity(doc, owner)
                && let Some(def) = linked_definition(doc, entity)
                && let Some(notation) = notation
            {
                want_use(doc, owner, sized, Some(entity), def, &notation, &mut out);
            }
        }
        "group" => {
            if is_item_owner(doc, owner) && draws_repeat(doc, owner, &doc.facets_of(owner).cloned().collect::<Vec<_>>()) && !in_repeat_template(doc, owner) {
                want_items(doc, owner, counts.get(&owner).map_or(&[][..], Vec::as_slice), &mut out);
            }
        }
        _ => {}
    }
    want_scopes(doc, counts, &mut out, Vec::new());
    out
}

fn want_scopes(doc: Tree<'_>, counts: &Items, out: &mut Wanted, mut repeats: Vec<NodeId>) {
    let mut done = BTreeSet::new();
    let mut seen = 0;
    loop {
        while seen < out.nodes.len() {
            let w = &out.nodes[seen];
            seen += 1;
            let (id, scoped) = (w.id, w.scoped);
            if draws_repeat(doc, id, &w.facets) {
                repeats.push(id);
            }
            if scoped || in_template(doc, out, id) {
                continue;
            }
            match w.ty.name() {
                "entity" => {
                    if let Some(def) = linked_def_of(doc, &w.facets) {
                        want_entity(doc, id, def, out);
                    }
                }
                "use" => {
                    let sized = w.facets.iter().any(|f| matches!(f, Facet::Size(_)));
                    let notation = w.facets.iter().find_map(|f| match f {
                        Facet::Use(x) => x.notation.constant().and_then(|v| v.as_str()).map(str::to_owned),
                        _ => None,
                    });
                    let entity = out.rels.iter().find(|r| r.rel.from == id && r.rel.rel == RelKind::Figures).map(|r| r.rel.to);
                    let def = entity.and_then(|e| match out.get(e) {
                        Some(copy) => linked_def_of(doc, &copy.facets),
                        None => linked_definition(doc, e),
                    });
                    if let (Some(entity), Some(def), Some(notation)) = (entity, def, notation) {
                        want_use(doc, id, sized, Some(entity), def, &notation, out);
                    }
                }
                _ => {}
            }
        }
        let Some(r) = repeats.pop() else { break };
        if done.insert(r) && !in_template(doc, out, r) {
            want_items(doc, r, counts.get(&r).map_or(&[][..], Vec::as_slice), out);
        }
    }
}

fn want_items(doc: Tree<'_>, r: NodeId, keys: &[String], out: &mut Wanted) {
    if keys.is_empty() || !out.enter(r) {
        return;
    }
    let (template, template_rels) = template_of_repeat(doc, r, out);
    let inside: BTreeSet<NodeId> = template.iter().map(|(id, ..)| *id).collect();
    let group = TypeId::lookup("group").expect("registered");
    let mut key = template.iter().filter(|(_, p, ..)| *p == r).map(|(_, _, _, o, _)| o.clone()).max().unwrap_or_else(OrderKey::first);
    for k in keys {
        let k = k.as_str();
        let Some(scope) = NodeId::item_scope(r, k) else { continue };
        key = OrderKey::after(&key);
        if !out.push(Want { id: scope, ty: group, parent: r, order: key.clone(), facets: vec![FacetKind::Item.default_facet()], scoped: true }) {
            return;
        }
        let follow = |leaf: Leaf| -> Leaf {
            if leaf.node == r {
                let item = |f: &str| Leaf { node: scope, path: LeafPath::new(FacetKind::Item, f).expect("declared leaf") };
                if leaf.path == LeafPath::new(FacetKind::Repeat, "item").expect("declared leaf") {
                    return item("value");
                }
                if leaf.path == LeafPath::new(FacetKind::Repeat, "index").expect("declared leaf") {
                    return item("index");
                }
                if owns_items(leaf.path.facet) && matches!(leaf.path.field, "presence" | "leaving") {
                    return item(leaf.path.field);
                }
            }
            let prefix = |n: NodeId| -> Option<NodeId> {
                let p = n.path();
                (1..=p.len()).find_map(|len| {
                    let head = NodeId::from_path(&p[..len])?;
                    if !inside.contains(&head) {
                        return None;
                    }
                    let mut moved = NodeId::item(r, k, head)?.path();
                    moved.extend_from_slice(&p[len..]);
                    NodeId::from_path(&moved)
                })
            };
            prefix(leaf.node).map_or(leaf, |node| Leaf { node, path: leaf.path })
        };
        let any = LeafPath::new(FacetKind::Name, "text").expect("declared");
        for (local, parent, ty, order, facets) in &template {
            let (Some(copy), Some(parent_copy)) = (NodeId::item(r, k, *local), if *parent == r { Some(scope) } else { NodeId::item(r, k, *parent) }) else { continue };
            let mut facets: Vec<Facet> = facets
                .iter()
                .map(|c| {
                    let mut c = c.clone();
                    for field in c.field_names() {
                        match c.get(field) {
                            Some(Slot::Bound(e)) => {
                                let e = e.map_refs(&follow);
                                c.set(field, Slot::Bound(e)).expect("field exists");
                            }
                            Some(Slot::Const(Value::Ref(to))) => {
                                let to = follow(Leaf { node: *to, path: any }).node;
                                c.set(field, Slot::Const(Value::Ref(to))).expect("field exists");
                            }
                            _ => {}
                        }
                    }
                    c
                })
                .collect();
            let target = local.path().last().copied().unwrap_or(*local);
            apply_overrides(doc, scope, target, *ty, &mut facets);
            if !out.push(Want { id: copy, ty: *ty, parent: parent_copy, order: order.clone(), facets, scoped: false }) {
                return;
            }
        }
        for rel in &template_rels {
            let Some(from) = NodeId::item(r, k, rel.from) else { continue };
            let to = follow(Leaf { node: rel.to, path: any }).node;
            out.rels.push(RelWant { rel: Relation { id: RelationId::expanded(scope, rel.id), from, to, rel: rel.rel, data: rel.data.clone() } });
        }
    }
}

fn want_entity(doc: Tree<'_>, entity: NodeId, def: NodeId, out: &mut Wanted) {
    if !out.enter(entity) {
        return;
    }
    let inputs = definition::inputs_of(doc, def).unwrap_or_default();
    let port = TypeId::lookup("port").expect("registered");
    let follow = |leaf: Leaf| -> Leaf {
        if is_body(doc, def, leaf.node.path()[0]) {
            relocate(entity, leaf.node).map_or(leaf, |node| Leaf { node, path: leaf.path })
        } else {
            leaf
        }
    };
    let mut inner: Vec<(NodeId, NodeId)> = Vec::new();
    for &child in doc.children(def).iter() {
        let Some(node) = doc.live(child).filter(|_| !child.is_session()) else { continue };
        let ty = match node.ty.name() {
            "port_spec" => port,
            "equation" | "entity" | "connection" | "channel" | "machine" | "let" | "trace" | "accumulate" | "query" => node.ty,
            _ => continue,
        };
        let Some(id) = NodeId::expanded(entity, child) else { continue };
        let mut facets = rewritten(doc, child, &follow);
        if let Some(input) = inputs.iter().find(|i| i.spec == child) {
            let arg = Leaf { node: entity, path: LeafPath::new(FacetKind::Args, input.name).expect("argument path") };
            for f in &mut facets {
                if let Facet::Port(p) = f {
                    p.signal = Slot::Bound(Expr::Ref(arg));
                }
            }
        }
        apply_overrides(doc, entity, child, ty, &mut facets);
        if node.ty.name() == "entity"
            && let Some(inner_def) = linked_def_of(doc, &facets)
        {
            inner.push((id, inner_def));
        }
        if !out.push(Want { id, ty, parent: entity, order: node.order.clone(), facets, scoped: true }) {
            return;
        }
        if matches!(node.ty.name(), "connection" | "port_spec") {
            let any = LeafPath::new(FacetKind::Name, "text").expect("declared");
            for r in doc.relations_from(child) {
                let to = follow(Leaf { node: r.to, path: any }).node;
                out.rels.push(RelWant { rel: Relation { id: RelationId::expanded(entity, r.id), from: id, to, rel: r.rel, data: r.data.clone() } });
            }
        }
        if let Some((copy, inner_def)) = inner.pop() {
            want_entity(doc, copy, inner_def, out);
        }
    }
    let copied = |n: NodeId| !n.is_expanded() && !n.is_session();
    for &machine in doc.children(def).iter().filter(|&&c| !c.is_session() && doc.live(c).is_some_and(|n| n.ty.name() == "machine")) {
        let mut stack: Vec<NodeId> = doc.children(machine).iter().rev().copied().filter(|&c| copied(c)).collect();
        while let Some(local) = stack.pop() {
            let Some(n) = doc.live(local) else { continue };
            let Some(parent) = n.parent.and_then(|p| NodeId::expanded(entity, p)) else { continue };
            let Some(id) = NodeId::expanded(entity, local) else { continue };
            let mut facets = rewritten(doc, local, &follow);
            apply_overrides(doc, entity, local, n.ty, &mut facets);
            if !out.push(Want { id, ty: n.ty, parent, order: n.order.clone(), facets, scoped: true }) {
                return;
            }
            stack.extend(doc.children(local).iter().rev().copied().filter(|&c| copied(c)));
        }
    }
}

fn want_use(doc: Tree<'_>, u: NodeId, sized: bool, entity: Option<NodeId>, def: NodeId, notation: &str, out: &mut Wanted) {
    if !out.enter(u) {
        return;
    }
    let Some(template) = definition::template_of(doc, def, notation) else { return };
    let inside: BTreeSet<NodeId> = doc.descendants(template).into_iter().filter(|d| !d.is_expanded()).collect();
    let follow = |leaf: Leaf| -> Leaf {
        if leaf.node == template {
            return if sized && leaf.path.facet == FacetKind::Size { Leaf { node: u, path: leaf.path } } else { leaf };
        }
        let root = leaf.node.path()[0];
        let to = if inside.contains(&root) {
            u
        } else if is_body(doc, def, root) {
            match entity {
                Some(e) => e,
                None => return leaf,
            }
        } else {
            return leaf;
        };
        relocate(to, leaf.node).map_or(leaf, |node| Leaf { node, path: leaf.path })
    };
    let copied = |n: NodeId| !n.is_expanded() && !n.is_session() && doc.live(n).is_some_and(|x| x.ty.name() != "override");
    let mut stack: Vec<NodeId> = doc.children(template).iter().rev().copied().filter(|&c| copied(c)).collect();
    while let Some(local) = stack.pop() {
        let Some(node) = doc.live(local) else { continue };
        let parent = match node.parent {
            Some(p) if p == template => Some(u),
            Some(p) => NodeId::expanded(u, p),
            None => None,
        };
        let (Some(id), Some(parent)) = (NodeId::expanded(u, local), parent) else { continue };
        let mut facets = rewritten(doc, local, &follow);
        apply_overrides(doc, u, local, node.ty, &mut facets);
        if node.ty.name() == "use" {
            let inner = doc.relations_from(local).find(|r| r.rel == RelKind::Figures);
            let own = inner.is_some_and(|r| !inside.contains(&r.to.path()[0]));
            let inner_sized = facets.iter().any(|f| matches!(f, Facet::Size(_)));
            let inner_notation = facets.iter().find_map(|f| match f {
                Facet::Use(x) => x.notation.constant().and_then(|v| v.as_str()).map(str::to_owned),
                _ => None,
            });
            if !out.push(Want { id, ty: node.ty, parent, order: node.order.clone(), facets, scoped: own }) {
                return;
            }
            if let Some(r) = inner {
                let any = LeafPath::new(FacetKind::Name, "text").expect("declared");
                let inner_entity = follow(Leaf { node: r.to, path: any }).node;
                out.rels.push(RelWant { rel: Relation { id: RelationId::expanded(u, r.id), from: id, to: inner_entity, rel: RelKind::Figures, data: r.data.clone() } });
                if own
                    && let Some(notation) = inner_notation
                    && let Some(d) = doc.node(r.to).and_then(|_| doc.get::<Instance>(r.to)).and_then(|i| (i.mode.constant().and_then(|v| v.as_str()) == Some("linked")).then(|| i.def.constant().and_then(|v| v.as_ref())).flatten())
                {
                    want_use(doc, id, inner_sized, Some(inner_entity), d, &notation, out);
                }
            }
            continue;
        }
        let scoped = node.ty.name() != "entity";
        if !out.push(Want { id, ty: node.ty, parent, order: node.order.clone(), facets, scoped }) {
            return;
        }
        stack.extend(doc.children(local).iter().rev().copied().filter(|&c| copied(c)));
    }
}

fn apply_overrides(doc: Tree<'_>, owner: NodeId, local: NodeId, ty: TypeId, facets: &mut Vec<Facet>) {
    for (_, target, leaf, value) in overrides(doc, owner) {
        if target != local {
            continue;
        }
        if !facets.iter().any(|f| f.kind() == leaf.facet) {
            if !ty.spec().allows(leaf.facet) {
                continue;
            }
            facets.push(leaf.facet.default_facet());
        }
        if let Some(f) = facets.iter_mut().find(|f| f.kind() == leaf.facet) {
            let _ = f.set(leaf.field, Slot::Const(value));
        }
    }
}

fn rewritten(doc: Tree<'_>, node: NodeId, follow: &dyn Fn(Leaf) -> Leaf) -> Vec<Facet> {
    doc.facets_of(node)
        .map(|c| {
            let mut c = c.clone();
            for field in c.field_names() {
                match c.get(field) {
                    Some(Slot::Bound(expr)) => {
                        let e = expr.map_refs(follow);
                        c.set(field, Slot::Bound(e)).expect("field exists");
                    }
                    Some(Slot::Const(Value::Ref(r))) => {
                        let any = LeafPath::new(FacetKind::Name, "text").expect("declared");
                        let to = follow(Leaf { node: *r, path: any }).node;
                        c.set(field, Slot::Const(Value::Ref(to))).expect("field exists");
                    }
                    _ => {}
                }
            }
            c
        })
        .collect()
}

type TemplateNode = (NodeId, NodeId, TypeId, OrderKey, Vec<Facet>);

fn template_of_repeat(doc: Tree<'_>, r: NodeId, out: &Wanted) -> (Vec<TemplateNode>, Vec<Relation>) {
    let mut nodes = Vec::new();
    let mut rels = Vec::new();
    if r.is_expanded() {
        let mut parents = BTreeSet::from([r]);
        for w in &out.nodes {
            let derived_by_parent = w.id.expanded_parts().is_some_and(|(o, _)| o == w.parent);
            if parents.contains(&w.parent) && w.id.item_of(r).is_none() && w.ty.name() != "override" && !derived_by_parent {
                parents.insert(w.id);
                nodes.push((w.id, w.parent, w.ty, w.order.clone(), w.facets.clone()));
            }
        }
        rels.extend(out.rels.iter().filter(|x| parents.contains(&x.rel.from) && x.rel.from != r).map(|x| x.rel.clone()));
    } else {
        let copied = |c: &NodeId| !c.is_expanded() && !c.is_session();
        let mut stack: Vec<NodeId> = doc.children(r).iter().rev().copied().filter(copied).collect();
        while let Some(n) = stack.pop() {
            let Some(node) = doc.live(n).filter(|x| x.ty.name() != "override") else { continue };
            nodes.push((n, node.parent.unwrap_or(r), node.ty, node.order.clone(), doc.facets_of(n).cloned().collect()));
            rels.extend(doc.relations_from(n).filter(|x| !x.id.is_expanded()).cloned());
            stack.extend(doc.children(n).iter().rev().copied().filter(copied));
        }
    }
    (nodes, rels)
}
