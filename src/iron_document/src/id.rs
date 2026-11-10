//! Stable identity.
//!
//! IDs are opaque integers rendered as short base62 (`#a3`, `r7`). They never
//! encode position and are never reused: deletion tombstones. The integer is
//! private so nothing outside this crate can depend on it, which is what leaves
//! room for an `(actor, counter)` form if collaboration ever arrives.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

const ALPHABET: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Session-tier ids (plans/specs/panels-and-session.md) start here: session nodes
/// and relations are allocated from their own range, so they never shift
/// the ids of content, and nothing at or above it is ever saved.
pub const SESSION_BASE: u64 = 1 << 40;

/// Expanded nodes (component-model.md §6): the nodes an instance derives
/// from its definition. A derived node is named by its **path**: the
/// authored instance it belongs to, then the definition-local id of the node
/// it copies at each level of nesting — `#50/20` is definition node `#20`
/// derived for instance `#50`; `#50/20/1I` is the fiber definition's port
/// `#1I`, derived for that inner instance. Rendered and parsed as that path.
///
/// A path does not fit in 64 bits, so its id is `EXPANDED | slot`, the slot
/// it holds in a process-wide, append-only path registry (as `sym.rs`
/// interns field names). The same path always gets the same slot, so an id
/// is stable across re-expansion; files store the path text, never the
/// slot, so it is stable across reloads too. Derived nodes are never saved
/// and never in undo; references to them are, by path.
pub const EXPANDED: u64 = 1 << 63;

mod paths {
    use std::collections::HashMap;
    use std::sync::{OnceLock, RwLock};

    #[derive(Default)]
    struct Registry {
        paths: Vec<Vec<u64>>,
        slots: HashMap<Vec<u64>, u64>,
    }

    fn registry() -> &'static RwLock<Registry> {
        static R: OnceLock<RwLock<Registry>> = OnceLock::new();
        R.get_or_init(Default::default)
    }

    /// The slot for `path`, giving it one the first time it is seen.
    pub fn intern(path: &[u64]) -> u64 {
        if let Some(&slot) = registry().read().expect("path registry").slots.get(path) {
            return slot;
        }
        let mut r = registry().write().expect("path registry");
        if let Some(&slot) = r.slots.get(path) {
            return slot;
        }
        let slot = r.paths.len() as u64;
        r.paths.push(path.to_vec());
        r.slots.insert(path.to_vec(), slot);
        slot
    }

    /// The path a slot holds.
    pub fn path(slot: u64) -> Option<Vec<u64>> {
        registry().read().expect("path registry").paths.get(slot as usize).cloned()
    }
}

/// An expanded id's path, if `raw` is one.
fn expanded_path(raw: u64) -> Option<Vec<u64>> {
    if raw & EXPANDED == 0 {
        return None;
    }
    paths::path(raw & !EXPANDED)
}

/// The expanded id of `path` (instance first, then definition-local ids).
fn expanded_raw(path: &[u64]) -> u64 {
    EXPANDED | paths::intern(path)
}

/// A repeat's copies (plans/specs/repeat.md, reactivity.md §3.2): the item
/// with key `k` of repeat `r` is a path step after `r`'s path, then the
/// template node it copies; shown and parsed as `~k` (`#g/~3/7`,
/// `#g/~B12/7`). The key is text (letters, digits, `_`, `-`): the item's
/// index, or what its `repeat.key` gives, so everything attached to the id
/// follows the item when the list is reordered. Keys are interned, as
/// paths are: step `ITEM_BASE + slot`. No authored id reaches this range.
const ITEM_BASE: u64 = SESSION_BASE - (1 << 32);

mod keys {
    use std::collections::HashMap;
    use std::sync::{OnceLock, RwLock};

    #[derive(Default)]
    struct Registry {
        keys: Vec<String>,
        slots: HashMap<String, u64>,
    }

    fn registry() -> &'static RwLock<Registry> {
        static R: OnceLock<RwLock<Registry>> = OnceLock::new();
        R.get_or_init(Default::default)
    }

    pub fn intern(key: &str) -> Option<u64> {
        if let Some(&slot) = registry().read().expect("key registry").slots.get(key) {
            return Some(slot);
        }
        let mut r = registry().write().expect("key registry");
        if let Some(&slot) = r.slots.get(key) {
            return Some(slot);
        }
        let slot = r.keys.len() as u64;
        if slot >= (1 << 32) {
            return None;
        }
        r.keys.push(key.to_owned());
        r.slots.insert(key.to_owned(), slot);
        Some(slot)
    }

    pub fn text(slot: u64) -> Option<String> {
        registry().read().expect("key registry").keys.get(slot as usize).cloned()
    }
}

fn is_item_step(n: u64) -> bool {
    (ITEM_BASE..SESSION_BASE).contains(&n)
}

fn is_key(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn item_step(key: &str) -> Option<u64> {
    is_key(key).then(|| keys::intern(key)).flatten().map(|slot| ITEM_BASE + slot)
}

fn step_key(n: u64) -> Option<String> {
    is_item_step(n).then(|| keys::text(n - ITEM_BASE)).flatten()
}

/// Any text as an item key: itself when it is letters, digits, `_` and
/// `-`; otherwise those of its characters, and a short hash of the whole
/// so two texts never share a key.
pub fn item_key(text: &str) -> String {
    if is_key(text) {
        return text.to_owned();
    }
    let kept: String = text.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(24).collect();
    // FNV-1a.
    let hash = text.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001b3));
    format!("{kept}_{}", base62(hash & 0xffff_ffff))
}

/// One step of an expanded path, as ids print it.
fn step(n: u64) -> String {
    match step_key(n) {
        Some(k) => format!("~{k}"),
        None => base62(n),
    }
}

fn parse_step(s: &str) -> Option<u64> {
    match s.strip_prefix('~') {
        Some(k) => item_step(k),
        None => parse_base62(s),
    }
}

pub(crate) fn base62(mut n: u64) -> String {
    if n == 0 {
        return "0".to_owned();
    }
    let mut buf = Vec::new();
    while n > 0 {
        buf.push(ALPHABET[(n % 62) as usize]);
        n /= 62;
    }
    buf.reverse();
    String::from_utf8(buf).expect("alphabet is ascii")
}

pub(crate) fn parse_base62(s: &str) -> Option<u64> {
    if s.is_empty() {
        return None;
    }
    let mut n: u64 = 0;
    for b in s.bytes() {
        let d = ALPHABET.iter().position(|&c| c == b)? as u64;
        n = n.checked_mul(62)?.checked_add(d)?;
    }
    Some(n)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdParseError(pub String);

impl fmt::Display for IdParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "not a valid id: {:?}", self.0)
    }
}
impl std::error::Error for IdParseError {}

macro_rules! id_type {
    ($(#[$m:meta])* $name:ident, $prefix:literal) => {
        $(#[$m])*
        #[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(u64);

        impl $name {
            pub(crate) fn from_raw(n: u64) -> Self {
                Self(n)
            }
            pub(crate) fn raw(self) -> u64 {
                self.0
            }

            /// The id `n` places after this one: for a builder that has
            /// reserved a run of ids and lays a few more after its last.
            pub fn offset(self, n: u64) -> Self {
                Self(self.0 + n)
            }
            /// In the session tier: never saved, never in undo.
            pub fn is_session(self) -> bool {
                self.0 >= SESSION_BASE && self.0 & EXPANDED == 0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                match expanded_path(self.0) {
                    Some(path) => {
                        let parts: Vec<String> = path.iter().map(|&n| step(n)).collect();
                        write!(f, "{}{}", $prefix, parts.join("/"))
                    }
                    None => write!(f, "{}{}", $prefix, base62(self.0)),
                }
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                fmt::Display::fmt(self, f)
            }
        }

        impl FromStr for $name {
            type Err = IdParseError;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let body = s.strip_prefix($prefix).unwrap_or(s);
                let raw = if body.contains('/') {
                    body.split('/')
                        .map(parse_step)
                        .collect::<Option<Vec<u64>>>()
                        .filter(|p| p.len() >= 2 && p.iter().all(|&n| n & EXPANDED == 0 && n < SESSION_BASE))
                        .map(|p| expanded_raw(&p))
                } else {
                    parse_base62(body)
                };
                raw.map(Self).ok_or_else(|| IdParseError(s.to_owned()))
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(&self.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = String::deserialize(d)?;
                s.parse().map_err(serde::de::Error::custom)
            }
        }
    };
}

id_type!(
    /// Identity of a document node. Rendered as `#<base62>`.
    NodeId,
    "#"
);
impl NodeId {
    /// The node `owner` (an instance: authored, or itself derived when
    /// instances nest) derives from the definition node `local`. `None` when
    /// `local` is not an authored id, or `owner` is in the session tier.
    pub fn expanded(owner: NodeId, local: NodeId) -> Option<NodeId> {
        if local.0 & EXPANDED != 0 || local.0 >= SESSION_BASE || owner.is_session() {
            return None;
        }
        let mut path = expanded_path(owner.0).unwrap_or_else(|| vec![owner.0]);
        path.push(local.0);
        Some(NodeId(expanded_raw(&path)))
    }

    /// Derived from a definition for one instance: never saved, never in
    /// undo, never edited directly (component-model.md §6).
    pub fn is_expanded(self) -> bool {
        self.0 & EXPANDED != 0
    }

    /// An expanded node's instance (the authored one, or the derived inner
    /// instance it belongs to) and the definition node it copies.
    pub fn expanded_parts(self) -> Option<(NodeId, NodeId)> {
        let path = expanded_path(self.0)?;
        let (&local, rest) = path.split_last()?;
        let owner = if rest.len() == 1 { NodeId(rest[0]) } else { NodeId(expanded_raw(rest)) };
        Some((owner, NodeId(local)))
    }

    /// The authored instance an expanded node ultimately belongs to.
    pub fn expanded_root(self) -> Option<NodeId> {
        expanded_path(self.0).and_then(|p| p.first().copied()).map(NodeId)
    }

    /// The item of repeat `repeat` with key `key` (letters, digits, `_`,
    /// `-`; see [`item_key`]): the derived group that holds the item's
    /// copies and its `item.value` and `item.index` (reactivity.md §3.1).
    pub fn item_scope(repeat: NodeId, key: &str) -> Option<NodeId> {
        if repeat.is_session() {
            return None;
        }
        let mut path: Vec<u64> = repeat.path().iter().map(|n| n.0).collect();
        path.push(item_step(key)?);
        Some(NodeId(expanded_raw(&path)))
    }

    /// Item `key` of repeat `repeat`'s copy of template node `local` (its
    /// last path step: the authored node a template node copies).
    pub fn item(repeat: NodeId, key: &str, local: NodeId) -> Option<NodeId> {
        let last = *local.path().last()?;
        if last.0 & EXPANDED != 0 || last.0 >= SESSION_BASE {
            return None;
        }
        let mut path: Vec<u64> = NodeId::item_scope(repeat, key)?.path().iter().map(|n| n.0).collect();
        path.push(last.0);
        Some(NodeId(expanded_raw(&path)))
    }

    /// Which item of `repeat` this node is, or is part of, by key: the
    /// item's own node, a copy of a template node, or anything inside one.
    /// `None` for the template itself.
    pub fn item_of(self, repeat: NodeId) -> Option<String> {
        let (mine, theirs) = (self.path(), repeat.path());
        let k = mine.get(theirs.len())?.0;
        if mine[..theirs.len()] != theirs[..] {
            return None;
        }
        step_key(k)
    }

    /// For a node inside a repeat item (or the item's own node): the
    /// repeat, the item's key, and the template node this is a copy of
    /// (the repeat itself, for the item's node). The innermost repeat's.
    pub fn repeat_item(self) -> Option<(NodeId, String, NodeId)> {
        let path = expanded_path(self.0)?;
        let at = path.iter().rposition(|&n| is_item_step(n))?;
        let node = |steps: &[u64]| match steps {
            [one] => Some(NodeId(*one)),
            many => Some(NodeId(expanded_raw(many))),
        };
        let repeat = node(&path[..at])?;
        let template: Vec<u64> = path[..at].iter().chain(&path[at + 1..]).copied().collect();
        // A template node of an authored repeat is authored: its own id.
        let template = if path[at + 1..].is_empty() { repeat } else if at == 1 && repeat.0 & EXPANDED == 0 { NodeId(*path.last()?) } else { node(&template)? };
        Some((repeat, step_key(path[at])?, template))
    }

    /// Whether this is a repeat item's own node (`#g/~k`).
    pub fn is_item_scope(self) -> bool {
        expanded_path(self.0).and_then(|p| p.last().copied()).is_some_and(is_item_step)
    }

    /// How many scopes this node is inside (reactivity.md §3.3): one for
    /// each instance it is derived for and each repeat item it is part of.
    /// 0 for an authored node.
    pub fn nesting(self) -> usize {
        match expanded_path(self.0) {
            Some(p) => p.iter().filter(|&&n| !is_item_step(n)).count().saturating_sub(1),
            None => 0,
        }
    }

    /// The node's path: `[self]` for an authored node, the instance and
    /// each level's definition-local id for an expanded one.
    pub fn path(self) -> Vec<NodeId> {
        expanded_path(self.0).map_or_else(|| vec![self], |p| p.into_iter().map(NodeId).collect())
    }

    /// The node a path names: the path's one node, or the expanded node it
    /// names. `None` for an empty path or one through a non-authored id.
    pub fn from_path(path: &[NodeId]) -> Option<NodeId> {
        match path {
            [] => None,
            [one] => Some(*one),
            _ => path.iter().all(|n| n.0 & EXPANDED == 0 && n.0 < SESSION_BASE).then(|| NodeId(expanded_raw(&path.iter().map(|n| n.0).collect::<Vec<_>>()))),
        }
    }
}

impl RelationId {
    /// The relation `owner` (an instance) derives from the definition's
    /// relation `local`, named by the owner's path and the relation's id.
    pub fn expanded(owner: NodeId, local: RelationId) -> RelationId {
        let mut path: Vec<u64> = owner.path().iter().map(|n| n.0).collect();
        // A relation that is itself derived (a repeat inside a picture)
        // is named by the authored relation it copies.
        path.push(expanded_path(local.0).and_then(|p| p.last().copied()).unwrap_or(local.0));
        RelationId(expanded_raw(&path))
    }

    /// Derived from a definition: never saved, never in undo.
    pub fn is_expanded(self) -> bool {
        self.0 & EXPANDED != 0
    }
}

id_type!(
    /// Identity of a relation (a non-containment edge). Rendered as `r<base62>`.
    RelationId,
    "r"
);

/// Monotonic document version, bumped once per applied transaction.
#[derive(
    Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Default, Serialize, Deserialize,
)]
pub struct Version(pub u64);

impl Version {
    pub fn next(self) -> Version {
        Version(self.0 + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base62_roundtrip() {
        for n in [0u64, 1, 61, 62, 63, 3843, 3844, u64::MAX] {
            assert_eq!(parse_base62(&base62(n)), Some(n));
        }
    }

    #[test]
    fn ids_render_and_parse() {
        let id = NodeId::from_raw(3844);
        assert_eq!(id.to_string(), "#100");
        assert_eq!("#100".parse::<NodeId>().unwrap(), id);
        assert_eq!("100".parse::<NodeId>().unwrap(), id);
        assert!("#".parse::<NodeId>().is_err());
        assert!("#-1".parse::<NodeId>().is_err());
        // An expanded id names its instance and the definition node it copies.
        let e = NodeId::expanded(NodeId::from_raw(100), NodeId::from_raw(7)).unwrap();
        assert_eq!(e.to_string(), "#1c/7");
        assert_eq!("#1c/7".parse::<NodeId>().unwrap(), e);
        assert_eq!(e.expanded_parts(), Some((NodeId::from_raw(100), NodeId::from_raw(7))));
        assert!(e.is_expanded() && !e.is_session() && !NodeId::from_raw(100).is_expanded());
        // Nested: the definition node #1 derived for that derived instance.
        let nested = NodeId::expanded(e, NodeId::from_raw(1)).unwrap();
        assert_eq!(nested.to_string(), "#1c/7/1");
        assert_eq!("#1c/7/1".parse::<NodeId>().unwrap(), nested);
        assert_eq!(nested.expanded_parts(), Some((e, NodeId::from_raw(1))));
        assert_eq!(nested.expanded_root(), Some(NodeId::from_raw(100)));
        assert!(NodeId::expanded(NodeId::from_raw(1), e).is_none());
        let r = RelationId::from_raw(7);
        assert_eq!(r.to_string(), "r7");
        assert_eq!("r7".parse::<RelationId>().unwrap(), r);
    }
}

/// A scope's identity (plans/specs/scopes.md §3): 128 random bits, written as
/// 26 characters of Crockford base32 (`0-9`, `A-Z` without `I L O U`). It
/// names the store, its op log and its history; a reference into another
/// scope is `(ScopeId, NodeId)`. The id is generated outside this crate (the
/// shell has a random source; this crate needs none) and is identity
/// metadata, not content: setting it is not an op.
#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ScopeId(u128);

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

impl ScopeId {
    pub const fn from_u128(raw: u128) -> ScopeId {
        ScopeId(raw)
    }
    pub const fn as_u128(self) -> u128 {
        self.0
    }
}

impl fmt::Display for ScopeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 26 digits of 5 bits is 130 bits; the top digit holds the top 3.
        let mut buf = [0u8; 26];
        for (i, b) in buf.iter_mut().enumerate() {
            let shift = 5 * (25 - i);
            *b = CROCKFORD[((self.0 >> shift) & 31) as usize];
        }
        f.write_str(std::str::from_utf8(&buf).expect("ascii"))
    }
}

impl fmt::Debug for ScopeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ScopeId({self})")
    }
}

impl FromStr for ScopeId {
    type Err = IdParseError;
    /// Case-insensitive, and reads `I L O` as `1 1 0` as Crockford asks.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = || IdParseError(s.to_owned());
        if s.len() != 26 {
            return Err(bad());
        }
        let mut raw: u128 = 0;
        for (i, c) in s.bytes().enumerate() {
            let c = match c.to_ascii_uppercase() {
                b'I' | b'L' => b'1',
                b'O' => b'0',
                c => c,
            };
            let d = CROCKFORD.iter().position(|&a| a == c).ok_or_else(bad)? as u128;
            if i == 0 && d > 7 {
                return Err(bad());
            }
            raw = (raw << 5) | d;
        }
        Ok(ScopeId(raw))
    }
}

impl Serialize for ScopeId {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for ScopeId {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}
