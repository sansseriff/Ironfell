//! Series, function values, colours and SVG path data in expressions
//! (plans/specs/plot.md §1–§2).
//!
//! A series is numbers with one unit. Arithmetic and the floor's scalar
//! functions broadcast over it, element by element, as NumPy's do: a
//! number meets every element; two series must be the same length. A
//! function plot is a `ys` bound to an expression over `xs`, or a function
//! (`x => …`) sampled with `sample`.
//!
//! Paths are drawn from **SVG path data**: the writers here (`points`,
//! `smooth`, `circles`, `segments`, `bars`, `area`) return path-data text,
//! which a bound `path.d` turns into geometry (`geometry::from_svg_path`).
//! Coordinates are plain numbers (document pixels); a non-finite sample
//! breaks the line, as plotting libraries do with NaN.

use crate::expr::{BinOp, Closure, EvalError, MAX_SAMPLES};
use crate::units::Dim;
use crate::value::Value;

fn err<T>(m: impl Into<String>) -> Result<T, EvalError> {
    Err(EvalError::Type(m.into()))
}

fn arity<T>(func: &str, expected: usize, got: usize) -> Result<T, EvalError> {
    Err(EvalError::Arity { func: func.to_owned(), expected, got })
}

fn describe(d: Dim) -> String {
    if d.is_none() { "a plain number".to_owned() } else { d.canonical_unit() }
}

/// A numeric argument: a whole series, or one number or quantity that
/// stands for every element.
#[derive(Clone, Copy)]
enum Arg<'a> {
    Many(&'a [f64], Dim),
    One(f64, Dim),
}

impl Arg<'_> {
    fn of(v: &Value) -> Option<Arg<'_>> {
        match v {
            Value::Series { data, dim } => Some(Arg::Many(data, *dim)),
            v => v.as_quantity().map(|(x, d)| Arg::One(x, d)),
        }
    }
    fn dim(self) -> Dim {
        match self {
            Arg::Many(_, d) | Arg::One(_, d) => d,
        }
    }
    fn len(self) -> Option<usize> {
        match self {
            Arg::Many(d, _) => Some(d.len()),
            Arg::One(..) => None,
        }
    }
    fn at(self, i: usize) -> f64 {
        match self {
            Arg::Many(d, _) => d[i],
            Arg::One(x, _) => x,
        }
    }
}

/// The common length of `args`' series (1 when none is a series).
fn common_len(func: &str, args: &[Arg]) -> Result<usize, EvalError> {
    let mut n = None;
    for a in args {
        if let Some(l) = a.len() {
            match n {
                Some(m) if m != l => return err(format!("{func}: series of different lengths, {m} and {l}")),
                _ => n = Some(l),
            }
        }
    }
    Ok(n.unwrap_or(1))
}

fn numeric<'a>(func: &str, v: &'a Value) -> Result<Arg<'a>, EvalError> {
    Arg::of(v).ok_or_else(|| EvalError::Type(format!("{func} takes numbers or series, got {}", v.kind().name())))
}

fn series_of<'a>(func: &str, v: &'a Value) -> Result<(&'a [f64], Dim), EvalError> {
    match v {
        Value::Series { data, dim } => Ok((data, *dim)),
        v => err(format!("{func} takes a series, got {}", v.kind().name())),
    }
}

fn plain_number(func: &str, v: &Value) -> Result<f64, EvalError> {
    match v.as_quantity() {
        Some((x, d)) if d.is_none() => Ok(x),
        Some((_, d)) => err(format!("{func}: expected a plain number, got {}", describe(d))),
        None => err(format!("{func}: expected a number, got {}", v.kind().name())),
    }
}

fn count(func: &str, v: &Value) -> Result<usize, EvalError> {
    let n = plain_number(func, v)?;
    if !n.is_finite() || n < 0.0 {
        return err(format!("{func}: a count must be a non-negative number, got {n}"));
    }
    let n = n.round() as usize;
    if n > MAX_SAMPLES {
        return err(format!("{func}: at most {MAX_SAMPLES} samples, asked for {n}"));
    }
    Ok(n)
}

fn function<'a>(func: &str, v: &'a Value, params: usize) -> Result<&'a Closure, EvalError> {
    match v {
        Value::Fn(c) if c.params.len() == params => Ok(c),
        Value::Fn(c) => err(format!("{func}: the function takes {} arguments; {func} gives it {params}", c.params.len())),
        v => err(format!("{func}: expected a function (x => …), got {}", v.kind().name())),
    }
}

/// Collect numeric results (one per element) into a series of one dimension.
fn collect(func: &str, results: impl Iterator<Item = Result<Value, EvalError>>) -> Result<Value, EvalError> {
    let mut data = Vec::new();
    let mut dim = None;
    for r in results {
        let v = r?;
        let (x, d) = v.as_quantity().ok_or_else(|| EvalError::Type(format!("{func}: each result must be a number, got {}", v.kind().name())))?;
        match dim {
            Some(e) if e != d => return err(format!("{func}: results in different units, {} and {}", describe(e), describe(d))),
            _ => dim = Some(d),
        }
        data.push(x);
    }
    Ok(Value::series(data, dim.unwrap_or(Dim::NONE)))
}

/// `a op b` when either is a series. `None` for operators that do not
/// broadcast (`==`, `!=`, `&&`, `||`), which then compare or refuse whole
/// values as before.
pub fn binary(op: BinOp, a: &Value, b: &Value) -> Option<Result<Value, EvalError>> {
    if matches!(op, BinOp::Eq | BinOp::Ne | BinOp::And | BinOp::Or) {
        return None;
    }
    Some((|| {
        let (x, y) = (numeric("an operator", a)?, numeric("an operator", b)?);
        let n = common_len("an operator", &[x, y])?;
        let (dx, dy) = (x.dim(), y.dim());
        let same = || if dx == dy { Ok(()) } else { err(format!("cannot combine {} and {} element by element", describe(dx), describe(dy))) };
        let (dim, f): (Dim, fn(f64, f64) -> f64) = match op {
            BinOp::Add => (same().map(|_| dx)?, |p, q| p + q),
            BinOp::Sub => (same().map(|_| dx)?, |p, q| p - q),
            BinOp::Mod => (same().map(|_| dx)?, |p, q| p % q),
            BinOp::Mul => (dx.mul(dy), |p, q| p * q),
            BinOp::Div => (dx.div(dy), |p, q| p / q),
            // Comparisons give 1 or 0 per element, for `where`.
            BinOp::Lt => (same().map(|_| Dim::NONE)?, |p, q| f64::from(p < q)),
            BinOp::Le => (same().map(|_| Dim::NONE)?, |p, q| f64::from(p <= q)),
            BinOp::Gt => (same().map(|_| Dim::NONE)?, |p, q| f64::from(p > q)),
            BinOp::Ge => (same().map(|_| Dim::NONE)?, |p, q| f64::from(p >= q)),
            BinOp::Eq | BinOp::Ne | BinOp::And | BinOp::Or => unreachable!("not broadcast"),
        };
        Ok(Value::series((0..n).map(|i| f(x.at(i), y.at(i))).collect::<Vec<_>>(), dim))
    })())
}

/// The scalar floor functions that apply per element to a series.
const ELEMENTWISE: &[&str] = &[
    "abs", "floor", "ceil", "round", "min", "max", "clamp", "lerp", "scale", "sin", "cos", "tan", "atan2", "sqrt", "pow", "hypot", "log10", "ln", "exp", "sinc", "sig",
];

/// The floor functions this module answers; `None` passes `func` on to
/// the scalar floor.
pub fn call(func: &str, args: &[Value]) -> Option<Result<Value, EvalError>> {
    let any_series = args.iter().any(|a| matches!(a, Value::Series { .. }));
    Some(match func {
        "series" => make(args),
        "range" => range(args),
        "step" => step(args),
        "len" | "first" | "last" | "sum" | "mean" => reduce(func, args),
        "min" | "max" if args.len() == 1 => reduce(func, args),
        "at" => at(args),
        "interp" => interp(args),
        "ticks" | "nice_lo" | "nice_hi" => ticks(func, args),
        "where" => select(args),
        "map" => map(args),
        "sample" => sample(args),
        "rgb" | "hsl" | "mix" | "spectrum" => colour(func, args),
        "points" | "smooth" | "circles" | "segments" | "bars" | "area" => path_data(func, args),
        "ease" => ease(args),
        "window" => window(args),
        "isnan" => isnan(args),
        "kind" => match args {
            [v] => Ok(Value::Str(v.kind().name().to_owned())),
            _ => arity(func, 1, args.len()),
        },
        "concat" => concat(args),
        "unit_of" => unit_of(args),
        "in_unit" => in_unit(args),
        _ if any_series && ELEMENTWISE.contains(&func) => broadcast(func, args),
        _ => return None,
    })
}

/// `ease(curve, t)`: progress t (clamped to 0..1) shaped by an easing curve,
/// a name (`easy_ease`) or `cubic-bezier(x1, y1, x2, y2)`; t may be a series.
fn ease(args: &[Value]) -> Result<Value, EvalError> {
    let [c, t] = args else { return arity("ease", 2, args.len()) };
    let name = c.as_str().ok_or_else(|| EvalError::Type(format!("ease: the curve is a name or 'cubic-bezier(…)', got {}", c.kind().name())))?;
    let curve = crate::time::curve(name).map_err(|e| EvalError::Type(format!("ease: {e}")))?;
    match numeric("ease", t)? {
        Arg::One(x, d) if d.is_none() => Ok(Value::Number(curve.at(x))),
        Arg::Many(xs, d) if d.is_none() => Ok(Value::series(xs.iter().map(|&x| curve.at(x)).collect::<Vec<_>>(), Dim::NONE)),
        a => err(format!("ease: progress is a plain number from 0 to 1, got {}", describe(a.dim()))),
    }
}

/// `window(t, a, b)`: the slice a..b of progress t mapped to 0..1, clamped:
/// a delay or a stagger.
fn window(args: &[Value]) -> Result<Value, EvalError> {
    let [t, a, b] = args else { return arity("window", 3, args.len()) };
    let (a, b) = (plain_number("window", a)?, plain_number("window", b)?);
    let one = |x: f64| if b == a { if x >= b { 1.0 } else { 0.0 } } else { ((x - a) / (b - a)).clamp(0.0, 1.0) };
    match numeric("window", t)? {
        Arg::One(x, d) if d.is_none() => Ok(Value::Number(one(x))),
        Arg::Many(xs, d) if d.is_none() => Ok(Value::series(xs.iter().map(|&x| one(x)).collect::<Vec<_>>(), Dim::NONE)),
        a => err(format!("window: progress is a plain number, got {}", describe(a.dim()))),
    }
}

/// `isnan(x)`: whether a number (or a quantity's magnitude) is NaN, the
/// value an input defaults to when it means "none" or "auto".
fn isnan(args: &[Value]) -> Result<Value, EvalError> {
    let [v] = args else { return arity("isnan", 1, args.len()) };
    Ok(Value::Bool(v.as_quantity().is_some_and(|(x, _)| x.is_nan())))
}

/// `concat(a, b, …)`: series joined end to end; empty ones are skipped
/// whatever their unit.
fn concat(args: &[Value]) -> Result<Value, EvalError> {
    let mut data = Vec::new();
    let mut dim = None;
    for a in args {
        let (d, u) = series_of("concat", a)?;
        if d.is_empty() {
            continue;
        }
        match dim {
            Some(e) if e != u => return err(format!("concat: series in different units, {} and {}", describe(e), describe(u))),
            _ => dim = Some(u),
        }
        data.extend_from_slice(d);
    }
    Ok(Value::series(data, dim.unwrap_or(Dim::NONE)))
}

/// `unit_of(x)`: the unit a value shows in (a series: the one its largest
/// magnitude picks), `""` for a plain number. An axis titles itself with it.
fn unit_of(args: &[Value]) -> Result<Value, EvalError> {
    let [v] = args else { return arity("unit_of", 1, args.len()) };
    let (biggest, dim) = match numeric("unit_of", v)? {
        Arg::Many(d, dim) => (d.iter().copied().filter(|x| x.is_finite()).fold(0.0f64, |m, x| m.max(x.abs())), dim),
        Arg::One(x, dim) => (x.abs(), dim),
    };
    Ok(Value::Str(if dim.is_none() { String::new() } else { crate::units::display_unit(biggest, dim).0 }))
}

/// `in_unit(x, 'nm')`: a value's magnitude in a unit, as plain numbers
/// (tick labels under an axis titled with the unit); `''` for a plain number.
fn in_unit(args: &[Value]) -> Result<Value, EvalError> {
    let [v, u] = args else { return arity("in_unit", 2, args.len()) };
    let name = u.as_str().ok_or_else(|| EvalError::Type(format!("in_unit: the unit must be text, got {}", u.kind().name())))?;
    let (factor, want) = if name.trim().is_empty() {
        (1.0, Dim::NONE)
    } else {
        let unit = crate::units::parse_unit(name.trim()).map_err(|e| EvalError::Type(format!("in_unit: {e}")))?;
        (unit.factor, unit.dim)
    };
    let x = numeric("in_unit", v)?;
    if x.dim() != want {
        return err(format!("in_unit: {} cannot be shown in {name:?}", describe(x.dim())));
    }
    Ok(match x {
        Arg::One(x, _) => Value::Number(crate::units::round12(x / factor)),
        Arg::Many(d, _) => Value::series(d.iter().map(|x| crate::units::round12(x / factor)).collect::<Vec<_>>(), Dim::NONE),
    })
}

fn broadcast(func: &str, args: &[Value]) -> Result<Value, EvalError> {
    let parts: Vec<Arg> = args.iter().map(|a| numeric(func, a)).collect::<Result<_, _>>()?;
    let n = common_len(func, &parts)?;
    collect(func, (0..n).map(|i| {
        let one: Vec<Value> = parts.iter().map(|p| Value::quantity(p.at(i), p.dim())).collect();
        crate::expr::floor::scalar(func, &one)
    }))
}

/// `series(a, b, …)`: numbers or quantities of one dimension.
fn make(args: &[Value]) -> Result<Value, EvalError> {
    collect("series", args.iter().map(|v| match v.as_quantity() {
        Some(_) => Ok(v.clone()),
        None => err(format!("series takes numbers, got {}", v.kind().name())),
    }))
}

fn two_same(func: &str, a: &Value, b: &Value) -> Result<(f64, f64, Dim), EvalError> {
    let (x, dx) = a.as_quantity().ok_or_else(|| EvalError::Type(format!("{func} takes numbers, got {}", a.kind().name())))?;
    let (y, dy) = b.as_quantity().ok_or_else(|| EvalError::Type(format!("{func} takes numbers, got {}", b.kind().name())))?;
    if dx != dy {
        return err(format!("{func}: ends in different units, {} and {}", describe(dx), describe(dy)));
    }
    Ok((x, y, dx))
}

/// `range(a, b, n)`: n evenly spaced values from a to b, both included.
fn range(args: &[Value]) -> Result<Value, EvalError> {
    let [a, b, n] = args else { return arity("range", 3, args.len()) };
    let (a, b, dim) = two_same("range", a, b)?;
    let n = count("range", n)?;
    let data: Vec<f64> = match n {
        0 => Vec::new(),
        1 => vec![a],
        n => (0..n).map(|i| a + (b - a) * i as f64 / (n - 1) as f64).collect(),
    };
    Ok(Value::series(data, dim))
}

/// `step(a, b, d)`: a, a + d, … up to b.
fn step(args: &[Value]) -> Result<Value, EvalError> {
    let [a, b, d] = args else { return arity("step", 3, args.len()) };
    let (a, b, dim) = two_same("step", a, b)?;
    let (d, dd) = d.as_quantity().ok_or_else(|| EvalError::Type("step: the step must be a number".into()))?;
    if dd != dim {
        return err(format!("step: the step is {}, the ends {}", describe(dd), describe(dim)));
    }
    if !(d > 0.0) || !d.is_finite() {
        return err("step: the step must be positive");
    }
    let n = ((b - a) / d + 1e-9).floor() + 1.0;
    if !(n >= 0.0) {
        return Ok(Value::series(Vec::new(), dim));
    }
    if n > MAX_SAMPLES as f64 {
        return err(format!("step: at most {MAX_SAMPLES} samples, asked for {n}"));
    }
    Ok(Value::series((0..n as usize).map(|i| a + d * i as f64).collect::<Vec<_>>(), dim))
}

fn reduce(func: &str, args: &[Value]) -> Result<Value, EvalError> {
    let [s] = args else { return arity(func, 1, args.len()) };
    let (data, dim) = series_of(func, s)?;
    if func == "len" {
        return Ok(Value::Number(data.len() as f64));
    }
    if data.is_empty() {
        return match func {
            "sum" => Ok(Value::quantity(0.0, dim)),
            _ => err(format!("{func} of an empty series")),
        };
    }
    let x = match func {
        "first" => data[0],
        "last" => data[data.len() - 1],
        "sum" => data.iter().sum(),
        "mean" => data.iter().sum::<f64>() / data.len() as f64,
        "min" => data.iter().copied().fold(f64::INFINITY, f64::min),
        _ => data.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    };
    Ok(Value::quantity(x, dim))
}

/// `at(s, i)`: element i, counting from 0; i is rounded and clamped.
fn at(args: &[Value]) -> Result<Value, EvalError> {
    let [s, i] = args else { return arity("at", 2, args.len()) };
    let (data, dim) = series_of("at", s)?;
    let i = plain_number("at", i)?;
    if data.is_empty() {
        return err("at: the series is empty");
    }
    let i = if i.is_finite() { i.round().clamp(0.0, (data.len() - 1) as f64) as usize } else { 0 };
    Ok(Value::quantity(data[i], dim))
}

/// `interp(xs, ys, x)`: y at x, straight between samples, held past the
/// ends (NumPy's `interp`). xs ascending or descending; x a number or a
/// series.
fn interp(args: &[Value]) -> Result<Value, EvalError> {
    let [xs, ys, x] = args else { return arity("interp", 3, args.len()) };
    let (xs, dx) = series_of("interp", xs)?;
    let (ys, dy) = series_of("interp", ys)?;
    if xs.len() != ys.len() || xs.is_empty() {
        return err(format!("interp: xs and ys must be the same, non-zero length, got {} and {}", xs.len(), ys.len()));
    }
    let at = numeric("interp", x)?;
    if at.dim() != dx {
        return err(format!("interp: x is {}, xs {}", describe(at.dim()), describe(dx)));
    }
    let rising = xs[0] <= xs[xs.len() - 1];
    let one = |x: f64| -> f64 {
        let key = |v: f64| if rising { v } else { -v };
        let k = key(x);
        let i = xs.partition_point(|&v| key(v) <= k);
        if i == 0 {
            return ys[0];
        }
        if i >= xs.len() {
            return ys[ys.len() - 1];
        }
        let (x0, x1, y0, y1) = (xs[i - 1], xs[i], ys[i - 1], ys[i]);
        if x1 == x0 { y1 } else { y0 + (y1 - y0) * (x - x0) / (x1 - x0) }
    };
    Ok(match at {
        Arg::One(x, _) => Value::quantity(one(x), dy),
        Arg::Many(data, _) => Value::series(data.iter().map(|&x| one(x)).collect::<Vec<_>>(), dy),
    })
}

/// The "nice" step nearest `raw`: 1, 2 or 5 × 10ⁿ, choosing between them
/// at their geometric midpoints (d3's `tickIncrement`).
pub fn nice_step(raw: f64) -> Option<f64> {
    if !(raw > 0.0) || !raw.is_finite() {
        return None;
    }
    let e = 10f64.powf(raw.log10().floor());
    let f = raw / e;
    let m = if f >= 50f64.sqrt() { 10.0 } else if f >= 10f64.sqrt() { 5.0 } else if f >= 2f64.sqrt() { 2.0 } else { 1.0 };
    Some(m * e)
}

/// `ticks(lo, hi, n)`: about n nice values covering [lo, hi];
/// `nice_lo`/`nice_hi`: the nice values at or just past each end.
fn ticks(func: &str, args: &[Value]) -> Result<Value, EvalError> {
    let [lo, hi, n] = args else { return arity(func, 3, args.len()) };
    let (lo, hi, dim) = two_same(func, lo, hi)?;
    let n = count(func, n)?.max(1);
    let (lo, hi) = if lo <= hi { (lo, hi) } else { (hi, lo) };
    let Some(step) = nice_step((hi - lo) / n as f64) else {
        return Ok(match func {
            "ticks" => Value::series(vec![lo], dim),
            "nice_lo" => Value::quantity(lo, dim),
            _ => Value::quantity(hi, dim),
        });
    };
    let first = (lo / step - 1e-9).ceil();
    let last = (hi / step + 1e-9).floor();
    Ok(match func {
        "nice_lo" => Value::quantity(crate::units::round12((lo / step + 1e-9).floor() * step), dim),
        "nice_hi" => Value::quantity(crate::units::round12((hi / step - 1e-9).ceil() * step), dim),
        _ => {
            let k = (last - first).max(-1.0) as i64 + 1;
            if k > 1000 {
                return err("ticks: more than 1000 ticks");
            }
            Value::series((0..k).map(|i| crate::units::round12((first + i as f64) * step)).collect::<Vec<_>>(), dim)
        }
    })
}

/// `where(cond, a, b)`: per element, a where cond is non-zero (or true),
/// else b.
fn select(args: &[Value]) -> Result<Value, EvalError> {
    let [c, a, b] = args else { return arity("where", 3, args.len()) };
    if let Value::Bool(t) = c {
        return Ok(if *t { a.clone() } else { b.clone() });
    }
    let (c, a, b) = (numeric("where", c)?, numeric("where", a)?, numeric("where", b)?);
    if a.dim() != b.dim() {
        return err(format!("where: the two choices are {} and {}", describe(a.dim()), describe(b.dim())));
    }
    let n = common_len("where", &[c, a, b])?;
    Ok(Value::series((0..n).map(|i| if c.at(i) != 0.0 { a.at(i) } else { b.at(i) }).collect::<Vec<_>>(), a.dim()))
}

/// `map(s, f)`: f applied to each element.
fn map(args: &[Value]) -> Result<Value, EvalError> {
    let [s, f] = args else { return arity("map", 2, args.len()) };
    let (data, dim) = series_of("map", s)?;
    let f = function("map", f, 1)?;
    collect("map", data.iter().map(|&x| f.apply(&[Value::quantity(x, dim)])))
}

/// `sample(f, a, b, n)`: f at n evenly spaced points from a to b.
fn sample(args: &[Value]) -> Result<Value, EvalError> {
    let [f, a, b, n] = args else { return arity("sample", 4, args.len()) };
    let f = function("sample", f, 1)?;
    let xs = range(&[a.clone(), b.clone(), n.clone()])?;
    let (data, dim) = xs.as_series().expect("range answers a series");
    collect("sample", data.iter().map(|&x| f.apply(&[Value::quantity(x, dim)])))
}

// ---------------------------------------------------------------------------
// Colours
// ---------------------------------------------------------------------------

fn colour(func: &str, args: &[Value]) -> Result<Value, EvalError> {
    let num = |i: usize| plain_number(func, &args[i]);
    let rgba = |r: f64, g: f64, b: f64, a: f64| Value::Color([r, g, b, a].map(|c| c.clamp(0.0, 1.0) as f32));
    match func {
        // CSS's: channels 0–255, alpha 0–1.
        "rgb" => {
            if !(3..=4).contains(&args.len()) {
                return arity(func, 3, args.len());
            }
            let a = if args.len() == 4 { num(3)? } else { 1.0 };
            Ok(rgba(num(0)? / 255.0, num(1)? / 255.0, num(2)? / 255.0, a))
        }
        // CSS's: hue in degrees, saturation and lightness 0–100, alpha 0–1.
        "hsl" => {
            if !(3..=4).contains(&args.len()) {
                return arity(func, 3, args.len());
            }
            let (h, s, l) = (num(0)?.rem_euclid(360.0) / 360.0, (num(1)? / 100.0).clamp(0.0, 1.0), (num(2)? / 100.0).clamp(0.0, 1.0));
            let a = if args.len() == 4 { num(3)? } else { 1.0 };
            let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
            let p = 2.0 * l - q;
            let hue = |t: f64| {
                let t = t.rem_euclid(1.0);
                if t < 1.0 / 6.0 {
                    p + (q - p) * 6.0 * t
                } else if t < 0.5 {
                    q
                } else if t < 2.0 / 3.0 {
                    p + (q - p) * (2.0 / 3.0 - t) * 6.0
                } else {
                    p
                }
            };
            Ok(rgba(hue(h + 1.0 / 3.0), hue(h), hue(h - 1.0 / 3.0), a))
        }
        "mix" => {
            let [c1, c2, t] = args else { return arity(func, 3, args.len()) };
            let (Some(c1), Some(c2)) = (c1.as_color(), c2.as_color()) else { return err("mix takes two colours and a fraction") };
            let t = plain_number(func, t)?.clamp(0.0, 1.0) as f32;
            Ok(Value::Color(std::array::from_fn(|i| c1[i] + (c2[i] - c1[i]) * t)))
        }
        // Visible light's colour for a wavelength (a length, or plain nm),
        // dimming toward the ends of the visible range.
        _ => {
            let [w] = args else { return arity(func, 1, args.len()) };
            let nm = match w.as_quantity() {
                Some((x, d)) if d.is_none() => x,
                Some((x, d)) if d == Dim::LENGTH => x * 1e9,
                _ => return err("spectrum takes a wavelength: a length (550 nm) or a plain number of nanometres"),
            };
            let (r, g, b) = match nm {
                w if (380.0..440.0).contains(&w) => (-(w - 440.0) / 60.0, 0.0, 1.0),
                w if (440.0..490.0).contains(&w) => (0.0, (w - 440.0) / 50.0, 1.0),
                w if (490.0..510.0).contains(&w) => (0.0, 1.0, -(w - 510.0) / 20.0),
                w if (510.0..580.0).contains(&w) => ((w - 510.0) / 70.0, 1.0, 0.0),
                w if (580.0..645.0).contains(&w) => (1.0, -(w - 645.0) / 65.0, 0.0),
                w if (645.0..=780.0).contains(&w) => (1.0, 0.0, 0.0),
                _ => (0.0, 0.0, 0.0),
            };
            let fade = match nm {
                w if (380.0..420.0).contains(&w) => 0.3 + 0.7 * (w - 380.0) / 40.0,
                w if (700.0..=780.0).contains(&w) => 0.3 + 0.7 * (780.0 - w) / 80.0,
                w if (420.0..700.0).contains(&w) => 1.0,
                _ => 0.0,
            };
            let a = if (380.0..=780.0).contains(&nm) { 1.0 } else { 0.0 };
            Ok(rgba(r * fade, g * fade, b * fade, a))
        }
    }
}

// ---------------------------------------------------------------------------
// SVG path data
// ---------------------------------------------------------------------------

/// A coordinate in path data: at most 4 decimals, no trailing zeros.
fn pn(x: f64) -> String {
    let s = format!("{x:.4}");
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.') } else { &s };
    if s == "-0" { "0".to_owned() } else { s.to_owned() }
}

fn xy(x: f64, y: f64) -> String {
    format!("{},{}", pn(x), pn(y))
}

/// Plain-number coordinate arguments, each a series or one number.
fn coords<'a>(func: &str, args: &'a [Value]) -> Result<(Vec<Arg<'a>>, usize), EvalError> {
    let parts: Vec<Arg> = args.iter().map(|a| numeric(func, a)).collect::<Result<_, _>>()?;
    for (i, p) in parts.iter().enumerate() {
        if !p.dim().is_none() {
            return err(format!("{func}: argument {} is {}; coordinates are plain numbers (pixels), so divide by a unit first", i + 1, describe(p.dim())));
        }
    }
    let n = common_len(func, &parts)?;
    Ok((parts, n))
}

/// The runs of finite points in `x`, `y`: a non-finite sample ends a run.
fn runs(x: Arg, y: Arg, n: usize) -> Vec<Vec<(f64, f64)>> {
    let mut out: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut cur = Vec::new();
    for i in 0..n {
        let (px, py) = (x.at(i), y.at(i));
        if px.is_finite() && py.is_finite() {
            cur.push((px, py));
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn path_data(func: &str, args: &[Value]) -> Result<Value, EvalError> {
    let need = match func {
        "points" | "smooth" => 2,
        "circles" | "area" => 3,
        _ => 4,
    };
    if args.len() != need {
        return arity(func, need, args.len());
    }
    let (p, n) = coords(func, args)?;
    let mut out: Vec<String> = Vec::new();
    match func {
        // SVG's `points` list; a break starts a new subpath with M, so
        // `'M ' + points(xs, ys)` is a polyline with gaps.
        "points" => {
            for (k, run) in runs(p[0], p[1], n).into_iter().enumerate() {
                if k > 0 {
                    out.push("M".into());
                }
                out.extend(run.iter().map(|&(x, y)| xy(x, y)));
            }
        }
        // Catmull-Rom through the points, as cubic Béziers.
        "smooth" => {
            for run in runs(p[0], p[1], n) {
                out.push(format!("M {}", xy(run[0].0, run[0].1)));
                for i in 0..run.len().saturating_sub(1) {
                    let at = |k: isize| run[k.clamp(0, run.len() as isize - 1) as usize];
                    let (p0, p1, p2, p3) = (at(i as isize - 1), at(i as isize), at(i as isize + 1), at(i as isize + 2));
                    let c1 = (p1.0 + (p2.0 - p0.0) / 6.0, p1.1 + (p2.1 - p0.1) / 6.0);
                    let c2 = (p2.0 - (p3.0 - p1.0) / 6.0, p2.1 - (p3.1 - p1.1) / 6.0);
                    out.push(format!("C {} {} {}", xy(c1.0, c1.1), xy(c2.0, c2.1), xy(p2.0, p2.1)));
                }
            }
        }
        "circles" => {
            for i in 0..n {
                let (x, y, r) = (p[0].at(i), p[1].at(i), p[2].at(i).abs());
                if x.is_finite() && y.is_finite() && r.is_finite() && r > 0.0 {
                    let (rr, d) = (pn(r), pn(2.0 * r));
                    out.push(format!("M {} a {rr},{rr} 0 1,0 {d},0 a {rr},{rr} 0 1,0 -{d},0 Z", xy(x - r, y)));
                }
            }
        }
        "segments" => {
            for i in 0..n {
                let (x0, y0, x1, y1) = (p[0].at(i), p[1].at(i), p[2].at(i), p[3].at(i));
                if [x0, y0, x1, y1].iter().all(|v| v.is_finite()) {
                    out.push(format!("M {} L {}", xy(x0, y0), xy(x1, y1)));
                }
            }
        }
        // bars(xs, ys, base, w): one rectangle per point, centred on x.
        "bars" => {
            for i in 0..n {
                let (x, y, base, w) = (p[0].at(i), p[1].at(i), p[2].at(i), p[3].at(i).abs());
                if [x, y, base, w].iter().all(|v| v.is_finite()) {
                    let (l, r) = (x - w / 2.0, x + w / 2.0);
                    out.push(format!("M {} L {} L {} L {} Z", xy(l, base), xy(l, y), xy(r, y), xy(r, base)));
                }
            }
        }
        // area(xs, ys, base): each run closed down to y = base.
        _ => {
            let b = p[2].at(0);
            if b.is_finite() {
                for run in runs(p[0], p[1], n) {
                    let (first, last) = (run[0], run[run.len() - 1]);
                    let mut s = format!("M {}", xy(first.0, b));
                    for &(x, y) in &run {
                        s.push_str(&format!(" L {}", xy(x, y)));
                    }
                    s.push_str(&format!(" L {} Z", xy(last.0, b)));
                    out.push(s);
                }
            }
        }
    }
    Ok(Value::Str(out.join(" ")))
}
