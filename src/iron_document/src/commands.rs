//! Commands (plans/specs/contexts.md §4.2, "Menus and commands"): the
//! editor's named actions, one registry for every way of reaching them.
//!
//! A command is a mechanism, so it is code: the engine (or, for the few that
//! need the browser, the shell) runs it, and an engine command applies one
//! transaction. Which commands a view's keys reach (`keys.commands`) and
//! which its right-click menu offers (`menu.canvas`) are context data, as a
//! palette arranges [`crate::contexts::TOOLS`]: a notation, a document or a
//! model rearranges them by writing a `<context>` rule, never by code.
//!
//! The registry is also what the shell draws from: a menu item's label and
//! icon, and whether a key must start a clipboard write while the key event
//! still counts as a user gesture.

use crate::document::Document;
use crate::id::NodeId;
use crate::op::Op;
use crate::order::OrderKey;

/// Where a command runs.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Runs {
    /// In the engine: the key queue's systems, in key order with every other
    /// key, so an undo never overtakes the change before it.
    Engine,
    /// In the shell: what needs the browser (reading the clipboard) or the
    /// dock (showing the model's chat).
    Shell,
}

/// What a command does with the system clipboard, which only the shell can
/// reach.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Clip {
    None,
    /// Puts text there: the shell opens the write inside the user gesture
    /// and the engine supplies the text.
    Write,
    /// Reads it: the shell reads and hands the text to the engine.
    Read,
}

/// A registered command.
#[derive(Copy, Clone, Debug)]
pub struct Command {
    pub id: &'static str,
    /// What a menu says.
    pub label: &'static str,
    /// A Lucide icon name (kebab case); the shell maps the names it knows.
    pub icon: &'static str,
    /// It acts on the selection, so it is offered disabled without one.
    pub selection: bool,
    pub clip: Clip,
    pub runs: Runs,
    pub doc: &'static str,
}

const fn engine(id: &'static str, label: &'static str, icon: &'static str, selection: bool, doc: &'static str) -> Command {
    Command { id, label, icon, selection, clip: Clip::None, runs: Runs::Engine, doc }
}

/// Every command, in no particular order (a context orders them).
pub const COMMANDS: &[Command] = &[
    engine("undo", "Undo", "undo-2", false, "undo the last change"),
    engine("redo", "Redo", "redo-2", false, "redo what was undone"),
    engine("select-all", "Select all", "lasso-select", false, "select every sibling of the selection, or everything at the top of the view"),
    Command { id: "copy", label: "Copy", icon: "copy", selection: true, clip: Clip::Write, runs: Runs::Engine, doc: "copy the selection as an Iron fragment" },
    Command { id: "cut", label: "Cut", icon: "scissors", selection: true, clip: Clip::Write, runs: Runs::Engine, doc: "copy the selection and delete it; the first paste moves it" },
    Command { id: "copy-svg", label: "Copy as SVG", icon: "file-code", selection: true, clip: Clip::Write, runs: Runs::Engine, doc: "copy the selection as SVG text" },
    Command { id: "paste-here", label: "Paste here", icon: "clipboard-paste", selection: false, clip: Clip::Read, runs: Runs::Shell, doc: "paste the clipboard with its top left at the pointer" },
    engine("duplicate", "Duplicate", "copy-plus", true, "copy the selection beside itself"),
    engine("create-control", "Create control", "square-dashed", true, "create a control: click its pivot or use the selection centre"),
    engine("place-pivot", "Place pivot", "crosshair", true, "reposition a static control origin without moving artwork"),
    engine("cancel-pivot", "Cancel pivot placement", "x", false, "cancel pivot placement without changing the document"),
    engine("pivot-centre", "Use selection centre", "scan", true, "finish pivot placement at the selection centre"),
    engine("show-controls", "Show controls", "scan", false, "toggle controls in the current editing scope"),
    engine("layout", "Add auto layout", "rows-3", true, "give a group a layout, or wrap siblings in a laid-out group"),
    engine("make-component", "Create component", "component", true, "make a component of the selection: one instance of a new definition takes its place"),
    engine("front", "Bring to front", "bring-to-front", true, "draw the selection above its siblings"),
    engine("back", "Send to back", "send-to-back", true, "draw the selection below its siblings"),
    engine("draw-on", "Draw on", "pen-line", true, "key Trim End from 0 at the playhead to 100% a second later"),
    Command { id: "ask", label: "Ask the model", icon: "sparkles", selection: false, clip: Clip::None, runs: Runs::Shell, doc: "open the model's chat about the selection" },
    engine("zoom-fit", "Zoom to fit", "maximize", false, "fit the view to its content"),
    engine("zoom-selection", "Zoom to selection", "scan", false, "fit the view to the selection"),
    engine("zoom-100", "Zoom to 100%", "scaling", false, "show the view at actual size"),
    engine("zoom-in", "Zoom in", "zoom-in", false, "zoom the view in"),
    engine("zoom-out", "Zoom out", "zoom-out", false, "zoom the view out"),
];

pub fn find(id: &str) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.id == id)
}

/// A menu's divider, among a `menu.*` field's command ids.
pub const DIVIDER: &str = "|";

/// The modifiers a chord may hold, in their canonical order. `mod` is
/// Cmd on macOS and Ctrl elsewhere.
pub const MODIFIERS: &[&str] = &["mod", "alt", "shift"];

/// The keys a chord ends in, besides letters and digits: punctuation by the
/// character its key types unshifted (US layout), as `KeyboardEvent.code`
/// names the key.
pub const PUNCTUATION: &[&str] = &["=", "-", "[", "]", ";", "'", ",", ".", "/", "\\", "`"];

/// A chord in canonical form (`alt+mod+g` → `mod+alt+g`), or why it is not
/// one: modifiers from [`MODIFIERS`], each at most once, then one key.
pub fn chord(s: &str) -> Result<String, String> {
    // `-` is a key, so split from the right: the last part is the key.
    let (mods, key) = match s.rsplit_once('+') {
        Some((m, k)) if !k.is_empty() => (m, k),
        Some((m, _)) if m.ends_with('+') || m.is_empty() => return Err(format!("{s}: write + as shift+=")),
        Some(_) => return Err(format!("{s}: a chord ends in its key")),
        None => ("", s),
    };
    let ok = key.chars().count() == 1 && (key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) || PUNCTUATION.contains(&key));
    if !ok {
        return Err(format!("{s}: {key} is not a key (a letter, a digit, or one of {})", PUNCTUATION.join(" ")));
    }
    let mut held = [false; 3];
    for m in mods.split('+').filter(|m| !m.is_empty()) {
        let i = MODIFIERS.iter().position(|x| *x == m).ok_or_else(|| format!("{s}: {m} is not a modifier ({})", MODIFIERS.join(", ")))?;
        if held[i] {
            return Err(format!("{s}: {m} twice"));
        }
        held[i] = true;
    }
    let mut out: Vec<&str> = MODIFIERS.iter().zip(held).filter(|(_, h)| *h).map(|(m, _)| *m).collect();
    out.push(key);
    Ok(out.join("+"))
}

/// A `keys.commands` value as (canonical chord, command) pairs, in order;
/// malformed pairs are skipped (the parser refused them already).
pub fn keymap(v: &str) -> Vec<(String, &'static str)> {
    v.split_whitespace()
        .filter_map(|pair| {
            let (c, id) = pair.split_once(':')?;
            Some((chord(c).ok()?, find(id)?.id))
        })
        .collect()
}

/// Two keymaps as one: `over`'s chords win, then `base`'s others.
pub fn merge_keymaps(base: &str, over: &str) -> String {
    let mine = keymap(over);
    let mut out: Vec<String> = mine.iter().map(|(c, id)| format!("{c}:{id}")).collect();
    out.extend(keymap(base).into_iter().filter(|(c, _)| !mine.iter().any(|(m, _)| m == c)).map(|(c, id)| format!("{c}:{id}")));
    out.join(" ")
}

/// Check a `menu.*` value: command ids and dividers.
pub fn check_menu(v: &str) -> Result<(), String> {
    match v.split_whitespace().find(|w| *w != DIVIDER && find(w).is_none()) {
        Some(w) => Err(format!("unknown command {w} ({}, or {DIVIDER} between groups)", COMMANDS.iter().map(|c| c.id).collect::<Vec<_>>().join(", "))),
        None => Ok(()),
    }
}

/// Check a `keys.commands` value: `chord:command` pairs.
pub fn check_keymap(v: &str) -> Result<(), String> {
    for pair in v.split_whitespace() {
        let (c, id) = pair.split_once(':').ok_or_else(|| format!("{pair} is not chord:command"))?;
        chord(c)?;
        if find(id).is_none() {
            return Err(format!("unknown command {id}"));
        }
    }
    Ok(())
}

/// The registry as JSON, for the shell and the model's manifest.
pub fn registry_json() -> serde_json::Value {
    serde_json::Value::Array(
        COMMANDS
            .iter()
            .map(|c| {
                serde_json::json!({
                    "id": c.id,
                    "label": c.label,
                    "icon": c.icon,
                    "selection": c.selection,
                    "clip": match c.clip { Clip::None => "none", Clip::Write => "write", Clip::Read => "read" },
                    "runs": match c.runs { Runs::Engine => "engine", Runs::Shell => "shell" },
                    "doc": c.doc,
                })
            })
            .collect(),
    )
}

/// Bring to front or send to back: the selection's outermost nodes, under
/// each parent, after (or before) every sibling, keeping their own order.
/// A later sibling draws on top.
pub fn restack_ops(doc: &Document, selected: &[NodeId], front: bool) -> Result<Vec<Op>, String> {
    let roots: Vec<NodeId> = selected.iter().copied().filter(|&id| !selected.iter().any(|&o| o != id && doc.is_ancestor(o, id))).collect();
    if roots.iter().any(|n| n.is_expanded()) {
        return Err("An instance's parts keep their definition's order".into());
    }
    let mut parents: Vec<NodeId> = Vec::new();
    for &n in &roots {
        if let Some(p) = doc.live(n).and_then(|n| n.parent)
            && !parents.contains(&p)
        {
            parents.push(p);
        }
    }
    let mut ops = Vec::new();
    for p in parents {
        let mut siblings: Vec<(OrderKey, NodeId)> = doc.children(p).iter().filter_map(|&c| doc.live(c).map(|n| (n.order.clone(), c))).collect();
        siblings.sort_by(|a, b| a.0.cmp(&b.0));
        let moving: Vec<&(OrderKey, NodeId)> = siblings.iter().filter(|(_, c)| roots.contains(c)).collect();
        // Already there: the selected ones are the last (or first) siblings.
        let k = moving.len();
        let there = if front { siblings[siblings.len() - k..].iter().all(|(_, c)| roots.contains(c)) } else { siblings[..k].iter().all(|(_, c)| roots.contains(c)) };
        if there {
            continue;
        }
        if front {
            let mut prev = siblings.last().expect("a parent of a selected node has children").0.clone();
            for (_, c) in moving {
                prev = OrderKey::after(&prev);
                ops.push(Op::Reorder { id: *c, order: prev.clone() });
            }
        } else {
            let mut next = siblings[0].0.clone();
            for (_, c) in moving.into_iter().rev() {
                next = OrderKey::before(&next);
                ops.push(Op::Reorder { id: *c, order: next.clone() });
            }
        }
    }
    if ops.is_empty() {
        return Err(if front { "Already in front" } else { "Already at the back" }.into());
    }
    Ok(ops)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chords_are_canonical() {
        assert_eq!(chord("alt+mod+g").unwrap(), "mod+alt+g");
        assert_eq!(chord("shift+1").unwrap(), "shift+1");
        assert_eq!(chord("mod+-").unwrap(), "mod+-");
        assert_eq!(chord("-").unwrap(), "-");
        assert_eq!(chord("]").unwrap(), "]");
        assert!(chord("ctrl+g").is_err());
        assert!(chord("mod+mod+g").is_err());
        assert!(chord("mod+").is_err());
        assert!(chord("mod+gg").is_err());
    }

    #[test]
    fn keymaps_merge_over_their_base() {
        let merged = merge_keymaps("mod+z:undo mod+c:copy", "mod+c:copy-svg ]:front");
        assert_eq!(keymap(&merged), vec![("mod+c".to_owned(), "copy-svg"), ("]".to_owned(), "front"), ("mod+z".to_owned(), "undo")]);
    }

    #[test]
    fn ids_are_unique() {
        for (i, c) in COMMANDS.iter().enumerate() {
            assert!(COMMANDS[..i].iter().all(|d| d.id != c.id), "{} twice", c.id);
        }
    }
}
