//! A model's reply, read in the one markup reader ([`crate::parts`]): the
//! blocks the agent acts on ([`read`]), the reply as it streams ([`stream`]),
//! and `<revise>`, which patches the edit the store just rejected ([`revise`]).
//!
//! A reply is prose with markup in it: one `<edit>` (or a `<revise>` of the
//! rejected edit), `<look picture="…"/>` and `<look expand="…"/>`,
//! `<topic name="…"/>` and `<continue/>` (the agent prompt, manifest.ts).

use crate::contexts::attribute_spans;
use crate::parts::{partial, El};
use serde::Serialize;

/// What a reply asks for.
#[derive(Debug, Default, Serialize, PartialEq)]
pub struct Reply {
    /// The first `<edit>` block, as written.
    pub edit: Option<String>,
    /// The first `<revise>` block, as written.
    pub revise: Option<String>,
    /// A `<look picture="…"/>`: the composition (a name or `#id`) to capture.
    pub picture: Option<String>,
    /// The instances a `<look expand="…"/>` names, each as `#id`.
    pub expand: Vec<String>,
    /// The topics `<topic name="…"/>` asks for, each once.
    pub topics: Vec<String>,
    /// `<continue/>`: more steps follow this one's edit.
    #[serde(rename = "continue")]
    pub continues: bool,
    /// The reply without its edit (or, with none, its revise) and its
    /// picture looks: what the model said, trimmed.
    pub note: String,
    /// The reply without its picture looks only, trimmed: what the model
    /// said when its edit is not acted on (an example in a chat reply).
    pub bare: String,
}

/// The tags a reply's markup starts with.
const BLOCKS: &[&str] = &["edit", "revise", "look", "topic", "continue"];

/// Where `<name` starts a tag at `i`: the name, then a space, `>` or `/`.
fn tag_at<'a>(text: &str, i: usize, names: &[&'a str]) -> Option<&'a str> {
    let rest = text[i..].strip_prefix('<')?;
    names.iter().copied().find(|n| rest.strip_prefix(n).is_some_and(|r| r.starts_with(|c: char| c.is_whitespace() || c == '>' || c == '/')))
}

/// The element starting at `i`, read as far as the text goes.
fn element(text: &str, i: usize) -> Option<El> {
    partial(&text[i..]).into_iter().next()
}

/// The end of the block element `name` opening at `i`: after its closing
/// tag; for one never closed or not well-formed, after its last
/// `</name>`, else the end of the text (the planner then says what is wrong).
fn block_end(text: &str, i: usize, name: &str) -> usize {
    match element(text, i) {
        Some(el) if el.closed => i + el.span.end,
        _ => {
            let close = format!("</{name}>");
            text[i..].rfind(&close).map_or(text.len(), |at| i + at + close.len())
        }
    }
}

/// A picture look's source: a name or id, at most 200 characters, with no
/// markup or quotes in it.
fn picture_source(el: &El) -> Option<String> {
    el.attr("picture").filter(|p| !p.is_empty() && p.chars().count() <= 200 && !p.contains(['<', '>', '"', '\''])).map(str::to_owned)
}

/// Read a reply: its edit or revise, and what else it asks for.
pub fn read(text: &str) -> Reply {
    let mut reply = Reply::default();
    let mut edit = None;
    let mut revise = None;
    let mut pictures: Vec<std::ops::Range<usize>> = Vec::new();
    let mut i = 0;
    while let Some(lt) = text[i..].find('<').map(|at| i + at) {
        i = lt + 1;
        let Some(name) = tag_at(text, lt, BLOCKS) else { continue };
        match name {
            "edit" | "revise" => {
                let end = block_end(text, lt, name);
                let slot = if name == "edit" { &mut edit } else { &mut revise };
                slot.get_or_insert(lt..end);
                // What a block holds is the edit's, not the reply's.
                i = end;
            }
            _ => {
                let Some(el) = element(text, lt) else { continue };
                let tag = lt..lt + el.open;
                match name {
                    "look" => {
                        if let Some(source) = picture_source(&el) {
                            reply.picture.get_or_insert(source);
                            pictures.push(tag);
                        }
                        if reply.expand.is_empty() {
                            if let Some(ids) = el.attr("expand") {
                                reply.expand = ids.split_whitespace().map(|id| if id.starts_with('#') { id.to_owned() } else { format!("#{id}") }).collect();
                            }
                        }
                    }
                    "topic" => {
                        for n in el.attr("name").unwrap_or("").split(|c: char| c.is_whitespace() || c == ',').filter(|n| !n.is_empty()) {
                            if !reply.topics.iter().any(|t| t == n) {
                                reply.topics.push(n.to_owned());
                            }
                        }
                    }
                    _ => reply.continues |= el.attrs.is_empty() && el.span.end == el.open,
                }
            }
        }
    }
    let without = |mut cut: Vec<std::ops::Range<usize>>| {
        cut.sort_by_key(|r| r.start);
        let mut out = String::new();
        let mut from = 0;
        for r in cut {
            out.push_str(&text[from..r.start.max(from)]);
            from = from.max(r.end);
        }
        out.push_str(&text[from..]);
        out.trim().to_owned()
    };
    reply.note = without(edit.clone().or(revise.clone()).into_iter().chain(pictures.iter().cloned()).collect());
    reply.bare = without(pictures);
    reply.edit = edit.map(|r| text[r].to_owned());
    reply.revise = revise.map(|r| text[r].to_owned());
    reply
}

/// A streaming reply as the chat shows it: the prose so far (with a
/// half-written `<edit`, `<look`, `<revise` or `::view` held back, so markup
/// never flashes as text), the edit or revise if one has begun, and its
/// verbs whose opening tags have ended.
#[derive(Debug, Default, Serialize, PartialEq)]
pub struct Stream {
    pub prose: String,
    pub edit: Option<String>,
    pub verbs: Vec<Verb>,
}

/// A verb of an edit (or a step of a revise) being written.
#[derive(Debug, Serialize, PartialEq)]
pub struct Verb {
    pub name: String,
    /// Its attributes, decoded, in written order.
    pub attrs: Vec<(String, String)>,
    /// Written to its end (its closing tag, or `/>`).
    pub closed: bool,
}

/// Read a reply as far as it has streamed.
pub fn stream(text: &str) -> Stream {
    let found = text.match_indices('<').map(|(i, _)| i).find(|&i| {
        // An edit inside a fenced code block is an example, not the edit.
        tag_at(text, i, &["edit", "look", "revise"]).is_some() && text[..i].matches("```").count() % 2 == 0
    });
    let Some(start) = found else {
        return Stream { prose: hold_back(text).to_owned(), ..Stream::default() };
    };
    let el = element(text, start);
    if tag_at(text, start, &["look"]).is_some() {
        // A look asks to see inside instances or a picture; it is neither
        // prose nor an edit.
        let after = el.map_or("", |el| hold_back(&text[start + el.open..]));
        return Stream { prose: format!("{}{after}", &text[..start]), ..Stream::default() };
    }
    let (end, after) = match &el {
        Some(el) if el.closed => (start + el.span.end, hold_back(&text[start + el.span.end..])),
        _ => (text.len(), ""),
    };
    let prose = format!("{}{after}", &text[..start]);
    let verbs = el.map(|el| el.children.into_iter().map(|v| Verb { name: v.name, attrs: v.attrs, closed: v.closed }).collect()).unwrap_or_default();
    Stream { prose: if prose.trim().is_empty() { String::new() } else { prose }, edit: Some(text[start..end].to_owned()), verbs }
}

/// `text` without a tag still being written at its end (one that may become
/// `<edit`, `<look` or `<revise`) or a last line starting `:` (a `::view`).
fn hold_back(text: &str) -> &str {
    let mut text = text;
    if let Some(lt) = text.rfind('<') {
        let tail = &text[lt..];
        if !tail.contains('>') && ["<edit", "<look", "<revise"].iter().any(|t| t.starts_with(tail) || tail.starts_with(t)) {
            text = &text[..lt];
        }
    }
    let line = &text[text.rfind('\n').map_or(0, |n| n + 1)..];
    if line.starts_with(':') {
        text = &text[..text.len() - line.len()];
    }
    text
}

/// An element at the top level of a block: its source and what precedes it.
struct Item<'a> {
    /// What lies between the element before (or the block's opening tag) and this one.
    gap: &'a str,
    src: &'a str,
    el: El,
}

/// A block as written: its opening tag, the elements in it, and what
/// follows the last of them, closing tag included.
struct Block<'a> {
    head: String,
    items: Vec<Item<'a>>,
    tail: String,
    attrs: Vec<(String, String)>,
}

fn block<'a>(src: &'a str, root: &str) -> Result<Block<'a>, String> {
    let els = crate::parts::elements(src.trim()).map_err(|e| format!("<{root}> is not well-formed: {e}"))?;
    let lead = src.len() - src.trim_start().len();
    let el = match els.as_slice() {
        [el] if el.name == root => el,
        _ => return Err(format!("expected one <{root}> element")),
    };
    let at = |r: std::ops::Range<usize>| &src[lead + r.start..lead + r.end];
    let head = at(el.span.start..el.open);
    if el.open == el.span.end {
        let head = head.strip_suffix("/>").unwrap_or(head).trim_end();
        return Ok(Block { head: format!("{head}>"), items: Vec::new(), tail: format!("</{root}>"), attrs: el.attrs.clone() });
    }
    let mut items = Vec::new();
    let mut from = el.open;
    for c in &el.children {
        items.push(Item { gap: at(from..c.span.start), src: at(c.span.clone()), el: c.clone() });
        from = c.span.end;
    }
    Ok(Block { head: head.to_owned(), items, tail: at(from..el.span.end).to_owned(), attrs: el.attrs.clone() })
}

/// A value written as an attribute's, in double quotes.
fn quoted(value: &str) -> String {
    format!("\"{}\"", value.replace('&', "&amp;").replace('<', "&lt;").replace('"', "&quot;"))
}

/// An opening tag with `name` set to `value`, the rest kept as written.
fn set_attr(tag: &str, name: &str, value: &str) -> Result<String, String> {
    let close = if tag.ends_with("/>") { "/>" } else { ">" };
    let name_end = tag[1..].find(|c: char| c.is_whitespace() || c == '/' || c == '>').map_or(tag.len(), |n| n + 1);
    let body = name_end..tag.len() - close.len();
    let spans = attribute_spans(&tag[body.clone()])?;
    let at = body.start;
    Ok(match spans.iter().find(|a| a.key == name) {
        Some(a) => match &a.value {
            Some(v) => format!("{}{}{}", &tag[..at + v.start], quoted(value), &tag[at + v.end..]),
            None => format!("{}={}{}", &tag[..at + a.key_end], quoted(value), &tag[at + a.key_end..]),
        },
        None => format!("{} {name}={}{close}", tag[..body.end].trim_end(), quoted(value)),
    })
}

/// An attribute of the tag opening `src`, as written (entities not read).
fn written(src: &str, el: &El, name: &str) -> Option<String> {
    let tag = &src[..el.open - el.span.start];
    let name_end = tag[1..].find(|c: char| c.is_whitespace() || c == '/' || c == '>').map_or(tag.len(), |n| n + 1);
    let body = &tag[name_end..tag.len() - if tag.ends_with("/>") { 2 } else { 1 }];
    let spans = attribute_spans(body).ok()?;
    spans.into_iter().find(|a| a.key == name).and_then(|a| a.value).map(|v| body[v.start + 1..v.end - 1].to_owned())
}

/// Apply a `<revise>` to the rejected `<edit>`. A revise names the edit's
/// verbs by handle (the `id="$name"` it gave, or its op number, counting
/// verbs from 0 as `<edit-failed>` does) and changes only those. Handles
/// always mean the rejected edit as sent, never as partly revised, so the
/// order of a revise's own steps does not move them.
///
/// ```text
/// <revise>
///   <change op="$sinv" port.signal="{= $sin.port.signal * 2}"/>   set or add attributes on one verb
///   <replace op="14"><rect …/></replace>                           swap one verb for any number
///   <drop op="$bfill"/>
///   <insert after="$ta"><rect …/></insert>                         or before="…"
///   <text old="$spump.port" new="$sp.port" all="true"/>            exact text, once unless all
/// </revise>
/// ```
///
/// `<revise expect-version="N">` (or `exact=`) changes the edit's own
/// attribute. `<text>` runs last, over the revised markup, in order. An
/// empty `<revise/>` gives back the edit as it was: after a merge conflict
/// (merge.ts), that keeps the agent's values over the person's.
pub fn revise(rejected: &str, patch: &str) -> Result<String, String> {
    let draft = block(rejected, "edit")?;
    let steps = block(patch, "revise")?;
    let count = draft.items.len();
    // Each verb of the rejected edit, by handle, as sent.
    let handle = |raw: Option<&str>, at: &str| -> Result<usize, String> {
        let raw = raw.filter(|r| !r.is_empty()).ok_or_else(|| format!("{at} needs op=\"$alias\" or op=\"N\""))?;
        if raw.chars().all(|c| c.is_ascii_digit()) {
            let n: usize = raw.parse().map_err(|_| format!("{at}: op=\"{raw}\" is not a number"))?;
            if n >= count {
                return Err(format!("{at}: there is no op {n}; the edit had ops 0 to {}", count as i64 - 1));
            }
            return Ok(n);
        }
        let hits: Vec<usize> = draft.items.iter().enumerate().filter(|(_, it)| it.el.attr("id") == Some(raw)).map(|(i, _)| i).collect();
        match hits.as_slice() {
            [one] => Ok(*one),
            [] => Err(format!("{at}: no op of the rejected edit has id \"{raw}\"")),
            _ => Err(format!("{at}: \"{raw}\" names ops {}; use op=\"N\"", hits.iter().map(|h| h.to_string()).collect::<Vec<_>>().join(" and "))),
        }
    };

    struct Slot {
        src: Option<String>,
        gap: String,
        before: Vec<String>,
        after: Vec<String>,
    }
    let mut slots: Vec<Slot> = draft.items.iter().map(|it| Slot { src: Some(it.src.to_owned()), gap: it.gap.to_owned(), before: Vec::new(), after: Vec::new() }).collect();
    let mut head = draft.head.clone();
    for (k, v) in &steps.attrs {
        head = set_attr(&head, k, v)?;
    }
    let mut texts: Vec<&Item> = Vec::new();
    for (i, step) in steps.items.iter().enumerate() {
        let at = format!("revise step {i} <{}>", step.el.name);
        let live = |slots: &mut Vec<Slot>, n: usize| -> Result<(), String> {
            if slots[n].src.is_none() {
                return Err(format!("{at}: op {n} was dropped earlier in this revise"));
            }
            Ok(())
        };
        match step.el.name.as_str() {
            "change" => {
                let n = handle(step.el.attr("op"), &at)?;
                live(&mut slots, n)?;
                let changes: Vec<&(String, String)> = step.el.attrs.iter().filter(|(k, _)| k != "op").collect();
                if changes.is_empty() {
                    return Err(format!("{at}: give the attributes to set, e.g. x=\"10\""));
                }
                let src = slots[n].src.take().expect("live");
                let open = draft.items[n].el.open - draft.items[n].el.span.start;
                let mut tag = src[..open].to_owned();
                for (k, v) in changes {
                    tag = set_attr(&tag, k, v)?;
                }
                slots[n].src = Some(tag + &src[open..]);
            }
            "replace" => {
                let n = handle(step.el.attr("op"), &at)?;
                live(&mut slots, n)?;
                slots[n].src = Some(step.el.inner.trim().to_owned());
            }
            "drop" => {
                let n = handle(step.el.attr("op"), &at)?;
                live(&mut slots, n)?;
                slots[n].src = None;
            }
            "insert" => {
                let (anchor, after) = match (step.el.attr("after"), step.el.attr("before")) {
                    (Some(a), _) => (a, true),
                    (None, Some(b)) => (b, false),
                    (None, None) => return Err(format!("{at} needs after=\"$alias|N\" or before=\"$alias|N\"")),
                };
                let verbs = step.el.inner.trim();
                if verbs.is_empty() {
                    return Err(format!("{at}: put the verbs to insert inside it"));
                }
                let n = handle(Some(anchor), &at)?;
                if after { slots[n].after.push(verbs.to_owned()) } else { slots[n].before.push(verbs.to_owned()) }
            }
            "text" => texts.push(step),
            other => return Err(format!("unknown revise step <{other}>; use change, replace, drop, insert or text")),
        }
    }

    // New verbs go in at the first verb's indent.
    let indent = draft.items.first().and_then(|it| it.gap.rsplit_once('\n')).map(|(_, last)| last).filter(|l| l.chars().all(|c| c == ' ' || c == '\t')).unwrap_or("  ");
    let mut body = String::new();
    for slot in &slots {
        for verbs in &slot.before {
            body += &format!("\n{indent}{verbs}");
        }
        if let Some(src) = &slot.src {
            if !src.is_empty() {
                body += if slot.gap.is_empty() { format!("\n{indent}") } else { slot.gap.clone() }.as_str();
                body += src;
            }
        }
        for verbs in &slot.after {
            body += &format!("\n{indent}{verbs}");
        }
    }
    let mut edit = format!("{head}{body}{}", draft.tail);

    for (i, step) in texts.into_iter().enumerate() {
        let at = format!("revise <text> {i}");
        let (Some(old), Some(new)) = (step.el.attr("old"), step.el.attr("new")) else {
            return Err(format!("{at} needs old=\"…\" and new=\"…\""));
        };
        let all = step.el.attr("all").is_some_and(|a| ["true", "yes", "1"].iter().any(|t| a.eq_ignore_ascii_case(t)));
        // The text as written in the revise's markup, or with its entities
        // read (old="&quot;mW&quot;" finds "mW" inside a bare attribute too).
        let forms = [(written(step.src, &step.el, "old"), written(step.src, &step.el, "new")), (Some(old.to_owned()), Some(new.to_owned()))];
        let hit = forms.into_iter().find_map(|(o, n)| Some((o?, n?)).filter(|(o, _)| !o.is_empty() && edit.contains(o.as_str())));
        let Some((old, new)) = hit else {
            return Err(format!("{at}: \"{}\" is not in the rejected edit", written(step.src, &step.el, "old").unwrap_or_else(|| old.to_owned())));
        };
        let n = edit.matches(old.as_str()).count();
        if n > 1 && !all {
            return Err(format!("{at}: \"{old}\" occurs {n} times; quote more of it, or add all=\"true\""));
        }
        edit = edit.replace(old.as_str(), &new);
    }
    Ok(edit)
}
