//! Compositions, the model tree, and the `figures` relation between them
//! (entity-composition-model.md §5, §7).
//!
//! ```text
//! model                      composition "scene" (2d)      composition "assembly" (3d)
//!   entity "thing"  ◀──figures── rect "box"                    mesh "torus"
//!                   ◀──figures──────────────────────────────── ┘
//! ```
//!
//! A composition is a root whose subtree one panel shows. A figure is any
//! node with a `figures` relation to an entity; it is a role, not a type.
//! Everything here is a read over the document: which composition a node
//! is drawn in, which entity a node pictures, and every picture of an
//! entity. Nothing is stored that these functions can compute.

use crate::Tree;
use crate::facet::{Composition, LeafStruct};
use crate::document::{RelKind, Relation};
use crate::id::{NodeId, RelationId};
use crate::op::Op;
use crate::registry::TypeId;
use serde::Serialize;
use std::collections::BTreeMap;

/// Whether a composition is drawn as 2D vector content or as a 3D scene.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Dims {
    Two,
    Three,
}

/// The composition `id` is drawn in: itself or its nearest composition
/// ancestor. `None` for nodes outside every composition (the model tree,
/// the library, the timeline, and loose roots).
pub fn composition_of(doc: Tree<'_>, id: NodeId) -> Option<NodeId> {
    let composition = TypeId::lookup("composition").expect("registered");
    let mut cur = Some(id);
    while let Some(n) = cur.and_then(|c| doc.live(c)) {
        if n.ty == composition {
            return Some(n.id);
        }
        cur = n.parent;
    }
    None
}

/// Every live composition, in root order.
pub fn compositions(doc: Tree<'_>) -> Vec<NodeId> {
    let composition = TypeId::lookup("composition").expect("registered");
    doc.roots().iter().copied().filter(|&r| doc.live(r).is_some_and(|n| n.ty == composition)).collect()
}

pub fn dims(doc: Tree<'_>, composition: NodeId) -> Option<Dims> {
    let c = doc.get::<Composition>(composition)?;
    Some(match c.dims.constant().and_then(|v| v.as_str()) {
        Some("3d") => Dims::Three,
        _ => Dims::Two,
    })
}

/// When a composition's views render (free-running.md §2).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RunMode {
    /// Free running when it is live or shows something that is.
    Auto,
    Always,
    /// Never free running, and nothing it shows bubbles out of it.
    OnDemand,
}

impl RunMode {
    pub fn parse(s: &str) -> RunMode {
        match s {
            "always" => RunMode::Always,
            "on_demand" => RunMode::OnDemand,
            _ => RunMode::Auto,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            RunMode::Auto => "auto",
            RunMode::Always => "always",
            RunMode::OnDemand => "on_demand",
        }
    }
}

pub fn run_mode(doc: Tree<'_>, composition: NodeId) -> RunMode {
    doc.get::<Composition>(composition).and_then(|c| c.run.constant()).and_then(|v| v.as_str()).map_or(RunMode::Auto, RunMode::parse)
}

/// What a view of `roots` reaches through embeds (free-running.md §2).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reach {
    /// The view renders every frame.
    pub live: bool,
    /// Sources reached on an open path: the producers that should run.
    pub running: std::collections::BTreeSet<NodeId>,
}

/// From a view's root compositions, walk embeds depth first. A composition
/// reached with no `on_demand` composition on its path (itself included) is
/// open; an open one that is a source, or `always`, makes the view live.
/// Embeds cannot loop (`validate_embed`), and each composition is walked
/// once, so this terminates in time linear in the embed graph.
pub fn reach(doc: Tree<'_>, roots: &[NodeId], sources: &std::collections::BTreeSet<NodeId>) -> Reach {
    let mut out = Reach::default();
    let mut seen = std::collections::BTreeSet::new();
    let mut stack: Vec<NodeId> = roots.to_vec();
    while let Some(c) = stack.pop() {
        if !seen.insert(c) {
            continue;
        }
        let mode = run_mode(doc, c);
        if mode == RunMode::OnDemand {
            continue;
        }
        if sources.contains(&c) {
            out.live = true;
            out.running.insert(c);
        }
        out.live |= mode == RunMode::Always;
        stack.extend(embeds_in_unsorted(doc, c).into_iter().filter_map(|e| embedded(doc, e)));
    }
    out
}

/// Every composition a view of `roots` draws: the roots and what their
/// embeds show, at any depth, `on_demand` or not (a frame drawn on demand
/// still draws current values; reactive-engine.md §8.2).
pub fn shown(doc: Tree<'_>, roots: &[NodeId]) -> std::collections::BTreeSet<NodeId> {
    let mut seen = std::collections::BTreeSet::new();
    let mut stack: Vec<NodeId> = roots.to_vec();
    while let Some(c) = stack.pop() {
        if seen.insert(c) {
            stack.extend(embeds_in_unsorted(doc, c).into_iter().filter_map(|e| embedded(doc, e)));
        }
    }
    seen
}

pub fn notation(doc: Tree<'_>, composition: NodeId) -> Option<String> {
    doc.get::<Composition>(composition)?.notation.constant()?.as_str().map(str::to_owned)
}

/// The entity `node` pictures directly: the target of its `figures`
/// relation, if that entity is live.
pub fn figured_by(doc: Tree<'_>, node: NodeId) -> Option<NodeId> {
    doc.relations_from(node).find(|r| r.rel == RelKind::Figures && doc.is_live(r.to)).map(|r| r.to)
}

/// The figure `node` belongs to and the entity it pictures: `node` itself
/// or its nearest ancestor with a `figures` relation, stopping at the
/// composition root. This is what hover and selection lift through, so
/// grabbing a part of a grouped figure selects the whole thing.
pub fn figure_of(doc: Tree<'_>, node: NodeId) -> Option<(NodeId, NodeId)> {
    let mut cur = Some(node);
    while let Some(n) = cur.and_then(|c| doc.live(c)) {
        if let Some(entity) = figured_by(doc, n.id) {
            return Some((n.id, entity));
        }
        cur = n.parent;
    }
    None
}

/// Every live figure of `entity`, in relation order.
pub fn figures_of(doc: Tree<'_>, entity: NodeId) -> Vec<NodeId> {
    doc.relations_to(entity).filter(|r| r.rel == RelKind::Figures && doc.is_live(r.from)).map(|r| r.from).collect()
}

/// The `model` root, if the document has one.
pub fn model_root(doc: Tree<'_>) -> Option<NodeId> {
    let model = TypeId::lookup("model").expect("registered");
    doc.roots().iter().copied().find(|&r| doc.live(r).is_some_and(|n| n.ty == model))
}

// ---------------------------------------------------------------------------
// Linking (ECM §7.2): two pictures become pictures of one entity.

/// One side of a merge question: enough for a person or the model to pick
/// which entity survives.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EntitySummary {
    pub entity: NodeId,
    pub name: Option<String>,
    /// The definition's name, or its id when it has none.
    pub def: Option<String>,
    /// Argument name and value, in the file's text form.
    pub args: Vec<(String, String)>,
    pub figures: Vec<NodeId>,
    /// Port names. Ports arrive in ECM phase 3; empty until then, but part
    /// of the question so collisions are shown where they will matter.
    pub ports: Vec<String>,
}

/// Both pictures already have entities: which one survives is asked, never
/// guessed (ECM I5, decided 2026-10-11).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LinkQuestion {
    pub a: EntitySummary,
    pub b: EntitySummary,
    /// Port names both entities have.
    pub port_collisions: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum LinkError {
    /// The request is malformed: the same node twice, a node that is not a
    /// live picture in a composition, or `keep` naming neither entity.
    Invalid(String),
    /// Both already picture the same entity; nothing to do.
    AlreadyLinked(NodeId),
    /// Both picture different entities and `keep` was not given.
    Question(LinkQuestion),
}

impl std::fmt::Display for LinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LinkError::Invalid(m) => f.write_str(m),
            LinkError::AlreadyLinked(e) => write!(f, "both already figure {e}"),
            LinkError::Question(q) => write!(
                f,
                "both already figure an entity ({} and {}); say which to keep",
                q.a.entity, q.b.entity
            ),
        }
    }
}

fn name_of(doc: Tree<'_>, id: NodeId) -> Option<String> {
    let n = doc.get::<crate::facet::Name>(id)?.text.constant()?.as_str()?.to_owned();
    (!n.is_empty()).then_some(n)
}

pub fn summarize(doc: Tree<'_>, entity: NodeId) -> EntitySummary {
    use crate::facet::{Instance, Args};
    let def = doc.get::<Instance>(entity).and_then(|i| i.def.constant()).and_then(|v| match v {
        crate::value::Value::Ref(d) => Some(name_of(doc, *d).unwrap_or_else(|| d.to_string())),
        _ => None,
    });
    let args = doc
        .get::<Args>(entity)
        .map(|p| {
            p.field_names()
                .into_iter()
                .filter_map(|f| {
                    p.get(f).map(|s| {
                        let text = match (s.constant(), s.expr()) {
                            (Some(v), _) => v.to_string(),
                            (_, Some(e)) => format!("{{= {e}}}"),
                            _ => String::new(),
                        };
                        (f.to_owned(), text)
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    EntitySummary { entity, name: name_of(doc, entity), def, args, figures: figures_of(doc, entity), ports: Vec::new() }
}

/// The ops that make `a` and `b` pictures of one entity, as one
/// transaction (so undo is one step):
///
/// - neither pictures anything: a new entity of the generic `thing`
///   definition, named after the first named picture, under `model` (the
///   `model` root, a library and the `thing` definition are created too if
///   the document lacks them);
/// - one pictures an entity: the other joins it;
/// - both picture different entities: with `keep` naming one of them, the
///   other's figures move to it and it is deleted; without, the question.
pub fn link_ops(doc: Tree<'_>, a: NodeId, b: NodeId, keep: Option<NodeId>) -> Result<Vec<Op>, LinkError> {
    if a == b {
        return Err(LinkError::Invalid(format!("{a} cannot be linked to itself")));
    }
    for n in [a, b] {
        if !doc.is_live(n) {
            return Err(LinkError::Invalid(format!("no live node {n}")));
        }
        if composition_of(doc, n).is_none_or(|c| c == n) {
            return Err(LinkError::Invalid(format!("{n} is not a picture inside a composition")));
        }
    }
    let ea = figure_of(doc, a).map(|(_, e)| e);
    let eb = figure_of(doc, b).map(|(_, e)| e);
    let figures = |from: NodeId, to: NodeId, id: RelationId| Op::Link {
        rel: Relation { id, from, to, rel: RelKind::Figures, data: BTreeMap::new() },
    };
    match (ea, eb) {
        (Some(x), Some(y)) if x == y => Err(LinkError::AlreadyLinked(x)),
        (Some(x), None) => Ok(vec![figures(b, x, doc.next_relation_id())]),
        (None, Some(y)) => Ok(vec![figures(a, y, doc.next_relation_id())]),
        (Some(x), Some(y)) => {
            let (keep, drop) = match keep {
                Some(k) if k == x => (x, y),
                Some(k) if k == y => (y, x),
                Some(k) => return Err(LinkError::Invalid(format!("keep={k} names neither {x} nor {y}"))),
                None => {
                    let (sa, sb) = (summarize(doc, x), summarize(doc, y));
                    let port_collisions = sa.ports.iter().filter(|p| sb.ports.contains(p)).cloned().collect();
                    return Err(LinkError::Question(LinkQuestion { a: sa, b: sb, port_collisions }));
                }
            };
            let moving: Vec<&Relation> = doc.relations_to(drop).filter(|r| r.rel == RelKind::Figures).collect();
            let ids = doc.next_relation_ids(moving.len());
            let mut ops: Vec<Op> = moving.iter().map(|r| Op::Unlink { id: r.id }).collect();
            ops.extend(moving.iter().zip(ids).map(|(r, id)| figures(r.from, keep, id)));
            ops.push(Op::Delete { id: drop });
            Ok(ops)
        }
        (None, None) => Ok(new_entity_ops(doc, a, b)),
    }
}

/// A `thing` entity for two pictures, creating whatever structure is
/// missing. Ids are minted in creation order (ascending, as apply requires).
fn new_entity_ops(doc: Tree<'_>, a: NodeId, b: NodeId) -> Vec<Op> {
    use crate::facet::{Instance, Name, Args};
    use crate::value::{Slot, Value};
    let ty = |s: &str| TypeId::lookup(s).expect("registered");
    let named = |s: &str| {
        let mut n = Name::default();
        n.text = Slot::Const(Value::Str(s.to_owned()));
        n.wrap()
    };
    let last_root = || doc.last_root().and_then(|r| doc.live(r)).map(|n| n.order.clone());
    let after = |k: Option<crate::order::OrderKey>| k.map_or_else(crate::order::OrderKey::first, |k| crate::order::OrderKey::after(&k));

    let model = model_root(doc);
    let library = doc.nodes_of_type(ty("library")).find(|&l| doc.is_live(l));
    let thing = library.and_then(|l| doc.children(l).iter().copied().find(|&d| doc.live(d).is_some_and(|n| n.ty == ty("definition")) && name_of(doc, d).as_deref() == Some("thing")));
    let wanted = model.is_none() as usize + library.is_none() as usize + thing.is_none() as usize + 1;
    let mut ids = doc.next_ids(wanted).into_iter();
    let mut ops = Vec::new();
    let mut root_key = last_root();

    let model = model.unwrap_or_else(|| {
        let id = ids.next().expect("reserved");
        let order = after(root_key.take());
        root_key = Some(order.clone());
        ops.push(Op::Create { id, ty: ty("model"), parent: None, order, facets: vec![named("model")] });
        id
    });
    let library = library.unwrap_or_else(|| {
        let id = ids.next().expect("reserved");
        let order = after(root_key.take());
        ops.push(Op::Create { id, ty: ty("library"), parent: None, order, facets: vec![named("library")] });
        id
    });
    let thing = thing.unwrap_or_else(|| {
        let id = ids.next().expect("reserved");
        let last = doc.children(library).last().and_then(|&c| doc.live(c)).map(|n| n.order.clone());
        ops.push(Op::Create { id, ty: ty("definition"), parent: Some(library), order: after(last), facets: vec![named("thing")] });
        id
    });
    let entity = ids.next().expect("reserved");
    let mut instance = Instance::default();
    instance.def = Slot::Const(Value::Ref(thing));
    let name = name_of(doc, a).or_else(|| name_of(doc, b)).unwrap_or_else(|| "thing".to_owned());
    let last = doc.children(model).last().and_then(|&c| doc.live(c)).map(|n| n.order.clone());
    ops.push(Op::Create {
        id: entity,
        ty: ty("entity"),
        parent: Some(model),
        order: after(last),
        facets: vec![instance.wrap(), Args::default().wrap(), named(&name)],
    });
    let rels = doc.next_relation_ids(2);
    for (from, id) in [a, b].into_iter().zip(rels) {
        ops.push(Op::Link { rel: Relation { id, from, to: entity, rel: RelKind::Figures, data: BTreeMap::new() } });
    }
    ops
}

// ---------------------------------------------------------------------------
// Embeds (ECM §7.4): one composition shown inside another.

/// The declared target `NodeId` an embed's `embed.source` names, even if that
/// target composition is currently tombstoned/deleted (so `EmbedTopology` keeps
/// reverse subscriptions intact across delete + undo).
fn declared_embed_target(doc: Tree<'_>, embed: NodeId) -> Option<NodeId> {
    match doc.get::<crate::facet::Embed>(embed)?.source.constant()? {
        crate::value::Value::Ref(c) => Some(*c),
        _ => None,
    }
}

/// The raw target composition an embed names, without cycle filtering.
pub(crate) fn raw_embedded(doc: Tree<'_>, embed: NodeId) -> Option<NodeId> {
    let c = declared_embed_target(doc, embed)?;
    (composition_of(doc, c) == Some(c)).then_some(c)
}

/// Canonical post-commit embed topology across BOTH 2D and 3D compositions:
/// tracks every live composition's `dims`, declared embed targets (even when
/// temporarily deleted, so delete+edit+undo still wakes callers), reverse raw
/// edges, last cycle-filtered resolved targets, and cached virtual embed
/// validation problems so `Store::problems()` never rescans every embed.
#[derive(Clone, Debug, Default)]
pub struct EmbedTopology {
    /// `home_comp -> { embed_id -> (declared_target, resolved_target) }`
    edges: BTreeMap<NodeId, BTreeMap<NodeId, (Option<NodeId>, Option<NodeId>)>>,
    /// Deduplicated unique declared target compositions per `home_comp`, so
    /// N repeated embeds to the same target do not multiply graph traversals.
    comp_targets: BTreeMap<NodeId, std::collections::BTreeSet<NodeId>>,
    /// Reverse declared edges: `target_comp -> { home_comp }`
    reverse_raw: BTreeMap<NodeId, std::collections::BTreeSet<NodeId>>,
    /// Last known `dims` per live composition (so 2D↔3D flips also notify incoming embedders).
    dims: BTreeMap<NodeId, Dims>,
    /// Live embeds currently blocked by a repeat-induced embed cycle (empty in
    /// every acyclic document; mirrored onto `Docs::cyclic_embeds` for O(1) `embedded()`).
    cyclic_embeds: std::collections::BTreeSet<NodeId>,
    /// Cached `validate_embed` problems on live virtual embeds, per home composition.
    problems_by_comp: BTreeMap<NodeId, Vec<crate::expand::Problem>>,
    /// Flattened sorted virtual embed problems returned by `Store::problems()`.
    virtual_problems: Vec<crate::expand::Problem>,
}

impl EmbedTopology {
    pub fn virtual_problems(&self) -> &[crate::expand::Problem] {
        &self.virtual_problems
    }

    pub fn cyclic_embeds(&self) -> &std::collections::BTreeSet<NodeId> {
        &self.cyclic_embeds
    }

    fn edge_reaches(&self, start: NodeId, goal: NodeId, cache: &mut std::collections::HashMap<(NodeId, NodeId), bool>) -> bool {
        if let Some(&ans) = cache.get(&(start, goal)) {
            return ans;
        }
        let mut stack = vec![start];
        let mut seen = std::collections::BTreeSet::new();
        let mut hit = false;
        while let Some(c) = stack.pop() {
            if c == goal {
                hit = true;
                break;
            }
            if seen.insert(c)
                && let Some(targets) = self.comp_targets.get(&c)
            {
                for &t in targets {
                    if self.dims.contains_key(&t) {
                        stack.push(t);
                    }
                }
            }
        }
        cache.insert((start, goal), hit);
        hit
    }

    /// Validate virtual embeds inside `comps` using the already-resolved
    /// cycle status in `self.edges` (no per-embed `reaches_composition` Tree DFS).
    fn refresh_virtual_problems(&mut self, doc: Tree<'_>, comps: &std::collections::BTreeSet<NodeId>) {
        let mut any_changed = false;
        for &c in comps {
            let mut ps = Vec::new();
            if let Some(map) = self.edges.get(&c) {
                for (&e, &(decl, res)) in map {
                    if !e.is_expanded() || !doc.is_live(e) {
                        continue;
                    }
                    let err = match decl {
                        None => Some(format!("embed {e}: embed.source must name a composition")),
                        Some(of) if !self.dims.contains_key(&of) => Some(format!("embed {e}: {of} is not a live composition")),
                        Some(of) if res.is_none() => Some(format!("embed {e}: {of} already shows {c}, directly or through its embeds; embedding it in {c} would loop")),
                        Some(of) => {
                            if let Some(cam) = doc.get::<crate::facet::EmbedCamera>(e).and_then(|x| x.camera.constant()) {
                                let ok = matches!(cam, crate::value::Value::Ref(cam_id) if doc.live(*cam_id).is_some_and(|n| n.ty.name() == "camera") && composition_of(doc, *cam_id) == Some(of));
                                (!ok).then(|| format!("embed {e}: embed_camera.camera must name a camera inside {of}"))
                            } else {
                                None
                            }
                        }
                    };
                    if let Some(message) = err {
                        ps.push(crate::expand::Problem { node: e, kind: "expansion", message });
                    }
                }
            }
            let prev = if ps.is_empty() {
                self.problems_by_comp.remove(&c)
            } else {
                self.problems_by_comp.insert(c, ps.clone())
            };
            if prev.unwrap_or_default() != ps {
                any_changed = true;
            }
        }
        if any_changed {
            self.virtual_problems = self.problems_by_comp.values().flatten().cloned().collect();
            crate::expand::sort_problems(&mut self.virtual_problems);
        }
    }

    pub fn rebuild(&mut self, doc: Tree<'_>) {
        self.edges.clear();
        self.comp_targets.clear();
        self.reverse_raw.clear();
        self.dims.clear();
        self.cyclic_embeds.clear();
        self.problems_by_comp.clear();
        self.virtual_problems.clear();
        let all_comps: std::collections::BTreeSet<NodeId> = compositions(doc).into_iter().collect();
        for &c in &all_comps {
            if let Some(d) = dims(doc, c) {
                self.dims.insert(c, d);
            }
            let mut map = BTreeMap::new();
            let mut targets = std::collections::BTreeSet::new();
            for e in embeds_in_unsorted(doc, c) {
                let declared = declared_embed_target(doc, e);
                if let Some(t) = declared {
                    targets.insert(t);
                    self.reverse_raw.entry(t).or_default().insert(c);
                }
                map.insert(e, (declared, None));
            }
            if !map.is_empty() {
                self.edges.insert(c, map);
            }
            if !targets.is_empty() {
                self.comp_targets.insert(c, targets);
            }
        }
        let mut cache = std::collections::HashMap::new();
        let comp_keys: Vec<NodeId> = self.edges.keys().copied().collect();
        for c in comp_keys {
            let updates: Vec<(NodeId, Option<NodeId>, bool)> = self.edges[&c]
                .iter()
                .map(|(&e, &(declared, _))| {
                    let cyclic = declared.is_some_and(|t| self.dims.contains_key(&t) && self.edge_reaches(t, c, &mut cache));
                    let res = declared.filter(|&t| self.dims.contains_key(&t) && !cyclic);
                    (e, res, cyclic)
                })
                .collect();
            if let Some(map) = self.edges.get_mut(&c) {
                for (e, res, cyclic) in updates {
                    if let Some(entry) = map.get_mut(&e) {
                        entry.1 = res;
                    }
                    if cyclic {
                        self.cyclic_embeds.insert(e);
                    }
                }
            }
        }
        self.refresh_virtual_problems(doc, &all_comps);
    }

    /// Update tracked topology for `dirty_comps` (compositions whose embeds,
    /// `embed.source`, `embed_camera`, `camera`, `Repeat`/`When`, or
    /// `composition.dims` changed, or that were added/deleted), returning every
    /// composition whose resolved embed targets, target `dims`, or target
    /// cameras changed so `GeometryRevisions` marks them shaped.
    pub fn sync_from_comps(
        &mut self,
        doc: Tree<'_>,
        dirty_comps: &std::collections::BTreeSet<NodeId>,
        camera_comps: &std::collections::BTreeSet<NodeId>,
    ) -> Vec<NodeId> {
        if dirty_comps.is_empty() && camera_comps.is_empty() {
            return Vec::new();
        }
        let mut seeds: std::collections::BTreeSet<NodeId> = std::collections::BTreeSet::new();
        let mut dims_changed: std::collections::BTreeSet<NodeId> = std::collections::BTreeSet::new();
        let mut reverse_seeds: std::collections::BTreeSet<NodeId> = std::collections::BTreeSet::new();

        for &c in dirty_comps {
            let live_comp = doc.is_live(c) && composition_of(doc, c) == Some(c);
            let old_dims = self.dims.get(&c).copied();
            let new_dims = live_comp.then(|| dims(doc, c)).flatten();
            if old_dims != new_dims {
                match new_dims {
                    Some(d) => {
                        self.dims.insert(c, d);
                    }
                    None => {
                        self.dims.remove(&c);
                    }
                }
                dims_changed.insert(c);
                seeds.insert(c);
                reverse_seeds.insert(c);
            }
            let old_edges = self.edges.remove(&c).unwrap_or_default();
            self.comp_targets.remove(&c);
            let old_declared: BTreeMap<NodeId, Option<NodeId>> = old_edges.iter().map(|(&e, &(decl, _))| (e, decl)).collect();
            for (_, &(decl, _)) in &old_edges {
                if let Some(t) = decl
                    && let Some(set) = self.reverse_raw.get_mut(&t)
                {
                    set.remove(&c);
                    if set.is_empty() {
                        self.reverse_raw.remove(&t);
                    }
                }
            }
            let mut new_edges = BTreeMap::new();
            let mut new_declared = BTreeMap::new();
            let mut new_targets = std::collections::BTreeSet::new();
            if live_comp {
                for e in embeds_in_unsorted(doc, c) {
                    let decl = declared_embed_target(doc, e);
                    let prev_resolved = old_edges.get(&e).and_then(|&(_, res)| res);
                    if let Some(t) = decl {
                        new_targets.insert(t);
                        self.reverse_raw.entry(t).or_default().insert(c);
                    }
                    new_declared.insert(e, decl);
                    new_edges.insert(e, (decl, prev_resolved));
                }
            }
            // Only remove `cyclic_embeds` entries for embeds that actually went away;
            // surviving unchanged edges preserve their cached cycle status if `seeds` stays empty.
            for &e in old_edges.keys() {
                if !new_edges.contains_key(&e) {
                    self.cyclic_embeds.remove(&e);
                }
            }
            if old_declared != new_declared {
                seeds.insert(c);
                reverse_seeds.insert(c);
                for t in old_declared.values().flatten().chain(new_declared.values().flatten()) {
                    reverse_seeds.insert(*t);
                }
            }
            if !new_edges.is_empty() {
                self.edges.insert(c, new_edges);
            }
            if !new_targets.is_empty() {
                self.comp_targets.insert(c, new_targets);
            }
        }

        // Direct callers of any composition whose cameras changed need both
        // virtual-problem refresh and Scene rebuild (e.g. deleted camera aspect).
        let mut camera_callers: std::collections::BTreeSet<NodeId> = std::collections::BTreeSet::new();
        for &c in camera_comps {
            if let Some(callers) = self.reverse_raw.get(&c) {
                camera_callers.extend(callers.iter().copied());
            }
        }

        if seeds.is_empty() {
            let mut problem_comps = dirty_comps.clone();
            problem_comps.extend(camera_callers.iter().copied());
            for &c in dirty_comps {
                if let Some(callers) = self.reverse_raw.get(&c) {
                    problem_comps.extend(callers.iter().copied());
                }
            }
            self.refresh_virtual_problems(doc, &problem_comps);
            return camera_callers.into_iter().collect();
        }

        // BFS backward along `reverse_raw` to reach every composition that can
        // transitively reach any changed composition (covering indirect cycles
        // across both 2D and 3D compositions and deleted-then-restored targets).
        let mut affected: std::collections::BTreeSet<NodeId> = seeds.clone();
        let mut queue: std::collections::VecDeque<NodeId> = reverse_seeds.into_iter().collect();
        while let Some(target) = queue.pop_front() {
            if let Some(callers) = self.reverse_raw.get(&target) {
                for &caller in callers {
                    if affected.insert(caller) {
                        queue.push_back(caller);
                    }
                }
            }
        }

        let mut cache = std::collections::HashMap::new();
        let mut changed_comps: std::collections::BTreeSet<NodeId> = camera_callers.clone();
        for &c in &affected {
            let live_comp = self.dims.contains_key(&c);
            if !live_comp {
                if dims_changed.contains(&c) {
                    changed_comps.insert(c);
                }
                continue;
            }
            let mut comp_changed = dims_changed.contains(&c);
            if let Some(map) = self.edges.get(&c) {
                let resolved_list: Vec<(NodeId, Option<NodeId>, Option<NodeId>, bool)> = map
                    .iter()
                    .map(|(&e, &(decl, _))| {
                        let cyclic = decl.is_some_and(|t| self.dims.contains_key(&t) && self.edge_reaches(t, c, &mut cache));
                        let now = decl.filter(|&t| self.dims.contains_key(&t) && !cyclic);
                        (e, decl, now, cyclic)
                    })
                    .collect();
                if let Some(map) = self.edges.get_mut(&c) {
                    for (e, decl, now, cyclic) in resolved_list {
                        if cyclic {
                            self.cyclic_embeds.insert(e);
                        } else {
                            self.cyclic_embeds.remove(&e);
                        }
                        if let Some((_, resolved)) = map.get_mut(&e) {
                            if *resolved != now {
                                *resolved = now;
                                comp_changed = true;
                            }
                        }
                        if let Some(t) = decl.or(now)
                            && dims_changed.contains(&t)
                        {
                            comp_changed = true;
                        }
                    }
                }
            }
            if comp_changed {
                changed_comps.insert(c);
            }
        }
        let mut problem_comps = affected;
        problem_comps.extend(dirty_comps.iter().copied());
        problem_comps.extend(camera_callers);
        self.refresh_virtual_problems(doc, &problem_comps);
        changed_comps.into_iter().collect()
    }
}

/// Whether following drawn embeds from `target` reaches `home` (direct or indirect cycle).
fn reaches_composition(doc: Tree<'_>, target: NodeId, home: NodeId) -> bool {
    let mut stack = vec![target];
    let mut seen = std::collections::BTreeSet::new();
    while let Some(c) = stack.pop() {
        if c == home {
            return true;
        }
        if seen.insert(c) {
            stack.extend(embeds_in_unsorted(doc, c).into_iter().filter_map(|e| raw_embedded(doc, e)));
        }
    }
    false
}

/// The composition an embed shows, if it names a live composition without
/// closing a direct or indirect embed cycle (same invariant as `validate_embed`).
/// O(1) on committed non-recording `Tree`s via `Docs::cyclic_embeds`; walks and
/// records `Key::In`/`Key::Facet` on recording `Tree`s (`LayoutNative`).
pub fn embedded(doc: Tree<'_>, embed: NodeId) -> Option<NodeId> {
    if crate::expand::in_repeat_template(doc, embed) {
        return None;
    }
    let of = raw_embedded(doc, embed)?;
    if let Some(is_cyclic) = doc.is_cyclic_embed(embed) {
        return (!is_cyclic).then_some(of);
    }
    let home = composition_of(doc, embed)?;
    (!reaches_composition(doc, of, home)).then_some(of)
}

/// The camera an embed looks through, if it names one.
pub fn embed_camera(doc: Tree<'_>, embed: NodeId) -> Option<NodeId> {
    match doc.get::<crate::facet::EmbedCamera>(embed)?.camera.constant()? {
        crate::value::Value::Ref(c) if doc.is_live(*c) => Some(*c),
        _ => None,
    }
}

/// Default on-screen size of a 3D embed with no `size`, in document px.
pub const EMBED_3D_SIZE: (f64, f64) = (320.0, 240.0);

/// The on-screen extent of a 3D embed in its parent's space, and the camera
/// it names: the composition shown, the camera (none: the derived default
/// view), and `size` (default [`EMBED_3D_SIZE`]); with a camera, the height
/// follows the camera's aspect. `None` unless the embed shows a live 3D
/// composition.
pub fn embed_3d_extent(store: &dyn crate::reads::Reads, embed: NodeId) -> Option<(NodeId, Option<NodeId>, f64, f64)> {
    let doc = store.tree();
    // What it shows and through which camera are read as values, so that a
    // layout reading this (through a native's `Cx`) follows their edits.
    let leaf = |l: &str| l.parse::<crate::facet::LeafPath>().expect("declared leaf");
    let _ = (store.constant(embed, leaf("embed.source")), store.constant(embed, leaf("embed_camera.camera")));
    let source = embedded(doc, embed)?;
    let _ = store.constant(source, leaf("composition.dims"));
    if dims(doc, source) != Some(Dims::Three) {
        return None;
    }
    let num = |id: NodeId, leaf: &str| leaf.parse().ok().and_then(|p| store.resolved(id, p)).and_then(|v| v.as_f64());
    let has_size = doc.facet(embed, crate::facet::FacetKind::Size).is_some();
    let w = if has_size { num(embed, "size.w").unwrap_or(EMBED_3D_SIZE.0) } else { EMBED_3D_SIZE.0 };
    let mut h = if has_size { num(embed, "size.h").unwrap_or(EMBED_3D_SIZE.1) } else { EMBED_3D_SIZE.1 };
    let camera = embed_camera(doc, embed);
    if let Some(aspect) = camera.and_then(|c| num(c, "camera.aspect")).filter(|a| *a > 0.0) {
        h = w / aspect;
    }
    Some((source, camera, w, h))
}

/// Every live drawn embed inside `composition` (excluding undrawn repeat/when
/// prototypes), in unspecified order without allocating `order_path` sort keys.
/// Uses the per-root scoped type index (`Key::In`) so callers on a recording
/// `Tree` (such as `LayoutNative`) record embed topology dependencies
/// automatically without scanning the whole document.
fn embeds_in_unsorted(doc: Tree<'_>, composition: NodeId) -> Vec<NodeId> {
    let embed = TypeId::lookup("embed").expect("registered");
    let root = doc.top_root(composition).unwrap_or(composition);
    doc.nodes_of_type_in(root, embed)
        .into_iter()
        .filter(|&e| composition_of(doc, e) == Some(composition) && e != composition && !crate::expand::in_repeat_template(doc, e))
        .collect()
}

/// Every live drawn embed inside `composition`, in preorder.
pub fn embeds_in(doc: Tree<'_>, composition: NodeId) -> Vec<NodeId> {
    let mut out = embeds_in_unsorted(doc, composition);
    out.sort_by_cached_key(|&e| doc.order_path(e).into_iter().cloned().collect::<Vec<_>>());
    out
}

/// An embed must sit inside a composition, name a live composition, and not
/// close a loop: the composition it shows must not, through its own embeds,
/// show the one the embed is in. A named camera must be a camera of the
/// embedded composition.
pub fn validate_embed(doc: Tree<'_>, embed: NodeId) -> Result<(), String> {
    let home = composition_of(doc, embed).ok_or_else(|| format!("embed {embed} is not inside a composition"))?;
    let of = match doc.get::<crate::facet::Embed>(embed).and_then(|e| e.source.constant()) {
        Some(crate::value::Value::Ref(c)) => *c,
        _ => return Err(format!("embed {embed}: embed.source must name a composition")),
    };
    if composition_of(doc, of) != Some(of) {
        return Err(format!("embed {embed}: {of} is not a live composition"));
    }
    // Walk what `of` shows, transitively; reaching `home` is a loop.
    if reaches_composition(doc, of, home) {
        return Err(format!("embed {embed}: {of} already shows {home}, directly or through its embeds; embedding it in {home} would loop"));
    }
    if let Some(cam) = doc.get::<crate::facet::EmbedCamera>(embed).and_then(|e| e.camera.constant()) {
        let ok = matches!(cam, crate::value::Value::Ref(c) if doc.live(*c).is_some_and(|n| n.ty.name() == "camera") && composition_of(doc, *c) == Some(of));
        if !ok {
            return Err(format!("embed {embed}: embed_camera.camera must name a camera inside {of}"));
        }
    }
    Ok(())
}
