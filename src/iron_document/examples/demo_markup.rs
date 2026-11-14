//! Writes one composition of a saved document as an edit (plans/specs/
//! markup.md), the form the demo's gallery is seeded from
//! (`src/document_bridge/gallery/*.xml`). Leaves at their facet's default
//! are left out; ids become `$names`; a `use` of a standard component
//! becomes a `<place def="std:…">` with its entity's arguments; a relation
//! from a node of the composition becomes a `<link>` after it.
//!
//! `cargo run -p iron_document --target aarch64-apple-darwin --example demo_markup -- doc.json <composition>`
//!
//! The saved document is what `__iron.save()` returns (the agent probe
//! writes one per turn, `turn-N-after.json`).

use iron_document::facet::{Facet, FacetKind};
use iron_document::*;
use std::collections::BTreeMap;
use std::fmt::Write as _;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, file, name] = args.as_slice() else {
        eprintln!("usage: demo_markup <saved.json> <composition name>");
        std::process::exit(2);
    };
    let text = std::fs::read_to_string(file).expect("read the saved document");
    let store = Store::load(&text).expect("load the saved document");
    let doc = store.document();
    let comp = doc.roots().iter().copied().find(|&r| doc.live(r).is_some_and(|n| n.ty.name() == "composition") && name_of(doc, r).as_deref() == Some(name.as_str())).unwrap_or_else(|| {
        eprintln!("no composition named {name}");
        std::process::exit(1)
    });
    // Every node of the composition, and the local name each is written by.
    let mut all = Vec::new();
    walk(doc, comp, &mut all);
    let mut locals: BTreeMap<NodeId, String> = BTreeMap::new();
    let mut taken = std::collections::BTreeSet::new();
    for &id in &all {
        let base: String = name_of(doc, id).unwrap_or_else(|| doc.live(id).map_or("node", |n| n.ty.name()).to_owned()).chars().map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '_' }).collect();
        let base = base.trim_matches('_').to_owned();
        let base = if base.is_empty() || base.starts_with(|c: char| c.is_ascii_digit()) { format!("n_{base}") } else { base };
        let mut local = base.clone();
        let mut i = 2;
        while !taken.insert(local.clone()) {
            local = format!("{base}_{i}");
            i += 1;
        }
        locals.insert(id, local);
    }
    let mut w = Writer { doc, locals: &locals, referenced: Default::default(), warnings: Vec::new(), links: Vec::new() };
    let body = w.node(comp, 1);
    let body = format!("<edit>\n{body}{}</edit>\n", w.links.iter().map(|l| format!("  {l}\n")).collect::<String>());
    // Only nodes something refers to keep their id= (written after the fact).
    let mut out = String::new();
    for line in body.lines() {
        let mut line = line.to_owned();
        if let Some(at) = line.find(" __ID__=\"") {
            let end = line[at + 9..].find('"').expect("closed") + at + 9;
            let local = line[at + 9..end].to_owned();
            line.replace_range(at..=end, &if w.referenced.contains(&local) { format!(" id=\"${local}\"") } else { String::new() });
        }
        out.push_str(&line);
        out.push('\n');
    }
    print!("{out}");
    for warning in &w.warnings {
        eprintln!("warning: {warning}");
    }
}

fn name_of(doc: &Document, id: NodeId) -> Option<String> {
    doc.get::<facet::Name>(id)?.text.constant()?.as_str().map(str::to_owned)
}

fn walk(doc: &Document, id: NodeId, out: &mut Vec<NodeId>) {
    out.push(id);
    for &c in doc.children(id) {
        if !c.is_expanded() && doc.live(c).is_some() {
            walk(doc, c, out);
        }
    }
}

struct Writer<'a> {
    doc: &'a Document,
    locals: &'a BTreeMap<NodeId, String>,
    referenced: std::collections::BTreeSet<String>,
    warnings: Vec<String>,
    /// Relations from the composition's nodes, written after it.
    links: Vec<String>,
}

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\n', "&#10;")
}

impl Writer<'_> {
    /// `#id` in an expression's text as the local `$name`; `#entity.args.k`
    /// of the entity being placed as `$self.k`.
    fn expr(&mut self, text: &str, entity: Option<NodeId>) -> String {
        let mut out = String::new();
        let mut rest = text;
        while let Some(i) = rest.find('#') {
            out.push_str(&rest[..i]);
            let after = &rest[i + 1..];
            let end = after.find(|c: char| !c.is_ascii_alphanumeric() && c != '/').unwrap_or(after.len());
            let raw = &rest[i..i + 1 + end];
            match raw.parse::<NodeId>() {
                Ok(id) if Some(id) == entity && after[end..].starts_with(".args.") => {
                    out.push_str("$self.");
                    rest = &after[end + ".args.".len()..];
                    continue;
                }
                // An output port of a placed component, as `$place.port`.
                Ok(id) if id.expanded_parts().is_some_and(|(owner, _)| self.placed(owner).is_some()) && after[end..].starts_with(".port.signal") => {
                    let (owner, spec) = id.expanded_parts().expect("expanded");
                    let place = self.placed(owner).expect("placed");
                    let local = self.locals[&place].clone();
                    self.referenced.insert(local.clone());
                    let port = name_of(self.doc, spec).unwrap_or_else(|| spec.to_string());
                    let _ = write!(out, "${local}.{port}");
                    rest = &after[end + ".port.signal".len()..];
                    continue;
                }
                Ok(id) if self.locals.contains_key(&id) => {
                    let local = self.locals[&id].clone();
                    self.referenced.insert(local.clone());
                    out.push('$');
                    out.push_str(&local);
                }
                Ok(id) => {
                    self.warnings.push(format!("{raw} is outside the composition (in {text:?})"));
                    out.push_str(&id.to_string());
                }
                Err(_) => out.push_str(raw),
            }
            rest = &after[end..];
        }
        out.push_str(rest);
        out
    }

    /// The `use` in this composition that places `entity`.
    fn placed(&self, entity: NodeId) -> Option<NodeId> {
        self.locals.keys().copied().find(|&u| self.doc.live(u).is_some_and(|n| n.ty.name() == "use") && self.doc.relations_from(u).any(|r| r.rel == RelKind::Figures && r.to == entity))
    }

    fn value(&mut self, v: &Value) -> String {
        match v {
            Value::Ref(id) => self.expr(&id.to_string(), None),
            Value::Geometry(g) => svg_d(g),
            Value::Vec2([x, y]) => format!("{x},{y}"),
            Value::Vec3([x, y, z]) => format!("{x},{y},{z}"),
            other if other.kind() == ValueKind::Series => {
                let unit = other.to_string();
                self.warnings.push(format!("a constant series is written in full ({unit})"));
                other.elements().unwrap_or_default().iter().map(ToString::to_string).collect::<Vec<_>>().join(" ")
            }
            other => other.to_string(),
        }
    }

    fn slot(&mut self, slot: &Slot, entity: Option<NodeId>) -> String {
        match slot {
            Slot::Const(v) => self.value(v),
            Slot::Bound(e) => {
                let text = e.to_string();
                // A colour token alone is written as CSS writes a variable.
                if text.starts_with('@') && !text.contains(' ') { text } else { format!("{{= {}}}", self.expr(&text, entity)) }
            }
            other => {
                self.warnings.push(format!("unwritten slot {other:?}"));
                String::new()
            }
        }
    }

    /// A facet's leaves that differ from its default, as attributes.
    fn attrs(&mut self, f: &Facet, entity: Option<NodeId>) -> Vec<(String, String)> {
        let mut out = Vec::new();
        if let Facet::Name(n) = f {
            if let Some(Value::Str(s)) = n.text.constant() {
                out.push(("name".into(), s.clone()));
            }
            return out;
        }
        let default = f.kind().default_facet();
        for field in f.field_names() {
            let Some(slot) = f.get(&field) else { continue };
            if f.kind() != FacetKind::Args && default.get(&field) == Some(slot) {
                continue;
            }
            let key = if f.kind() == FacetKind::Args { field.to_string() } else { format!("{}.{}", f.kind().name(), field) };
            out.push((key, self.slot(slot, entity)));
        }
        out
    }

    fn node(&mut self, id: NodeId, depth: usize) -> String {
        let doc = self.doc;
        let node = doc.live(id).expect("live");
        let pad = "  ".repeat(depth);
        let mut s = String::new();
        if node.ty.name() == "use" {
            return self.place(id, depth);
        }
        let _ = write!(s, "{pad}<{} __ID__=\"{}\"", node.ty.name(), self.locals[&id]);
        let facets: Vec<Facet> = doc.facets_of(id).cloned().collect();
        for f in &facets {
            for (k, v) in self.attrs(f, None) {
                let _ = write!(s, " {k}=\"{}\"", escape(&v));
            }
        }
        for r in doc.relations_from(id) {
            let end = |n: NodeId, w: &mut Self| match w.locals.get(&n) {
                Some(l) => {
                    w.referenced.insert(l.clone());
                    format!("${l}")
                }
                None => n.to_string(),
            };
            let (from, to) = (end(id, self), end(r.to, self));
            let data: String = r.data.iter().map(|(k, v)| format!(" {k}=\"{}\"", escape(&iron_document::text::display(v)))).collect();
            self.links.push(format!("<link from=\"{from}\" to=\"{to}\" rel=\"{}\"{data}/>", r.rel.name()));
        }
        let kids: Vec<NodeId> = doc.children(id).iter().copied().filter(|c| !c.is_expanded() && doc.live(*c).is_some()).collect();
        if kids.is_empty() {
            s.push_str("/>\n");
        } else {
            s.push_str(">\n");
            for c in kids {
                s.push_str(&self.node(c, depth + 1));
            }
            let _ = writeln!(s, "{pad}</{}>", node.ty.name());
        }
        s
    }

    /// A `use` as the `<place>` that makes it.
    fn place(&mut self, id: NodeId, depth: usize) -> String {
        let doc = self.doc;
        let pad = "  ".repeat(depth);
        let Some(entity) = doc.relations_from(id).find(|r| r.rel == RelKind::Figures).map(|r| r.to) else {
            self.warnings.push(format!("{id}: a use with no entity"));
            return String::new();
        };
        let def = doc.get::<facet::Instance>(entity).and_then(|i| i.def.constant().cloned());
        let Some(Value::Ref(def)) = def else {
            self.warnings.push(format!("{id}: its entity has no definition"));
            return String::new();
        };
        let source = doc.get::<facet::Import>(def).and_then(|i| i.source.constant()).and_then(|v| v.as_str().map(str::to_owned));
        let Some(source) = source.filter(|s| s.starts_with("std:")) else {
            self.warnings.push(format!("{id}: places {def}, which is not a standard component; write its definition by hand"));
            return String::new();
        };
        let mut s = format!("{pad}<place __ID__=\"{}\" def=\"{source}\"", self.locals[&id]);
        if let Some(n) = name_of(doc, entity) {
            let _ = write!(s, " name=\"{}\"", escape(&n));
        }
        if let Some(t) = doc.get::<facet::Transform2d>(id) {
            for (k, slot, scale) in [("x", &t.x, 1.0), ("y", &t.y, 1.0), ("rot", &t.rot, 180.0 / std::f64::consts::PI)] {
                if let Some(Value::Number(v)) = slot.constant()
                    && (*v != 0.0 || k != "rot")
                {
                    let _ = write!(s, " {k}=\"{}\"", v * scale);
                }
            }
        }
        if let Some(sz) = doc.get::<facet::Size>(id) {
            for (k, slot) in [("w", &sz.w), ("h", &sz.h)] {
                if let Some(Value::Number(v)) = slot.constant() {
                    let _ = write!(s, " {k}=\"{v}\"");
                }
            }
        }
        // An argument at its input's default is left out.
        let defaults: BTreeMap<String, Slot> = doc
            .children(def)
            .iter()
            .filter_map(|&c| Some((name_of(doc, c)?, doc.get::<facet::Port>(c)?.signal.clone())))
            .collect();
        if let Some(mut args) = doc.facets_of(entity).find(|f| f.kind() == FacetKind::Args).cloned() {
            let names: Vec<String> = args.field_names().into_iter().map(|k| k.to_string()).collect();
            if let Facet::Args(a) = &mut args {
                let same = |k: &str, v: &Slot| match (defaults.get(k), v) {
                    (Some(Slot::Const(Value::Number(d))), Slot::Const(Value::Number(n))) => d == n || (d.is_nan() && n.is_nan()),
                    (Some(Slot::Const(Value::Str(d) | Value::Enum(d))), Slot::Const(Value::Str(n) | Value::Enum(n))) => d == n,
                    (Some(d), v) => d == v,
                    _ => false,
                };
                let keep: Vec<(String, Slot)> = names.iter().filter_map(|k| a.get(k).filter(|v| !same(k, v)).map(|v| (k.clone(), v.clone()))).collect();
                let mut fresh = facet::Args::default();
                for (k, v) in keep {
                    fresh.insert(&k, v);
                }
                **a = fresh;
            }
            for (k, v) in self.attrs(&args, Some(entity)) {
                let _ = write!(s, " {k}=\"{}\"", escape(&v));
            }
        }
        s.push_str("/>\n");
        s
    }
}

/// SVG path data for packed contours: each anchor a cubic from the last.
fn svg_d(g: &geometry::Geometry) -> String {
    let mut s = String::new();
    let num = |v: f64| {
        let r = (v * 1000.0).round() / 1000.0;
        if r == r.trunc() { format!("{}", r as i64) } else { format!("{r}") }
    };
    type A = ([f64; 2], [f64; 2], [f64; 2]);
    for c in g.contours() {
        let a: Vec<A> = c.anchors.iter().map(|a| ([a.at.x, a.at.y], [a.inn.x, a.inn.y], [a.out.x, a.out.y])).collect();
        let Some(first) = a.first() else { continue };
        let _ = write!(s, "M {} {}", num(first.0[0]), num(first.0[1]));
        let seg = |s: &mut String, p: &A, q: &A| {
            if p.2 == [0.0, 0.0] && q.1 == [0.0, 0.0] {
                let _ = write!(s, " L {} {}", num(q.0[0]), num(q.0[1]));
            } else {
                let _ = write!(s, " C {} {} {} {} {} {}", num(p.0[0] + p.2[0]), num(p.0[1] + p.2[1]), num(q.0[0] + q.1[0]), num(q.0[1] + q.1[1]), num(q.0[0]), num(q.0[1]));
            }
        };
        for w in a.windows(2) {
            seg(&mut s, &w[0], &w[1]);
        }
        if c.closed && a.len() > 2 {
            seg(&mut s, &a[a.len() - 1], &a[0]);
            s.push_str(" Z");
        }
        s.push(' ');
    }
    s.trim_end().to_owned()
}
