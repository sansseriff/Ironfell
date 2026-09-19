//! Views: typed, elided reads of the document for a model or a host
//! (plans/document-spine.md §8; doc 04 §6).
//!
//! Only the tree relation exists in slice one. Output is XML-ish because
//! nesting is the data. Elision is always marked: a reader must be able to
//! tell "empty" from "not shown".

use crate::component::{Component, ComponentKind};
use crate::document::Document;
use crate::id::NodeId;
use serde::{Deserialize, Serialize};
use std::fmt::Write;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fidelity {
    /// Types, ids, names and counts. No leaves.
    Skeleton,
    /// Leaves, but large child lists are elided to a count and two samples.
    Summary,
    /// Everything to the depth limit.
    Full,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewQuery {
    /// `None` renders every root.
    #[serde(default)]
    pub scope: Option<NodeId>,
    pub fidelity: Fidelity,
    /// Levels below the scope to descend. `None` is unlimited.
    #[serde(default)]
    pub depth: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ViewError(pub String);

impl std::fmt::Display for ViewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for ViewError {}

/// Child lists longer than this are elided at summary fidelity.
const SUMMARY_MAX_CHILDREN: usize = 8;
const SUMMARY_SAMPLES: usize = 2;

pub fn render_tree(
    doc: &Document,
    reactive: &crate::graph::Reactive,
    q: &ViewQuery,
) -> Result<String, ViewError> {
    let mut out = String::new();
    writeln!(out, "<document version=\"{}\">", doc.version().0).expect("string write");
    let roots: Vec<NodeId> = match q.scope {
        Some(id) => {
            if !doc.is_live(id) {
                return Err(ViewError(format!("{id} is not a live node")));
            }
            vec![id]
        }
        None => doc.roots().to_vec(),
    };
    for id in roots {
        node(doc, reactive, q, id, 0, &mut out);
    }
    out.push_str("</document>\n");
    Ok(out)
}

fn node(
    doc: &Document,
    reactive: &crate::graph::Reactive,
    q: &ViewQuery,
    id: NodeId,
    level: u32,
    out: &mut String,
) {
    let n = doc.live(id).expect("live");
    let indent = "  ".repeat(level as usize + 1);
    let mut attrs: Vec<(String, String)> = vec![("id".into(), id.to_string())];

    let comps: Vec<&Component> = doc.components_of(id).collect();
    if let Some(name) = comps.iter().find(|c| c.kind() == ComponentKind::Name) {
        if let Some(text) = name
            .get("text")
            .and_then(|s| s.constant())
            .and_then(|v| v.as_str())
            && !text.is_empty()
        {
            attrs.push(("name".into(), text.to_owned()));
        }
    }
    if q.fidelity != Fidelity::Skeleton {
        // Attribute names are bare field names unless two components on this
        // node share one, in which case both get the full `kind.field` path.
        let mut seen: Vec<&str> = Vec::new();
        let mut dup: Vec<&str> = Vec::new();
        for c in &comps {
            for (f, _) in c.fields() {
                if seen.contains(f) {
                    dup.push(f);
                } else {
                    seen.push(f);
                }
            }
        }
        for c in &comps {
            if c.kind() == ComponentKind::Name {
                continue;
            }
            for (f, _) in c.fields() {
                let slot = c.get(f).expect("declared");
                // A bound slot shows its current value like any other, and
                // its binding as `<attr>.bind`, so a reader sees both what a
                // value is and why.
                let leaf = crate::expr::Leaf {
                    node: id,
                    path: crate::component::LeafPath::new(c.kind(), f).expect("declared"),
                };
                let value = reactive
                    .read(doc, leaf)
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "?".to_owned());
                let key = if dup.contains(f) {
                    format!("{}.{f}", c.kind().name())
                } else {
                    (*f).to_owned()
                };
                attrs.push((key.clone(), value));
                if let Some(e) = slot.expr() {
                    attrs.push((format!("{key}.bind"), e.to_string()));
                }
            }
        }
    }

    let kids = doc.children(id);
    let rels: Vec<_> = doc.relations_from(id).collect();
    let at_limit = q.depth.is_some_and(|d| level >= d);
    let shown: &[NodeId] = if at_limit {
        &[]
    } else if q.fidelity == Fidelity::Summary && kids.len() > SUMMARY_MAX_CHILDREN {
        &kids[..SUMMARY_SAMPLES]
    } else {
        kids
    };
    let elided = shown.len() < kids.len();
    if !kids.is_empty() || matches!(n.ty.spec().children, crate::registry::ChildPolicy::Any) {
        attrs.push(("count".into(), kids.len().to_string()));
    }
    if elided {
        attrs.push(("elided".into(), String::new()));
    }

    write!(out, "{indent}<{}", n.ty.name()).expect("string write");
    for (k, v) in &attrs {
        if k == "elided" {
            out.push_str(" elided");
        } else {
            write!(out, " {k}=\"{}\"", escape(v)).expect("string write");
        }
    }
    if shown.is_empty() && rels.is_empty() {
        out.push_str("/>\n");
        return;
    }
    out.push_str(">\n");
    for r in rels {
        write!(out, "{indent}  <{} to=\"{}\"", r.rel.name(), r.to).expect("string write");
        for (k, v) in &r.data {
            write!(out, " {k}=\"{}\"", escape(&v.to_string())).expect("string write");
        }
        out.push_str("/>\n");
    }
    for &k in shown {
        node(doc, reactive, q, k, level + 1, out);
    }
    writeln!(out, "{indent}</{}>", n.ty.name()).expect("string write");
}

fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            _ => o.push(c),
        }
    }
    o
}
