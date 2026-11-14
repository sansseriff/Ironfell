//! Node markup (plans/specs/repeat.md §8): nodes written as XML, the way the
//! model reads a view and writes `<create>`. The element is the node type;
//! attributes are its leaves, `facet.field` or a bare field (the first of
//! the type's facets, in the registry's order, that has it), with `name`
//! the node's name; children are child nodes. A value is a constant of the
//! leaf's kind, a colour token (`@ink`), or a binding `{= expr}`.
//!
//! `id="$x"` names a new node for the rest of the markup: `$x` is its id,
//! in a binding (`{= $x.size.w}`) or a reference (`anchor.port="$x"`). Bare
//! `$x` in a binding reads the value a node holds: a `let`'s `let.value`, a
//! `port_spec`'s `port.signal`, a `channel`'s `channel.value`. `k.bind="e"`
//! is `k="{= e}"` (the form a view prints a binding in, beside its value).
//!
//! The same language is the model's edits (`edit.rs`, plans/specs/markup.md)
//! and the standard components (`std/*.xml`), so a component is data the
//! engine reads, not code: anything one does, an agent's edit can do too.

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

/// What a markup's `$names` stand for: the nodes it makes, the components
/// an edit places (compose.rs), and the document they are read against
/// (for `$p.port`, `#use.port`).
#[derive(Default)]
pub(crate) struct Names<'a> {
    /// `x` (from `id="$x"`) to the node and its type.
    pub(crate) nodes: BTreeMap<String, (NodeId, TypeId)>,
    /// `p` (from a `<place id="$p">`) to what it placed.
    pub(crate) places: BTreeMap<String, crate::compose::Named>,
    /// The document, once the places exist in it.
    pub(crate) doc: Option<&'a crate::document::Document>,
}

impl Names<'_> {
    /// The node `$name` stands for: a node, or a place's picture.
    pub(crate) fn node(&self, name: &str) -> Option<NodeId> {
        self.nodes.get(name).map(|(id, _)| *id).or_else(|| self.places.get(name).map(|p| p.picture))
    }
}

/// A new node's name, `id="$x"`, without its `$`.
pub(crate) fn local_name(el: &El) -> Result<Option<&str>, String> {
    match el.attr("id") {
        None => Ok(None),
        Some(n) => match n.strip_prefix('$') {
            Some(local) if !local.is_empty() && local.chars().all(|c| c.is_alphanumeric() || c == '_') => Ok(Some(local)),
            _ if n.starts_with('#') => Err(format!("<{} id=\"{n}\">: {n} is a node that exists; change it with <set id=\"{n}\" …/>. A new node is named id=\"$x\"", el.name)),
            _ => Err(format!("<{} id=\"{n}\">: a new node is named id=\"$x\", a $ then letters, digits or _", el.name)),
        },
    }
}

/// [`ops`] on parsed elements.
fn build(els: &[El], parent: Option<NodeId>, order: OrderKey, first: NodeId) -> Result<Built, String> {
    let mut names = Names::default();
    let mut ids = Vec::new();
    number(els, &mut first.raw(), &mut names.nodes, &mut ids)?;
    let mut built = Built { ops: Vec::new(), roots: Vec::new(), named: names.nodes.iter().map(|(k, (id, _))| (k.clone(), *id)).collect() };
    let mut ids = ids.into_iter();
    let mut order = order;
    for el in els {
        emit(el, parent, order.clone(), &mut ids, &names, &mut built.ops)?;
        order = OrderKey::after(&order);
    }
    built.roots = built.ops.iter().filter_map(|op| match op {
        Op::Create { id, parent: p, .. } if *p == parent => Some(*id),
        _ => None,
    }).collect();
    Ok(built)
}

/// Ids for `els` and their descendants in document order, from `next`;
/// each `id="$x"` recorded in `named`. Ids first, so a node may name one
/// written after it.
pub(crate) fn number(els: &[El], next: &mut u64, named: &mut BTreeMap<String, (NodeId, TypeId)>, ids: &mut Vec<NodeId>) -> Result<(), String> {
    for el in els {
        let ty = TypeId::lookup(&el.name).ok_or_else(|| format!("<{}>: no node type {:?}", el.name, el.name))?;
        let id = NodeId::from_raw(*next);
        *next += 1;
        ids.push(id);
        if let Some(n) = local_name(el)?
            && named.insert(n.to_owned(), (id, ty)).is_some()
        {
            return Err(format!("id=\"${n}\" names two nodes"));
        }
        number(&el.children, next, named, ids)?;
    }
    Ok(())
}

/// The ops creating `el` (its id the next of `ids`) under `parent` at
/// `order`, then its children.
pub(crate) fn emit(el: &El, parent: Option<NodeId>, order: OrderKey, ids: &mut std::vec::IntoIter<NodeId>, names: &Names, out: &mut Vec<Op>) -> Result<(), String> {
    let id = ids.next().expect("numbered");
    let ty = TypeId::lookup(&el.name).expect("numbered");
    let facets = facets(el, ty, names).map_err(|e| format!("<{}{}>: {e}", el.name, el.attr("name").map(|n| format!(" name=\"{n}\"")).unwrap_or_default()))?;
    out.push(Op::Create { id, ty, parent, order, facets });
    let mut child = OrderKey::first();
    for c in &el.children {
        emit(c, Some(id), child.clone(), ids, names, out)?;
        child = OrderKey::after(&child);
    }
    Ok(())
}

/// The facet and field attribute `attr` names on a node of type `ty`.
pub(crate) fn leaf(ty: TypeId, attr: &str) -> Result<(FacetKind, String), String> {
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

/// The value kind of `kind`'s field `field`.
pub(crate) fn kind_of(kind: FacetKind, field: &str) -> ValueKind {
    if kind.is_dynamic() { ValueKind::Any } else { kind.fields().iter().find(|(n, _)| *n == field).map(|(_, k)| *k).unwrap_or(ValueKind::Any) }
}

/// An element's leaf attributes, `k.bind="e"` read as `k="{= e}"` (a
/// binding written so wins over the value beside it), without `skip`.
pub(crate) fn leaf_attrs<'a>(el: &'a El, skip: &[&str]) -> Vec<(String, std::borrow::Cow<'a, str>)> {
    let bound: Vec<&str> = el.attrs.iter().filter_map(|(k, _)| k.strip_suffix(".bind")).collect();
    let mut out = Vec::new();
    for (k, v) in &el.attrs {
        if skip.contains(&k.as_str()) {
            continue;
        }
        match k.strip_suffix(".bind") {
            Some(base) => out.push((base.to_owned(), std::borrow::Cow::Owned(format!("{{= {v}}}")))),
            None if bound.contains(&k.as_str()) => {}
            None => out.push((k.clone(), std::borrow::Cow::Borrowed(v.as_str()))),
        }
    }
    out
}

/// A node's facets: those its attributes name, then its type's required
/// ones it lacks, at their defaults.
pub(crate) fn facets(el: &El, ty: TypeId, names: &Names) -> Result<Vec<Facet>, String> {
    let mut out: Vec<Facet> = Vec::new();
    for (attr, raw) in leaf_attrs(el, &["id", "in", "after", "first"]) {
        // A leaf written later (edit.rs): its facet, at its default.
        if let Some(later) = attr.strip_prefix("__defer:") {
            let (kind, _) = leaf(ty, later)?;
            if !out.iter().any(|f| f.kind() == kind) {
                out.push(kind.default_facet());
            }
            continue;
        }
        let (kind, field) = leaf(ty, &attr)?;
        if !out.iter().any(|f| f.kind() == kind) {
            out.push(kind.default_facet());
        }
        let f = out.iter_mut().find(|f| f.kind() == kind).expect("added");
        let slot = slot(kind_of(kind, &field), &raw, names).map_err(|e| format!("{attr}=\"{raw}\": {e}"))?;
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

/// The port `name` of the instance `node` stands for (an entity, or a
/// `use` picturing one), as the expanded node holding its signal.
pub(crate) fn instance_port(doc: &crate::document::Document, node: NodeId, name: &str) -> Option<NodeId> {
    let entity = match doc.live(node)?.ty.name() {
        "use" => crate::composition::figure_of(doc, node).map(|(_, e)| e)?,
        _ => node,
    };
    let def = crate::expand::linked_definition(doc, entity)?;
    let spec = doc.children(def).iter().copied().find(|&c| {
        doc.get::<crate::facet::Port>(c).is_some() && doc.get::<crate::facet::Name>(c).and_then(|n| n.text.constant()?.as_str().map(str::to_owned)).as_deref() == Some(name)
    })?;
    NodeId::expanded(entity, spec)
}

/// A name's characters, from the start of `s`.
fn word(s: &str) -> usize {
    s.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(s.len())
}

/// `$x` and `$x.facet.field` in a binding's text, as ids; bare `$x` reads
/// the value `x` holds. A place's `$p.facet.field` is its picture's leaf,
/// `$p.port` its instance's port; so is `#use.port` for an instance the
/// document has.
pub(crate) fn resolve(text: &str, names: &Names) -> Result<String, String> {
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let quoted = |at: usize| text[..at].matches('\'').count() % 2 == 1;
    while i < text.len() {
        let c = text[i..].chars().next().expect("in range");
        if (c != '$' && c != '#') || quoted(i) {
            out.push(c);
            i += c.len_utf8();
            continue;
        }
        if c == '#' {
            // `#use.port`: an id, then one name not followed by another.
            let id_end = i + 1 + text[i + 1..].find(|c: char| !(c.is_alphanumeric() || c == '/' || c == '~')).unwrap_or(text.len() - i - 1);
            let rest = &text[id_end..];
            if let (Some(doc), Some(after_dot)) = (names.doc, rest.strip_prefix('.')) {
                let n = word(after_dot);
                let follows = &after_dot[n..];
                if n > 0 && !follows.starts_with('.')
                    && let Ok(id) = text[i..id_end].parse::<NodeId>()
                    && let Some(port) = instance_port(doc, id, &after_dot[..n])
                {
                    out.push_str(&format!("{port}.port.signal"));
                    i = id_end + 1 + n;
                    continue;
                }
            }
            out.push_str(&text[i..id_end]);
            i = id_end;
            continue;
        }
        let start = i + 1;
        let end = start + word(&text[start..]);
        let name = &text[start..end];
        let rest = &text[end..];
        if let Some((id, ty)) = names.nodes.get(name) {
            out.push_str(&id.to_string());
            if !rest.starts_with('.') {
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
            i = end;
            continue;
        }
        if let Some(placed) = names.places.get(name) {
            let path_len = rest.strip_prefix('.').map_or(0, |r| r.find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.')).unwrap_or(r.len()));
            let path = rest.get(1..1 + path_len).unwrap_or("").trim_end_matches('.');
            if path.is_empty() {
                out.push_str(&placed.picture.to_string());
                i = end;
            } else if path.contains('.') {
                out.push_str(&format!("{}.{path}", placed.picture));
                i = end + 1 + path.len();
            } else {
                let doc = names.doc.ok_or_else(|| format!("${name}.{path}: a place's port is read once it is placed"))?;
                let entity = placed.entity.ok_or_else(|| format!("${name} placed no entity"))?;
                let port = instance_port(doc, entity, path).ok_or_else(|| format!("${name} has no port {path}"))?;
                out.push_str(&format!("{port}.port.signal"));
                i = end + 1 + path.len();
            }
            continue;
        }
        return Err(format!("${name} names no node made in this edit"));
    }
    Ok(out)
}

/// Whether `text` reads one of `places` (`$p`, not `$pq`).
pub(crate) fn reads_place(text: &str, places: &[String]) -> bool {
    places.iter().any(|p| text.match_indices(&format!("${p}")).any(|(i, m)| !text[i + m.len()..].starts_with(|c: char| c.is_alphanumeric() || c == '_')))
}

/// One attribute's slot, for a leaf of kind `kind`.
pub(crate) fn slot(kind: ValueKind, raw: &str, names: &Names) -> Result<Slot, String> {
    let t = raw.trim();
    if let Some(e) = t.strip_prefix("{=").and_then(|r| r.strip_suffix('}')) {
        let e = resolve(e.trim(), names)?;
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
                Some(n) => names.node(n).ok_or_else(|| format!("${n} names no node made in this edit"))?,
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
              <port_spec id="$a" name="a" kind="value" dir="in" signal="3 mW"/>
              <template notation="free" w="40" h="20">
                <let id="$half" value="{= $a / 2}"/>
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
