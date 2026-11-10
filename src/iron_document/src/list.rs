//! Lists and records in expressions (plans/specs/reactivity.md §7).
//!
//! A list is an ordered run of any values; a record is named fields. Both
//! are plain values: changing one element makes a new list, and a repeat's
//! per-item leaves give the fine-grained updates. A series (numbers with
//! one unit, packed) is accepted wherever a list is, and `map` and
//! `filter` over a series give a series back when every result is a number
//! in one unit.

use crate::expr::{charge, Closure, EvalError};
use crate::value::Value;
use std::cmp::Ordering;

fn err<T>(m: impl Into<String>) -> Result<T, EvalError> {
    Err(EvalError::Type(m.into()))
}

fn arity<T>(func: &str, expected: usize, got: usize) -> Result<T, EvalError> {
    Err(EvalError::Arity { func: func.to_owned(), expected, got })
}

fn elements(func: &str, v: &Value) -> Result<Vec<Value>, EvalError> {
    let items = v.elements().ok_or_else(|| EvalError::Type(format!("{func} takes a list or a series, got {}", v.kind().name())))?;
    charge(items.len())?;
    Ok(items)
}

fn function<'a>(func: &str, v: &'a Value) -> Result<&'a Closure, EvalError> {
    match v {
        Value::Fn(c) if c.params.len() == 1 => Ok(c),
        Value::Fn(c) => err(format!("{func}: the function takes {} arguments; {func} gives it 1", c.params.len())),
        v => err(format!("{func}: expected a function (x => …), got {}", v.kind().name())),
    }
}

/// A series when `packed` and every item is a number in one unit, else a list.
fn gather(items: Vec<Value>, packed: bool) -> Value {
    if packed && !items.is_empty() {
        let dim = items[0].as_quantity().map(|q| q.1);
        if let Some(dim) = dim
            && items.iter().all(|v| v.as_quantity().is_some_and(|q| q.1 == dim))
        {
            return Value::series(items.iter().map(|v| v.as_quantity().expect("checked").0).collect::<Vec<_>>(), dim);
        }
    }
    Value::list(items)
}

/// An order for sorting and a test for `unique`: numbers by magnitude,
/// text by its characters, anything else by how it prints.
fn compare(a: &Value, b: &Value) -> Ordering {
    match (a.as_quantity(), b.as_quantity()) {
        (Some((x, _)), Some((y, _))) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
        _ => match (a.as_str(), b.as_str()) {
            (Some(x), Some(y)) => x.cmp(y),
            _ => a.to_string().cmp(&b.to_string()),
        },
    }
}

/// The list and record functions of the floor. `None` when `func` is not
/// one, or is one the series module answers for these arguments.
pub fn call(func: &str, args: &[Value]) -> Option<Result<Value, EvalError>> {
    let first_is_list = matches!(args.first(), Some(Value::List(_)));
    Some(match func {
        // list(a, b, c): the values, in order. list() is empty.
        "list" => charge(args.len()).map(|_| Value::list(args.to_vec())),
        // record(name: value, …), parsed to name and value in turn.
        "record" => {
            if args.len() % 2 != 0 {
                return Some(err("record takes name: value pairs, record(id: 3, label: 'a')"));
            }
            let mut fields = Vec::new();
            for pair in args.chunks(2) {
                let Some(name) = pair[0].as_str() else { return Some(err(format!("record: a field name must be text, got {}", pair[0].kind().name()))) };
                fields.push((name.to_owned(), pair[1].clone()));
            }
            Ok(Value::record("record", fields))
        }
        // get(r, 'field'): a record's field by name; get(r, 'field', fallback).
        "get" => match args {
            [r, name] | [r, name, _] => match (r, name.as_str()) {
                (Value::Record { kind, fields }, Some(name)) => match (fields.get(name), args.get(2)) {
                    (Some(v), _) => Ok(v.clone()),
                    (None, Some(fallback)) => Ok(fallback.clone()),
                    (None, None) => err(format!("{kind} has no field {name:?}; fields: {}", fields.keys().copied().collect::<Vec<_>>().join(", "))),
                },
                (Value::Record { .. }, None) => err("get: the field name must be text"),
                (v, _) => err(format!("get takes a record, got {}", v.kind().name())),
            },
            _ => arity(func, 2, args.len()),
        },
        "len" if first_is_list => match args {
            [v] => Ok(Value::Number(v.len().unwrap_or(0) as f64)),
            _ => arity(func, 1, args.len()),
        },
        // at(xs, i): element i from 0, rounded and clamped, as for a series.
        "at" if first_is_list => match args {
            [Value::List(items), i] => match i.as_f64() {
                _ if items.is_empty() => err("at: the list is empty"),
                Some(i) => Ok(items[if i.is_finite() { i.round().clamp(0.0, (items.len() - 1) as f64) as usize } else { 0 }].clone()),
                None => err(format!("at: the index must be a number, got {}", i.kind().name())),
            },
            _ => arity(func, 2, args.len()),
        },
        "first" | "last" if first_is_list => match args {
            [Value::List(items)] => match if func == "first" { items.first() } else { items.last() } {
                Some(v) => Ok(v.clone()),
                None => err(format!("{func}: the list is empty")),
            },
            _ => arity(func, 1, args.len()),
        },
        // map(xs, f): f of each element.
        "map" => match args {
            [xs, f] => (|| {
                let f = function(func, f)?;
                let out = elements(func, xs)?.iter().map(|x| f.apply(std::slice::from_ref(x))).collect::<Result<Vec<_>, _>>()?;
                Ok(gather(out, !first_is_list))
            })(),
            _ => arity(func, 2, args.len()),
        },
        // filter(xs, f): the elements f is true of.
        "filter" => match args {
            [xs, f] => (|| {
                let f = function(func, f)?;
                let mut out = Vec::new();
                for x in elements(func, xs)? {
                    match f.apply(std::slice::from_ref(&x))? {
                        Value::Bool(true) => out.push(x),
                        Value::Bool(false) => {}
                        v => return err(format!("filter: the function must give true or false, got {}", v.kind().name())),
                    }
                }
                // An empty result of a series stays a series.
                if out.is_empty() && !first_is_list {
                    return Ok(Value::series(Vec::<f64>::new(), xs.as_series().map_or(crate::units::Dim::NONE, |s| s.1)));
                }
                Ok(gather(out, !first_is_list))
            })(),
            _ => arity(func, 2, args.len()),
        },
        // sort_by(xs, f): ascending by f of each element; equal keys keep
        // their order.
        "sort_by" => match args {
            [xs, f] => (|| {
                let f = function(func, f)?;
                let mut keyed = elements(func, xs)?.into_iter().map(|x| f.apply(std::slice::from_ref(&x)).map(|k| (k, x))).collect::<Result<Vec<_>, _>>()?;
                keyed.sort_by(|a, b| compare(&a.0, &b.0));
                Ok(gather(keyed.into_iter().map(|(_, x)| x).collect(), !first_is_list))
            })(),
            _ => arity(func, 2, args.len()),
        },
        // zip(a, b, …): a list of lists, one per position, as long as the
        // shortest.
        "zip" => (|| {
            if args.is_empty() {
                return arity(func, 2, 0);
            }
            let cols = args.iter().map(|a| elements(func, a)).collect::<Result<Vec<_>, _>>()?;
            let n = cols.iter().map(Vec::len).min().unwrap_or(0);
            Ok(Value::list((0..n).map(|i| Value::list(cols.iter().map(|c| c[i].clone()).collect::<Vec<_>>())).collect::<Vec<_>>()))
        })(),
        // enumerate(xs): a record of index and value for each element.
        "enumerate" => match args {
            [xs] => elements(func, xs).map(|items| Value::list(items.into_iter().enumerate().map(|(i, v)| Value::record("record", [("index", Value::Number(i as f64)), ("value", v)])).collect::<Vec<_>>())),
            _ => arity(func, 1, args.len()),
        },
        // unique(xs): each distinct element once, in order of first sight.
        "unique" => match args {
            [xs] => elements(func, xs).map(|items| {
                let mut out: Vec<Value> = Vec::new();
                for v in items {
                    if !out.contains(&v) {
                        out.push(v);
                    }
                }
                gather(out, !first_is_list)
            }),
            _ => arity(func, 1, args.len()),
        },
        // concat with any list in it joins as lists.
        "concat" if args.iter().any(|a| matches!(a, Value::List(_))) => (|| {
            let mut out = Vec::new();
            for a in args {
                out.extend(elements(func, a)?);
            }
            Ok(Value::list(out))
        })(),
        _ => return None,
    })
}
