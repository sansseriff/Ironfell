//! Lower a typeset equation into ordinary groups, state cells and packed paths.
use crate::facet::{Appearance, Facet, FacetKind, LeafStruct, Let, Name, PathShape, Transform2d};
use crate::geometry::Geometry;
use crate::markup::{self, Names};
use crate::parts::El;
use crate::value::{Slot, Value, ValueKind};
use crate::{NodeId, Op, OrderKey, Tree, TypeId};
use std::collections::BTreeMap;
use std::sync::Arc;

pub const VERB: &str = "typeset-math";
const SOURCE: &str = "LaTeX source";
const SIZE: &str = "LaTeX size";
const FONT: &str = "LaTeX font";
const CONTENT: &str = "LaTeX outlines";
const KEY: &str = "LaTeX part key";
const COLOR: &str = "LaTeX color";
const DRAW: &str = "LaTeX draw";
const GLYPH: &str = "LaTeX glyph";

pub struct MathPath {
    pub part: Option<String>,
    pub geometry: Geometry,
    pub even_odd: bool,
}

pub(crate) type Aliases = BTreeMap<String, (NodeId, TypeId)>;

pub(crate) enum Target {
    New { parent: NodeId, order: OrderKey },
    Existing(NodeId),
}

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).expect("built-in type")
}

pub fn valid_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
}

fn has_name(doc: Tree<'_>, id: NodeId, name: &str) -> bool {
    doc.get::<Name>(id)
        .and_then(|n| n.text.constant())
        .and_then(Value::as_str)
        == Some(name)
}

fn named_child(doc: Tree<'_>, parent: NodeId, kind: TypeId, name: &str) -> Option<NodeId> {
    doc.children(parent)
        .iter()
        .copied()
        .find(|&id| doc.live(id).is_some_and(|node| node.ty == kind) && has_name(doc, id, name))
}

fn cell(doc: Tree<'_>, parent: NodeId, name: &str) -> Result<NodeId, String> {
    named_child(doc, parent, ty("let"), name).ok_or_else(|| format!("{parent} has no {name}"))
}

fn value(doc: Tree<'_>, id: NodeId) -> Option<&Value> {
    doc.get::<Let>(id)?.value.constant()
}

pub(crate) fn source_and_font<'a>(
    doc: Tree<'a>,
    el: &'a El,
    existing: Option<NodeId>,
) -> Result<(&'a str, &'a str), String> {
    let saved = |name| {
        existing
            .and_then(|root| named_child(doc, root, ty("let"), name))
            .and_then(|id| value(doc, id))
            .and_then(Value::as_str)
    };
    let source = el
        .attr("source")
        .or_else(|| saved(SOURCE))
        .ok_or("<typeset-math> needs source")?;
    let font = el.attr("font").or_else(|| saved(FONT)).unwrap_or("tex");
    if !matches!(font, "tex" | "fira") {
        return Err(format!("unknown LaTeX font {font:?}; use tex or fira"));
    }
    Ok((source, font))
}

fn name_facet(name: &str) -> Facet {
    Name {
        text: Slot::Const(Value::Str(name.into())),
    }
    .wrap()
}

struct Builder<'a> {
    doc: Tree<'a>,
    names: &'a Names<'a>,
    next: NodeId,
    orders: BTreeMap<NodeId, OrderKey>,
    aliases: Aliases,
    ops: Vec<Op>,
}

impl Builder<'_> {
    fn order(&mut self, parent: NodeId) -> OrderKey {
        let next = self.orders.entry(parent).or_insert_with(|| {
            self.doc
                .children(parent)
                .iter()
                .filter_map(|&id| self.doc.live(id))
                .map(|node| &node.order)
                .max()
                .map_or_else(OrderKey::first, OrderKey::after)
        });
        let order = next.clone();
        *next = OrderKey::after(next);
        order
    }

    fn create(
        &mut self,
        kind: TypeId,
        parent: NodeId,
        order: OrderKey,
        facets: Vec<Facet>,
    ) -> NodeId {
        let id = self.next;
        self.next = id.offset(1);
        self.ops.push(Op::Create {
            id,
            ty: kind,
            parent: Some(parent),
            order,
            facets,
        });
        id
    }

    fn append(&mut self, kind: TypeId, parent: NodeId, facets: Vec<Facet>) -> NodeId {
        let order = self.order(parent);
        self.create(kind, parent, order, facets)
    }

    fn alias(&mut self, el: &El, id: NodeId) -> Result<(), String> {
        if let Some(alias) = markup::local_name(el)? {
            if self.names.node(alias).is_some()
                || self
                    .aliases
                    .insert(alias.into(), (id, ty("group")))
                    .is_some()
            {
                return Err(format!("id ${alias} names two nodes"));
            }
        }
        Ok(())
    }

    fn new_cell(&mut self, parent: NodeId, name: &str, value: Slot) -> NodeId {
        self.append(
            ty("let"),
            parent,
            vec![
                name_facet(name),
                Let {
                    value,
                    ..Let::default()
                }
                .wrap(),
            ],
        )
    }

    fn set_cell(&mut self, id: NodeId, slot: Slot) {
        self.ops.push(Op::Set {
            id,
            path: "let.value".parse().unwrap(),
            slot,
        });
    }

    fn control(
        &mut self,
        group: NodeId,
        el: &El,
        attr: &str,
        name: &str,
        kind: ValueKind,
        default: String,
    ) -> Result<NodeId, String> {
        let raw = markup::leaf_attrs(el, &[])
            .into_iter()
            .find(|(key, _)| key == attr)
            .map(|(_, raw)| raw);
        let old = named_child(self.doc, group, ty("let"), name);
        match (raw, old) {
            (None, Some(id)) => Ok(id),
            (raw, old) => {
                let slot = markup::slot(kind, raw.as_deref().unwrap_or(&default), self.names)
                    .map_err(|error| format!("{attr}: {error}"))?;
                if let Some(id) = old {
                    self.set_cell(id, slot);
                    Ok(id)
                } else {
                    Ok(self.new_cell(group, name, slot))
                }
            }
        }
    }

    fn controls(
        &mut self,
        group: NodeId,
        el: &El,
        inherit: Option<(NodeId, NodeId)>,
    ) -> Result<(NodeId, NodeId), String> {
        let default = |id: Option<NodeId>, fallback: &str| {
            id.map_or_else(
                || fallback.to_owned(),
                |id| format!("{{= {id}.let.value }}"),
            )
        };
        let color = self.control(
            group,
            el,
            "fill.color",
            COLOR,
            ValueKind::Color,
            default(inherit.map(|p| p.0), "@ink"),
        )?;
        let draw = self.control(
            group,
            el,
            "draw",
            DRAW,
            ValueKind::Number,
            default(inherit.map(|p| p.1), "1"),
        )?;
        Ok((color, draw))
    }

    fn group_facets(
        &self,
        el: &El,
        default_name: &str,
        skip: &[&str],
    ) -> Result<Vec<Facet>, String> {
        let mut group = El {
            name: "group".into(),
            ..El::default()
        };
        group.attrs.extend(
            el.attrs
                .iter()
                .filter(|(key, _)| !skip.contains(&key.strip_suffix(".bind").unwrap_or(key)))
                .cloned(),
        );
        if group.attr("name").is_none() {
            group.attrs.push(("name".into(), default_name.into()));
        }
        let mut facets = markup::facets(&group, ty("group"), self.names)?;
        if !facets
            .iter()
            .any(|facet| facet.kind() == FacetKind::Transform2d)
        {
            facets.push(Transform2d::default().wrap());
        }
        if !facets
            .iter()
            .any(|facet| facet.kind() == FacetKind::Appearance)
        {
            facets.push(Appearance::default().wrap());
        }
        Ok(facets)
    }

    fn clear_paths(&mut self, parent: NodeId) {
        self.ops.extend(
            self.doc
                .children(parent)
                .iter()
                .copied()
                .filter(|&id| {
                    self.doc.live(id).is_some_and(|node| node.ty == ty("path"))
                        && has_name(self.doc, id, GLYPH)
                })
                .map(|id| Op::Delete { id }),
        );
    }

    fn glyph(
        &mut self,
        parent: NodeId,
        path: MathPath,
        controls: (NodeId, NodeId),
        progress: (usize, usize),
        size: f64,
    ) -> Result<(), String> {
        let (color, draw) = controls;
        let (index, count) = progress;
        // Each glyph overlaps the preceding one. The fill always paints its
        // intact contours, so holes survive every stage of the reveal.
        let step = 0.65 / count as f64;
        let local = format!(
            "window({draw}.let.value, {}, {})",
            index as f64 * step,
            if index + 1 == count {
                1.0
            } else {
                0.35 + (index + 1) as f64 * step
            }
        );
        let fill = format!("window({local}, 0.65, 1)");
        let el = El {
            name: "path".into(),
            attrs: vec![
                ("name".into(), GLYPH.into()),
                ("fill.color.bind".into(), format!("{color}.let.value")),
                ("fill.opacity.bind".into(), fill.clone()),
                ("trim.end.bind".into(), format!("window({local}, 0, 0.65)")),
                ("trim.mode".into(), "individually".into()),
                ("stroke.color.bind".into(), format!("{color}.let.value")),
                (
                    "stroke.width.bind".into(),
                    format!("{} * window({local}, 0, 0.01) * (1 - {fill})", size * 0.018),
                ),
                ("stroke.join".into(), "round".into()),
                ("stroke.cap".into(), "round".into()),
            ],
            ..El::default()
        };
        let mut facets = markup::facets(&el, ty("path"), self.names)?;
        let shape = facets
            .iter_mut()
            .find(|f| matches!(f, Facet::PathShape(_)))
            .expect("required path facet");
        *shape = PathShape {
            d: Slot::Const(Value::Geometry(Arc::new(path.geometry))),
            fill_rule: Slot::Const(Value::Enum(
                if path.even_odd { "evenodd" } else { "nonzero" }.into(),
            )),
        }
        .wrap();
        self.append(ty("path"), parent, facets);
        Ok(())
    }
}

pub(crate) fn build(
    doc: Tree<'_>,
    names: &Names<'_>,
    next: NodeId,
    el: &El,
    target: Target,
    typeset: impl FnOnce(&str, &str, f64) -> Result<Vec<MathPath>, String>,
) -> Result<(Vec<Op>, Aliases), String> {
    let existing = match target {
        Target::Existing(root) => Some(root),
        Target::New { .. } => None,
    };
    let (source, font) = source_and_font(doc, el, existing)?;
    if !el.text.trim().is_empty() {
        return Err("give LaTeX as a source attribute".into());
    }
    let mut specs = BTreeMap::new();
    for part in &el.children {
        if part.name != "part" || !part.children.is_empty() || !part.text.trim().is_empty() {
            return Err("<typeset-math> children must be empty <part> elements".into());
        }
        let key = part.need("key")?;
        if !valid_key(key) || specs.insert(key, part).is_some() {
            return Err(format!("invalid or duplicate LaTeX part key {key:?}"));
        }
    }

    let old_size = match target {
        Target::Existing(root) => Some(cell(doc, root, SIZE)?),
        Target::New { .. } => None,
    };
    let size = match el.attr("size") {
        Some(_) => el.num("size")?,
        None => old_size
            .and_then(|id| value(doc, id))
            .and_then(Value::as_f64)
            .unwrap_or(48.0),
    };
    if !size.is_finite() || !(0.5..=4096.0).contains(&size) {
        return Err("LaTeX size must be between 0.5 and 4096 px".into());
    }
    let paths = typeset(source, font, size)?;
    let mut counts = BTreeMap::new();
    for path in &paths {
        *counts.entry(path.part.clone()).or_insert(0) += 1;
    }
    for key in specs.keys() {
        if !counts.contains_key(&Some((*key).to_owned())) {
            return Err(format!("LaTeX has no visible part marked {key:?}"));
        }
    }

    let mut b = Builder {
        doc,
        names,
        next,
        orders: BTreeMap::new(),
        aliases: BTreeMap::new(),
        ops: Vec::new(),
    };
    let source = Slot::Const(Value::Str(source.into()));
    let font = Slot::Const(Value::Str(font.into()));
    let size_value = Slot::Const(Value::Number(size));
    let (root, content) = match target {
        Target::New { parent, order } => {
            let facets = b.group_facets(
                el,
                "Equation",
                &["source", "font", "size", "fill.color", "draw"],
            )?;
            let root = b.create(ty("group"), parent, order, facets);
            b.alias(el, root)?;
            b.new_cell(root, SOURCE, source);
            b.new_cell(root, SIZE, size_value);
            b.new_cell(root, FONT, font);
            let content = b.append(ty("group"), root, vec![name_facet(CONTENT)]);
            (root, content)
        }
        Target::Existing(root) => {
            for (attr, _) in &el.attrs {
                if !matches!(
                    attr.as_str(),
                    "id" | "source"
                        | "font"
                        | "size"
                        | "fill.color"
                        | "fill.color.bind"
                        | "draw"
                        | "draw.bind"
                ) {
                    return Err(format!("use <set> to edit an existing equation's {attr}"));
                }
            }
            let content = named_child(doc, root, ty("group"), CONTENT)
                .ok_or("expected a group made by <typeset-math>")?;
            b.set_cell(cell(doc, root, SOURCE)?, source);
            b.set_cell(old_size.expect("existing equation size"), size_value);
            b.set_cell(cell(doc, root, FONT)?, font);
            (root, content)
        }
    };
    let root_controls = b.controls(root, el, None)?;
    let mut parts = BTreeMap::new();
    b.clear_paths(content);
    for &group in doc.children(content) {
        if let Some(key) = named_child(doc, group, ty("let"), KEY)
            .and_then(|id| value(doc, id))
            .and_then(Value::as_str)
        {
            parts.insert(key.to_owned(), group);
            b.clear_paths(group);
        }
    }

    let mut used = BTreeMap::new();
    let mut unmarked = 0;
    for path in paths {
        let Some(key) = path.part.clone() else {
            b.glyph(
                content,
                path,
                root_controls,
                (unmarked, counts[&None]),
                size,
            )?;
            unmarked += 1;
            continue;
        };
        let (group, controls, ordinal) =
            if let Some((group, controls, ordinal)) = used.get_mut(&key) {
                *ordinal += 1;
                (*group, *controls, *ordinal)
            } else {
                let default = El::default();
                let spec = specs.get(key.as_str()).copied().unwrap_or(&default);
                let group = if let Some(&group) = parts.get(&key) {
                    if spec.attrs.iter().any(|(attr, _)| {
                        !matches!(
                            attr.as_str(),
                            "id" | "key" | "fill.color" | "fill.color.bind" | "draw" | "draw.bind"
                        )
                    }) {
                        return Err(format!(
                            "use <set> to change the existing group for LaTeX part {key:?}"
                        ));
                    }
                    let order = b.order(content);
                    b.ops.push(Op::Reorder { id: group, order });
                    group
                } else {
                    let facets = b.group_facets(spec, &key, &["key", "fill.color", "draw"])?;
                    let group = b.append(ty("group"), content, facets);
                    b.new_cell(group, KEY, Slot::Const(Value::Str(key.clone())));
                    group
                };
                b.alias(spec, group)?;
                let controls = b.controls(group, spec, Some(root_controls))?;
                used.insert(key.clone(), (group, controls, 0));
                (group, controls, 0)
            };
        b.glyph(group, path, controls, (ordinal, counts[&Some(key)]), size)?;
    }
    Ok((b.ops, b.aliases))
}
