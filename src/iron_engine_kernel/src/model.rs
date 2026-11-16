//! What a program is and what its expressions mean. Shared by the engine and
//! the reference so the two differ only in *strategy*: the reference
//! recomputes everything from the roots, the engine does what changed.

use std::rc::Rc;

#[derive(Clone, Debug)]
pub enum Value {
    Num(f64),
    List(Vec<Value>),
}

/// Sameness of data, so NaN equals NaN (the real engine's rule, `value.rs`).
impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Num(a), Value::Num(b)) => a.to_bits() == b.to_bits(),
            (Value::List(a), Value::List(b)) => a == b,
            _ => false,
        }
    }
}

/// A cell's value: good, or failed with a reason (spec §6.4).
pub type Slot = Result<Value, String>;

/// One key of a Collection: the element's value where elements are distinct
/// numbers, else its index (spec §7.1, step 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Key {
    Val(u64),
    Idx(usize),
}

/// Where an owner sits: for each enclosing Collection, its entry and the key.
pub type Path = Vec<(usize, Key)>;

#[derive(Clone, Debug)]
pub enum Expr {
    Const(f64),
    Input(usize),
    /// Entry `k` of this owner; must be a cell declared before the reader.
    Local(usize),
    /// Entry `k` of the owner `d` levels up (`d >= 1`).
    Up(usize, usize),
    ItemValue,
    ItemIndex,
    Add(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
    /// The multiples of `step` in `[lo, hi]`: the plot's ticks.
    Ticks(Box<Expr>, Box<Expr>, Box<Expr>),
    Sum(Box<Expr>),
    /// The values of entry `l` in every item of this owner's Collection `k`,
    /// in order: a reader from outside (spec §7.4).
    Collect(usize, usize),
    /// A tracked read (the real engine's `deref`): evaluates the index, then
    /// reads the root entry `candidates[index mod len]`. Which cell it reads
    /// is known only by running.
    Deref(Box<Expr>, Rc<Vec<usize>>),
}

pub enum Entry {
    Cell(Expr),
    Collection { each: Expr, template: Rc<Template> },
}

pub struct Template {
    pub entries: Vec<Entry>,
}

/// The most items one Collection may have (the real engine's `max`).
pub const MAX_ITEMS: usize = 64;

/// How an expression reaches other values. The engine's implementation
/// records each read as a dependency; the reference's looks it up.
pub trait Reads {
    fn input(&mut self, i: usize) -> Slot;
    fn local(&mut self, k: usize) -> Slot;
    fn up(&mut self, d: usize, k: usize) -> Slot;
    fn item_value(&mut self) -> Slot;
    fn item_index(&mut self) -> Slot;
    fn collect(&mut self, k: usize, l: usize) -> Vec<Slot>;
    fn root(&mut self, entry: usize) -> Slot;
}

fn num(s: Slot) -> Result<f64, String> {
    match s? {
        Value::Num(n) => Ok(n),
        Value::List(_) => Err("a list where a number is needed".into()),
    }
}

/// Evaluate. Every operand is evaluated even after one fails, so the set of
/// static reads does not depend on values.
pub fn eval(e: &Expr, r: &mut dyn Reads) -> Slot {
    match e {
        Expr::Const(c) => Ok(Value::Num(*c)),
        Expr::Input(i) => r.input(*i),
        Expr::Local(k) => r.local(*k),
        Expr::Up(d, k) => r.up(*d, *k),
        Expr::ItemValue => r.item_value(),
        Expr::ItemIndex => r.item_index(),
        Expr::Add(a, b) => {
            let (a, b) = (num(eval(a, r)), num(eval(b, r)));
            Ok(Value::Num(a? + b?))
        }
        Expr::Mul(a, b) => {
            let (a, b) = (num(eval(a, r)), num(eval(b, r)));
            Ok(Value::Num(a? * b?))
        }
        Expr::Ticks(lo, hi, step) => {
            let (lo, hi, step) = (num(eval(lo, r)), num(eval(hi, r)), num(eval(step, r)));
            let (lo, hi, step) = (lo?, hi?, step?);
            if !(step > 0.0) || !lo.is_finite() || !hi.is_finite() {
                return Err("bad tick range".into());
            }
            let (first, last) = ((lo / step).ceil(), (hi / step).floor());
            if last - first >= MAX_ITEMS as f64 {
                return Err("too many ticks".into());
            }
            let mut out = Vec::new();
            let mut k = first;
            while k <= last {
                out.push(Value::Num(k * step));
                k += 1.0;
            }
            Ok(Value::List(out))
        }
        Expr::Sum(a) => match eval(a, r)? {
            Value::List(items) => {
                let mut total = 0.0;
                for v in items {
                    total += num(Ok(v))?;
                }
                Ok(Value::Num(total))
            }
            Value::Num(_) => Err("a number where a list is needed".into()),
        },
        Expr::Collect(k, l) => {
            let mut out = Vec::new();
            let mut failed = None;
            for s in r.collect(*k, *l) {
                match s {
                    Ok(v) => out.push(v),
                    Err(why) => failed = Some(why),
                }
            }
            match failed {
                Some(why) => Err(format!("depends on a failed item: {why}")),
                None => Ok(Value::List(out)),
            }
        }
        Expr::Deref(index, candidates) => {
            let n = num(eval(index, r))?;
            if !n.is_finite() || candidates.is_empty() {
                return Err("nothing to dereference".into());
            }
            let i = (n.abs().floor() as usize) % candidates.len();
            r.root(candidates[i])
        }
    }
}

/// A Collection's elements and keys from the value of its `each`: a list's
/// elements, or `0..n` for a number; none when it failed.
pub fn items_of(each: &Slot) -> Vec<(Key, Value)> {
    let elements: Vec<Value> = match each {
        Ok(Value::List(items)) => items.iter().take(MAX_ITEMS).cloned().collect(),
        Ok(Value::Num(n)) if n.is_finite() && *n >= 1.0 => {
            (0..(n.floor() as usize).min(MAX_ITEMS)).map(|i| Value::Num(i as f64)).collect()
        }
        _ => Vec::new(),
    };
    let mut bits: Vec<u64> = elements
        .iter()
        .filter_map(|v| match v {
            Value::Num(n) => Some(n.to_bits()),
            Value::List(_) => None,
        })
        .collect();
    let all_numbers = bits.len() == elements.len();
    bits.sort_unstable();
    bits.dedup();
    let by_value = all_numbers && bits.len() == elements.len();
    elements
        .into_iter()
        .enumerate()
        .map(|(i, v)| {
            let key = match (&v, by_value) {
                (Value::Num(n), true) => Key::Val(n.to_bits()),
                _ => Key::Idx(i),
            };
            (key, v)
        })
        .collect()
}
