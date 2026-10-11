//! Dimensions and units (entity-composition-model.md §3.1; vision doc 06 §5).
//!
//! A quantity is stored as an SI magnitude plus a [`Dim`]: the exponents of
//! the seven base dimensions. `9.58 mW` is `{si: 0.00958, dim: W}`. The unit
//! someone typed or wants to read is presentation: this module parses any
//! prefixed or compound unit string into a scale and a dimension, names each
//! dimension's canonical coherent unit for the file format, and formats a
//! magnitude with an engineering prefix for people and the model. Plain
//! numbers are dimensionless, and so are angles: `rad` and `deg` are scales
//! on a bare number, so geometry stays in pixels and radians.

use std::fmt;

/// Exponents of m, kg, s, A, K, mol, cd.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug)]
pub struct Dim(pub [i8; 7]);

const BASE_NAMES: [&str; 7] = ["m", "kg", "s", "A", "K", "mol", "cd"];

impl Dim {
    pub const NONE: Dim = Dim([0; 7]);
    pub const LENGTH: Dim = Dim([1, 0, 0, 0, 0, 0, 0]);
    pub const MASS: Dim = Dim([0, 1, 0, 0, 0, 0, 0]);
    pub const TIME: Dim = Dim([0, 0, 1, 0, 0, 0, 0]);
    pub const CURRENT: Dim = Dim([0, 0, 0, 1, 0, 0, 0]);
    pub const TEMPERATURE: Dim = Dim([0, 0, 0, 0, 1, 0, 0]);
    pub const AMOUNT: Dim = Dim([0, 0, 0, 0, 0, 1, 0]);
    pub const LUMINOSITY: Dim = Dim([0, 0, 0, 0, 0, 0, 1]);

    pub fn is_none(self) -> bool {
        self == Dim::NONE
    }

    pub fn mul(self, o: Dim) -> Dim {
        let mut d = [0i8; 7];
        for i in 0..7 {
            d[i] = self.0[i] + o.0[i];
        }
        Dim(d)
    }

    pub fn div(self, o: Dim) -> Dim {
        let mut d = [0i8; 7];
        for i in 0..7 {
            d[i] = self.0[i] - o.0[i];
        }
        Dim(d)
    }

    pub fn pow(self, n: i8) -> Dim {
        let mut d = [0i8; 7];
        for i in 0..7 {
            d[i] = self.0[i] * n;
        }
        Dim(d)
    }

    /// The dimension of a square root, if every exponent is even.
    pub fn sqrt(self) -> Option<Dim> {
        if self.0.iter().any(|e| e % 2 != 0) {
            return None;
        }
        let mut d = [0i8; 7];
        for i in 0..7 {
            d[i] = self.0[i] / 2;
        }
        Some(Dim(d))
    }

    /// The coherent SI unit this dimension is written in: a named unit where
    /// one exists (`W`, `N`, `Hz`), else a product of base units
    /// (`kg*m2/s3`). The file format uses this, so a saved quantity is
    /// always `[si, canonical]` and diffs stay readable.
    pub fn canonical_unit(self) -> String {
        if self.is_none() {
            return String::new();
        }
        if let Some((name, _, _)) = NAMED.iter().find(|(_, d, f)| *d == self && *f == 1.0) {
            return (*name).to_owned();
        }
        let atom = |i: usize, e: i8| if e == 1 { BASE_NAMES[i].to_owned() } else { format!("{}{}", BASE_NAMES[i], e) };
        let num: Vec<String> = (0..7).filter(|&i| self.0[i] > 0).map(|i| atom(i, self.0[i])).collect();
        let den: Vec<String> = (0..7).filter(|&i| self.0[i] < 0).map(|i| atom(i, -self.0[i])).collect();
        match (num.is_empty(), den.is_empty()) {
            (false, true) => num.join("*"),
            (true, false) => format!("1/{}", den.join("*")),
            _ => format!("{}/{}", num.join("*"), den.join("*")),
        }
    }
}

impl fmt::Display for Dim {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.canonical_unit())
    }
}

/// Named units: symbol, dimension, and the factor to SI. Coherent units
/// (factor 1) are the canonical ones; the rest are accepted on input and
/// available for display.
const NAMED: &[(&str, Dim, f64)] = &[
    ("m", Dim::LENGTH, 1.0),
    ("g", Dim::MASS, 1e-3),
    ("kg", Dim::MASS, 1.0),
    ("s", Dim::TIME, 1.0),
    ("A", Dim::CURRENT, 1.0),
    ("K", Dim::TEMPERATURE, 1.0),
    ("mol", Dim::AMOUNT, 1.0),
    ("cd", Dim::LUMINOSITY, 1.0),
    ("Hz", Dim([0, 0, -1, 0, 0, 0, 0]), 1.0),
    ("N", Dim([1, 1, -2, 0, 0, 0, 0]), 1.0),
    ("Pa", Dim([-1, 1, -2, 0, 0, 0, 0]), 1.0),
    ("J", Dim([2, 1, -2, 0, 0, 0, 0]), 1.0),
    ("W", Dim([2, 1, -3, 0, 0, 0, 0]), 1.0),
    ("C", Dim([0, 0, 1, 1, 0, 0, 0]), 1.0),
    ("V", Dim([2, 1, -3, -1, 0, 0, 0]), 1.0),
    ("F", Dim([-2, -1, 4, 2, 0, 0, 0]), 1.0),
    ("Ohm", Dim([2, 1, -3, -2, 0, 0, 0]), 1.0),
    ("Ω", Dim([2, 1, -3, -2, 0, 0, 0]), 1.0),
    ("S", Dim([-2, -1, 3, 2, 0, 0, 0]), 1.0),
    ("Wb", Dim([2, 1, -2, -1, 0, 0, 0]), 1.0),
    ("T", Dim([0, 1, -2, -1, 0, 0, 0]), 1.0),
    ("H", Dim([2, 1, -2, -2, 0, 0, 0]), 1.0),
    // Accepted, not canonical.
    ("min", Dim::TIME, 60.0),
    ("h", Dim::TIME, 3600.0),
    ("L", Dim([3, 0, 0, 0, 0, 0, 0]), 1e-3),
    ("eV", Dim([2, 1, -2, 0, 0, 0, 0]), 1.602_176_634e-19),
    // Angles are dimensionless scales on a plain number.
    ("rad", Dim::NONE, 1.0),
    ("deg", Dim::NONE, std::f64::consts::PI / 180.0),
    ("°", Dim::NONE, std::f64::consts::PI / 180.0),
];

/// Units that do not take SI prefixes (`kmin` is not a thing).
const NO_PREFIX: &[&str] = &["kg", "min", "h", "deg", "°", "rad"];

const PREFIXES: &[(&str, i32)] = &[
    ("Y", 24),
    ("Z", 21),
    ("E", 18),
    ("P", 15),
    ("T", 12),
    ("G", 9),
    ("M", 6),
    ("k", 3),
    ("h", 2),
    ("da", 1),
    ("d", -1),
    ("c", -2),
    ("m", -3),
    ("µ", -6),
    ("μ", -6),
    ("u", -6),
    ("n", -9),
    ("p", -12),
    ("f", -15),
    ("a", -18),
    ("z", -21),
    ("y", -24),
];

/// A parsed unit string: multiply a magnitude in this unit by `factor` to
/// get SI.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Unit {
    pub dim: Dim,
    pub factor: f64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitError(pub String);

impl fmt::Display for UnitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown unit {:?}", self.0)
    }
}
impl std::error::Error for UnitError {}

/// One symbol, optionally prefixed: `W`, `mW`, `kg`, `µs`, `Ohm`.
fn atom(text: &str) -> Option<Unit> {
    if let Some((_, d, f)) = NAMED.iter().find(|(n, _, _)| *n == text) {
        return Some(Unit { dim: *d, factor: *f });
    }
    for (p, exp) in PREFIXES {
        if let Some(rest) = text.strip_prefix(p)
            && !rest.is_empty()
            && !NO_PREFIX.contains(&rest)
            && let Some((_, d, f)) = NAMED.iter().find(|(n, _, _)| *n == rest)
        {
            return Some(Unit { dim: *d, factor: f * 10f64.powi(*exp) });
        }
    }
    None
}

/// An atom with an integer exponent: `m2`, `s^-1`, `s-1`.
fn power(text: &str) -> Option<Unit> {
    let text = text.trim();
    if let Some(u) = atom(text) {
        return Some(u);
    }
    let split = text.find(|c: char| c == '^' || c.is_ascii_digit() || c == '-').filter(|&i| i > 0)?;
    let (name, exp) = text.split_at(split);
    let exp: i32 = exp.trim_start_matches('^').parse().ok()?;
    let u = atom(name)?;
    Some(Unit { dim: u.dim.pow(exp as i8), factor: u.factor.powi(exp) })
}

/// Parse a unit string: atoms with exponents joined by `*` or `·`, with at
/// most one `/` separating numerator and denominator (`kg*m2/s3`, `W/m2`,
/// `1/s`, `mW`).
pub fn parse_unit(text: &str) -> Result<Unit, UnitError> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(Unit { dim: Dim::NONE, factor: 1.0 });
    }
    let (num, den) = match text.split_once('/') {
        Some((n, d)) => (n, Some(d)),
        None => (text, None),
    };
    let mut unit = Unit { dim: Dim::NONE, factor: 1.0 };
    let combine = |unit: &mut Unit, part: &str, sign: i8| -> Result<(), UnitError> {
        for a in part.split(['*', '·']) {
            let a = a.trim();
            if a.is_empty() || (a == "1" && sign > 0) {
                continue;
            }
            let u = power(a).ok_or_else(|| UnitError(text.to_owned()))?;
            unit.dim = if sign > 0 { unit.dim.mul(u.dim) } else { unit.dim.div(u.dim) };
            unit.factor = if sign > 0 { unit.factor * u.factor } else { unit.factor / u.factor };
        }
        Ok(())
    };
    combine(&mut unit, num, 1)?;
    if let Some(d) = den {
        combine(&mut unit, d, -1)?;
    }
    Ok(unit)
}

/// Magnitude of `si` expressed in `unit`.
pub fn convert(si: f64, unit: Unit) -> f64 {
    si / unit.factor
}

/// Human- and model-facing rendering: the canonical unit with an engineering
/// prefix so the mantissa reads in `1..1000` (`0.00958 W` → `9.58 mW`,
/// `0.5 kg` → `500 g`). Not the file format, which stores `[si, canonical]`.
pub fn format(si: f64, dim: Dim) -> String {
    let canonical = dim.canonical_unit();
    // Mass is the one base whose coherent unit already carries a prefix.
    let (base, value) = if dim == Dim::MASS { ("g", si * 1e3) } else { (canonical.as_str(), si) };
    let prefixable = !base.is_empty() && !base.contains(['*', '/']) && atom(base).is_some();
    if value == 0.0 || !value.is_finite() || !prefixable {
        return join(value, base);
    }
    let exp = ((value.abs().log10() / 3.0).floor() as i32 * 3).clamp(-24, 24);
    if exp == 0 {
        return join(value, base);
    }
    let Some((prefix, _)) = PREFIXES.iter().find(|(p, e)| *e == exp && *p != "μ" && *p != "u") else {
        return join(value, base);
    };
    join(value / 10f64.powi(exp), &format!("{prefix}{base}"))
}

fn join(value: f64, unit: &str) -> String {
    // Twelve significant digits: enough for any display, and it hides the
    // noise a prefix division introduces (9.579999999999998).
    let rounded = if value == 0.0 || !value.is_finite() {
        value
    } else {
        let digits = 12 - value.abs().log10().floor() as i32 - 1;
        let k = 10f64.powi(digits.clamp(-300, 300));
        (value * k).round() / k
    };
    let n = crate::value::fmt_num(rounded);
    if unit.is_empty() { n } else { format!("{n} {unit}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_prefixed_named_and_compound_units() {
        let w = parse_unit("W").unwrap();
        assert_eq!(w.dim, Dim([2, 1, -3, 0, 0, 0, 0]));
        let mw = parse_unit("mW").unwrap();
        assert_eq!(mw.dim, w.dim);
        assert!((mw.factor - 1e-3).abs() < 1e-18);
        assert_eq!(parse_unit("nm").unwrap().dim, Dim::LENGTH);
        assert!((parse_unit("nm").unwrap().factor - 1e-9).abs() < 1e-24);
        assert_eq!(parse_unit("kg").unwrap(), Unit { dim: Dim::MASS, factor: 1.0 });
        assert!((parse_unit("mg").unwrap().factor - 1e-6).abs() < 1e-20);
        assert_eq!(parse_unit("µs").unwrap().dim, Dim::TIME);
        assert_eq!(parse_unit("kg*m2/s3").unwrap().dim, w.dim);
        assert_eq!(parse_unit("kg*m^2/s^3").unwrap().dim, w.dim);
        assert_eq!(parse_unit("W/m2").unwrap().dim, Dim([0, 1, -3, 0, 0, 0, 0]));
        assert_eq!(parse_unit("1/s").unwrap().dim, Dim([0, 0, -1, 0, 0, 0, 0]));
        assert_eq!(parse_unit("GHz").unwrap().factor, 1e9);
        assert_eq!(parse_unit("min").unwrap().factor, 60.0);
        assert!(parse_unit("kmin").is_err(), "no prefixes on minutes");
        assert!(parse_unit("furlong").is_err());
        assert_eq!(parse_unit("deg").unwrap().dim, Dim::NONE);
        assert_eq!(parse_unit("").unwrap().dim, Dim::NONE);
    }

    #[test]
    fn canonical_units_round_trip_every_named_dimension() {
        for (name, dim, _) in NAMED.iter().filter(|(_, d, _)| !d.is_none()) {
            let canonical = dim.canonical_unit();
            let back = parse_unit(&canonical).unwrap();
            assert_eq!(back.dim, *dim, "{name} -> {canonical}");
            assert_eq!(back.factor, 1.0, "{name} -> {canonical}");
        }
        assert_eq!(Dim([2, 1, -3, 0, 0, 0, 0]).canonical_unit(), "W");
        assert_eq!(Dim([0, 1, -3, 0, 0, 0, 0]).canonical_unit(), "kg/s3");
        assert_eq!(Dim([0, 0, -2, 0, 0, 0, 0]).canonical_unit(), "1/s2");
        assert_eq!(Dim([1, 0, -1, 0, 0, 0, 0]).canonical_unit(), "m/s");
        assert_eq!(Dim::NONE.canonical_unit(), "");
    }

    #[test]
    fn dimension_algebra() {
        let w = parse_unit("W").unwrap().dim;
        let v = parse_unit("V").unwrap().dim;
        let a = parse_unit("A").unwrap().dim;
        assert_eq!(v.mul(a), w);
        assert_eq!(w.div(a), v);
        assert_eq!(Dim::LENGTH.pow(2).sqrt(), Some(Dim::LENGTH));
        assert_eq!(Dim::LENGTH.sqrt(), None);
    }

    #[test]
    fn formats_with_engineering_prefixes() {
        let w = parse_unit("W").unwrap().dim;
        assert_eq!(format(0.00958, w), "9.58 mW");
        assert_eq!(format(1500.0, w), "1.5 kW");
        assert_eq!(format(2.0, w), "2 W");
        assert_eq!(format(0.0, w), "0 W");
        assert_eq!(format(0.5, Dim::MASS), "500 g");
        assert_eq!(format(1550e-9, Dim::LENGTH), "1.55 µm");
        assert_eq!(format(3.0, Dim([0, 1, -3, 0, 0, 0, 0])), "3 kg/s3");
        assert_eq!(format(0.25, Dim::NONE), "0.25");
        assert_eq!(convert(0.00958, parse_unit("mW").unwrap()), 9.58);
    }
}
