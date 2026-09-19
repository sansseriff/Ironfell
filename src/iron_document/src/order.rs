//! Fractional sibling order.
//!
//! Keys are strings over a base62 alphabet, compared lexicographically.
//! Inserting between two siblings mints a key strictly between theirs and
//! touches nothing else, which is what keeps a one-node insert from becoming
//! an N-node invalidation (doc 04 §2.5).
//!
//! A key is an *integer part* followed by an optional *fraction*. The first
//! character encodes the integer part's length (`a`..`z` for 2..27 characters,
//! `A`..`Z` for negatives), so appending after the last sibling increments the
//! integer and stays short, while inserting between two neighbours bisects
//! the fraction. This is the scheme from `rocicorp/fractional-indexing`, which
//! descends from David Greenspan's write-up. Repeatedly bisecting the same gap
//! still costs one bit per insert; that is inherent to any order-key scheme,
//! and the remedy is a rebalance op (deferred; see `plans/document-spine.md`
//! §5.5).

use serde::{Deserialize, Deserializer, Serialize};
use std::fmt;

const DIGITS: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
const INTEGER_ZERO: &str = "a0";
const SMALLEST_INTEGER: &str = "A00000000000000000000000000";

#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct OrderKey(String);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrderError {
    Empty,
    BadDigit(char),
    BadHead(char),
    TooShort,
    TrailingZero,
    Smallest,
    /// `between(a, b)` was called with `a >= b`.
    NotOrdered,
    /// The integer range is exhausted at one end.
    Exhausted,
}

impl fmt::Display for OrderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OrderError::Empty => write!(f, "order key is empty"),
            OrderError::BadDigit(c) => write!(f, "order key contains {c:?}"),
            OrderError::BadHead(c) => write!(f, "order key starts with {c:?}, not a letter"),
            OrderError::TooShort => write!(f, "order key is shorter than its integer part"),
            OrderError::TrailingZero => write!(f, "order key ends in '0'"),
            OrderError::Smallest => write!(f, "order key is the reserved smallest integer"),
            OrderError::NotOrdered => write!(f, "lower bound is not below upper bound"),
            OrderError::Exhausted => write!(f, "order key range exhausted"),
        }
    }
}
impl std::error::Error for OrderError {}

fn digit(c: u8) -> usize {
    DIGITS
        .iter()
        .position(|&d| d == c)
        .expect("validated digit")
}

fn integer_len(head: u8) -> Result<usize, OrderError> {
    match head {
        b'a'..=b'z' => Ok((head - b'a') as usize + 2),
        b'A'..=b'Z' => Ok((b'Z' - head) as usize + 2),
        other => Err(OrderError::BadHead(other as char)),
    }
}

/// Split a validated key into (integer part, fraction).
fn split(key: &str) -> (&str, &str) {
    let n = integer_len(key.as_bytes()[0]).expect("validated");
    key.split_at(n)
}

impl OrderKey {
    pub fn parse(s: &str) -> Result<OrderKey, OrderError> {
        if s.is_empty() {
            return Err(OrderError::Empty);
        }
        if let Some(c) = s
            .chars()
            .find(|c| !c.is_ascii() || !DIGITS.contains(&(*c as u8)))
        {
            return Err(OrderError::BadDigit(c));
        }
        let n = integer_len(s.as_bytes()[0])?;
        if s.len() < n {
            return Err(OrderError::TooShort);
        }
        if s == SMALLEST_INTEGER {
            return Err(OrderError::Smallest);
        }
        let (_, frac) = s.split_at(n);
        if frac.ends_with('0') {
            return Err(OrderError::TrailingZero);
        }
        Ok(OrderKey(s.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// A key for the only child.
    pub fn first() -> OrderKey {
        OrderKey(INTEGER_ZERO.to_owned())
    }

    /// A key strictly between `a` and `b`, where `None` means the respective
    /// end of the range.
    pub fn between(a: Option<&OrderKey>, b: Option<&OrderKey>) -> Result<OrderKey, OrderError> {
        let a = a.map(|k| k.0.as_str());
        let b = b.map(|k| k.0.as_str());
        if let (Some(a), Some(b)) = (a, b)
            && a >= b
        {
            return Err(OrderError::NotOrdered);
        }
        let key = match (a, b) {
            (None, None) => INTEGER_ZERO.to_owned(),
            (None, Some(b)) => {
                let (ib, fb) = split(b);
                if ib == SMALLEST_INTEGER {
                    format!("{ib}{}", midpoint("", Some(fb)))
                } else if ib < b {
                    ib.to_owned()
                } else {
                    decrement_integer(ib).ok_or(OrderError::Exhausted)?
                }
            }
            (Some(a), None) => {
                let (ia, fa) = split(a);
                match increment_integer(ia) {
                    Some(i) => i,
                    None => format!("{ia}{}", midpoint(fa, None)),
                }
            }
            (Some(a), Some(b)) => {
                let (ia, fa) = split(a);
                let (ib, fb) = split(b);
                if ia == ib {
                    format!("{ia}{}", midpoint(fa, Some(fb)))
                } else {
                    let i = increment_integer(ia).ok_or(OrderError::Exhausted)?;
                    if i.as_str() < b {
                        i
                    } else {
                        format!("{ia}{}", midpoint(fa, None))
                    }
                }
            }
        };
        Ok(OrderKey(key))
    }

    pub fn after(a: &OrderKey) -> OrderKey {
        OrderKey::between(Some(a), None).expect("upper bound is open")
    }

    pub fn before(b: &OrderKey) -> OrderKey {
        OrderKey::between(None, Some(b)).expect("lower bound is open")
    }
}

/// `a` is a fraction string (`""` is the open lower bound); `b` is `None` for
/// the open upper bound. Requires `a < b` and no trailing zeros.
fn midpoint(a: &str, b: Option<&str>) -> String {
    let zero = b'0';
    let ab = a.as_bytes();
    if let Some(b) = b {
        let bb = b.as_bytes();
        let mut n = 0;
        while n < bb.len() && ab.get(n).copied().unwrap_or(zero) == bb[n] {
            n += 1;
        }
        if n > 0 {
            let rest = midpoint(&a[n.min(a.len())..], Some(&b[n..]));
            return format!("{}{}", &b[..n], rest);
        }
    }
    let digit_a = ab.first().map(|&c| digit(c)).unwrap_or(0);
    let digit_b = b
        .and_then(|b| b.as_bytes().first())
        .map(|&c| digit(c))
        .unwrap_or(DIGITS.len());
    if digit_b - digit_a > 1 {
        let mid = (digit_a + digit_b).div_ceil(2);
        (DIGITS[mid] as char).to_string()
    } else if let Some(b) = b
        && b.len() > 1
    {
        b[..1].to_owned()
    } else {
        let rest = midpoint(if a.is_empty() { "" } else { &a[1..] }, None);
        format!("{}{}", DIGITS[digit_a] as char, rest)
    }
}

fn increment_integer(x: &str) -> Option<String> {
    let bytes = x.as_bytes();
    let head = bytes[0];
    let mut digs: Vec<u8> = bytes[1..].to_vec();
    let mut carry = true;
    for d in digs.iter_mut().rev() {
        if !carry {
            break;
        }
        let next = digit(*d) + 1;
        if next == DIGITS.len() {
            *d = b'0';
        } else {
            *d = DIGITS[next];
            carry = false;
        }
    }
    if carry {
        if head == b'Z' {
            return Some("a0".to_owned());
        }
        if head == b'z' {
            return None;
        }
        let h = head + 1;
        if h > b'a' {
            digs.push(b'0');
        } else {
            digs.pop();
        }
        digs.insert(0, h);
        return Some(String::from_utf8(digs).expect("ascii"));
    }
    digs.insert(0, head);
    Some(String::from_utf8(digs).expect("ascii"))
}

fn decrement_integer(x: &str) -> Option<String> {
    let bytes = x.as_bytes();
    let head = bytes[0];
    let last = DIGITS[DIGITS.len() - 1];
    let mut digs: Vec<u8> = bytes[1..].to_vec();
    let mut borrow = true;
    for d in digs.iter_mut().rev() {
        if !borrow {
            break;
        }
        match digit(*d).checked_sub(1) {
            None => *d = last,
            Some(prev) => {
                *d = DIGITS[prev];
                borrow = false;
            }
        }
    }
    if borrow {
        if head == b'a' {
            return Some(format!("Z{}", last as char));
        }
        if head == b'A' {
            return None;
        }
        let h = head - 1;
        if h < b'Z' {
            digs.push(last);
        } else {
            digs.pop();
        }
        digs.insert(0, h);
        return Some(String::from_utf8(digs).expect("ascii"));
    }
    digs.insert(0, head);
    Some(String::from_utf8(digs).expect("ascii"))
}

impl fmt::Display for OrderKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for OrderKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "OrderKey({:?})", self.0)
    }
}

impl<'de> Deserialize<'de> for OrderKey {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        OrderKey::parse(&s).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(s: &str) -> OrderKey {
        OrderKey::parse(s).unwrap()
    }

    #[test]
    fn known_values() {
        // Reference cases from the upstream test vector.
        assert_eq!(OrderKey::first().as_str(), "a0");
        assert_eq!(OrderKey::after(&k("a0")).as_str(), "a1");
        assert_eq!(OrderKey::before(&k("a0")).as_str(), "Zz");
        assert_eq!(
            OrderKey::between(Some(&k("a0")), Some(&k("a1")))
                .unwrap()
                .as_str(),
            "a0V"
        );
        assert_eq!(OrderKey::after(&k("az")).as_str(), "b00");
        assert_eq!(OrderKey::before(&k("a1")).as_str(), "a0");
        assert_eq!(
            OrderKey::between(Some(&k("a0V")), Some(&k("a1")))
                .unwrap()
                .as_str(),
            "a0l"
        );
        assert_eq!(OrderKey::after(&k("Zz")).as_str(), "a0");
        assert_eq!(OrderKey::before(&k("Zz")).as_str(), "Zy");
        assert_eq!(OrderKey::after(&k("b0z")).as_str(), "b10");
        assert_eq!(
            OrderKey::between(Some(&k("a0")), Some(&k("a0V")))
                .unwrap()
                .as_str(),
            "a0G"
        );
    }

    #[test]
    fn between_is_strictly_between() {
        let a = OrderKey::first();
        let b = OrderKey::after(&a);
        let m = OrderKey::between(Some(&a), Some(&b)).unwrap();
        assert!(a < m && m < b, "{a} < {m} < {b}");
        assert!(OrderKey::before(&a) < a);
        assert_eq!(
            OrderKey::between(Some(&b), Some(&a)),
            Err(OrderError::NotOrdered)
        );
    }

    #[test]
    fn appending_stays_short() {
        let mut key = OrderKey::first();
        for _ in 0..10_000 {
            let next = OrderKey::after(&key);
            assert!(key < next);
            key = next;
        }
        assert!(key.as_str().len() <= 4, "{key}");
        let mut key = OrderKey::first();
        for _ in 0..1000 {
            let prev = OrderKey::before(&key);
            assert!(prev < key);
            key = prev;
        }
        assert!(key.as_str().len() <= 4, "{key}");
    }

    #[test]
    fn repeated_bisection_stays_sorted() {
        let lo = OrderKey::first();
        let hi = OrderKey::after(&lo);
        let mut prev = lo.clone();
        for _ in 0..1000 {
            let next = OrderKey::between(Some(&prev), Some(&hi)).unwrap();
            assert!(prev < next && next < hi);
            assert!(OrderKey::parse(next.as_str()).is_ok());
            prev = next;
        }
        // One bit per insert is the floor for any scheme; this checks the
        // constant factor has not regressed, not that it is small.
        assert!(
            prev.as_str().len() < 250,
            "key grew to {}",
            prev.as_str().len()
        );
    }

    #[test]
    fn parse_rejects_bad_keys() {
        assert_eq!(OrderKey::parse(""), Err(OrderError::Empty));
        assert_eq!(OrderKey::parse("a00"), Err(OrderError::TrailingZero));
        assert_eq!(OrderKey::parse("a-"), Err(OrderError::BadDigit('-')));
        assert_eq!(OrderKey::parse("0a"), Err(OrderError::BadHead('0')));
        assert_eq!(OrderKey::parse("b0"), Err(OrderError::TooShort));
        assert_eq!(OrderKey::parse(SMALLEST_INTEGER), Err(OrderError::Smallest));
        assert!(OrderKey::parse("a0").is_ok());
        assert!(OrderKey::parse("a0V").is_ok());
    }
}
