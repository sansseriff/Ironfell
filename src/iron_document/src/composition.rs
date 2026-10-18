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

use crate::facet::{Composition, LeafStruct};
use crate::document::{Document, RelKind, Relation};
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
/// the library, the timeline, and loose roots from older files).
pub fn composition_of(doc: &Document, id: NodeId) -> Option<NodeId> {
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
pub fn compositions(doc: &Document) -> Vec<NodeId> {
    let composition = TypeId::lookup("composition").expect("registered");
    doc.roots().iter().copied().filter(|&r| doc.live(r).is_some_and(|n| n.ty == composition)).collect()
}

pub fn dims(doc: &Document, composition: NodeId) -> Option<Dims> {
    let c = doc.get::<Composition>(composition)?;
    Some(match c.dims.constant().and_then(|v| v.as_str()) {
        Some("3d") => Dims::Three,
        _ => Dims::Two,
    })
}

pub fn notation(doc: &Document, composition: NodeId) -> Option<String> {
    doc.get::<Composition>(composition)?.notation.constant()?.as_str().map(str::to_owned)
}

/// The entity `node` pictures directly: the target of its `figures`
/// relation, if that entity is live.
pub fn figured_by(doc: &Document, node: NodeId) -> Option<NodeId> {
    doc.relations_from(node).find(|r| r.rel == RelKind::Figures && doc.is_live(r.to)).map(|r| r.to)
}

/// The figure `node` belongs to and the entity it pictures: `node` itself
/// or its nearest ancestor with a `figures` relation, stopping at the
/// composition root. This is what hover and selection lift through, so
/// grabbing a part of a grouped figure selects the whole thing.
pub fn figure_of(doc: &Document, node: NodeId) -> Option<(NodeId, NodeId)> {
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
pub fn figures_of(doc: &Document, entity: NodeId) -> Vec<NodeId> {
    doc.relations_to(entity).filter(|r| r.rel == RelKind::Figures && doc.is_live(r.from)).map(|r| r.from).collect()
}

/// The `model` root, if the document has one.
pub fn model_root(doc: &Document) -> Option<NodeId> {
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

fn name_of(doc: &Document, id: NodeId) -> Option<String> {
    let n = doc.get::<crate::facet::Name>(id)?.text.constant()?.as_str()?.to_owned();
    (!n.is_empty()).then_some(n)
}

pub fn summarize(doc: &Document, entity: NodeId) -> EntitySummary {
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
pub fn link_ops(doc: &Document, a: NodeId, b: NodeId, keep: Option<NodeId>) -> Result<Vec<Op>, LinkError> {
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
fn new_entity_ops(doc: &Document, a: NodeId, b: NodeId) -> Vec<Op> {
    use crate::facet::{Instance, Name, Args};
    use crate::value::{Slot, Value};
    let ty = |s: &str| TypeId::lookup(s).expect("registered");
    let named = |s: &str| {
        let mut n = Name::default();
        n.text = Slot::Const(Value::Str(s.to_owned()));
        n.wrap()
    };
    let last_root = || doc.roots().last().and_then(|&r| doc.live(r)).map(|n| n.order.clone());
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

/// The composition an embed shows, if it names a live one.
pub fn embedded(doc: &Document, embed: NodeId) -> Option<NodeId> {
    match doc.get::<crate::facet::Embed>(embed)?.source.constant()? {
        crate::value::Value::Ref(c) if composition_of(doc, *c) == Some(*c) => Some(*c),
        _ => None,
    }
}

/// The camera an embed looks through, if it names one.
pub fn embed_camera(doc: &Document, embed: NodeId) -> Option<NodeId> {
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
pub fn embed_3d_extent(store: &crate::Store, embed: NodeId) -> Option<(NodeId, Option<NodeId>, f64, f64)> {
    let doc = store.document();
    let source = embedded(doc, embed)?;
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

/// Every live embed inside `composition`, in preorder.
pub fn embeds_in(doc: &Document, composition: NodeId) -> Vec<NodeId> {
    let embed = TypeId::lookup("embed").expect("registered");
    doc.descendants(composition).into_iter().filter(|&n| doc.live(n).is_some_and(|x| x.ty == embed)).collect()
}

/// An embed must sit inside a composition, name a live composition, and not
/// close a loop: the composition it shows must not, through its own embeds,
/// show the one the embed is in. A named camera must be a camera of the
/// embedded composition.
pub fn validate_embed(doc: &Document, embed: NodeId) -> Result<(), String> {
    let home = composition_of(doc, embed).ok_or_else(|| format!("embed {embed} is not inside a composition"))?;
    let of = match doc.get::<crate::facet::Embed>(embed).and_then(|e| e.source.constant()) {
        Some(crate::value::Value::Ref(c)) => *c,
        _ => return Err(format!("embed {embed}: embed.source must name a composition")),
    };
    if composition_of(doc, of) != Some(of) {
        return Err(format!("embed {embed}: {of} is not a live composition"));
    }
    // Walk what `of` shows, transitively; reaching `home` is a loop.
    let mut stack = vec![of];
    let mut seen = std::collections::BTreeSet::new();
    while let Some(c) = stack.pop() {
        if c == home {
            return Err(format!("embed {embed}: {of} already shows {home}, directly or through its embeds; embedding it in {home} would loop"));
        }
        if seen.insert(c) {
            stack.extend(embeds_in(doc, c).into_iter().filter_map(|e| embedded(doc, e)));
        }
    }
    if let Some(cam) = doc.get::<crate::facet::EmbedCamera>(embed).and_then(|e| e.camera.constant()) {
        let ok = matches!(cam, crate::value::Value::Ref(c) if doc.live(*c).is_some_and(|n| n.ty.name() == "camera") && composition_of(doc, *c) == Some(of));
        if !ok {
            return Err(format!("embed {embed}: embed_camera.camera must name a camera inside {of}"));
        }
    }
    Ok(())
}
