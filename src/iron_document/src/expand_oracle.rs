//! The whole-document expansion as phase 2b built it (reactive-engine
//! handoff §4.6, stage 2): the owners' check (`IRON_OWNER_CHECK=1`). The
//! functions that make each owner's copies are the code templates
//! replaced, kept unchanged; the driver takes the owners in the order they
//! expand (D12), so past the budget the cut falls where theirs does. The
//! check compares what the templates want with what these want, problems
//! included (`expand::parity`), and the document's copies with these
//! (`expand`).

use super::*;

/// What every owner wants, in the order the owners expand: one owner at a
/// time, in id order, each taking what is left of the budget (the spec's
/// D12). This is `expand_owners`' driver; what each owner wants comes from
/// the old functions below.
pub(super) struct Whole {
    pub(super) nodes: Vec<Want>,
    pub(super) rels: Vec<RelWant>,
    pub(super) problems: Vec<Problem>,
}

pub(super) fn wanted(doc: Tree<'_>, counts: &Items) -> Whole {
    let mut out = Whole { nodes: Vec::new(), rels: Vec::new(), problems: Vec::new() };
    let mut room = NODE_BUDGET;
    for owner in top_owners(doc) {
        let w = wanted_for(doc, owner, counts, room);
        room = room.saturating_sub(w.nodes.len());
        out.nodes.extend(w.nodes);
        out.rels.extend(w.rels);
        out.problems.extend(w.problems);
    }
    sort_problems(&mut out.problems);
    out
}

/// What one top-level owner derives, from the old functions.
fn wanted_for(doc: Tree<'_>, owner: NodeId, counts: &Items, cap: usize) -> Wanted {
    let mut out = Wanted::new(cap);
    out.root = Some(owner);
    let Some(node) = doc.live(owner).filter(|_| !owner.is_expanded()) else { return out };
    let base_scopes: Vec<(NodeId, usize)> = definition_of(doc, owner).into_iter().map(|d| (d, 0)).collect();
    match node.ty.name() {
        "entity" => {
            if let Some(def) = linked_definition(doc, owner) {
                let mut scopes = base_scopes;
                scopes.push((def, 1));
                want_entity(doc, owner, def, &scopes, &mut out);
            }
        }
        "use" => {
            let sized = doc.facet(owner, FacetKind::Size).is_some();
            if let Some(def) = main_definition(doc, owner) {
                if let Some(notation) = effective_notation(doc, &[], owner, None, &base_scopes, None) {
                    let mut scopes = base_scopes;
                    scopes.push((def, 1));
                    want_use(doc, owner, sized, None, def, &notation, &scopes, &mut out);
                }
            } else if let Some(entity) = use_entity(doc, owner)
                && let Some(def) = linked_definition(doc, entity)
                && let Some(notation) = effective_notation(doc, &[], owner, None, &base_scopes, None)
            {
                let mut scopes = base_scopes;
                scopes.push((def, 1));
                want_use(doc, owner, sized, Some(entity), def, &notation, &scopes, &mut out);
            }
        }
        "group" => {
            if is_item_owner(doc, owner) && draws_repeat(doc, owner, &doc.facets_of(owner).cloned().collect::<Vec<_>>()) && !in_repeat_template(doc, owner) {
                want_items(doc, owner, &base_scopes, counts.get(&owner).map_or(&[][..], Vec::as_slice), &mut out);
            }
        }
        _ => {}
    }
    want_scopes(doc, counts, &mut out, Vec::new());
    out
}

/// Expand every scope nobody has expanded yet (reactivity.md §3.1).
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
                        let mut scopes = out.scopes.get(&id).cloned().unwrap_or_default();
                        scopes.push((def, id.path().len()));
                        want_entity(doc, id, def, &scopes, out);
                    }
                }
                "use" => {
                    let sized = w.facets.iter().any(|f| matches!(f, Facet::Size(_)));
                    let id_path = id.path();
                    let parent_path = &id_path[..id_path.len().saturating_sub(1)];
                    let local_u = id_path.last().copied();
                    let base_scopes = out.scopes.get(&id).cloned().unwrap_or_default();
                    let facets = w.facets.clone();
                    let entity = out.rels.iter().find(|r| r.rel.from == id && r.rel.rel == RelKind::Figures).map(|r| r.rel.to);
                    let def = entity.and_then(|e| match out.get(e) {
                        Some(copy) => linked_def_of(doc, &copy.facets),
                        None => linked_definition(doc, e),
                    });
                    if let (Some(entity), Some(def)) = (entity, def)
                        && let Some(notation) = effective_notation(doc, parent_path, id, local_u, &base_scopes, Some(&facets))
                    {
                        let mut scopes = base_scopes;
                        scopes.push((def, id_path.len()));
                        want_use(doc, id, sized, Some(entity), def, &notation, &scopes, out);
                    }
                }
                _ => {}
            }
        }
        let Some(r) = repeats.pop() else { break };
        if done.insert(r) && !in_template(doc, out, r) {
            let scopes = out.scopes.get(&r).cloned().unwrap_or_default();
            want_items(doc, r, &scopes, counts.get(&r).map_or(&[][..], Vec::as_slice), out);
        }
    }
}

/// The items of repeat (or conditional) `r`, by key.
fn want_items(doc: Tree<'_>, r: NodeId, scopes: &[(NodeId, usize)], keys: &[String], out: &mut Wanted) {
    if keys.is_empty() || !out.enter(r) {
        return;
    }
    let (template, rels) = template_of_repeat(doc, r, out);
    let path = |f: &str| LeafPath::new(FacetKind::Repeat, f).expect("declared leaf");
    let item_path = |f: &str| LeafPath::new(FacetKind::Item, f).expect("declared leaf");
    let any = LeafPath::new(FacetKind::Name, "text").expect("declared");
    let inside: BTreeSet<NodeId> = template.iter().map(|t| t.0).collect();
    let group = TypeId::lookup("group").expect("registered");
    let mut key = template.iter().filter(|t| t.1 == r).map(|t| t.3.clone()).max().unwrap_or_else(OrderKey::first);
    for k in keys {
        let k = k.as_str();
        let Some(scope) = NodeId::item_scope(r, k) else { continue };
        key = OrderKey::after(&key);
        if !out.push(Want { id: scope, ty: group, parent: r, order: key.clone(), facets: vec![FacetKind::Item.default_facet()], scoped: true }) {
            return;
        }
        let follow = |leaf: Leaf| -> Leaf {
            if leaf.node == r && leaf.path == path("item") {
                return Leaf { node: scope, path: item_path("value") };
            }
            if leaf.node == r && leaf.path == path("index") {
                return Leaf { node: scope, path: item_path("index") };
            }
            if leaf.node == r && owns_items(leaf.path.facet) && matches!(leaf.path.field, "presence" | "leaving") {
                return Leaf { node: scope, path: item_path(leaf.path.field) };
            }
            let p = leaf.node.path();
            let under = (1..=p.len()).rev().find_map(|i| NodeId::from_path(&p[..i]).filter(|n| inside.contains(n)).map(|n| (n, i)));
            match under.and_then(|(n, i)| NodeId::item(r, k, n).map(|c| (c, i))) {
                Some((copy, i)) => {
                    let mut q = copy.path();
                    q.extend_from_slice(&p[i..]);
                    NodeId::from_path(&q).map_or(leaf, |node| Leaf { node, path: leaf.path })
                }
                None => leaf,
            }
        };
        for (id, parent, ty, order, facets) in &template {
            let (Some(copy), Some(parent_copy)) = (NodeId::item(r, k, *id), if *parent == r { Some(scope) } else { NodeId::item(r, k, *parent) }) else { continue };
            let mut facets: Vec<Facet> = facets
                .iter()
                .map(|f| {
                    let mut f = f.clone();
                    for field in f.field_names() {
                        match f.get(field) {
                            Some(Slot::Bound(e)) => {
                                let e = e.map_refs(&follow);
                                f.set(field, Slot::Bound(e)).expect("field exists");
                            }
                            Some(Slot::Const(Value::Ref(to))) => {
                                let to = follow(Leaf { node: *to, path: any }).node;
                                f.set(field, Slot::Const(Value::Ref(to))).expect("field exists");
                            }
                            _ => {}
                        }
                    }
                    f
                })
                .collect();
            if let Some(&local) = id.path().last() {
                apply_overrides(doc, scope, scopes, local, *ty, &mut facets);
            }
            if !out.push(Want { id: copy, ty: *ty, parent: parent_copy, order: order.clone(), facets, scoped: false }) {
                return;
            }
            out.scopes.insert(copy, scopes.to_vec());
        }
        for rel in &rels {
            let Some(from) = NodeId::item(r, k, rel.from) else { continue };
            let to = follow(Leaf { node: rel.to, path: any }).node;
            out.rels.push(RelWant { rel: Relation { id: RelationId::expanded(scope, rel.id), from, to, rel: rel.rel, data: rel.data.clone() } });
        }
    }
}

/// What entity `entity` derives from `def`.
fn want_entity(doc: Tree<'_>, entity: NodeId, def: NodeId, scopes: &[(NodeId, usize)], out: &mut Wanted) {
    if !out.enter(entity) {
        return;
    }
    let inputs = definition::inputs_of(doc, def).unwrap_or_default();
    let follow = |leaf: Leaf| -> Leaf {
        match leaf.node.path().first().copied().filter(|&root| is_body(doc, def, root)).and_then(|_| relocate(entity, leaf.node)) {
            Some(n) => Leaf { node: n, path: leaf.path },
            None => leaf,
        }
    };
    for &child in doc.base_children(def).iter() {
        let Some(node) = doc.base_live(child).filter(|_| !child.is_session()) else { continue };
        let ty = match node.ty.name() {
            "port_spec" => TypeId::lookup("port").expect("registered"),
            "equation" | "entity" | "connection" | "channel" | "machine" | "let" | "trace" | "accumulate" | "query" => node.ty,
            _ => continue,
        };
        let Some(id) = NodeId::expanded(entity, child) else { continue };
        let mut facets = rewritten_base(doc, child, &follow);
        if let Some(input) = inputs.iter().find(|i| i.spec == child) {
            let arg = Leaf { node: entity, path: LeafPath::new(FacetKind::Args, input.name).expect("argument path") };
            for f in &mut facets {
                if let Facet::Port(p) = f {
                    p.signal = Slot::Bound(Expr::Ref(arg));
                }
            }
        }
        apply_overrides(doc, entity, scopes, child, ty, &mut facets);
        let inner_def = (node.ty.name() == "entity").then(|| linked_def_of(doc, &facets)).flatten();
        if !out.push(Want { id, ty, parent: entity, order: node.order.clone(), facets, scoped: true }) {
            return;
        }
        out.scopes.insert(id, scopes.to_vec());
        if matches!(node.ty.name(), "connection" | "port_spec") {
            for r in doc.relations_from(child) {
                let to = follow(Leaf { node: r.to, path: LeafPath::new(FacetKind::Name, "text").expect("declared") }).node;
                out.rels.push(RelWant { rel: Relation { id: RelationId::expanded(entity, r.id), from: id, to, rel: r.rel, data: r.data.clone() } });
            }
        }
        if let Some(inner) = inner_def {
            let mut inner_scopes = scopes.to_vec();
            inner_scopes.push((inner, id.path().len()));
            want_entity(doc, id, inner, &inner_scopes, out);
        }
    }
    for &machine in doc.base_children(def).iter().filter(|&&c| !c.is_session() && doc.base_live(c).is_some_and(|n| n.ty.name() == "machine")) {
        let mut stack: Vec<NodeId> = doc.base_children(machine).iter().rev().copied().filter(|c| !c.is_expanded() && !c.is_session()).collect();
        while let Some(local) = stack.pop() {
            let Some(n) = doc.base_live(local) else { continue };
            let (Some(copy), Some(parent)) = (NodeId::expanded(entity, local), n.parent.and_then(|p| NodeId::expanded(entity, p))) else { continue };
            let mut facets = rewritten_base(doc, local, &follow);
            apply_overrides(doc, entity, scopes, local, n.ty, &mut facets);
            if !out.push(Want { id: copy, ty: n.ty, parent, order: n.order.clone(), facets, scoped: true }) {
                return;
            }
            out.scopes.insert(copy, scopes.to_vec());
            stack.extend(doc.base_children(local).iter().rev().copied().filter(|c| !c.is_expanded() && !c.is_session()));
        }
    }
}

/// What `use` `u`, figuring `entity` (an instance of `def`), derives.
#[allow(clippy::too_many_arguments)]
fn want_use(doc: Tree<'_>, u: NodeId, sized: bool, entity: Option<NodeId>, def: NodeId, notation: &str, scopes: &[(NodeId, usize)], out: &mut Wanted) {
    if let Some(r) = entity.filter(|e| e.is_expanded()).and_then(|e| e.path().first().copied())
        && out.root.is_some_and(|o| o != r)
    {
        out.reads.insert(r);
    }
    if !out.enter(u) {
        return;
    }
    let Some(template) = definition::template_of(doc, def, notation) else { return };
    let inside: BTreeSet<NodeId> = doc.base_descendants(template).into_iter().filter(|d| !d.is_expanded()).collect();
    let u_path = u.path();
    let follow = |leaf: Leaf| -> Leaf {
        if leaf.node == template {
            return if sized && leaf.path.facet == FacetKind::Size { Leaf { node: u, path: leaf.path } } else { leaf };
        }
        let root = leaf.node.path()[0];
        let owner = if inside.contains(&root) {
            u
        } else if is_body(doc, def, root) {
            match entity {
                Some(e) => e,
                None => return leaf,
            }
        } else {
            return leaf;
        };
        match relocate(owner, leaf.node) {
            Some(n) => Leaf { node: n, path: leaf.path },
            None => leaf,
        }
    };
    let copied = |n: NodeId| !n.is_expanded() && !n.is_session() && doc.base_live(n).is_some();
    let mut stack: Vec<NodeId> = doc.base_children(template).iter().rev().copied().filter(|&c| copied(c)).collect();
    while let Some(local) = stack.pop() {
        let Some(node) = doc.base_live(local) else { continue };
        let parent = match node.parent {
            Some(p) if p == template => Some(u),
            Some(p) => NodeId::expanded(u, p),
            None => None,
        };
        let (Some(id), Some(parent)) = (NodeId::expanded(u, local), parent) else { continue };
        let mut facets = rewritten_base(doc, local, &follow);
        apply_overrides(doc, u, scopes, local, node.ty, &mut facets);
        let inner = (node.ty.name() == "use").then(|| doc.relations_from(local).find(|r| r.rel == RelKind::Figures)).flatten();
        let own = inner.is_some_and(|r| !inside.contains(&r.to.path()[0]));
        if !out.push(Want { id, ty: node.ty, parent, order: node.order.clone(), facets: facets.clone(), scoped: !matches!(node.ty.name(), "entity" | "use") || own }) {
            return;
        }
        out.scopes.insert(id, scopes.to_vec());
        if node.ty.name() == "use" {
            if let Some(r) = doc.relations_from(local).find(|r| r.rel == RelKind::Figures) {
                let inner_entity = follow(Leaf { node: r.to, path: LeafPath::new(FacetKind::Name, "text").expect("declared") }).node;
                out.rels.push(RelWant { rel: Relation { id: RelationId::expanded(u, r.id), from: id, to: inner_entity, rel: RelKind::Figures, data: r.data.clone() } });
                let inner_def = doc.node(r.to).and_then(|_| doc.get::<Instance>(r.to)).and_then(|i| {
                    (i.mode.constant().and_then(|v| v.as_str()) == Some("linked")).then(|| i.def.constant().and_then(|v| v.as_ref())).flatten()
                });
                let inner_notation = effective_notation(doc, &u_path, id, Some(local), scopes, Some(&facets));
                let mut inner_scopes = scopes.to_vec();
                if let Some(d) = inner_def {
                    inner_scopes.push((d, id.path().len()));
                }
                if let (Some(n), Some(d)) = (inner_notation, inner_def)
                    && own
                {
                    want_use(doc, id, facets.iter().any(|f| matches!(f, Facet::Size(_))), Some(inner_entity), d, &n, &inner_scopes, out);
                }
            }
            continue;
        }
        stack.extend(doc.base_children(local).iter().rev().copied().filter(|&c| copied(c)));
    }
}

/// Apply `owner`'s patch overrides of `local` to the facets its copy will have.
fn apply_overrides(doc: Tree<'_>, owner: NodeId, scopes: &[(NodeId, usize)], local: NodeId, ty: TypeId, facets: &mut Vec<Facet>) {
    let ovs = patch_overrides_for(doc, owner, scopes, &[local]);
    if ovs.is_empty() {
        return;
    }
    let mut suppressed: std::collections::HashSet<FacetKind> = std::collections::HashSet::new();
    for (_, key, val) in &ovs {
        if let (PatchKey::FacetPresent(fk), PatchValue::Bool { v }) = (key, val) {
            if *v {
                if !facets.iter().any(|f| f.kind() == *fk) && ty.spec().allows(*fk) {
                    facets.push(fk.default_facet());
                }
            } else {
                suppressed.insert(*fk);
                facets.retain(|f| f.kind() != *fk);
            }
        }
    }
    for (_, key, val) in ovs {
        match (key, val) {
            (PatchKey::Leaf(leaf), PatchValue::Slot { slot }) => {
                // P21: field writes never synthesize missing optional facets implicitly.
                if suppressed.contains(&leaf.facet) {
                    continue;
                }
                if let Some(f) = facets.iter_mut().find(|f| f.kind() == leaf.facet) {
                    let _ = f.set(leaf.field, slot);
                }
            }
            (PatchKey::Notation, PatchValue::Notation { notation }) => {
                if let Some(Facet::Use(u)) = facets.iter_mut().find(|f| matches!(f, Facet::Use(_))) {
                    u.notation = Slot::Const(Value::Str(notation));
                }
            }
            _ => {}
        }
    }
    facets.sort_by_key(Facet::kind);
}

/// `node`'s unpatched base facets with every reference passed through `follow`:
/// those in bindings, and node references held as constants (an anchor's `port`).
fn rewritten_base(doc: Tree<'_>, node: NodeId, follow: &dyn Fn(Leaf) -> Leaf) -> Vec<Facet> {
    doc.base_facets_of(node)
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

/// Bring the document's expanded nodes to what its linked instances derive,
/// all at once. Returns the ops applied and the problems met (a template
/// node the registry no longer admits, say), which leave that node out
/// rather than failing the transaction. Since phase 2b's swap only the
/// differential harness's owner check runs it (`IRON_OWNER_CHECK=1`): the
/// owners' copies must leave it nothing to do.
pub(crate) fn expand(docs: &mut Docs, stamp: Version, counts: &Items) -> (Vec<Op>, Vec<Problem>) {
    let Whole { nodes: want, rels: want_rels, mut problems } = wanted(Tree::of(docs), counts);
    let have: Vec<NodeId> = Tree::of(docs).expanded_ids().filter(|id| Tree::of(docs).is_live(*id)).collect();
    let have_rels: Vec<Relation> = Tree::of(docs).relations().filter(|r| r.id.is_expanded()).cloned().collect();
    let mut ignored = HashMap::new();
    let ops = apply_wanted(docs, stamp, want, want_rels, have, have_rels, &mut problems, (&BTreeSet::new(), &BTreeSet::new()), &HashMap::new(), &mut ignored);
    sort_problems(&mut problems);
    (ops, problems)
}


/// One node of a repeat's template: id, parent, type, order, facets.
type TemplateNode = (NodeId, NodeId, TypeId, OrderKey, Vec<Facet>);

/// Repeat `r`'s template: its children that are not its items, and their
/// subtrees, parents first, with the relations from them. A derived
/// repeat's come from what is wanted, an authored one's from the document.
fn template_of_repeat(doc: Tree<'_>, r: NodeId, out: &Wanted) -> (Vec<TemplateNode>, Vec<Relation>) {
    let mut nodes = Vec::new();
    let mut rels = Vec::new();
    if r.is_expanded() && out.get(r).is_none() {
        let mut stack: Vec<NodeId> = doc.children(r).iter().rev().copied().filter(|c| c.item_of(r).is_none()).collect();
        while let Some(n) = stack.pop() {
            let Some(node) = doc.live(n) else { continue };
            nodes.push((n, node.parent.unwrap_or(r), node.ty, node.order.clone(), doc.facets_of(n).cloned().collect()));
            rels.extend(doc.relations_from(n).cloned());
            stack.extend(doc.children(n).iter().rev().copied().filter(|c| c.expanded_parts().map(|(o, _)| o) != Some(n)));
        }
    } else if r.is_expanded() {
        let mut parents = BTreeSet::from([r]);
        for w in &out.nodes {
            let derived_by_parent = w.id.expanded_parts().is_some_and(|(o, _)| o == w.parent);
            if parents.contains(&w.parent) && w.id.item_of(r).is_none() && !derived_by_parent {
                parents.insert(w.id);
                nodes.push((w.id, w.parent, w.ty, w.order.clone(), w.facets.clone()));
            }
        }
        rels.extend(out.rels.iter().filter(|x| parents.contains(&x.rel.from) && x.rel.from != r).map(|x| x.rel.clone()));
    } else {
        let copied = |c: &NodeId| !c.is_expanded() && !c.is_session();
        let mut stack: Vec<NodeId> = doc.children(r).iter().rev().copied().filter(copied).collect();
        while let Some(n) = stack.pop() {
            let Some(node) = doc.live(n) else { continue };
            nodes.push((n, node.parent.unwrap_or(r), node.ty, node.order.clone(), doc.facets_of(n).cloned().collect()));
            rels.extend(doc.relations_from(n).filter(|x| !x.id.is_expanded()).cloned());
            stack.extend(doc.children(n).iter().rev().copied().filter(copied));
        }
    }
    (nodes, rels)
}

