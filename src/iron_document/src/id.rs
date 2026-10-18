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
            /// In the session tier: never saved, never in undo.
            pub fn is_session(self) -> bool {
                self.0 >= SESSION_BASE && self.0 & EXPANDED == 0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                match expanded_path(self.0) {
                    Some(path) => {
                        let parts: Vec<String> = path.iter().map(|&n| base62(n)).collect();
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
                        .map(parse_base62)
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
