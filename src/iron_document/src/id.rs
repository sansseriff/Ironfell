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
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}{}", $prefix, base62(self.0))
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
                parse_base62(body)
                    .map(Self)
                    .ok_or_else(|| IdParseError(s.to_owned()))
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
        let r = RelationId::from_raw(7);
        assert_eq!(r.to_string(), "r7");
        assert_eq!("r7".parse::<RelationId>().unwrap(), r);
    }
}
