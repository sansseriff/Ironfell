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
    let els = crate::parts::elements(src)?;
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
    number(&els, &mut next, &mut named, &mut ids)?;
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
    emit(&els, parent, order, &mut ids, &named, &mut built)?;
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
