//! Narratives (plans/specs/narrative-panel.md): flowing text stored as
//! nodes, so it has ops, undo, history and durability like everything else.
//!
//! A `narrative` root holds `block` children in order. Each block has a
//! `block` facet (its kind, a heading's level, a view block's composition)
//! and a `richtext` facet: its words and their marks, the marks as JSON with
//! offsets in UTF-16 code units (what the editor counts). This module reads
//! blocks, plans block-level edits as ops, keeps the span index (which
//! blocks mention which nodes), and converts to and from markdown with
//! generic directives (`:e[words]{#id}`, `:val{#id.facet.field}`,
//! `::view{show=name}`), the exchange form for import, export and the model.

use crate::document::Document;
use crate::facet::{Block, Facet, LeafStruct, Name, RichText};
use crate::id::NodeId;
use crate::op::Op;
use crate::order::OrderKey;
use crate::registry::TypeId;
use crate::value::{Slot, Value};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The character a value atom occupies in `richtext.text`.
pub const OBJECT: char = '\u{FFFC}';

pub const KINDS: &[&str] = &["paragraph", "heading", "bullet", "ordered", "quote", "code", "view"];

/// One mark over a range of a block's text.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    /// `mention`, `value`, `strong`, `em`, `code` or `link`.
    #[serde(rename = "type")]
    pub kind: String,
    pub start: usize,
    pub end: usize,
    /// A mention's node (`#4`), or a value's leaf (`#4.size.w`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// A value's display unit and digits (`fmt(target, unit, digits)`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digits: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
}

/// What a block holds, as the editor and edits exchange it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BlockContent {
    pub kind: String,
    #[serde(default = "one")]
    pub level: u32,
    #[serde(default)]
    pub show: String,
    #[serde(default)]
    pub focus: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub marks: Vec<Mark>,
}

fn one() -> u32 {
    1
}

impl Default for BlockContent {
    fn default() -> Self {
        BlockContent { kind: "paragraph".into(), level: 1, show: String::new(), focus: String::new(), text: String::new(), marks: Vec::new() }
    }
}

impl BlockContent {
    pub fn paragraph(text: &str) -> Self {
        BlockContent { text: text.into(), ..Default::default() }
    }

    /// The `block` and `richtext` facets that store this.
    pub fn facets(&self) -> Vec<Facet> {
        let mut b = Block::default();
        b.kind = Slot::Const(Value::Enum(self.kind.clone()));
        b.level = Slot::Const(Value::Number(self.level as f64));
        b.show = Slot::Const(Value::Str(self.show.clone()));
        b.focus = Slot::Const(Value::Str(self.focus.clone()));
        let mut r = RichText::default();
        r.text = Slot::Const(Value::Str(self.text.clone()));
        r.marks = Slot::Const(Value::Str(marks_json(&self.marks)));
        vec![b.wrap(), r.wrap()]
    }

    fn leaves(&self) -> [(&'static str, Value); 6] {
        [
            ("block.kind", Value::Enum(self.kind.clone())),
            ("block.level", Value::Number(self.level as f64)),
            ("block.show", Value::Str(self.show.clone())),
            ("block.focus", Value::Str(self.focus.clone())),
            ("richtext.text", Value::Str(self.text.clone())),
            ("richtext.marks", Value::Str(marks_json(&self.marks))),
        ]
    }
}

fn marks_json(marks: &[Mark]) -> String {
    if marks.is_empty() { String::new() } else { serde_json::to_string(marks).expect("marks serialise") }
}

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).expect("registered type")
}

fn str_of(slot: &Slot) -> String {
    slot.constant().and_then(|v| v.as_str()).unwrap_or_default().to_owned()
}

/// Every live narrative root, in document order.
pub fn narratives(doc: &Document) -> Vec<NodeId> {
    let t = ty("narrative");
    doc.roots().iter().copied().filter(|&r| doc.live(r).is_some_and(|n| n.ty == t)).collect()
}

/// A narrative by id (`#N`) or by name.
pub fn find(doc: &Document, source: &str) -> Option<NodeId> {
    let all = narratives(doc);
    if let Ok(id) = source.parse::<NodeId>()
        && all.contains(&id)
    {
        return Some(id);
    }
    all.into_iter().find(|&n| name(doc, n) == source)
}

pub fn name(doc: &Document, id: NodeId) -> String {
    doc.get::<Name>(id).map(|n| str_of(&n.text)).unwrap_or_default()
}

/// A block's content; `None` for a node that is not a live block.
pub fn content(doc: &Document, id: NodeId) -> Option<BlockContent> {
    let node = doc.live(id)?;
    if node.ty != ty("block") {
        return None;
    }
    let b = doc.get::<Block>(id).cloned().unwrap_or_default();
    let r = doc.get::<RichText>(id).cloned().unwrap_or_default();
    let marks_text = str_of(&r.marks);
    let marks = if marks_text.trim().is_empty() { Vec::new() } else { serde_json::from_str(&marks_text).unwrap_or_default() };
    Some(BlockContent {
        kind: b.kind.constant().and_then(|v| v.as_str()).unwrap_or("paragraph").to_owned(),
        level: b.level.constant().and_then(|v| v.as_f64()).map_or(1, |l| l.clamp(1.0, 6.0) as u32),
        show: str_of(&b.show),
        focus: str_of(&b.focus),
        text: str_of(&r.text),
        marks,
    })
}

/// A narrative's blocks in order.
pub fn blocks(doc: &Document, narrative: NodeId) -> Vec<(NodeId, BlockContent)> {
    doc.children(narrative).iter().filter_map(|&b| content(doc, b).map(|c| (b, c))).collect()
}

/// The ops creating a narrative root named `name` holding `blocks`, with
/// ids from `first` on (the root first). Ordered after `order`.
pub fn create_ops(doc: &Document, name: &str, blocks: &[BlockContent]) -> (NodeId, Vec<Op>) {
    let ids = doc.next_ids(blocks.len() + 1);
    let order = doc.roots().last().and_then(|&r| doc.live(r)).map_or_else(OrderKey::first, |n| OrderKey::after(&n.order));
    let mut nm = Name::default();
    nm.text = Slot::Const(Value::Str(name.into()));
    let mut ops = vec![Op::Create { id: ids[0], ty: ty("narrative"), parent: None, order, facets: vec![nm.wrap()] }];
    let mut key = OrderKey::first();
    for (b, id) in blocks.iter().zip(&ids[1..]) {
        ops.push(Op::Create { id: *id, ty: ty("block"), parent: Some(ids[0]), order: key.clone(), facets: b.facets() });
        key = OrderKey::after(&key);
    }
    (ids[0], ops)
}

// ---- edits --------------------------------------------------------------

/// One block-level change (narrative-panel.md §2). `after` names a block by
/// id (`#B`) or by the `key` of a block inserted earlier in the same list;
/// absent means first.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Edit {
    Insert {
        key: String,
        #[serde(default)]
        after: Option<String>,
        block: BlockContent,
    },
    Set {
        id: String,
        block: BlockContent,
    },
    Delete {
        id: String,
    },
    Move {
        id: String,
        #[serde(default)]
        after: Option<String>,
    },
}

/// The ops for a list of edits, and the ids minted for inserted keys.
#[derive(Debug, Default)]
pub struct Planned {
    pub ops: Vec<Op>,
    pub minted: BTreeMap<String, NodeId>,
}

pub fn plan(doc: &Document, narrative: NodeId, edits: &[Edit]) -> Result<Planned, String> {
    if !narratives(doc).contains(&narrative) {
        return Err(format!("{narrative} is not a live narrative"));
    }
    let current: Vec<NodeId> = doc.children(narrative).to_vec();
    let mine = |s: &str| -> Result<NodeId, String> {
        let id = s.parse::<NodeId>().map_err(|_| format!("{s:?} is not a block id"))?;
        if current.contains(&id) { Ok(id) } else { Err(format!("{s} is not a block of {narrative}")) }
    };
    let inserts = edits.iter().filter(|e| matches!(e, Edit::Insert { .. })).count();
    let mut fresh = doc.next_ids(inserts).into_iter();
    let mut out = Planned::default();
    // The final sequence, by id; moved and inserted ones get new keys.
    let mut seq: Vec<NodeId> = current.clone();
    let mut placed: std::collections::BTreeSet<NodeId> = Default::default();
    let mut created: BTreeMap<NodeId, BlockContent> = BTreeMap::new();
    let resolve = |s: &str, minted: &BTreeMap<String, NodeId>| -> Result<NodeId, String> {
        if let Some(id) = minted.get(s) {
            return Ok(*id);
        }
        mine(s)
    };
    let position = |seq: &[NodeId], after: Option<NodeId>| -> Result<usize, String> {
        match after {
            None => Ok(0),
            Some(a) => seq.iter().position(|&x| x == a).map(|i| i + 1).ok_or_else(|| format!("{a} is not in the narrative")),
        }
    };
    for e in edits {
        match e {
            Edit::Insert { key, after, block } => {
                check(block)?;
                if out.minted.contains_key(key) {
                    return Err(format!("key {key:?} is used twice"));
                }
                let id = fresh.next().expect("reserved");
                let after = after.as_deref().map(|a| resolve(a, &out.minted)).transpose()?;
                let at = position(&seq, after)?;
                seq.insert(at, id);
                placed.insert(id);
                created.insert(id, block.clone());
                out.minted.insert(key.clone(), id);
            }
            Edit::Set { id, block } => {
                check(block)?;
                let id = resolve(id, &out.minted)?;
                if let Some(c) = created.get_mut(&id) {
                    *c = block.clone();
                    continue;
                }
                let old = content(doc, id).ok_or_else(|| format!("{id} is not a block"))?;
                for ((path, new), (_, was)) in block.leaves().into_iter().zip(old.leaves()) {
                    if new != was {
                        out.ops.push(Op::Set { id, path: path.parse().expect("declared leaf"), slot: Slot::Const(new) });
                    }
                }
            }
            Edit::Delete { id } => {
                let id = resolve(id, &out.minted)?;
                seq.retain(|&x| x != id);
                if created.remove(&id).is_none() {
                    out.ops.push(Op::Delete { id });
                }
            }
            Edit::Move { id, after } => {
                let id = resolve(id, &out.minted)?;
                let after = after.as_deref().map(|a| resolve(a, &out.minted)).transpose()?;
                if after == Some(id) {
                    return Err(format!("{id} cannot follow itself"));
                }
                seq.retain(|&x| x != id);
                let at = position(&seq, after)?;
                seq.insert(at, id);
                placed.insert(id);
            }
        }
    }
    // Keys: blocks that did not move keep theirs (their relative order is
    // unchanged); each placed one goes between its neighbours.
    let key_of = |id: NodeId| doc.live(id).map(|n| n.order.clone());
    let mut prev: Option<OrderKey> = None;
    let mut creates = Vec::new();
    let mut reorders = Vec::new();
    for (i, &id) in seq.iter().enumerate() {
        if !placed.contains(&id) {
            prev = key_of(id);
            continue;
        }
        let next = seq[i + 1..].iter().find(|x| !placed.contains(x)).and_then(|&x| key_of(x));
        let key = OrderKey::between(prev.as_ref(), next.as_ref()).map_err(|e| format!("ordering: {e:?}"))?;
        match created.get(&id) {
            Some(block) => creates.push(Op::Create { id, ty: ty("block"), parent: Some(narrative), order: key.clone(), facets: block.facets() }),
            None => reorders.push(Op::Reorder { id, order: key.clone() }),
        }
        prev = Some(key);
    }
    out.ops.extend(creates);
    out.ops.extend(reorders);
    Ok(out)
}

fn check(b: &BlockContent) -> Result<(), String> {
    if !KINDS.contains(&b.kind.as_str()) {
        return Err(format!("unknown block kind {:?} ({})", b.kind, KINDS.join(", ")));
    }
    let len: usize = b.text.encode_utf16().count();
    for m in &b.marks {
        if m.start > m.end || m.end > len {
            return Err(format!("mark {} {}..{} is outside the text ({len})", m.kind, m.start, m.end));
        }
    }
    Ok(())
}

// ---- the span index -----------------------------------------------------

/// A reference from a block to a node: a mention or a live value.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Reference {
    pub narrative: NodeId,
    pub block: NodeId,
    /// Index into the block's marks.
    pub mark: usize,
    pub kind: String,
    pub target: NodeId,
    /// The target is not a live node: the reference is stale.
    pub stale: bool,
}

/// The node a mark's target names: `#4` or the node of `#4.size.w`.
pub fn target_node(target: &str) -> Option<NodeId> {
    let head = target.split('.').next()?;
    head.parse().ok()
}

/// Every mention and value in every narrative (the span index, derived).
pub fn references(doc: &Document) -> Vec<Reference> {
    let mut out = Vec::new();
    for n in narratives(doc) {
        for (b, c) in blocks(doc, n) {
            for (i, m) in c.marks.iter().enumerate() {
                if let Some(t) = m.target.as_deref().and_then(target_node) {
                    out.push(Reference { narrative: n, block: b, mark: i, kind: m.kind.clone(), target: t, stale: !doc.is_live(t) });
                }
            }
        }
    }
    out
}

/// Which blocks refer to `target`.
pub fn references_to(doc: &Document, target: NodeId) -> Vec<Reference> {
    references(doc).into_iter().filter(|r| r.target == target).collect()
}

/// The expression a value mark shows: `fmt(target)`, with its unit and
/// digits when it has them.
pub fn value_expr(m: &Mark) -> Option<String> {
    let t = m.target.as_deref()?;
    Some(match (&m.unit, m.digits) {
        (Some(u), Some(d)) => format!("fmt({t}, \"{u}\", {d})"),
        (Some(u), None) => format!("fmt({t}, \"{u}\")"),
        (None, Some(d)) => format!("fmt({t}, {d})"),
        (None, None) => format!("fmt({t})"),
    })
}

// ---- markdown out -------------------------------------------------------

/// A narrative as markdown: one block per paragraph. With `ids`, each
/// block's first line starts `[#B] ` (the model's view).
pub fn to_markdown(doc: &Document, narrative: NodeId, ids: bool) -> String {
    let mut out = String::new();
    for (id, c) in blocks(doc, narrative) {
        if !out.is_empty() {
            out.push('\n');
        }
        if ids {
            out.push_str(&format!("[{id}] "));
        }
        out.push_str(&block_markdown(&c));
        out.push('\n');
    }
    out
}

pub fn block_markdown(c: &BlockContent) -> String {
    let inline = || inline_markdown(&c.text, &c.marks);
    match c.kind.as_str() {
        "heading" => format!("{} {}", "#".repeat(c.level.clamp(1, 6) as usize), inline()),
        "bullet" => format!("- {}", inline()),
        "ordered" => format!("1. {}", inline()),
        "quote" => format!("> {}", inline()),
        "code" => format!("```\n{}\n```", c.text),
        "view" => {
            let mut attrs = format!("show={}", attr_value(&c.show));
            if !c.focus.is_empty() {
                attrs.push_str(&format!(" focus={}", attr_value(&c.focus)));
            }
            let caption = inline();
            if caption.is_empty() { format!("::view{{{attrs}}}") } else { format!("::view[{caption}]{{{attrs}}}") }
        }
        _ => {
            let s = inline();
            // A paragraph that starts like a block marker would read back as one.
            match s.chars().next() {
                Some('#' | '>' | '-' | '+') => format!("\\{s}"),
                _ => s,
            }
        }
    }
}

fn attr_value(v: &str) -> String {
    if !v.is_empty() && v.chars().all(|c| c.is_alphanumeric() || "#._-/".contains(c)) { v.to_owned() } else { format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\"")) }
}

/// A block's text with its marks as markdown and directives.
pub fn inline_markdown(text: &str, marks: &[Mark]) -> String {
    // Marks by start, longest first, so an outer one opens before an inner.
    let mut order: Vec<usize> = (0..marks.len()).filter(|&i| marks[i].kind != "value").collect();
    order.sort_by_key(|&i| (marks[i].start, std::cmp::Reverse(marks[i].end)));
    let values: BTreeMap<usize, &Mark> = marks.iter().filter(|m| m.kind == "value").map(|m| (m.start, m)).collect();
    let mut out = String::new();
    let mut open: Vec<usize> = Vec::new();
    let mut pos = 0usize;
    let chars: Vec<char> = text.chars().collect();
    let close = |out: &mut String, m: &Mark| match m.kind.as_str() {
        "strong" => out.push_str("**"),
        "em" => out.push('*'),
        "code" => out.push('`'),
        "mention" => out.push_str(&format!("]{{{}}}", m.target.as_deref().unwrap_or(""))),
        "link" => out.push_str(&format!("]({})", m.href.as_deref().unwrap_or(""))),
        _ => {}
    };
    let opener = |out: &mut String, m: &Mark| match m.kind.as_str() {
        "strong" => out.push_str("**"),
        "em" => out.push('*'),
        "code" => out.push('`'),
        "mention" => out.push_str(":e["),
        "link" => out.push('['),
        _ => {}
    };
    let mut next = 0usize;
    for (ci, &ch) in chars.iter().enumerate() {
        // Close what ends here (reopening anything closed early).
        let mut reopen = Vec::new();
        while open.iter().any(|&i| marks[i].end <= pos) {
            let top = open.pop().expect("non-empty");
            close(&mut out, &marks[top]);
            if marks[top].end > pos {
                reopen.push(top);
            }
        }
        for i in reopen.into_iter().rev() {
            opener(&mut out, &marks[i]);
            open.push(i);
        }
        while next < order.len() && marks[order[next]].start <= pos {
            let i = order[next];
            next += 1;
            if marks[i].end > pos {
                opener(&mut out, &marks[i]);
                open.push(i);
            }
        }
        let in_code = open.iter().any(|&i| marks[i].kind == "code");
        if ch == OBJECT
            && let Some(m) = values.get(&pos)
        {
            out.push_str(&value_directive(m));
        } else if in_code {
            out.push(ch);
        } else {
            match ch {
                '\\' | '*' | '`' | '[' | ']' => {
                    out.push('\\');
                    out.push(ch);
                }
                ':' if directive_follows(&chars[ci + 1..]) => out.push_str("\\:"),
                _ => out.push(ch),
            }
        }
        pos += ch.len_utf16();
    }
    while let Some(top) = open.pop() {
        close(&mut out, &marks[top]);
    }
    out
}

fn directive_follows(rest: &[char]) -> bool {
    let word: String = rest.iter().take_while(|c| c.is_ascii_alphabetic()).collect();
    !word.is_empty() && matches!(rest.get(word.len()), Some('[' | '{'))
}

fn value_directive(m: &Mark) -> String {
    let mut s = format!(":val{{{}", m.target.as_deref().unwrap_or(""));
    if let Some(u) = &m.unit {
        s.push_str(&format!(" unit={}", attr_value(u)));
    }
    if let Some(d) = m.digits {
        s.push_str(&format!(" digits={d}"));
    }
    s.push('}');
    s
}

// ---- markdown in --------------------------------------------------------

/// Blocks from markdown: headings, `-` and `1.` items, `>` quotes, fenced
/// code, `::view{show=…}` and paragraphs (consecutive lines join). A
/// leading `[#B] ` (the model view's id) is ignored.
pub fn parse_markdown(md: &str) -> Vec<BlockContent> {
    let mut out = Vec::new();
    let mut para: Vec<String> = Vec::new();
    let flush = |para: &mut Vec<String>, out: &mut Vec<BlockContent>| {
        if !para.is_empty() {
            let (text, marks) = parse_inline(&para.join(" "));
            out.push(BlockContent { text, marks, ..Default::default() });
            para.clear();
        }
    };
    let mut lines = md.lines();
    while let Some(raw) = lines.next() {
        let line = strip_id(raw.trim_end());
        let t = line.trim_start();
        if t.is_empty() {
            flush(&mut para, &mut out);
            continue;
        }
        if t.starts_with("```") {
            flush(&mut para, &mut out);
            let mut code = Vec::new();
            for l in lines.by_ref() {
                if l.trim_start().starts_with("```") {
                    break;
                }
                code.push(l);
            }
            out.push(BlockContent { kind: "code".into(), text: code.join("\n"), ..Default::default() });
            continue;
        }
        if let Some(rest) = t.strip_prefix("::view") {
            flush(&mut para, &mut out);
            let (caption, rest) = match rest.strip_prefix('[') {
                Some(r) => {
                    let end = closing(r, '[', ']').unwrap_or(r.len());
                    (&r[..end], r.get(end + 1..).unwrap_or(""))
                }
                None => ("", rest),
            };
            let attrs = rest.strip_prefix('{').and_then(|r| closing(r, '{', '}').map(|e| &r[..e])).map(parse_attrs).unwrap_or_default();
            let (text, marks) = parse_inline(caption);
            out.push(BlockContent {
                kind: "view".into(),
                show: attrs.named.get("show").cloned().unwrap_or_default(),
                focus: attrs.named.get("focus").cloned().or(attrs.id.clone()).unwrap_or_default(),
                text,
                marks,
                ..Default::default()
            });
            continue;
        }
        let hashes = t.chars().take_while(|&c| c == '#').count();
        if (1..=6).contains(&hashes) && t[hashes..].starts_with(' ') {
            flush(&mut para, &mut out);
            let (text, marks) = parse_inline(t[hashes..].trim());
            out.push(BlockContent { kind: "heading".into(), level: hashes as u32, text, marks, ..Default::default() });
            continue;
        }
        let item = |kind: &str, body: &str, out: &mut Vec<BlockContent>| {
            let (text, marks) = parse_inline(body.trim());
            out.push(BlockContent { kind: kind.into(), text, marks, ..Default::default() });
        };
        if let Some(body) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")).or_else(|| t.strip_prefix("+ ")) {
            flush(&mut para, &mut out);
            item("bullet", body, &mut out);
            continue;
        }
        let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
        if digits > 0 && (t[digits..].starts_with(". ") || t[digits..].starts_with(") ")) {
            flush(&mut para, &mut out);
            item("ordered", &t[digits + 2..], &mut out);
            continue;
        }
        if let Some(body) = t.strip_prefix('>') {
            flush(&mut para, &mut out);
            item("quote", body, &mut out);
            continue;
        }
        para.push(t.to_owned());
    }
    flush(&mut para, &mut out);
    out
}

fn strip_id(line: &str) -> &str {
    let t = line.trim_start();
    if let Some(rest) = t.strip_prefix("[#")
        && let Some(end) = rest.find("] ")
        && rest[..end].chars().all(|c| c.is_ascii_alphanumeric() || c == '/')
    {
        return &rest[end + 2..];
    }
    line
}

/// The index of the bracket closing the one just before `s`, skipping
/// escapes and nested pairs.
fn closing(s: &str, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    let mut escaped = false;
    for (i, c) in s.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' {
            escaped = true;
        } else if c == open {
            depth += 1;
        } else if c == close {
            if depth == 0 {
                return Some(i);
            }
            depth -= 1;
        }
    }
    None
}

#[derive(Default, Debug)]
struct Attrs {
    id: Option<String>,
    named: BTreeMap<String, String>,
}

/// `#id key=value key="quoted value"`.
fn parse_attrs(s: &str) -> Attrs {
    let mut a = Attrs::default();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '=' {
            i += 1;
        }
        let word: String = chars[start..i].iter().collect();
        if i < chars.len() && chars[i] == '=' {
            i += 1;
            let value = if i < chars.len() && chars[i] == '"' {
                i += 1;
                let mut v = String::new();
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        i += 1;
                    }
                    v.push(chars[i]);
                    i += 1;
                }
                i += 1;
                v
            } else {
                let s = i;
                while i < chars.len() && !chars[i].is_whitespace() {
                    i += 1;
                }
                chars[s..i].iter().collect()
            };
            a.named.insert(word, value);
        } else if word.starts_with('#') && a.id.is_none() {
            a.id = Some(word);
        }
    }
    a
}

/// Inline markdown to text and marks: `**strong**`, `*em*`, `` `code` ``,
/// `[words](href)`, `:e[words]{#id}`, `:val{#id.facet.field unit=… digits=…}`,
/// and `\` escapes. Unclosed emphasis ends with the text.
pub fn parse_inline(s: &str) -> (String, Vec<Mark>) {
    let mut text = String::new();
    let mut marks = Vec::new();
    inline_into(s, &mut text, &mut marks);
    marks.sort_by_key(|m: &Mark| (m.start, std::cmp::Reverse(m.end)));
    (text, marks)
}

fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

fn inline_into(s: &str, text: &mut String, marks: &mut Vec<Mark>) {
    let mut open: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut i = 0;
    let toggle = |kind: &'static str, open: &mut BTreeMap<&'static str, usize>, text: &String, marks: &mut Vec<Mark>| {
        let here = utf16_len(text);
        match open.remove(kind) {
            Some(start) if start < here => marks.push(Mark { kind: kind.into(), start, end: here, ..Default::default() }),
            Some(_) => {}
            None => {
                open.insert(kind, here);
            }
        }
    };
    while i < s.len() {
        let rest = &s[i..];
        let c = rest.chars().next().expect("in bounds");
        if c == '\\'
            && let Some(n) = rest[1..].chars().next()
        {
            text.push(n);
            i += 1 + n.len_utf8();
            continue;
        }
        if rest.starts_with("**") {
            toggle("strong", &mut open, text, marks);
            i += 2;
            continue;
        }
        if c == '*' {
            toggle("em", &mut open, text, marks);
            i += 1;
            continue;
        }
        if c == '`'
            && let Some(end) = rest[1..].find('`')
        {
            let start = utf16_len(text);
            text.push_str(&rest[1..1 + end]);
            marks.push(Mark { kind: "code".into(), start, end: utf16_len(text), ..Default::default() });
            i += end + 2;
            continue;
        }
        if let Some(r) = rest.strip_prefix(":e[")
            && let Some(close) = closing(r, '[', ']')
            && r[close + 1..].starts_with('{')
            && let Some(attr_end) = closing(&r[close + 2..], '{', '}')
        {
            let start = utf16_len(text);
            inline_into(&r[..close], text, marks);
            let attrs = parse_attrs(&r[close + 2..close + 2 + attr_end]);
            marks.push(Mark { kind: "mention".into(), start, end: utf16_len(text), target: attrs.id.or_else(|| attrs.named.get("ref").cloned()), ..Default::default() });
            i += 3 + close + 2 + attr_end + 1;
            continue;
        }
        if let Some(r) = rest.strip_prefix(":val{")
            && let Some(end) = closing(r, '{', '}')
        {
            let attrs = parse_attrs(&r[..end]);
            let start = utf16_len(text);
            text.push(OBJECT);
            marks.push(Mark {
                kind: "value".into(),
                start,
                end: start + 1,
                target: attrs.id,
                unit: attrs.named.get("unit").cloned(),
                digits: attrs.named.get("digits").and_then(|d| d.parse().ok()),
                ..Default::default()
            });
            i += 5 + end + 1;
            continue;
        }
        if c == '['
            && let Some(close) = closing(&rest[1..], '[', ']')
            && rest[1 + close + 1..].starts_with('(')
            && let Some(end) = rest[1 + close + 2..].find(')')
        {
            let start = utf16_len(text);
            inline_into(&rest[1..1 + close], text, marks);
            let href = rest[1 + close + 2..1 + close + 2 + end].to_owned();
            marks.push(Mark { kind: "link".into(), start, end: utf16_len(text), href: Some(href), ..Default::default() });
            i += 1 + close + 2 + end + 1;
            continue;
        }
        text.push(c);
        i += c.len_utf8();
    }
    let here = utf16_len(text);
    for (kind, start) in open {
        if start < here {
            marks.push(Mark { kind: kind.into(), start, end: here, ..Default::default() });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_round_trip() {
        for md in [
            "We have a :e[10 mW laser]{#4} feeding **the** fiber.",
            "Ours is :val{#9.size.w unit=mm digits=1} wide, *about* `w0`.",
            "See [the datasheet](https://example.com/sm980) and :e[**bold** words]{#1c}.",
            "Escapes: \\*not em\\*, \\[not a link\\], a time 10\\:e\\[x\\] stays.",
        ] {
            let (text, marks) = parse_inline(md);
            let back = inline_markdown(&text, &marks);
            assert_eq!(parse_inline(&back), (text.clone(), marks.clone()), "{md} → {back}");
        }
        let (text, marks) = parse_inline("a :e[laser]{#4} b");
        assert_eq!(text, "a laser b");
        assert_eq!(marks, vec![Mark { kind: "mention".into(), start: 2, end: 7, target: Some("#4".into()), ..Default::default() }]);
        let (text, marks) = parse_inline("x :val{#9.size.w} y");
        assert_eq!(text, format!("x {OBJECT} y"));
        assert_eq!(marks[0].kind, "value");
        assert_eq!(value_expr(&marks[0]).as_deref(), Some("fmt(#9.size.w)"));
    }

    #[test]
    fn offsets_are_utf16() {
        let (text, marks) = parse_inline("µ😀 :e[x]{#4}");
        assert_eq!(marks[0].start, 4, "µ is 1 unit, 😀 is 2, the space 1");
        assert_eq!(inline_markdown(&text, &marks), "µ😀 :e[x]{#4}");
    }

    #[test]
    fn blocks_parse() {
        let md = "# Getting light in\n\nWe have a :e[laser]{#4}\nand a fiber.\n\n- one\n- two\n1. first\n> said\n\n```\nlet x = 1;\n```\n::view[The bench]{show=bench focus=#9}\n[#2a] tagged line";
        let b = parse_markdown(md);
        let kinds: Vec<&str> = b.iter().map(|c| c.kind.as_str()).collect();
        assert_eq!(kinds, ["heading", "paragraph", "bullet", "bullet", "ordered", "quote", "code", "view", "paragraph"]);
        assert_eq!(b[1].text, "We have a laser and a fiber.");
        assert_eq!(b[6].text, "let x = 1;");
        assert_eq!((b[7].show.as_str(), b[7].focus.as_str(), b[7].text.as_str()), ("bench", "#9", "The bench"));
        assert_eq!(b[8].text, "tagged line");
        for c in &b {
            let again = parse_markdown(&block_markdown(c));
            assert_eq!(again, vec![c.clone()], "{}", block_markdown(c));
        }
    }
}
