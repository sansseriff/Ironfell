//! Node markup (plans/specs/repeat.md §8): nodes written as XML, the way the
//! model reads a view and writes `<create>`. The element is the node type;
//! attributes are its leaves, `facet.field` or a bare field (the first of
//! the type's facets, in the registry's order, that has it), with `name`
//! the node's name; children are child nodes. A value is a constant of the
//! leaf's kind, a colour token (`@ink`), or a binding `{= expr}`.
//!
//! `id="x"` names a node for the rest of the markup: `$x` is its id, in a
//! binding (`{= $x.size.w}`) or a reference (`anchor.port="$x"`). Bare `$x`
//! in a binding reads the value a node holds: a `let`'s `let.value`, a
//! `port_spec`'s `port.signal`, a `channel`'s `channel.value`.
//!
//! The standard components are written in it (`std/*.xml`), so a component
//! is data the engine reads, not code: anything one does, an agent's edit
//! can do too.

use crate::expr::Expr;
use crate::facet::{Facet, FacetKind};
use crate::id::NodeId;
use crate::op::Op;
use crate::order::OrderKey;
use crate::parts::El;
use crate::registry::TypeId;
use crate::value::{Slot, Value, ValueKind};
use std::collections::BTreeMap;

/// What markup builds: the ops creating it, its top-level nodes, and the
/// nodes it named, by name.
#[derive(Debug)]
pub struct Built {
    pub ops: Vec<Op>,
    pub roots: Vec<NodeId>,
    pub named: BTreeMap<String, NodeId>,
}

/// The ops creating `src`'s nodes under `parent` (a root when `None`), the
/// first at `order` and the rest after it, with ids counting up from
/// `first` in document order.
pub fn ops(src: &str, parent: Option<NodeId>, order: OrderKey, first: NodeId) -> Result<Built, String> {
    build(&crate::parts::elements(src)?, parent, order, first)
}

/// Node markup that also places components: a `<place>` element among the
/// nodes, at any depth, is the model's `<place>` verb (compose.rs) with
/// `in` its parent node (or `parent` at the top), applied after the nodes.
/// Its attributes may name the markup's nodes as `$x`, in a binding or as
/// `def`; `$self` keeps its meaning there. A place's `id="p"` names it for
/// the nodes: `$p.transform2d.x` is its picture's leaf and `$p.x_lo` its
/// instance's port, so a mark can sit on a plot; such leaves are set once
/// the places exist. This is how a whole composition is written as data
/// (`document_bridge::demo`'s gallery). Returns the ops, starting with the
/// nodes', and what the markup named.
pub fn compose(store: &crate::Store, src: &str, parent: Option<NodeId>, order: OrderKey) -> Result<Built, String> {
    let mut els = crate::parts::elements(src)?;
    // The places' own names, each given an alias for compose.rs.
    let mut place_names: BTreeMap<String, String> = BTreeMap::new();
    fn name_places(els: &mut [El], out: &mut BTreeMap<String, String>) -> Result<(), String> {
        for el in els.iter_mut() {
            if el.name == "place" {
                let alias = format!("$__place_{}", out.len());
                if let Some(pos) = el.attrs.iter().position(|(k, _)| k == "id") {
                    let (_, local) = el.attrs.remove(pos);
                    if out.insert(local.clone(), alias.clone()).is_some() {
                        return Err(format!("id=\"{local}\" names two places"));
                    }
                } else {
                    out.insert(format!("__anonymous_{}", out.len()), alias.clone());
                }
                if el.attr("as").is_some() {
                    return Err("a <place> in node markup is named by id=, not as=".into());
                }
                el.attrs.push(("as".into(), alias));
            }
            name_places(&mut el.children, out)?;
        }
        Ok(())
    }
    name_places(&mut els, &mut place_names)?;
    let names_a_place = |v: &str| place_names.keys().any(|p| v.match_indices(&format!("${p}")).any(|(i, m)| !v[i + m.len()..].starts_with(|c: char| c.is_alphanumeric() || c == '_')));
    // Lift the places out, each with the local name of the node it sits in
    // (one is given to a node that has none), and set aside the leaves that
    // read a place: they are written after it.
    let mut places: Vec<(Option<String>, El)> = Vec::new();
    let mut deferred: Vec<(String, String, String)> = Vec::new();
    // Siblings among which a place sits, in markup order (a node's local
    // name, or a place's alias): the places are made after the nodes, so
    // the siblings are put back in this order once they all exist.
    let mut sequences: Vec<(Option<String>, Vec<Sibling>)> = Vec::new();
    let mut synthetic = 0;
    enum Sibling {
        Node(String),
        Place(String),
    }
    #[allow(clippy::too_many_arguments)]
    fn lift(els: &mut Vec<El>, owner: Option<&str>, places: &mut Vec<(Option<String>, El)>, deferred: &mut Vec<(String, String, String)>, sequences: &mut Vec<(Option<String>, Vec<Sibling>)>, synthetic: &mut usize, reads_place: &dyn Fn(&str) -> bool) {
        let has_place = els.iter().any(|e| e.name == "place");
        let mut sequence = Vec::new();
        let mut kept = Vec::with_capacity(els.len());
        for mut el in els.drain(..) {
            if el.name == "place" {
                sequence.push(Sibling::Place(el.attr("as").expect("aliased above").to_owned()));
                places.push((owner.map(str::to_owned), el));
                continue;
            }
            let defers = el.attrs.iter().any(|(_, v)| reads_place(v));
            if (defers || has_place || el.children.iter().any(|c| c.name == "place")) && el.attr("id").is_none() {
                el.attrs.push(("id".into(), format!("__node_{synthetic}")));
                *synthetic += 1;
            }
            if defers {
                let local = el.attr("id").expect("named above").to_owned();
                for (k, v) in el.attrs.iter_mut() {
                    if reads_place(v) {
                        deferred.push((local.clone(), k.clone(), std::mem::take(v)));
                        *k = format!("__defer:{k}");
                    }
                }
            }
            let id = el.attr("id").map(str::to_owned);
            if has_place {
                sequence.push(Sibling::Node(id.clone().expect("named above")));
            }
            lift(&mut el.children, id.as_deref(), places, deferred, sequences, synthetic, reads_place);
            kept.push(el);
        }
        if has_place {
            sequences.push((owner.map(str::to_owned), sequence));
        }
        *els = kept;
    }
    lift(&mut els, None, &mut places, &mut deferred, &mut sequences, &mut synthetic, &names_a_place);
    let mut built = build(&els, parent, order, store.document().next_id())?;
    let type_of: BTreeMap<NodeId, TypeId> = built.ops.iter().filter_map(|op| match op {
        Op::Create { id, ty, .. } => Some((*id, *ty)),
        _ => None,
    }).collect();
    let types: BTreeMap<String, (NodeId, TypeId)> = built.named.iter().map(|(k, id)| (k.clone(), (*id, type_of[id]))).collect();
    built.named.retain(|k, _| !k.starts_with("__node_"));
    if places.is_empty() {
        return Ok(built);
    }
    let mut verbs = Vec::new();
    for (owner, mut el) in places {
        for (k, v) in el.attrs.iter_mut() {
            if k != "as" {
                *v = resolve_known(v, &types);
            }
        }
        if el.attr("in").is_none() {
            let target = match owner {
                Some(local) => types.get(&local).map(|(id, _)| *id).ok_or_else(|| format!("no node {local}"))?,
                None => parent.ok_or("a <place> at the top level needs in=")?,
            };
            el.attrs.push(("in".into(), target.to_string()));
        }
        verbs.push(el);
    }
    let (mut ops, aliases) = crate::compose::compose_els(store, built.ops, verbs).map_err(|e| match e {
        crate::compose::ComposeError::Ops(errors) => errors.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "),
        crate::compose::ComposeError::Verb(m) => m,
    })?;
    // The leaves that read a place, now that it exists: `$p.a.b` is its
    // picture's leaf, `$p.port` its entity's port.
    let mut scratch = crate::Store::with_document(store.document().clone());
    scratch
        .apply(crate::TransactionInput { label: "scratch".into(), actor: crate::Actor::System, ts: 0, idempotency_key: None, ops: ops.clone(), exact: false })
        .map_err(|e| e.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "))?;
    let doc = scratch.document();
    // Siblings back in markup order: a mark written after a plot paints
    // over it.
    for (owner, sequence) in &sequences {
        let parent_id = match owner {
            Some(local) => Some(types[local].0),
            None => parent,
        };
        let ids: Vec<NodeId> = sequence.iter().map(|s| match s {
            Sibling::Node(local) => types[local].0,
            Sibling::Place(alias) => aliases[alias].picture,
        }).collect();
        let Some(mut key) = ids.first().and_then(|&id| doc.live(id)).map(|n| n.order.clone()) else { continue };
        for &id in &ids[1..] {
            key = OrderKey::after(&key);
            ops.push(Op::Reparent { id, parent: parent_id, order: key.clone() });
        }
    }
    for (local, key, raw) in deferred {
        let (id, ty) = types[&local];
        let mut text = raw.trim().strip_prefix("{=").and_then(|r| r.strip_suffix('}')).ok_or_else(|| format!("{key}=\"{raw}\": a leaf that reads a place is a binding, {{= …}}"))?.trim().to_owned();
        for (place, alias) in &place_names {
            let named = aliases[alias];
            let needle = format!("${place}.");
            while let Some(at) = text.find(&needle) {
                let rest = &text[at + needle.len()..];
                let end = rest.find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.')).unwrap_or(rest.len());
                let path = rest[..end].trim_end_matches('.');
                let with = if path.contains('.') {
                    format!("{}.{path}", named.picture)
                } else {
                    let entity = named.entity.ok_or_else(|| format!("${place} placed no entity"))?;
                    let spec = doc.children(named.definition).iter().copied().find(|&c| doc.get::<crate::facet::Port>(c).is_some() && doc.get::<crate::facet::Name>(c).and_then(|n| n.text.constant()?.as_str().map(str::to_owned)).as_deref() == Some(path)).ok_or_else(|| format!("${place} has no port {path}"))?;
                    format!("{}.port.signal", NodeId::expanded(entity, spec).ok_or("no derived id")?)
                };
                text.replace_range(at..at + needle.len() + path.len(), &with);
            }
        }
        let text = resolve(&text, &types)?;
        let (kind, field) = leaf(ty, &key)?;
        let path = crate::facet::LeafPath::new(kind, &field).map_err(|e| format!("{key}: {e}"))?;
        let expr = Expr::parse(&text).map_err(|e| format!("{key}=\"{raw}\": {e}"))?;
        ops.push(Op::Set { id, path, slot: Slot::Bound(expr) });
    }
    built.ops = ops;
    Ok(built)
}

/// `$x` in `text` as the id markup gave `x` (bare, the value it holds, as
/// [`resolve`]); a `$name` the markup did not name is left as written.
fn resolve_known(text: &str, named: &BTreeMap<String, (NodeId, TypeId)>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('$') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let end = after.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(after.len());
        let name = &after[..end];
        let next = &after[end..after.len().min(end + 1)];
        match resolve(&format!("${name}{next}"), named) {
            // `resolve` saw the name and the character after it, so it
            // added the value suffix only for a bare reference.
            Ok(r) if named.contains_key(name) => out.push_str(r.strip_suffix(next).unwrap_or(&r)),
            _ => {
                out.push('$');
                out.push_str(name);
            }
        }
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

/// [`ops`] on parsed elements.
fn build(els: &[El], parent: Option<NodeId>, order: OrderKey, first: NodeId) -> Result<Built, String> {
    // Ids first, so a node may name one written after it.
    let mut next = first.raw();
    let mut named: BTreeMap<String, (NodeId, TypeId)> = BTreeMap::new();
    fn number(els: &[El], next: &mut u64, named: &mut BTreeMap<String, (NodeId, TypeId)>, ids: &mut Vec<NodeId>) -> Result<(), String> {
        for el in els {
            let ty = TypeId::lookup(&el.name).ok_or_else(|| format!("<{}>: no node type {:?}", el.name, el.name))?;
            let id = NodeId::from_raw(*next);
            *next += 1;
            ids.push(id);
            if let Some(n) = el.attr("id")
                && named.insert(n.to_owned(), (id, ty)).is_some()
            {
                return Err(format!("id=\"{n}\" names two nodes"));
            }
            number(&el.children, next, named, ids)?;
        }
        Ok(())
    }
    let mut ids = Vec::new();
    number(els, &mut next, &mut named, &mut ids)?;
    let mut built = Built { ops: Vec::new(), roots: Vec::new(), named: named.iter().map(|(k, (id, _))| (k.clone(), *id)).collect() };
    let mut ids = ids.into_iter();
    fn emit(els: &[El], parent: Option<NodeId>, mut order: OrderKey, ids: &mut std::vec::IntoIter<NodeId>, named: &BTreeMap<String, (NodeId, TypeId)>, out: &mut Built) -> Result<(), String> {
        for el in els {
            let id = ids.next().expect("numbered");
            let ty = TypeId::lookup(&el.name).expect("numbered");
            let facets = facets(el, ty, named).map_err(|e| format!("<{}{}>: {e}", el.name, el.attr("name").map(|n| format!(" name=\"{n}\"")).unwrap_or_default()))?;
            out.ops.push(Op::Create { id, ty, parent, order: order.clone(), facets });
            emit(&el.children, Some(id), OrderKey::first(), ids, named, out)?;
            order = OrderKey::after(&order);
        }
        Ok(())
    }
    emit(els, parent, order, &mut ids, &named, &mut built)?;
    // Top-level nodes are those created under `parent`.
    built.roots = built.ops.iter().filter_map(|op| match op {
        Op::Create { id, parent: p, .. } if *p == parent => Some(*id),
        _ => None,
    }).collect();
    Ok(built)
}

/// The facet and field attribute `attr` names on a node of type `ty`.
fn leaf(ty: TypeId, attr: &str) -> Result<(FacetKind, String), String> {
    if attr == "name" {
        return Ok((FacetKind::Name, "text".into()));
    }
    let spec = ty.spec();
    if let Some((f, field)) = attr.split_once('.') {
        let kind = FacetKind::from_name(f).ok_or_else(|| format!("no facet {f:?}"))?;
        if !spec.allows(kind) {
            return Err(format!("a {} has no {f} facet", spec.name));
        }
        if !kind.is_dynamic() && !kind.fields().iter().any(|(n, _)| *n == field) {
            return Err(format!("{f} has no field {field:?}"));
        }
        return Ok((kind, field.to_owned()));
    }
    if let Some(found) = spec.facets.iter().map(|(k, _)| *k).find(|k| k.fields().iter().any(|(n, _)| *n == attr)) {
        return Ok((found, attr.to_owned()));
    }
    // A conditional by its facet's name: `when="{= … }"` is `when.is`.
    match FacetKind::from_name(attr).filter(|k| spec.allows(*k)) {
        Some(FacetKind::When) => Ok((FacetKind::When, "is".to_owned())),
        _ => Err(format!("{attr:?} is not a field of {}", spec.name)),
    }
}

/// A node's facets: those its attributes name, then its type's required
/// ones it lacks, at their defaults.
fn facets(el: &El, ty: TypeId, named: &BTreeMap<String, (NodeId, TypeId)>) -> Result<Vec<Facet>, String> {
    let mut out: Vec<Facet> = Vec::new();
    for (attr, raw) in &el.attrs {
        if attr == "id" {
            continue;
        }
        // A leaf written later (`compose`): its facet, at its default.
        if let Some(later) = attr.strip_prefix("__defer:") {
            let (kind, _) = leaf(ty, later)?;
            if !out.iter().any(|f| f.kind() == kind) {
                out.push(kind.default_facet());
            }
            continue;
        }
        let (kind, field) = leaf(ty, attr)?;
        if !out.iter().any(|f| f.kind() == kind) {
            out.push(kind.default_facet());
        }
        let f = out.iter_mut().find(|f| f.kind() == kind).expect("added");
        let vk = if kind.is_dynamic() { ValueKind::Any } else { kind.fields().iter().find(|(n, _)| *n == field).map(|(_, k)| *k).expect("checked") };
        let slot = slot(vk, raw, named).map_err(|e| format!("{attr}=\"{raw}\": {e}"))?;
        match f {
            Facet::Args(a) => a.insert(&field, slot),
            f => {
                f.set(&field, slot).map_err(|e| format!("{attr}: {e}"))?;
            }
        }
    }
    // A choice's value is one of its names (types-and-units.md): an input
    // declared `choice(…)` holds an enum.
    if let Some(Facet::Port(p)) = out.iter_mut().find(|f| f.kind() == FacetKind::Port)
        && p.datatype.constant().and_then(|v| v.as_str()).is_some_and(|t| t.starts_with("choice("))
        && let Some(Value::Str(s)) = p.signal.constant()
    {
        p.signal = Slot::Const(Value::Enum(s.clone()));
    }
    for (kind, required) in ty.spec().facets {
        if *required && !out.iter().any(|f| f.kind() == *kind) {
            out.push(kind.default_facet());
        }
    }
    Ok(out)
}

/// `$x` and `$x.facet.field` in a binding's text, as ids; bare `$x` reads
/// the value `x` holds.
fn resolve(text: &str, named: &BTreeMap<String, (NodeId, TypeId)>) -> Result<String, String> {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c != '$' {
            out.push(c);
            continue;
        }
        let start = i + 1;
        let mut end = start;
        while let Some(&(j, d)) = chars.peek() {
            if d.is_alphanumeric() || d == '_' {
                end = j + d.len_utf8();
                chars.next();
            } else {
                break;
            }
        }
        let name = &text[start..end];
        let (id, ty) = named.get(name).ok_or_else(|| format!("${name} names no node"))?;
        out.push_str(&id.to_string());
        if text[end..].starts_with('.') {
            continue;
        }
        out.push_str(match ty.name() {
            "let" => ".let.value",
            "trace" => ".trace.out",
            "query" => ".query.value",
            "accumulate" => ".accumulate.value",
            "port_spec" => ".port.signal",
            "channel" => ".channel.value",
            // Any other node, bare, is the node itself: `list($a, $b)`.
            _ => "",
        });
    }
    Ok(out)
}

/// One attribute's slot, for a leaf of kind `kind`.
fn slot(kind: ValueKind, raw: &str, named: &BTreeMap<String, (NodeId, TypeId)>) -> Result<Slot, String> {
    let t = raw.trim();
    if let Some(e) = t.strip_prefix("{=").and_then(|r| r.strip_suffix('}')) {
        let e = resolve(e.trim(), named)?;
        return Expr::parse(&e).map(Slot::Bound).map_err(|err| err.to_string());
    }
    // A colour token, as CSS writes a variable (palette.rs).
    if t.starts_with('@') && matches!(kind, ValueKind::Color | ValueKind::Any) {
        return Expr::parse(t).map(Slot::Bound).map_err(|err| err.to_string());
    }
    let number = |t: &str| -> Option<f64> { if t.eq_ignore_ascii_case("nan") { Some(f64::NAN) } else { t.strip_suffix("px").unwrap_or(t).parse().ok() } };
    let empty_series = || Value::series(Vec::<f64>::new(), crate::units::Dim::NONE);
    let v = match kind {
        ValueKind::Number => Value::Number(number(t).ok_or("expected a number")?),
        ValueKind::Bool => Value::Bool(match t {
            "true" => true,
            "false" => false,
            _ => return Err("expected true or false".into()),
        }),
        ValueKind::Str => Value::Str(raw.to_owned()),
        ValueKind::Enum => Value::Enum(t.to_owned()),
        ValueKind::Color => crate::compose::hex_color(t).ok_or("expected #rrggbb, #rrggbbaa or a token, @name")?,
        ValueKind::Vec2 | ValueKind::Vec3 => {
            let n: Vec<f64> = t.split(|c: char| c == ',' || c.is_whitespace()).filter(|w| !w.is_empty()).map(|w| w.parse::<f64>()).collect::<Result<_, _>>().map_err(|_| "expected numbers")?;
            match (kind, n.as_slice()) {
                (ValueKind::Vec2, [x, y]) => Value::Vec2([*x, *y]),
                (ValueKind::Vec3, [x, y, z]) => Value::Vec3([*x, *y, *z]),
                _ => return Err("wrong number of components".into()),
            }
        }
        ValueKind::Ref => {
            let id = match t.strip_prefix('$') {
                Some(n) => named.get(n).map(|(id, _)| *id).ok_or_else(|| format!("${n} names no node"))?,
                None => t.parse::<NodeId>().map_err(|e| e.to_string())?,
            };
            Value::Ref(id)
        }
        ValueKind::Quantity => Value::parse_quantity(t)?,
        ValueKind::Series if t == "[]" => empty_series(),
        ValueKind::Series => Value::parse_series(t).unwrap_or_else(|| Err("expected two or more numbers, then an optional unit".into()))?,
        ValueKind::Geometry => Value::Str(t.to_owned()),
        ValueKind::Fn => return Err("a function is written as a binding, {= x => …}".into()),
        ValueKind::List => return Err("a list is written as a binding, {= list(…)}".into()),
        ValueKind::Record => return Err("a record is set by a binding".into()),
        ValueKind::Any => {
            if t == "[]" {
                empty_series()
            } else if let Some(n) = number(t) {
                Value::Number(n)
            } else if t == "true" || t == "false" {
                Value::Bool(t == "true")
            } else if let Some(s) = Value::parse_series(t) {
                s?
            } else if let Ok(q) = Value::parse_quantity(t) {
                q
            } else if let Some(c) = crate::compose::hex_color(t) {
                c
            } else {
                Value::Str(raw.to_owned())
            }
        }
    };
    Ok(Slot::Const(v))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_names_resolve_and_values_take_their_leaf_s_kind() {
        let src = r#"
            <definition name="d">
              <port_spec id="a" name="a" kind="value" dir="in" signal="3 mW"/>
              <template notation="free" w="40" h="20">
                <let id="half" value="{= $a / 2}"/>
                <rect name="r" x="1" y="{= $half / 1 mW}" w="10" h="10" fill.color="@ink" corners.tl="2"/>
                <empty name="e" anchor.port="$a"/>
              </template>
            </definition>"#;
        let b = ops(src, None, OrderKey::first(), NodeId::from_raw(10)).unwrap();
        assert_eq!(b.roots.len(), 1);
        assert_eq!(b.ops.len(), 6);
        let a = b.named["a"];
        let Op::Create { facets, .. } = &b.ops[4] else { panic!() };
        let y = facets.iter().find_map(|f| match f { Facet::Transform2d(t) => Some(t.y.clone()), _ => None }).unwrap();
        assert!(y.expr().unwrap().to_string().starts_with(&format!("{}.let.value / ", b.named["half"])));
        let Op::Create { facets, .. } = &b.ops[5] else { panic!() };
        assert!(facets.iter().any(|f| matches!(f, Facet::Anchor(x) if x.port.constant() == Some(&Value::Ref(a)))));
        assert!(ops(r#"<rect nope="1"/>"#, None, OrderKey::first(), NodeId::from_raw(1)).unwrap_err().contains("nope"));
    }
}
