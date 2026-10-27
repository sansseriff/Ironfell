//! Contexts: Iron's behaviour as data (plans/specs/contexts.md).
//!
//! A **context** is a list of **rules**; a rule matches nodes by position
//! (`self`, `children`, `all`), type and state, and declares `domain.field`
//! values. The engine registers the fields ([`FIELDS`]) and owns the
//! mechanisms that read them; which value a node gets is data. The built-in
//! contexts (`std:*`, `std_contexts.xml`) are the user-agent stylesheet:
//! embedded, read-only, never saved.
//!
//! Resolution is local (contexts.md §1.1). A node's values come from three
//! places only, weakest first: its notation's context (rules that match by
//! type), its own context's `self` rules, and its direct container's
//! `children` rules. A container whose node is `through` is passed over, as
//! CSS's `display: contents` is, so a table's rules reach its cells through
//! their rows. Nothing comes from a grandparent. Later rules win within a
//! context, field by field.

use crate::document::Document;
use crate::id::NodeId;
use crate::registry::TypeId;
use std::sync::{Arc, OnceLock};

/// What values a field takes.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// One of the field's `options`; the first is the default.
    Choice,
    /// A palette: tools from [`TOOLS`], space-separated, `|` joining tools
    /// that share a slot (`select rect|ellipse pen`).
    Tools,
    /// A keymap: `key:tool` pairs, space-separated (`v:select r:rect`).
    Keys,
    /// Any of these words, space-separated (`box shape`); empty for none.
    Words(&'static [&'static str]),
    /// A declared transition (affordances.md §8.2): `<ease> <ms> [<when>]`,
    /// or `none`.
    Motion,
    /// One of the node's own leaves (`transform2d.rot`), or empty.
    Leaf,
    /// A number.
    Number,
    /// A colour, `#rrggbb` or `#rrggbbaa`.
    Color,
}

/// A declared transition, parsed (`motion.placement="out 160 layout"`).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Motion {
    /// `linear`, `in`, `out` or `in_out`.
    pub ease: &'static str,
    pub ms: f64,
    /// `any`, `layout` (the change came from layout), `state` or `gesture`.
    pub when: &'static str,
}

const EASES: &[&str] = &["linear", "in", "out", "in_out"];
const WHENS: &[&str] = &["any", "layout", "state", "gesture"];

/// A motion value, or `None` for `none`.
pub fn motion(value: &str) -> Option<Motion> {
    let mut w = value.split_whitespace();
    let ease = EASES.iter().find(|&&e| Some(e) == w.clone().next())?;
    w.next();
    let ms = w.next()?.parse::<f64>().ok().filter(|m| *m >= 0.0)?;
    let when = match w.next() {
        Some(x) => WHENS.iter().find(|&&e| e == x)?,
        None => &"any",
    };
    Some(Motion { ease, ms, when })
}

/// A registered field: its domain, its name, and the values it takes. For a
/// choice the first option is the universal default, what a node has when no
/// rule says otherwise; a list's default is its one option.
pub struct FieldSpec {
    pub domain: &'static str,
    pub name: &'static str,
    pub kind: Kind,
    /// A view's field (`match="view"` rules), or a node's.
    pub view: bool,
    pub options: &'static [&'static str],
    pub doc: &'static str,
}

/// Every field a context may declare. A domain is added with the mechanism
/// that reads it (contexts.md §4.1's admission test). `affords`, `chrome`
/// and `snap` fields are a node's; `palette` and `keys` a view's (§2.7).
pub const FIELDS: &[FieldSpec] = &[
    FieldSpec {
        domain: "affords",
        name: "select",
        view: false,
        kind: Kind::Choice,
        options: &["none", "target", "through", "closed", "window", "inert"],
        doc: "what a press reaches: a target; passed through to its children; a target never entered; a window (at the top of a view its content selects directly); inert; or nothing",
    },
    FieldSpec { domain: "affords", name: "enter", view: false, kind: Kind::Choice, options: &["select", "edit"], doc: "what entering this node from its container does: select it, or start its edit" },
    FieldSpec { domain: "affords", name: "edit", view: false, kind: Kind::Choice, options: &["none", "text", "points"], doc: "what editing in place means: its words, or its points" },
    FieldSpec { domain: "chrome", name: "lit", view: false, kind: Kind::Choice, options: &["shape", "frame", "none"], doc: "how a selected or hovered node lights: its outline, its frame, or not at all" },
    FieldSpec { domain: "snap", name: "target", view: false, kind: Kind::Choice, options: &["no", "yes"], doc: "whether a moving selection snaps to it" },
    FieldSpec {
        domain: "affords",
        name: "grips",
        view: false,
        kind: Kind::Words(&["box", "shape"]),
        options: &[""],
        doc: "the handles a selected node offers: box (resize and rotate), shape (a rect's corner radii, an ellipse's arc)",
    },
    FieldSpec {
        domain: "affords",
        name: "drag",
        view: false,
        kind: Kind::Choice,
        options: &["none", "point", "track", "ring", "sequence"],
        doc: "what a drag on it does: moves it (point); drives a value along a track (a slider's, or `drives` along `axis`) or around its origin (ring, `drives` in radians); or moves it among its container's items (sequence; the others open a gap)",
    },
    FieldSpec { domain: "affords", name: "drives", view: false, kind: Kind::Leaf, options: &[""], doc: "the node's own leaf a track, ring or activate changes, through its binding (a bound leaf moves its source; one that cannot follow refuses)" },
    FieldSpec { domain: "affords", name: "activate", view: false, kind: Kind::Choice, options: &["none", "toggle", "step", "cycle"], doc: "what a click does to `drives`: flips a yes/no, adds `step`, or picks the next of an enum's values" },
    FieldSpec { domain: "affords", name: "step", view: false, kind: Kind::Number, options: &["1"], doc: "what `activate=step` adds; a ring's snap with Shift (radians)" },
    FieldSpec { domain: "affords", name: "axis", view: false, kind: Kind::Choice, options: &["x", "y", "-x", "-y"], doc: "the direction a track drag increases `drives`, in the node's parent's space" },
    FieldSpec {
        domain: "keys",
        name: "nav",
        view: false,
        kind: Kind::Choice,
        options: &["none", "list-x", "list-y", "grid", "tree"],
        doc: "how arrows, Tab and Enter move among a container's items (ARIA's composites): along a row or a column (Alt moves the item), cell to cell, or not (nudge)",
    },
    FieldSpec {
        domain: "motion",
        name: "placement",
        view: false,
        kind: Kind::Motion,
        options: &["none"],
        doc: "how a node's position eases when it changes: <ease> <ms> [<when>], ease linear|in|out|in_out, when any|layout|state|gesture; or none",
    },
    FieldSpec { domain: "palette", name: "tools", view: true, kind: Kind::Tools, options: &[""], doc: "a view's tools, in order; a|b share one slot (match=\"view\")" },
    FieldSpec { domain: "keys", name: "tools", view: true, kind: Kind::Keys, options: &[""], doc: "a view's tool keys, key:tool pairs (match=\"view\")" },
    FieldSpec { domain: "tolerance", name: "slop", view: true, kind: Kind::Number, options: &["3"], doc: "window px a press may travel and still be a click (match=\"view\")" },
    FieldSpec { domain: "tolerance", name: "double_click", view: true, kind: Kind::Number, options: &["400"], doc: "ms between two clicks that make a double-click (match=\"view\")" },
    FieldSpec { domain: "tolerance", name: "hit", view: true, kind: Kind::Number, options: &["1"], doc: "how far grips and handles reach, times their usual size (match=\"view\"; a touch context's is larger)" },
    FieldSpec {
        domain: "inspector",
        name: "sections",
        view: true,
        kind: Kind::Words(SECTIONS),
        options: &["main port component table position text layout appearance fill stroke effects other add"],
        doc: "the inspector's sections for a selection in this view, in order; each still shows only where its facets are (match=\"view\")",
    },
    FieldSpec { domain: "chrome", name: "handle", view: false, kind: Kind::Choice, options: &["none", "dot"], doc: "a control part's own handle, shown at its origin while it or its container is selected or hovered (chrome.md §5)" },
    FieldSpec { domain: "chrome", name: "accent", view: true, kind: Kind::Color, options: &["#0d99ffff"], doc: "the colour of selection boxes, grips, outlines and handles (match=\"view\")" },
    FieldSpec { domain: "chrome", name: "grip", view: true, kind: Kind::Number, options: &["8"], doc: "a selection box's grip size, CSS px (match=\"view\")" },
    FieldSpec { domain: "chrome", name: "grip_shape", view: true, kind: Kind::Choice, options: &["square", "round"], doc: "a selection box's grips (match=\"view\")" },
    FieldSpec { domain: "chrome", name: "hover", view: true, kind: Kind::Choice, options: &["on", "off"], doc: "whether hovering lights things (off for touch, where nothing hovers; match=\"view\")" },
];

/// The inspector's sections (contexts.md §2.7): rich widgets the shell draws,
/// each showing only where its node has (or may add) what it edits.
pub const SECTIONS: &[&str] = &["main", "port", "component", "table", "position", "text", "layout", "appearance", "fill", "stroke", "effects", "other", "add"];

/// The tools the engine has: mechanisms a palette arranges and a keymap
/// reaches.
pub const TOOLS: &[&str] = &["select", "pen", "rect", "ellipse", "text", "table", "port", "empty"];

/// The states a rule may match: a node's interaction state (affordances.md
/// §8.1; nothing sets these yet) and a view's focus (§2.7).
pub const STATES: &[&str] = &["hovered", "selected", "focused", "pressed", "dragged", "drop-target", "editing-text", "editing-points", "main"];

/// A field, by its index in [`FIELDS`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Field(u8);

impl Field {
    pub const SELECT: Field = Field(0);
    pub const ENTER: Field = Field(1);
    pub const EDIT: Field = Field(2);
    pub const LIT: Field = Field(3);
    pub const SNAP_TARGET: Field = Field(4);
    pub const GRIPS: Field = Field(5);
    pub const DRAG: Field = Field(6);
    pub const DRIVES: Field = Field(7);
    pub const ACTIVATE: Field = Field(8);
    pub const STEP: Field = Field(9);
    pub const AXIS: Field = Field(10);
    pub const NAV: Field = Field(11);
    pub const PLACEMENT: Field = Field(12);
    pub const PALETTE: Field = Field(13);
    pub const KEYS: Field = Field(14);
    pub const SLOP: Field = Field(15);
    pub const DOUBLE_CLICK: Field = Field(16);
    pub const HIT: Field = Field(17);
    pub const SECTIONS: Field = Field(18);
    pub const HANDLE: Field = Field(19);
    pub const ACCENT: Field = Field(20);
    pub const GRIP: Field = Field(21);
    pub const GRIP_SHAPE: Field = Field(22);
    pub const HOVER: Field = Field(23);

    pub fn spec(self) -> &'static FieldSpec {
        &FIELDS[self.0 as usize]
    }

    /// `domain.field`.
    pub fn path(self) -> String {
        format!("{}.{}", self.spec().domain, self.spec().name)
    }

    pub fn lookup(path: &str) -> Option<Field> {
        let (domain, name) = path.split_once('.')?;
        FIELDS.iter().position(|f| f.domain == domain && f.name == name).map(|i| Field(i as u8))
    }

    pub fn all() -> impl Iterator<Item = Field> {
        (0..FIELDS.len()).map(|i| Field(i as u8))
    }

    /// A node's fields, or a view's.
    pub fn of_view(self) -> bool {
        self.spec().view
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Position {
    /// The context's own node and its children.
    All,
    /// The context's own node.
    Own,
    /// Its direct children.
    Children,
    /// A view whose focus is in this context (§2.7).
    View,
}

/// A declared value: an index into a choice's options, or a list's text.
#[derive(Clone, Debug, PartialEq)]
enum Val {
    Choice(u8),
    Text(Arc<str>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    position: Position,
    /// Any of these types; empty for any type.
    types: Vec<TypeId>,
    /// A state from [`STATES`]. A view's focus supplies its states; a node's
    /// come from the host's interaction state (`Store::set_interaction`,
    /// affordances.md §8.1).
    state: Option<String>,
    decls: Vec<(Field, Val)>,
    /// The document `context` node a rule came from, and its index there.
    origin: Option<(NodeId, u16)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Context {
    pub id: String,
    /// The node types that carry this context as containers (`for`).
    pub for_types: Vec<TypeId>,
    /// And a leaf those nodes' constant must be one of (`for="type:group
    /// layout.mode=stack_x"`): a stack is a group laid out in a line.
    pub for_leaf: Option<(String, Vec<String>)>,
    /// A built-in context whose rules come first (a document context's
    /// `extends`; one level, built-ins do not chain).
    pub extends: Option<String>,
    pub rules: Vec<Rule>,
}

/// Where a resolved value came from: a context and the index of its rule.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Source {
    pub context: u16,
    pub rule: u16,
    /// The node whose inline `affords` facet declared it, for an inline
    /// exception (`context` is then [`INLINE`]).
    pub node: Option<NodeId>,
}

/// [`Source::context`] for a value an inline `affords` facet declared.
pub const INLINE: u16 = u16::MAX;

/// A node's or a view's resolved values, each with its source (`None`: the
/// universal default).
#[derive(Clone, Debug, PartialEq)]
pub struct Values {
    slots: [(Val, Option<Source>); FIELDS_LEN],
}

const FIELDS_LEN: usize = FIELDS.len();

impl Default for Values {
    fn default() -> Self {
        // Every field's first option is its default, a list's included: no
        // allocation for a node no rule touches.
        Values { slots: std::array::from_fn(|_| (Val::Choice(0), None)) }
    }
}

impl Values {
    pub fn get(&self, f: Field) -> &str {
        match &self.slots[f.0 as usize].0 {
            Val::Choice(i) => f.spec().options[*i as usize],
            Val::Text(t) => t,
        }
    }

    pub fn is(&self, f: Field, value: &str) -> bool {
        self.get(f) == value
    }

    /// Whether a words field holds `word`.
    pub fn has(&self, f: Field, word: &str) -> bool {
        self.get(f).split_whitespace().any(|w| w == word)
    }

    pub fn source(&self, f: Field) -> Option<Source> {
        self.slots[f.0 as usize].1
    }

    /// Selectable by a press, a marquee or select-all.
    pub fn selectable(&self) -> bool {
        matches!(self.get(Field::SELECT), "target" | "closed" | "window")
    }

    fn apply(&mut self, rule: &Rule, source: Source) {
        for (f, v) in &rule.decls {
            self.slots[f.0 as usize] = (v.clone(), Some(source));
        }
    }
}

/// A view's focus (contexts.md §2.7): the world its tools create into, the
/// open scope (the chain the selection is inside), and its states
/// (`editing-text`, `editing-points`, `main`). What the view offers, its
/// palette and keymap, resolves against it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Focus {
    /// The composition the view's tools create into.
    pub world: Option<NodeId>,
    /// The containers the selection is inside, outermost first; empty at
    /// the top level.
    pub scope: Vec<NodeId>,
    pub states: Vec<&'static str>,
}

/// A set of contexts: the built-in library, or it with a document's
/// overrides.
#[derive(Clone, Debug, PartialEq)]
pub struct Library {
    pub contexts: Vec<Context>,
}

static BUILTIN: OnceLock<Library> = OnceLock::new();

impl Library {
    /// The built-in contexts, parsed once.
    pub fn builtin() -> &'static Library {
        BUILTIN.get_or_init(|| Library { contexts: parse(include_str!("std_contexts.xml")).expect("std_contexts.xml parses") })
    }

    /// The built-in library with a document's contexts over it: one whose
    /// name is already there adds its rules after that context's (so they
    /// win), any other is new (a notation may name it, and it may `extend`
    /// a built-in one). Markup that does not parse is left out; the store
    /// refuses it when it is written.
    pub fn with_document(markups: &[(NodeId, String)]) -> Library {
        let mut lib = Library::builtin().clone();
        for (node, markup) in markups {
            let Ok(contexts) = parse(markup) else { continue };
            for mut c in contexts {
                for (i, r) in c.rules.iter_mut().enumerate() {
                    r.origin = Some((*node, i as u16));
                }
                match lib.get(&c.id) {
                    Some(k) => {
                        let into = &mut lib.contexts[k];
                        into.rules.extend(c.rules);
                        into.for_types.extend(c.for_types.into_iter().filter(|t| !into.for_types.contains(t)).collect::<Vec<_>>());
                        into.for_leaf = c.for_leaf.or(into.for_leaf.take());
                    }
                    None => lib.contexts.push(c),
                }
            }
        }
        lib
    }

    pub fn get(&self, id: &str) -> Option<usize> {
        self.contexts.iter().position(|c| c.id == id)
    }

    /// The context a container carries, if any: the first whose `for`
    /// names its type (and, if it says, a leaf's value).
    fn for_node(&self, doc: &Document, id: NodeId) -> Option<usize> {
        let ty = doc.live(id)?.ty;
        // A `context` facet names the container's context outright.
        if let Some(name) = doc.get::<crate::facet::ContextOf>(id).and_then(|c| c.name.constant().and_then(|v| v.as_str()).map(str::to_owned)).filter(|n| !n.is_empty()) {
            return self.get(&name);
        }
        self.contexts.iter().position(|c| {
            c.for_types.contains(&ty)
                && c.for_leaf.as_ref().is_none_or(|(leaf, values)| {
                    let value = leaf.parse().ok().and_then(|p| doc.leaf(id, p)).and_then(|s| s.constant()).and_then(|v| v.as_str()).map(str::to_owned);
                    value.is_some_and(|v| values.contains(&v))
                })
        })
    }

    /// `std:free-2d` for a 2D world, `std:3d` for a 3D one; a notation that
    /// names a context in the library is that context.
    fn notation(&self, notation: &str, three_d: bool) -> Option<usize> {
        self.get(notation).or_else(|| self.get(if three_d { "std:3d" } else { "std:free-2d" }))
    }

    /// Where a value came from, for views: `std:table rule 1`.
    pub fn describe(&self, s: Source) -> String {
        if s.context == INLINE {
            return match s.node {
                Some(n) => format!("inline on {n}"),
                None => "inline".into(),
            };
        }
        let c = &self.contexts[s.context as usize];
        match c.rules[s.rule as usize].origin {
            Some((node, i)) => format!("{} rule {} from {node}", c.id, i + 1),
            None => format!("{} rule {}", c.id, s.rule + 1),
        }
    }

    /// The library in its markup: what a model reads.
    pub fn print(&self) -> String {
        self.contexts.iter().map(print_context).collect::<Vec<_>>().join("\n")
    }
}

pub fn print_context(c: &Context) -> String {
    let mut out = format!("<context name=\"{}\"", c.id);
    if let Some(e) = &c.extends {
        out += &format!(" extends=\"{e}\"");
    }
    if !c.for_types.is_empty() {
        out += &format!(" for=\"type:{}", c.for_types.iter().map(|t| t.name()).collect::<Vec<_>>().join("|"));
        if let Some((leaf, values)) = &c.for_leaf {
            out += &format!(" {leaf}={}", values.join("|"));
        }
        out += "\"";
    }
    out += ">\n";
    for r in &c.rules {
        let mut m: Vec<String> = Vec::new();
        match r.position {
            Position::All => {}
            Position::Own => m.push("self".into()),
            Position::Children => m.push("children".into()),
            Position::View => m.push("view".into()),
        }
        if !r.types.is_empty() {
            m.push(format!("type:{}", r.types.iter().map(|t| t.name()).collect::<Vec<_>>().join("|")));
        }
        if let Some(s) = &r.state {
            m.push(format!("state:{s}"));
        }
        let m = if m.is_empty() { "all".to_owned() } else { m.join(" ") };
        out += &format!("  <rule match=\"{m}\"");
        for (f, v) in &r.decls {
            let v = match v {
                Val::Choice(i) => f.spec().options[*i as usize],
                Val::Text(t) => t,
            };
            out += &format!(" {}=\"{v}\"", f.path());
        }
        out += "/>\n";
    }
    out + "</context>"
}

/// Resolves nodes' values against a library, for one document.
pub struct Resolver<'a> {
    pub doc: &'a Document,
    pub lib: &'a Library,
    /// Nodes' interaction states (`hovered`, `selected`, …), for rules that
    /// match `state:`; none without a host.
    pub states: Option<&'a std::collections::BTreeMap<NodeId, Vec<&'static str>>>,
}

impl<'a> Resolver<'a> {
    pub fn new(doc: &'a Document, lib: &'a Library) -> Self {
        Resolver { doc, lib, states: None }
    }

    /// The values of `chain[i]`, where `chain` is the path of containers a
    /// view reaches it through (embeds and instances included).
    pub fn values(&self, chain: &[NodeId], i: usize) -> Values {
        let mut v = Values::default();
        let Some(node) = self.doc.live(chain[i]) else { return v };
        let ty = node.ty;
        let container = self.container(chain, i);
        let states: &[&str] = self.states.and_then(|s| s.get(&chain[i])).map_or(&[], |v| v.as_slice());
        // 1. The notation's context, for the node's type.
        if let Some(c) = self.notation_of(chain[i]) {
            self.apply(&mut v, c, Some(ty), states, |p| p == Position::All || (p == Position::Children && container.is_none()));
        }
        // 2. Its own context's rules for itself.
        if let Some(c) = self.lib.for_node(self.doc, chain[i]) {
            self.apply(&mut v, c, Some(ty), states, |p| p == Position::All || p == Position::Own);
        }
        // 3. Its direct container's rules for its children.
        if let Some(c) = container.and_then(|n| self.lib.for_node(self.doc, n)) {
            self.apply(&mut v, c, Some(ty), states, |p| p == Position::All || p == Position::Children);
        }
        // 4. Inline exceptions (`affords` facets): its container's for its
        //    children, then its own.
        if let Some(c) = container {
            self.apply_inline(&mut v, c, "children");
        }
        self.apply_inline(&mut v, chain[i], "self");
        v
    }

    fn apply_inline(&self, v: &mut Values, node: NodeId, scope: &str) {
        let Some(f) = self.doc.get::<crate::facet::Affords>(node) else { return };
        if f.scope.constant().and_then(|s| s.as_str()).unwrap_or("self") != scope {
            return;
        }
        let style = f.style.constant().and_then(|s| s.as_str()).unwrap_or("");
        if let Ok(rule) = parse_style(style, scope) {
            v.apply(&rule, Source { context: INLINE, rule: 0, node: Some(node) });
        }
    }

    /// The effective container of `chain[i]`: the nearest node before it
    /// along the chain that is not passed through.
    fn container(&self, chain: &[NodeId], i: usize) -> Option<NodeId> {
        (0..i).rev().find(|&k| !self.values(chain, k).is(Field::SELECT, "through")).map(|k| chain[k])
    }

    pub fn get(&self, chain: &[NodeId], i: usize, f: Field) -> String {
        self.values(chain, i).get(f).to_owned()
    }

    /// What a view offers (contexts.md §2.7), from its focus: the view rules
    /// of its world's notation context, then those of the context of the
    /// container its selection is open in, each matching the focus's states.
    pub fn view_values(&self, focus: &Focus) -> Values {
        let mut v = Values::default();
        let notation = match focus.world {
            Some(w) => self.notation_of(w),
            None => self.lib.notation("free", false),
        };
        if let Some(c) = notation {
            self.apply(&mut v, c, None, &focus.states, |p| p == Position::View);
        }
        let container = match focus.scope.len() {
            0 => None,
            n => {
                let mut chain = focus.scope.clone();
                chain.push(NodeId::from_raw(0));
                // The open scope's innermost node not passed through.
                (0..n).rev().find(|&k| !self.values(&chain, k).is(Field::SELECT, "through")).map(|k| chain[k])
            }
        };
        if let Some(c) = container.and_then(|n| self.lib.for_node(self.doc, n)) {
            self.apply(&mut v, c, None, &focus.states, |p| p == Position::View);
        }
        v
    }

    /// Apply a context's matching rules (after those of the built-in it
    /// extends). `ty` is `None` for a view, whose rules match no type.
    fn apply(&self, v: &mut Values, c: usize, ty: Option<TypeId>, states: &[&str], position: impl Fn(Position) -> bool) {
        let ctx = &self.lib.contexts[c];
        if let Some(base) = ctx.extends.as_deref().and_then(|b| self.lib.get(b)).filter(|&b| b != c) {
            self.apply_own(v, base, ty, states, &position);
        }
        self.apply_own(v, c, ty, states, &position);
    }

    fn apply_own(&self, v: &mut Values, c: usize, ty: Option<TypeId>, states: &[&str], position: &impl Fn(Position) -> bool) {
        for (r, rule) in self.lib.contexts[c].rules.iter().enumerate() {
            let typed = match ty {
                Some(t) => rule.types.is_empty() || rule.types.contains(&t),
                None => rule.types.is_empty(),
            };
            let state = rule.state.as_deref().is_none_or(|s| states.contains(&s));
            if position(rule.position) && typed && state {
                v.apply(rule, Source { context: c as u16, rule: r as u16, node: None });
            }
        }
    }

    /// The notation context of the world a node is in (a composition is its
    /// own world): its composition's, or, in a definition's template, the
    /// template's notation.
    fn notation_of(&self, id: NodeId) -> Option<usize> {
        let mut cur = self.doc.live(id);
        while let Some(n) = cur {
            match n.ty.name() {
                "composition" => {
                    let c = self.doc.get::<crate::facet::Composition>(n.id)?;
                    let notation = c.notation.constant().and_then(|v| v.as_str()).unwrap_or("free");
                    let dims = c.dims.constant().and_then(|v| v.as_str()).unwrap_or("2d");
                    return self.lib.notation(notation, dims == "3d");
                }
                "template" => {
                    let notation = self.doc.get::<crate::facet::Template>(n.id).and_then(|t| t.notation.constant().and_then(|v| v.as_str()).map(str::to_owned));
                    return self.lib.notation(notation.as_deref().unwrap_or("free"), false);
                }
                _ => cur = n.parent.and_then(|p| self.doc.live(p)),
            }
        }
        self.lib.notation("free", false)
    }

    /// Where a field's value came from: a rule, an inline facet, or the
    /// default.
    pub fn describe_source(&self, v: &Values, f: Field) -> String {
        v.source(f).map(|s| self.lib.describe(s)).unwrap_or_else(|| "default".into())
    }

    fn describe_all(&self, v: &Values, view: bool) -> Vec<(String, String, String)> {
        Field::all()
            .filter(|f| f.of_view() == view)
            .map(|f| (f.path(), v.get(f).to_owned(), v.source(f).map(|s| self.lib.describe(s)).unwrap_or_else(|| "default".into())))
            .collect()
    }

    /// Every node field's value for a node, with where it came from: the
    /// computed view (contexts.md §2.6).
    pub fn computed(&self, chain: &[NodeId], i: usize) -> Vec<(String, String, String)> {
        self.describe_all(&self.values(chain, i), false)
    }

    /// Every view field's value for a focus, with where it came from.
    pub fn computed_view(&self, focus: &Focus) -> Vec<(String, String, String)> {
        self.describe_all(&self.view_values(focus), true)
    }

    /// The computed view as a model reads it: one `domain.field="value"`
    /// per line, with `.by` naming the rule wherever one set it.
    pub fn computed_markup(&self, chain: &[NodeId], i: usize) -> String {
        markup(&format!("<computed node=\"{}\">", chain[i]), self.computed(chain, i))
    }

    pub fn computed_view_markup(&self, label: &str, focus: &Focus) -> String {
        let states = if focus.states.is_empty() { String::new() } else { format!(" states=\"{}\"", focus.states.join(" ")) };
        markup(&format!("<computed view=\"{label}\"{states}>"), self.computed_view(focus))
    }
}

fn markup(open: &str, values: Vec<(String, String, String)>) -> String {
    let mut out = format!("{open}\n");
    for (field, value, by) in values {
        out += &format!("  {field}=\"{value}\"");
        if by != "default" {
            out += &format!(" {field}.by=\"{by}\"");
        }
        out += "\n";
    }
    out + "</computed>"
}

/// The tools of a palette value, in order, one slot per inner list.
pub fn palette(value: &str) -> Vec<Vec<&str>> {
    value.split_whitespace().map(|slot| slot.split('|').collect()).collect()
}

/// The `(key, tool)` pairs of a keymap value.
pub fn keymap(value: &str) -> Vec<(&str, &str)> {
    value.split_whitespace().filter_map(|p| p.split_once(':')).collect()
}

/// The chain a node is reached by through its own containers, from its
/// world's top level down: what a read of one node resolves against when no
/// view says otherwise.
pub fn chain_of(doc: &Document, id: NodeId) -> Vec<NodeId> {
    let mut chain = Vec::new();
    let mut cur = doc.live(id);
    while let Some(n) = cur {
        if matches!(n.ty.name(), "composition" | "template" | "library" | "definition") {
            break;
        }
        chain.push(n.id);
        cur = n.parent.and_then(|p| doc.live(p));
    }
    chain.reverse();
    chain
}

/// An inline `affords` facet's style (affordances.md §4.4), CSS's `style`
/// attribute for behaviour: `field: value` pairs separated by `;`, a bare
/// field meaning the `affords` domain (`drag: ring; drives: transform2d.rot;
/// motion.placement: out 120`). Parsed as one rule for `scope` (`self` or
/// `children`).
pub fn parse_style(style: &str, scope: &str) -> Result<Rule, String> {
    let mut attrs = vec![("match".to_owned(), scope.to_owned())];
    for part in style.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let (k, v) = part.split_once(':').ok_or_else(|| format!("{part}: expected field: value"))?;
        let k = k.trim();
        let k = if k.contains('.') { k.to_owned() } else { format!("affords.{k}") };
        attrs.push((k, v.trim().to_owned()));
    }
    rule(attrs)
}

/// Ops writing one context into a document (the model's `<context>` verb):
/// a document context with the same id has its markup replaced; otherwise
/// a `context` node is made in the library, and the library too if there
/// is none. Returns the ops and the context node.
pub fn put_ops(doc: &Document, markup: &str) -> Result<(Vec<crate::op::Op>, NodeId), String> {
    use crate::facet::{ContextDef, LeafStruct, Name};
    use crate::op::Op;
    use crate::order::OrderKey;
    use crate::value::{Slot, Value};
    let parsed = parse(markup)?;
    let [c] = &parsed[..] else { return Err("write one <context> at a time".into()) };
    let ty = |n: &str| TypeId::lookup(n).expect("registered");
    let markup_of = |n: NodeId| doc.get::<ContextDef>(n).and_then(|d| d.markup.constant().and_then(Value::as_str).map(str::to_owned));
    for n in doc.nodes_of_type(ty("context")).filter(|&n| doc.is_live(n)) {
        if markup_of(n).and_then(|m| parse(&m).ok()).is_some_and(|p| p.iter().any(|x| x.id == c.id)) {
            let path = "context_def.markup".parse().expect("declared leaf");
            return Ok((vec![Op::Set { id: n, path, slot: Slot::Const(Value::Str(markup.to_owned())) }], n));
        }
    }
    let mut ids = doc.next_ids(2).into_iter();
    let mut ops = Vec::new();
    let library = match doc.roots().iter().copied().find(|&r| doc.live(r).is_some_and(|n| n.ty.name() == "library")) {
        Some(l) => l,
        None => {
            let l = ids.next().expect("id");
            let after = doc.roots().iter().filter_map(|&r| doc.live(r)).map(|n| n.order.clone()).max();
            ops.push(Op::Create { id: l, ty: ty("library"), parent: None, order: after.map_or_else(OrderKey::first, |k| OrderKey::after(&k)), facets: vec![] });
            l
        }
    };
    let node = ids.next().expect("id");
    let last = doc.children(library).iter().filter_map(|&k| doc.live(k)).map(|n| n.order.clone()).max();
    let def = ContextDef { markup: Slot::Const(Value::Str(markup.to_owned())) };
    let name = Name { text: Slot::Const(Value::Str(c.id.clone())) };
    ops.push(Op::Create { id: node, ty: ty("context"), parent: Some(library), order: last.map_or_else(OrderKey::first, |k| OrderKey::after(&k)), facets: vec![def.wrap(), name.wrap()] });
    Ok((ops, node))
}

// ---- Markup ----

/// Parse contexts written in markup: `<context name=… for=…>` holding
/// `<rule match=… domain.field=…/>`, with `<!-- comments -->`.
pub fn parse(src: &str) -> Result<Vec<Context>, String> {
    let mut out: Vec<Context> = Vec::new();
    let mut open: Option<Context> = None;
    let mut rest = src;
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        if let Some(r) = rest.strip_prefix("<!--") {
            let end = r.find("-->").ok_or("unclosed comment")?;
            rest = &r[end + 3..];
            continue;
        }
        if let Some(r) = rest.strip_prefix("</context>") {
            out.push(open.take().ok_or("</context> without <context>")?);
            rest = r;
            continue;
        }
        let r = rest.strip_prefix('<').ok_or_else(|| format!("expected a tag at \"{}\"", head(rest)))?;
        let end = r.find('>').ok_or("unclosed tag")?;
        let (tag, self_closing) = match r[..end].strip_suffix('/') {
            Some(t) => (t, true),
            None => (&r[..end], false),
        };
        rest = &r[end + 1..];
        let (name, attrs) = tag.split_once(char::is_whitespace).unwrap_or((tag, ""));
        let attrs = attributes(attrs)?;
        match name {
            "context" => {
                if open.is_some() {
                    return Err("contexts do not nest".into());
                }
                let mut c = Context { id: String::new(), for_types: Vec::new(), for_leaf: None, extends: None, rules: Vec::new() };
                for (k, v) in attrs {
                    match k.as_str() {
                        "name" => c.id = v,
                        "extends" => c.extends = Some(v),
                        "for" => {
                            let mut words = v.split_whitespace();
                            let t = words.next().and_then(|w| w.strip_prefix("type:")).ok_or_else(|| format!("for=\"{v}\": expected type:<t>, then perhaps facet.field=a|b"))?;
                            c.for_types = types(t)?;
                            if let Some(cond) = words.next() {
                                let (leaf, values) = cond.split_once('=').ok_or_else(|| format!("for=\"{v}\": {cond} is not facet.field=a|b"))?;
                                leaf.parse::<crate::facet::LeafPath>().map_err(|e| format!("for=\"{v}\": {e}"))?;
                                c.for_leaf = Some((leaf.to_owned(), values.split('|').map(str::to_owned).collect()));
                            }
                        }
                        _ => return Err(format!("<context> has no attribute {k} (name, extends, for)")),
                    }
                }
                if c.id.is_empty() {
                    return Err("<context> needs a name".into());
                }
                if self_closing {
                    out.push(c);
                } else {
                    open = Some(c);
                }
            }
            "rule" => {
                let c = open.as_mut().ok_or("<rule> outside a <context>")?;
                if !self_closing {
                    return Err("<rule> is empty: write <rule …/>".into());
                }
                c.rules.push(rule(attrs)?);
            }
            _ => return Err(format!("unknown tag <{name}> (context, rule)")),
        }
    }
    if let Some(c) = open {
        return Err(format!("<context name=\"{}\"> is not closed", c.id));
    }
    Ok(out)
}

fn head(s: &str) -> &str {
    &s[..s.char_indices().nth(20).map(|(i, _)| i).unwrap_or(s.len())]
}

fn rule(attrs: Vec<(String, String)>) -> Result<Rule, String> {
    let mut r = Rule { position: Position::All, types: Vec::new(), state: None, decls: Vec::new(), origin: None };
    let mut matched = false;
    for (k, v) in attrs {
        if k == "match" {
            matched = true;
            for word in v.split_whitespace() {
                match word {
                    "all" => r.position = Position::All,
                    "self" => r.position = Position::Own,
                    "children" => r.position = Position::Children,
                    "view" => r.position = Position::View,
                    w if w.starts_with("type:") => r.types = types(&w[5..])?,
                    w if w.starts_with("state:") && STATES.contains(&&w[6..]) => r.state = Some(w[6..].to_owned()),
                    w if w.starts_with("state:") => return Err(format!("match=\"{v}\": unknown state {} ({})", &w[6..], STATES.join(", "))),
                    w => return Err(format!("match=\"{v}\": unknown word {w} (all, self, children, view, type:<t>, state:<s>)")),
                }
            }
            continue;
        }
        let f = Field::lookup(&k).ok_or_else(|| {
            let domain = k.split('.').next().unwrap_or("");
            let known: Vec<String> = Field::all().filter(|f| f.spec().domain == domain).map(|f| f.path()).collect();
            if known.is_empty() {
                let domains: Vec<&str> = FIELDS.iter().map(|f| f.domain).fold(Vec::new(), |mut a, d| {
                    if !a.contains(&d) {
                        a.push(d);
                    }
                    a
                });
                format!("unknown domain in {k} (domains: {})", domains.join(", "))
            } else {
                format!("unknown field {k} ({})", known.join(", "))
            }
        })?;
        let tool = |t: &str| if TOOLS.contains(&t) { Ok(()) } else { Err(format!("{k}=\"{v}\": unknown tool {t} ({})", TOOLS.join(", "))) };
        let val = match f.spec().kind {
            Kind::Choice => Val::Choice(f.spec().options.iter().position(|o| *o == v).ok_or_else(|| format!("{k}=\"{v}\": expected one of {}", f.spec().options.join(", ")))? as u8),
            Kind::Tools => {
                v.split_whitespace().flat_map(|slot| slot.split('|')).try_for_each(tool)?;
                Val::Text(Arc::from(v.split_whitespace().collect::<Vec<_>>().join(" ")))
            }
            Kind::Words(vocab) => {
                if let Some(w) = v.split_whitespace().find(|w| !vocab.contains(w)) {
                    return Err(format!("{k}=\"{v}\": unknown word {w} ({})", vocab.join(", ")));
                }
                Val::Text(Arc::from(v.split_whitespace().collect::<Vec<_>>().join(" ")))
            }
            Kind::Leaf => {
                if !v.is_empty() {
                    v.parse::<crate::facet::LeafPath>().map_err(|e| format!("{k}=\"{v}\": {e}"))?;
                }
                Val::Text(Arc::from(v.as_str()))
            }
            Kind::Color => {
                let hex = v.trim().strip_prefix('#').filter(|h| (h.len() == 6 || h.len() == 8) && h.chars().all(|c| c.is_ascii_hexdigit()));
                hex.ok_or_else(|| format!("{k}=\"{v}\": expected a colour, #rrggbb or #rrggbbaa"))?;
                Val::Text(Arc::from(v.trim()))
            }
            Kind::Number => {
                v.trim().parse::<f64>().ok().filter(|n| n.is_finite()).ok_or_else(|| format!("{k}=\"{v}\": expected a number"))?;
                Val::Text(Arc::from(v.trim()))
            }
            Kind::Motion => {
                let v = v.split_whitespace().collect::<Vec<_>>().join(" ");
                if v != "none" && motion(&v).is_none() {
                    return Err(format!("{k}=\"{v}\": expected none, or <ease> <ms> [<when>] with ease {} and when {}", EASES.join("|"), WHENS.join("|")));
                }
                Val::Text(Arc::from(v))
            }
            Kind::Keys => {
                for pair in v.split_whitespace() {
                    let (key, t) = pair.split_once(':').ok_or_else(|| format!("{k}=\"{v}\": {pair} is not key:tool"))?;
                    if key.chars().count() != 1 || !key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()) {
                        return Err(format!("{k}=\"{v}\": key {key} is not one letter or digit"));
                    }
                    tool(t)?;
                }
                Val::Text(Arc::from(v.split_whitespace().collect::<Vec<_>>().join(" ")))
            }
        };
        r.decls.push((f, val));
    }
    if !matched {
        return Err("<rule> needs match=".into());
    }
    for (f, _) in &r.decls {
        if (r.position == Position::View) != f.of_view() {
            let k = f.path();
            return Err(if f.of_view() { format!("{k} is a view's: write it in a match=\"view\" rule") } else { format!("{k} is a node's: a match=\"view\" rule cannot declare it") });
        }
    }
    Ok(r)
}

fn types(s: &str) -> Result<Vec<TypeId>, String> {
    s.split('|').map(|t| TypeId::lookup(t).ok_or_else(|| format!("unknown node type {t}"))).collect()
}

fn attributes(s: &str) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    let mut rest = s.trim();
    while !rest.is_empty() {
        let eq = rest.find('=').ok_or_else(|| format!("expected name=\"value\" at \"{}\"", head(rest)))?;
        let key = rest[..eq].trim().to_owned();
        let r = rest[eq + 1..].trim_start().strip_prefix('"').ok_or_else(|| format!("{key}: expected a quoted value"))?;
        let end = r.find('"').ok_or_else(|| format!("{key}: unclosed value"))?;
        let value = r[..end].replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&amp;", "&");
        out.push((key, value));
        rest = r[end + 1..].trim_start();
    }
    Ok(out)
}
