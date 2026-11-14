//! The model's component verbs (component-model.md §5, ECM §5): place an
//! instance of a definition in any 2D composition, picture an entity it
//! already has in another one, and make a component from a selection, as
//! one transaction. The `<circuit>` markup does the same for schematic parts
//! (`parts::circuit_ops`); these are its counterpart outside schematics.
//!
//! ```xml
//! <make-component of="#a #b" name="pump" as="$pump"/>
//! <place def="$pump" in="#bench" x="300" y="80" name="P1" as="$p1" rate="2"/>
//! <place entity="$p1" in="#overview" x="40" y="40"/>
//! <place def="std:laser" in="#bench" x="40" y="200" power="5 mW"/>
//! ```
//!
//! A `place` names a definition (`def`: an id, `std:<key>` for a standard
//! component, copied in on first use, or an earlier verb's `$alias`) or an
//! entity it already has (`entity`), and where the picture goes (`in`: a
//! composition, by id or name, or a group in one). Its `use.notation` is the
//! composition's notation unless `notation` says otherwise. A new entity
//! goes under the model root, named by `name`; any other attribute
//! (`args.k` or a bare `k`) is an argument, a value of the input's kind or
//! a `{= expr}` binding. A `make-component` makes a component of `of`
//! (plain shapes become a definition shown by its main; uses of linked
//! instances become one instance of a new definition that nests them).

use crate::composition::{self, Dims};
use crate::definition::{self, Placement};
use crate::expr::Expr;
use crate::facet::{Composition, Facet, Name};
use crate::id::NodeId;
use crate::op::Op;
use crate::parts::{elements, El};
use crate::registry::TypeId;
use crate::value::{Slot, Value, ValueKind};
use std::collections::BTreeMap;

fn ty(n: &str) -> TypeId {
    TypeId::lookup(n).expect("registered type")
}

/// What an `as="$alias"` names: the definition a verb made or placed, the
/// entity it placed (or made), and that entity's picture.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Named {
    pub(crate) definition: NodeId,
    pub(crate) entity: Option<NodeId>,
    pub(crate) picture: NodeId,
}

/// Attributes of `<place>` that are not arguments.
const PLACE_ATTRS: &[&str] = &["def", "entity", "in", "x", "y", "rot", "notation", "name", "as", "size.w", "size.h"];

/// The ops for a model's component verbs (see the module doc), and a
/// summary of what each made, one line per verb.
pub fn compose_ops(store: &crate::Store, markup: &str) -> Result<(Vec<Op>, String), String> {
    compose_after(store, Vec::new(), markup).map_err(|e| match e {
        ComposeError::Ops(errors) => errors.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "),
        ComposeError::Verb(message) => message,
    })
}

/// Why [`compose_after`] failed: one of the ops before the verbs (each
/// error indexes `before`), or a verb.
#[derive(Debug)]
pub enum ComposeError {
    Ops(Vec<crate::ApplyError>),
    Verb(String),
}

/// One edit that both writes ops and places components: `before` applied
/// first, then the verbs, each seeing what came before it, so a `<place>`
/// can name a definition `before` creates. The ops returned start with
/// `before` and make one transaction.
pub fn compose_after(store: &crate::Store, before: Vec<Op>, markup: &str) -> Result<(Vec<Op>, String), ComposeError> {
    let els = elements(markup).map_err(ComposeError::Verb)?;
    if els.is_empty() {
        return Err(ComposeError::Verb("no <place> or <make-component>".into()));
    }
    run_verbs(store, before, els).map(|c| (c.ops, c.lines.join("\n")))
}

/// What the component verbs made: every op (`before`'s first), one summary
/// line and the end of its ops for each verb, and what each `as` named.
pub(crate) struct Composed {
    pub(crate) ops: Vec<Op>,
    pub(crate) lines: Vec<String>,
    pub(crate) ends: Vec<usize>,
    pub(crate) named: BTreeMap<String, Named>,
}

/// [`compose_after`] on verbs already parsed (an edit's `<place>`s).
pub(crate) fn compose_els(store: &crate::Store, before: Vec<Op>, els: Vec<El>) -> Result<Composed, ComposeError> {
    run_verbs(store, before, els)
}

/// The verbs run on a scratch copy holding `before`.
fn run_verbs(store: &crate::Store, before: Vec<Op>, els: Vec<El>) -> Result<Composed, ComposeError> {
    let mut scratch = crate::Store::with_document(store.document().clone());
    if !before.is_empty() {
        scratch
            .apply(crate::TransactionInput { label: "scratch".into(), actor: crate::Actor::Model, ts: 0, idempotency_key: None, ops: before.clone(), exact: false })
            .map_err(ComposeError::Ops)?;
    }
    compose_verbs(scratch, before, els).map_err(ComposeError::Verb)
}

fn compose_verbs(mut scratch: crate::Store, mut ops: Vec<Op>, els: Vec<El>) -> Result<Composed, String> {
    let mut aliases: BTreeMap<String, Named> = BTreeMap::new();
    let mut lines = Vec::new();
    let mut ends = Vec::new();
    for (i, el) in els.iter().enumerate() {
        let at = |m: String| format!("<{}> {}: {m}", el.name, i + 1);
        let (new, line, named) = match el.name.as_str() {
            "place" => place(&scratch, el, &aliases).map_err(at)?,
            "make-component" => make(&scratch, el, &aliases).map_err(at)?,
            other => return Err(format!("<{other}>: these verbs are place and make-component")),
        };
        scratch
            .apply(crate::TransactionInput { label: "scratch".into(), actor: crate::Actor::Model, ts: 0, idempotency_key: None, ops: new.clone(), exact: false })
            .map_err(|e| at(e.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ")))?;
        ops.extend(new);
        if let Some(a) = el.attr("as") {
            if !a.starts_with('$') {
                return Err(at(format!("as=\"{a}\": an alias starts with $")));
            }
            aliases.insert(a.to_owned(), named);
        }
        lines.push(line);
        ends.push(ops.len());
    }
    Ok(Composed { ops, lines, ends, named: aliases })
}

/// A node id as written: `#3F`, or an earlier verb's `$alias` through `pick`.
fn node(doc: &crate::document::Document, raw: &str, aliases: &BTreeMap<String, Named>, pick: impl Fn(&Named) -> Option<NodeId>, what: &str) -> Result<NodeId, String> {
    if raw.starts_with('$') {
        let named = aliases.get(raw).ok_or_else(|| format!("{raw} is not named by an earlier verb's as=\"{raw}\""))?;
        return pick(named).ok_or_else(|| format!("{raw} names no {what}"));
    }
    let id: NodeId = raw.parse().map_err(|_| format!("{raw:?} is not a node id"))?;
    doc.live(id).map(|_| id).ok_or_else(|| format!("{raw} is not a live node"))
}

fn name_of(doc: &crate::document::Document, id: NodeId) -> Option<String> {
    doc.get::<Name>(id)?.text.constant()?.as_str().map(str::to_owned)
}

/// The notations `def` has templates for.
fn notations(doc: &crate::document::Document, def: NodeId) -> Vec<String> {
    doc.children(def)
        .iter()
        .filter_map(|&c| doc.get::<crate::facet::Template>(c)?.notation.constant()?.as_str().map(str::to_owned))
        .collect()
}

/// The last authored child's order key under `parent`.
fn last(doc: &crate::document::Document, parent: NodeId) -> Option<crate::order::OrderKey> {
    doc.children(parent).iter().filter(|c| !c.is_expanded()).filter_map(|&c| doc.live(c)).map(|n| n.order.clone()).max()
}

/// One argument's slot: a `{= expr}` binding, or a constant of the kind
/// the input's default has.
/// `$self.k` in a binding is the entity being placed's argument `k`
/// (`#entity.args.k`), so one argument can read another (plot.md §10 Q4).
fn arg_slot(kind: ValueKind, raw: &str, entity: NodeId) -> Result<Slot, String> {
    let t = raw.trim();
    if let Some(e) = t.strip_prefix("{=").and_then(|r| r.strip_suffix('}')) {
        let e = e.trim().replace("$self.", &format!("{entity}.args."));
        return Expr::parse(&e).map(Slot::Bound).map_err(|err| format!("{raw:?}: {err}"));
    }
    let v = match kind {
        // One value is a series of one.
        ValueKind::Series => match Value::parse_series(t) {
            Some(s) => s.map_err(|e| format!("{raw:?}: {e}"))?,
            None => match Value::parse_quantity(t).ok().or_else(|| t.parse::<f64>().ok().map(Value::Number)).and_then(|v| v.as_quantity()) {
                Some((si, dim)) => Value::series(vec![si], dim),
                None => return Err(format!("{raw:?}: expected numbers, then an optional unit (400 450 500 nm), or {{= range(…)}}")),
            },
        },
        ValueKind::Enum => Value::Enum(t.to_owned()),
        ValueKind::Color => hex_color(t).ok_or_else(|| format!("{raw:?}: expected a colour, #rrggbb or #rrggbbaa, or {{= rgb(…)}}"))?,
        ValueKind::Number if t.eq_ignore_ascii_case("nan") || t == "auto" => Value::Number(f64::NAN),
        ValueKind::Number => Value::Number(t.parse().map_err(|_| format!("{raw:?}: expected a number"))?),
        ValueKind::Bool => Value::Bool(match t {
            "true" => true,
            "false" => false,
            _ => return Err(format!("{raw:?}: expected true or false")),
        }),
        ValueKind::Str => Value::Str(raw.to_owned()),
        ValueKind::Quantity => Value::parse_quantity(t)?,
        _ => Value::parse_quantity(t).or_else(|_| t.parse::<f64>().map(Value::Number)).unwrap_or_else(|_| Value::Str(raw.to_owned())),
    };
    Ok(Slot::Const(v))
}

/// `#rrggbb` or `#rrggbbaa`.
pub(crate) fn hex_color(t: &str) -> Option<Value> {
    let h = t.strip_prefix('#')?;
    if !matches!(h.len(), 6 | 8) || !h.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let ch = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok().map(|b| b as f32 / 255.0);
    Some(Value::Color([ch(0)?, ch(2)?, ch(4)?, if h.len() == 8 { ch(6)? } else { 1.0 }]))
}

fn place(store: &crate::Store, el: &El, aliases: &BTreeMap<String, Named>) -> Result<(Vec<Op>, String, Named), String> {
    let doc = store.document();
    let parent = {
        let raw = el.need("in")?;
        if raw.starts_with('#') { node(doc, raw, aliases, |_| None, "node")? } else { crate::parts::sheet_named(doc, raw)? }
    };
    let comp = composition::composition_of(doc, parent).ok_or_else(|| format!("{parent} is not in a composition"))?;
    if composition::dims(doc, comp) == Some(Dims::Three) {
        return Err(format!(
            "{comp} is a 3D composition; a component's picture cannot be placed in 3D yet. Picture the entity with a mesh in it and join them with <link-figures>"
        ));
    }
    let comp_notation = doc.get::<Composition>(comp).and_then(|c| c.notation.constant()).and_then(|v| v.as_str()).unwrap_or("free").to_owned();
    let notation = el.attr("notation").map_or(comp_notation, str::to_owned);
    let (x, y, rot) = (el.num("x")?, el.num("y")?, el.num("rot")?.to_radians());

    let mut ops = Vec::new();
    let mut scratch = doc.clone();
    let stamp = scratch.version().next();
    let mut push = |scratch: &mut crate::document::Document, new: Vec<Op>| -> Result<(), String> {
        for op in &new {
            crate::op::apply_op(scratch, 0, op, stamp).map_err(|e| e.message)?;
        }
        ops.extend(new);
        Ok(())
    };
    let root = |scratch: &mut crate::document::Document, push: &mut dyn FnMut(&mut crate::document::Document, Vec<Op>) -> Result<(), String>, t: &str| -> Result<NodeId, String> {
        if let Some(r) = crate::standard::root_of(scratch, t) {
            return Ok(r);
        }
        let r = scratch.next_id();
        let order = crate::standard::after_last(scratch, None);
        push(scratch, vec![Op::Create { id: r, ty: ty(t), parent: None, order, facets: vec![] }])?;
        Ok(r)
    };

    let (existing, def) = if let Some(raw) = el.attr("entity") {
        if el.attr("def").is_some() {
            return Err("give def (a new instance) or entity (another picture of one), not both".into());
        }
        let e = node(doc, raw, aliases, |n| n.entity, "entity")?;
        let def = crate::expand::linked_definition(doc, e).ok_or_else(|| {
            format!("{e} is not a linked instance of a definition; to picture a plain entity, draw the shape and join it with <link-figures>")
        })?;
        (Some(e), def)
    } else {
        let raw = el.need("def").map_err(|_| "<place> needs def (a definition, or std:<key>) or entity".to_owned())?;
        let def = if let Some(key) = raw.strip_prefix("std:") {
            match crate::standard::find(&scratch, key) {
                Some(d) => d,
                None => {
                    let library = root(&mut scratch, &mut push, "library")?;
                    let order = crate::standard::after_last(&scratch, Some(library));
                    let first = scratch.next_id().raw();
                    let (def_ops, d) = crate::standard::definition_ops(key, library, order, first).map_err(|e| {
                        format!("{e}; standard components: {}", crate::standard::ENTRIES.iter().map(|e| format!("std:{}", e.key)).collect::<Vec<_>>().join(", "))
                    })?;
                    push(&mut scratch, def_ops)?;
                    d
                }
            }
        } else {
            let d = node(doc, raw, aliases, |n| Some(n.definition), "definition")?;
            if doc.live(d).is_none_or(|n| n.ty.name() != "definition") {
                return Err(format!("{raw} is not a definition"));
            }
            d
        };
        (None, def)
    };
    if definition::template_of(&scratch, def, &notation).is_none() {
        let have = notations(&scratch, def);
        return Err(format!(
            "{def} has no template for notation {notation:?} (it has {}); write notation=\"…\" to use one of those, or create a template with that notation in the definition first",
            if have.is_empty() { "none".to_owned() } else { have.join(", ") }
        ));
    }

    let fresh = existing.is_none();
    let (entity, inst) = if let Some(entity) = existing {
        (entity, definition::place_figures(&scratch, entity, &notation, Some(parent), last(&scratch, parent))?)
    } else {
        let model = root(&mut scratch, &mut push, "model")?;
        let name = match el.attr("name") {
            Some(n) => n.to_owned(),
            None => name_of(&scratch, def).unwrap_or_else(|| "component".into()),
        };
        let place = Placement { entity_parent: Some(model), entity_after: last(&scratch, model), figure_parent: Some(parent), figure_after: last(&scratch, parent) };
        definition::instantiate(&scratch, def, &notation, &name, &place)?
    };
    // A sized picture's box: `w`/`h` (unless the definition has inputs of
    // those names, which they then set) or `size.w`/`size.h`.
    let input_names: Vec<&str> = definition::inputs_of(&scratch, def)?.iter().map(|i| i.name).collect();
    let size_word = |k: &str| -> Option<&str> {
        if input_names.contains(&k) {
            return None;
        }
        el.attr(k)
    };
    let box_w = el.attr("size.w").or_else(|| size_word("w"));
    let box_h = el.attr("size.h").or_else(|| size_word("h"));
    let as_size = |k: &str, v: Option<&str>| -> Result<Option<f64>, String> {
        v.map(|v| v.trim().trim_end_matches("px").trim().parse::<f64>().map_err(|_| format!("{k}={v:?} is not a number"))).transpose()
    };
    let (box_w, box_h) = (as_size("w", box_w)?, as_size("h", box_h)?);
    let mut sized = false;
    let mut inst = inst;
    let mut picture = None;
    for op in inst.iter_mut() {
        let Op::Create { id, ty: t, facets, .. } = op else { continue };
        if t.name() != "use" {
            continue;
        }
        picture = Some(*id);
        for f in facets.iter_mut() {
            if let Facet::Transform2d(tr) = f {
                tr.x = Slot::Const(Value::Number(x));
                tr.y = Slot::Const(Value::Number(y));
                tr.rot = Slot::Const(Value::Number(rot));
            }
            if let Facet::Size(s) = f {
                sized = true;
                if let Some(w) = box_w {
                    s.w = Slot::Const(Value::Number(w));
                }
                if let Some(h) = box_h {
                    s.h = Slot::Const(Value::Number(h));
                }
            }
        }
    }
    if !sized && (box_w.is_some() || box_h.is_some()) {
        return Err(format!("{def}'s template has no size, so its picture cannot be given w or h; give the template a size (size.w, size.h) and bind its parts to it"));
    }
    let picture = picture.ok_or("no picture placed")?;
    push(&mut scratch, inst)?;

    // Arguments: `args.k` or a bare `k`, each an input of the definition.
    let inputs = definition::inputs_of(&scratch, def)?;
    let mut sets = Vec::new();
    for (k, v) in &el.attrs {
        if PLACE_ATTRS.contains(&k.as_str()) || (matches!(k.as_str(), "w" | "h") && !input_names.contains(&k.as_str())) {
            continue;
        }
        let name = k.strip_prefix("args.").unwrap_or(k);
        let input = inputs.iter().find(|i| i.name == name).ok_or_else(|| {
            let declared: Vec<&str> = inputs.iter().map(|i| i.name).collect();
            format!("{def} has no input {name:?} ({})", if declared.is_empty() { "it has none".to_owned() } else { declared.join(", ") })
        })?;
        if !fresh {
            return Err(format!("{name}=…: set an existing entity's arguments with <set id=\"{entity}\" args.{name}=…/>"));
        }
        // The declared type says how to read the text (types-and-units.md);
        // a NaN default ("auto") takes a quantity as well as a number.
        use crate::types::Base;
        let declared = match &input.ty.base {
            _ if v.trim() == "auto" && input.ty.auto => None,
            Base::Choice(_) => Some(ValueKind::Enum),
            Base::Color => Some(ValueKind::Color),
            Base::Text => Some(ValueKind::Str),
            Base::Bool => Some(ValueKind::Bool),
            Base::Series(_) => Some(ValueKind::Series),
            Base::Quantity { .. } | Base::Like(_) if Value::parse_quantity(v.trim()).is_ok() => Some(ValueKind::Quantity),
            _ => None,
        };
        // A colour token (`@series-2`) is a binding to it.
        if v.trim().starts_with('@') {
            let slot = Expr::parse(v.trim()).map(Slot::Bound).map_err(|e| format!("{name}: {e}"))?;
            sets.push(Op::Set { id: entity, path: format!("args.{name}").parse().map_err(|e| format!("args.{name}: {e:?}"))?, slot });
            continue;
        }
        let kind = declared.unwrap_or(if matches!(input.default, Value::Number(x) if x.is_nan()) && Value::parse_quantity(v.trim()).is_ok() { ValueKind::Quantity } else { input.kind });
        let slot = arg_slot(kind, v, entity).map_err(|e| format!("{name}: {e}"))?;
        sets.push(Op::Set { id: entity, path: format!("args.{name}").parse().map_err(|e| format!("args.{name}: {e:?}"))?, slot });
    }
    push(&mut scratch, sets)?;

    let line = format!("  <placed entity=\"{entity}\" use=\"{picture}\" in=\"{comp}\" definition=\"{def}\" notation=\"{notation}\"/>");
    Ok((ops, line, Named { definition: def, entity: Some(entity), picture }))
}

fn make(store: &crate::Store, el: &El, aliases: &BTreeMap<String, Named>) -> Result<(Vec<Op>, String, Named), String> {
    let doc = store.document();
    let of = el.need("of")?;
    let selection = of.split_whitespace().map(|raw| node(doc, raw, aliases, |n| Some(n.picture), "picture")).collect::<Result<Vec<_>, _>>()?;
    if selection.is_empty() {
        return Err("of=\"…\" lists no nodes".into());
    }
    let name = el.attr("name").filter(|n| !n.trim().is_empty()).unwrap_or("component");
    let (made, tx) = store.make_component(&selection, name, crate::Actor::Model)?;
    // From plain shapes the picture is the definition's main, which figures
    // no entity; from instances it is the new instance's use.
    let main = made.entity == made.picture;
    let line = if main {
        format!("  <made definition=\"{}\" main=\"{}\"/>", made.definition, made.picture)
    } else {
        format!("  <made definition=\"{}\" entity=\"{}\" use=\"{}\"/>", made.definition, made.entity, made.picture)
    };
    Ok((tx.ops, line, Named { definition: made.definition, entity: (!main).then_some(made.entity), picture: made.picture }))
}
