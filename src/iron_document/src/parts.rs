//! Parts as data (plans/specs/schematic.md §2): the standard electrical
//! parts, written as markup in `std_parts.xml`, and the definitions they
//! become.
//!
//! A part is an ordinary definition: inputs (`value` in-ports, an
//! instance's arguments), pins (`electrical` ports), behaviour as `equation`
//! nodes over them, and a template for notation `schematic`, its symbol.
//! Each pin is a port and its place: the port_spec, and in the symbol an
//! `empty` whose `anchor` names it, so wires can bind to where it is. The
//! library is embedded and read-only, like the built-in contexts; a document
//! gets a copy of a part on first placement (`import.source = std:<key>`).

use crate::expr::Expr;
use crate::facet::{Anchor, Equation, Facet, Fill, Font, Import, LeafStruct, Name, PathShape, Port, Radius, Stroke, Template, Text, Transform2d};
use crate::geometry::{Geometry, PackedContour};
use crate::id::NodeId;
use crate::op::Op;
use crate::order::OrderKey;
use crate::registry::TypeId;
use crate::shape;
use crate::value::{Slot, Value};
use kurbo::{Point, Vec2};
use std::sync::{Arc, OnceLock};

/// The notation part symbols are drawn in.
pub const NOTATION: &str = "schematic";
/// The port kind of a pin.
pub const PIN_KIND: &str = "electrical";

/// A part, as the markup says.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub key: String,
    pub name: String,
    /// A designator's letters: `R` makes `R1`, `R2`, …
    pub prefix: String,
    /// Inputs and their defaults. Every part also has `designator`.
    pub inputs: Vec<(String, Value)>,
    pub pins: Vec<Pin>,
    /// `lhs = rhs`, in the part's own names.
    pub equations: Vec<(String, String)>,
    pub symbol: Vec<Mark>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pin {
    pub name: String,
    pub at: [f64; 2],
    /// The way a wire leaves it, radians (`right` 0, `down` π/2).
    pub face: f64,
}

/// One piece of a symbol.
#[derive(Clone, Debug, PartialEq)]
pub enum Mark {
    /// Stroked lines and curves.
    Path(Vec<PackedContour>),
    Circle { at: [f64; 2], r: f64 },
    /// A filled circle.
    Dot { at: [f64; 2], r: f64 },
    /// Words: literal, or `{expr}` over the part's names. Upright on a
    /// turned part, beside it at `turned` (sheet px from the origin) when a
    /// quarter turn stands it on end; or turning with it (`with`).
    Label { at: [f64; 2], text: String, turned: Option<[f64; 2]>, with: bool },
}

/// The standard parts, parsed once.
pub fn library() -> &'static [Part] {
    static LIB: OnceLock<Vec<Part>> = OnceLock::new();
    LIB.get_or_init(|| parse(include_str!("std_parts.xml")).expect("std_parts.xml parses"))
}

pub fn find(key: &str) -> Option<&'static Part> {
    library().iter().find(|p| p.key == key)
}

/// An element of the markup: its name, attributes and children, the text
/// it holds (entities decoded), and its own source.
#[derive(Debug, Clone)]
pub(crate) struct El {
    pub(crate) name: String,
    pub(crate) attrs: Vec<(String, String)>,
    pub(crate) children: Vec<El>,
    /// Text between its tags, decoded; for a raw element ([`RAW`]), all of
    /// its content as written, decoded.
    pub(crate) text: String,
    /// The element as written, tags included.
    pub(crate) src: String,
    /// What lies between its tags, as written.
    pub(crate) inner: String,
}

/// Elements whose content is text, not elements: markdown and SVG.
const RAW: &[&str] = &["write", "rewrite", "import-svg"];

impl El {
    pub(crate) fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
    pub(crate) fn need(&self, k: &str) -> Result<&str, String> {
        self.attr(k).ok_or_else(|| format!("<{}> needs {k}", self.name))
    }
    pub(crate) fn num(&self, k: &str) -> Result<f64, String> {
        match self.attr(k) {
            None => Ok(0.0),
            Some(v) => v.trim().parse().map_err(|_| format!("<{}> {k}=\"{v}\": expected a number", self.name)),
        }
    }
}

/// XML's entities in text, decoded.
pub(crate) fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&#10;", "\n").replace("&amp;", "&")
}

/// Elements in `src`, comments, `<?…?>` and `<!…>` declarations skipped.
/// Text between tags is the element's `text`; the elements in [`RAW`]
/// hold text only, so markdown or SVG inside them is not read as markup.
pub(crate) fn elements(src: &str) -> Result<Vec<El>, String> {
    fn level(all: &str, rest: &mut &str, closing: Option<&str>, text: &mut String) -> Result<Vec<El>, String> {
        let mut out = Vec::new();
        loop {
            let t = rest.find('<').unwrap_or(rest.len());
            if !rest[..t].trim().is_empty() {
                if closing.is_none() {
                    return Err(format!("text outside an element: \"{}\"", rest[..t].trim().chars().take(40).collect::<String>()));
                }
                text.push_str(&unescape(&rest[..t]));
            }
            *rest = &rest[t..];
            if rest.is_empty() {
                return match closing {
                    Some(n) => Err(format!("<{n}> is not closed")),
                    None => Ok(out),
                };
            }
            // A `<` that starts no tag is text (a table's merge mark, `a < b`).
            if !rest[1..].starts_with(|c: char| c.is_alphabetic() || matches!(c, '/' | '!' | '?')) {
                if closing.is_none() {
                    return Err(format!("text outside an element: \"{}\"", rest.chars().take(40).collect::<String>()));
                }
                text.push('<');
                *rest = &rest[1..];
                continue;
            }
            if let Some(r) = rest.strip_prefix("<!--") {
                let end = r.find("-->").ok_or("unclosed comment")?;
                *rest = &r[end + 3..];
                continue;
            }
            if let Some(r) = rest.strip_prefix("<![CDATA[") {
                let end = r.find("]]>").ok_or("unclosed CDATA")?;
                text.push_str(&r[..end]);
                *rest = &r[end + 3..];
                continue;
            }
            if rest.starts_with("<?") || rest.starts_with("<!") {
                let end = rest.find('>').ok_or("unclosed declaration")?;
                *rest = &rest[end + 1..];
                continue;
            }
            if let Some(r) = rest.strip_prefix("</") {
                let end = r.find('>').ok_or("unclosed tag")?;
                let name = r[..end].trim();
                if Some(name) != closing {
                    return Err(format!("</{name}> does not close {}", closing.map_or("anything".to_owned(), |c| format!("<{c}>"))));
                }
                *rest = &r[end + 1..];
                return Ok(out);
            }
            let start = all.len() - rest.len();
            let r = &rest[1..];
            // The tag ends at the first `>` outside a quoted value.
            let mut quote: Option<char> = None;
            let end = r.char_indices().find(|&(_, c)| {
                match quote {
                    Some(q) if c == q => quote = None,
                    None if c == '"' || c == '\'' => quote = Some(c),
                    _ => {}
                }
                c == '>' && quote.is_none()
            }).map(|(i, _)| i).ok_or("unclosed tag")?;
            let (tag, empty) = match r[..end].strip_suffix('/') {
                Some(t) => (t, true),
                None => (&r[..end], false),
            };
            *rest = &r[end + 1..];
            let (name, attrs) = tag.trim().split_once(char::is_whitespace).unwrap_or((tag.trim(), ""));
            if name.is_empty() || !name.chars().next().is_some_and(|c| c.is_alphabetic()) {
                return Err(format!("expected a tag at \"<{}\"", tag.chars().take(20).collect::<String>()));
            }
            let attrs = crate::contexts::attributes(attrs).map_err(|e| format!("<{name}>: {e}"))?;
            let inner_start = all.len() - rest.len();
            let mut own = String::new();
            let children = if empty {
                Vec::new()
            } else if RAW.contains(&name) {
                let close = format!("</{name}>");
                let at = rest.find(&close).ok_or_else(|| format!("<{name}> is not closed"))?;
                let body = &rest[..at];
                own = match body.trim().strip_prefix("<![CDATA[").and_then(|b| b.strip_suffix("]]>")) {
                    Some(raw) => raw.to_owned(),
                    None => unescape(body),
                };
                *rest = &rest[at + close.len()..];
                Vec::new()
            } else {
                level(all, rest, Some(name), &mut own)?
            };
            let end_at = all.len() - rest.len();
            let inner = if empty { String::new() } else { all[inner_start..end_at].rsplit_once("</").map_or(String::new(), |(i, _)| i.to_owned()) };
            out.push(El { name: name.to_owned(), attrs, children, text: own, src: all[start..end_at].to_owned(), inner });
        }
    }
    let mut rest = src;
    let mut stray = String::new();
    level(src, &mut rest, None, &mut stray)
}

/// Parse parts markup.
pub fn parse(src: &str) -> Result<Vec<Part>, String> {
    let mut out = Vec::new();
    for el in elements(src)? {
        if el.name != "part" {
            return Err(format!("expected <part>, got <{}>", el.name));
        }
        let key = el.need("key")?.to_owned();
        let at = |m: &str| format!("part {key}: {m}");
        let mut part = Part { key: key.clone(), name: el.need("name")?.to_owned(), prefix: el.attr("prefix").unwrap_or("U").to_owned(), inputs: Vec::new(), pins: Vec::new(), equations: Vec::new(), symbol: Vec::new() };
        for c in &el.children {
            match c.name.as_str() {
                "in" => {
                    let v = c.need("value").map_err(|e| at(&e))?;
                    let value = Value::parse_quantity(v).or_else(|_| v.trim().parse::<f64>().map(Value::Number)).unwrap_or_else(|_| Value::Str(v.to_owned()));
                    part.inputs.push((c.need("name").map_err(|e| at(&e))?.to_owned(), value));
                }
                "pin" => {
                    let face = match c.attr("face").unwrap_or("right") {
                        "right" => 0.0,
                        "down" => std::f64::consts::FRAC_PI_2,
                        "left" => std::f64::consts::PI,
                        "up" => -std::f64::consts::FRAC_PI_2,
                        f => return Err(at(&format!("face=\"{f}\": expected left, right, up or down"))),
                    };
                    part.pins.push(Pin { name: c.need("name").map_err(|e| at(&e))?.to_owned(), at: [c.num("x").map_err(|e| at(&e))?, c.num("y").map_err(|e| at(&e))?], face });
                }
                "equation" => part.equations.push((c.need("lhs").map_err(|e| at(&e))?.to_owned(), c.need("rhs").map_err(|e| at(&e))?.to_owned())),
                "symbol" => {
                    for m in &c.children {
                        let xy = || -> Result<[f64; 2], String> { Ok([m.num("x")?, m.num("y")?]) };
                        part.symbol.push(match m.name.as_str() {
                            "path" => Mark::Path(path_data(m.need("d").map_err(|e| at(&e))?).map_err(|e| at(&e))?),
                            "circle" => Mark::Circle { at: xy().map_err(|e| at(&e))?, r: m.num("r").map_err(|e| at(&e))? },
                            "dot" => Mark::Dot { at: xy().map_err(|e| at(&e))?, r: m.num("r").map_err(|e| at(&e))? },
                            "label" => {
                                let turned = match m.attr("turned") {
                                    None => None,
                                    Some(t) => {
                                        let (x, y) = t.split_once(',').ok_or_else(|| at(&format!("turned=\"{t}\": expected x,y")))?;
                                        Some([x.trim().parse().map_err(|_| at(&format!("turned=\"{t}\"")))?, y.trim().parse().map_err(|_| at(&format!("turned=\"{t}\"")))?])
                                    }
                                };
                                let with = match m.attr("turn") {
                                    None | Some("upright") => false,
                                    Some("with") => true,
                                    Some(t) => return Err(at(&format!("turn=\"{t}\": upright or with"))),
                                };
                                Mark::Label { at: xy().map_err(|e| at(&e))?, text: m.need("text").map_err(|e| at(&e))?.to_owned(), turned, with }
                            }
                            other => return Err(at(&format!("a symbol has path, circle, dot and label, not <{other}>"))),
                        });
                    }
                }
                other => return Err(at(&format!("a part has in, pin, equation and symbol, not <{other}>"))),
            }
        }
        if part.pins.is_empty() {
            return Err(at("a part needs a pin"));
        }
        // Check every expression resolves now, not at placement.
        let names = Names::fake(&part);
        for (l, r) in &part.equations {
            for e in [l, r] {
                Expr::parse(&names.resolve(e)).map_err(|err| at(&format!("{e}: {err}")))?;
            }
        }
        for m in &part.symbol {
            if let Mark::Label { text, .. } = m
                && let Some(e) = text.strip_prefix('{').and_then(|t| t.strip_suffix('}'))
            {
                Expr::parse(&names.resolve(e)).map_err(|err| at(&format!("{e}: {err}")))?;
            }
        }
        out.push(part);
    }
    Ok(out)
}

/// SVG path data, absolute and relative `M L H V C Z`, as contours.
fn path_data(d: &str) -> Result<Vec<PackedContour>, String> {
    let mut contours: Vec<PackedContour> = Vec::new();
    let mut nums: Vec<f64> = Vec::new();
    let mut cmd: Option<char> = None;
    let mut cur = Point::ZERO;
    let mut tokens: Vec<Result<char, f64>> = Vec::new();
    let mut chars = d.char_indices().peekable();
    while let Some(&(i, c)) = chars.peek() {
        if c.is_ascii_alphabetic() {
            tokens.push(Ok(c));
            chars.next();
        } else if c.is_ascii_digit() || c == '-' || c == '.' || c == '+' {
            let mut end = i + c.len_utf8();
            chars.next();
            while let Some(&(j, n)) = chars.peek() {
                let exp = matches!(n, '-' | '+') && d[..j].ends_with(['e', 'E']);
                if n.is_ascii_digit() || n == '.' || n == 'e' || n == 'E' || exp {
                    end = j + n.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
            tokens.push(Err(d[i..end].parse().map_err(|_| format!("d: bad number {:?}", &d[i..end]))?));
        } else {
            chars.next();
        }
    }
    let line = |contours: &mut Vec<PackedContour>, to: Point| {
        if let Some(c) = contours.last_mut() {
            c.anchors.push(shape::Anchor { at: to, inn: Vec2::ZERO, out: Vec2::ZERO });
        }
    };
    let flush = |cmd: char, nums: &mut Vec<f64>, cur: &mut Point, contours: &mut Vec<PackedContour>| -> Result<(), String> {
        let rel = cmd.is_ascii_lowercase();
        let base = |p: Point, x: f64, y: f64| if rel { Point::new(p.x + x, p.y + y) } else { Point::new(x, y) };
        let need = match cmd.to_ascii_uppercase() {
            'M' | 'L' => 2,
            'H' | 'V' => 1,
            'C' => 6,
            'Z' => 0,
            other => return Err(format!("d: {other} is not one of M L H V C Z")),
        };
        if need == 0 {
            if let Some(c) = contours.last_mut() {
                c.closed = true;
            }
            return Ok(());
        }
        if nums.is_empty() || nums.len() % need != 0 {
            return Err(format!("d: {cmd} takes {need} numbers at a time, got {}", nums.len()));
        }
        for (k, a) in nums.chunks(need).enumerate() {
            match cmd.to_ascii_uppercase() {
                'M' if k == 0 => {
                    *cur = base(*cur, a[0], a[1]);
                    contours.push(PackedContour { closed: false, anchors: vec![shape::Anchor { at: *cur, inn: Vec2::ZERO, out: Vec2::ZERO }] });
                }
                'M' | 'L' => {
                    *cur = base(*cur, a[0], a[1]);
                    line(contours, *cur);
                }
                'H' => {
                    *cur = if rel { Point::new(cur.x + a[0], cur.y) } else { Point::new(a[0], cur.y) };
                    line(contours, *cur);
                }
                'V' => {
                    *cur = if rel { Point::new(cur.x, cur.y + a[0]) } else { Point::new(cur.x, a[0]) };
                    line(contours, *cur);
                }
                _ => {
                    let (c1, c2, to) = (base(*cur, a[0], a[1]), base(*cur, a[2], a[3]), base(*cur, a[4], a[5]));
                    let c = contours.last_mut().ok_or("d: C before M")?;
                    let last = c.anchors.last_mut().expect("a contour starts with its M");
                    last.out = c1 - last.at;
                    c.anchors.push(shape::Anchor { at: to, inn: c2 - to, out: Vec2::ZERO });
                    *cur = to;
                }
            }
        }
        nums.clear();
        Ok(())
    };
    for t in tokens {
        match t {
            Ok(c) => {
                if let Some(prev) = cmd {
                    flush(prev, &mut nums, &mut cur, &mut contours)?;
                }
                if contours.is_empty() && !c.eq_ignore_ascii_case(&'m') {
                    return Err("d: a path starts with M".into());
                }
                cmd = Some(c);
            }
            Err(n) => nums.push(n),
        }
    }
    if let Some(prev) = cmd {
        flush(prev, &mut nums, &mut cur, &mut contours)?;
    }
    Ok(contours)
}

/// How a part's names become its nodes' leaves.
struct Names {
    /// Input name → its port_spec.
    inputs: Vec<(String, NodeId)>,
    /// Pin name → its port_spec.
    pins: Vec<(String, NodeId)>,
}

impl Names {
    /// Stand-in ids, to check expressions parse.
    fn fake(p: &Part) -> Names {
        let id = |i: usize| NodeId::from_raw(1 + i as u64);
        Names {
            inputs: p.inputs.iter().map(|(n, _)| n.clone()).chain(["designator".to_owned()]).enumerate().map(|(i, n)| (n, id(i))).collect(),
            pins: p.pins.iter().enumerate().map(|(i, x)| (x.name.clone(), id(100 + i))).collect(),
        }
    }

    /// `e` with the part's names as leaves: `a.across` is the pin's
    /// `#id.port.across`, an input `R` its `#id.port.signal`. A word right
    /// after a number is a unit (`0 A`), and words in quotes are text.
    fn resolve(&self, e: &str) -> String {
        let mut out = String::with_capacity(e.len());
        let mut after_number = false;
        let mut chars = e.char_indices().peekable();
        while let Some((i, c)) = chars.next() {
            if c == '"' {
                out.push(c);
                for (_, d) in chars.by_ref() {
                    out.push(d);
                    if d == '"' {
                        break;
                    }
                }
                after_number = false;
            } else if c.is_ascii_digit() || (c == '.' && chars.peek().is_some_and(|(_, d)| d.is_ascii_digit())) {
                out.push(c);
                while let Some(&(_, d)) = chars.peek() {
                    if d.is_ascii_digit() || d == '.' || d == 'e' {
                        out.push(d);
                        chars.next();
                    } else {
                        break;
                    }
                }
                after_number = true;
            } else if c.is_alphabetic() || c == '_' {
                let mut end = i + c.len_utf8();
                while let Some(&(j, d)) = chars.peek() {
                    if d.is_alphanumeric() || d == '_' || d == '.' {
                        end = j + d.len_utf8();
                        chars.next();
                    } else {
                        break;
                    }
                }
                let word = &e[i..end];
                let pin = word.split_once('.').and_then(|(p, f)| {
                    let id = self.pins.iter().find(|(n, _)| n == p)?.1;
                    matches!(f, "across" | "through").then(|| format!("{id}.port.{f}"))
                });
                match (after_number, pin, self.inputs.iter().find(|(n, _)| n == word)) {
                    (true, ..) => out.push_str(word),
                    (_, Some(p), _) => out.push_str(&p),
                    (_, None, Some((_, id))) => out.push_str(&format!("{id}.port.signal")),
                    _ => out.push_str(word),
                }
                after_number = false;
            } else {
                if !c.is_whitespace() {
                    after_number = false;
                }
                out.push(c);
            }
        }
        out
    }
}

fn ty(n: &str) -> TypeId {
    TypeId::lookup(n).expect("registered type")
}
fn named(t: &str) -> Facet {
    let mut n = Name::default();
    n.text = Slot::Const(Value::Str(t.into()));
    n.wrap()
}
fn at(x: f64, y: f64, rot: f64) -> Facet {
    let mut t = Transform2d::default();
    t.x = Slot::Const(Value::Number(x));
    t.y = Slot::Const(Value::Number(y));
    t.rot = Slot::Const(Value::Number(rot));
    t.wrap()
}
/// Parts draw in the theme's colour tokens (palette.rs), so a schematic
/// reads in light and dark: symbols and wires in `@ink`, labels `@ink-2`.
fn token(name: &str) -> Slot {
    Slot::Bound(crate::Expr::parse(&format!("@{name}")).expect("a colour token"))
}
fn stroke(width: f64) -> Facet {
    let mut s = Stroke::default();
    s.color = token("ink");
    s.width = Slot::Const(Value::Number(width));
    s.wrap()
}
fn fill(name: &str) -> Facet {
    let mut f = Fill::default();
    f.color = token(name);
    f.wrap()
}

/// The ops creating `part`'s definition under `library`, ids from `first`.
/// Returns the ops and the definition.
pub fn definition_ops(part: &Part, library: NodeId, order: OrderKey, first: u64) -> Result<(Vec<Op>, NodeId), String> {
    let mut next = first;
    let mut id = || {
        let n = NodeId::from_raw(next);
        next += 1;
        n
    };
    let def = id();
    let mut imp = Import::default();
    imp.source = Slot::Const(Value::Str(format!("std:{}", part.key)));
    imp.version = Slot::Const(Value::Str("1".into()));
    let mut ops = vec![Op::Create { id: def, ty: ty("definition"), parent: Some(library), order, facets: vec![named(&part.name.to_lowercase()), imp.wrap()] }];
    let mut key = OrderKey::first();
    let mut child = |ops: &mut Vec<Op>, id: NodeId, t: &str, parent: NodeId, facets: Vec<Facet>| {
        ops.push(Op::Create { id, ty: ty(t), parent: Some(parent), order: key.clone(), facets });
        key = OrderKey::after(&key);
    };
    let mut names = Names { inputs: Vec::new(), pins: Vec::new() };
    let inputs = part.inputs.iter().cloned().chain([("designator".to_owned(), Value::Str(format!("{}?", part.prefix)))]);
    for (name, value) in inputs {
        let spec = id();
        let mut p = Port::default();
        p.kind = Slot::Const(Value::Str("value".into()));
        p.dir = Slot::Const(Value::Enum("in".into()));
        p.signal = Slot::Const(value);
        child(&mut ops, spec, "port_spec", def, vec![named(&name), p.wrap()]);
        names.inputs.push((name, spec));
    }
    for pin in &part.pins {
        let spec = id();
        let mut p = Port::default();
        p.kind = Slot::Const(Value::Str(PIN_KIND.into()));
        p.dir = Slot::Const(Value::Enum("both".into()));
        p.across = Slot::Const(Value::parse_quantity("0 V").expect("quantity"));
        p.through = Slot::Const(Value::parse_quantity("0 A").expect("quantity"));
        child(&mut ops, spec, "port_spec", def, vec![named(&pin.name), p.wrap()]);
        names.pins.push((pin.name.clone(), spec));
    }
    let expr = |e: &str| Expr::parse(&names.resolve(e)).map_err(|err| format!("part {}: {e}: {err}", part.key));
    for (l, r) in &part.equations {
        let mut eq = Equation::default();
        eq.lhs = Slot::Bound(expr(l)?);
        eq.rhs = Slot::Bound(expr(r)?);
        let e = id();
        child(&mut ops, e, "equation", def, vec![eq.wrap()]);
    }
    let template = id();
    let mut t = Template::default();
    t.notation = Slot::Const(Value::Str(NOTATION.into()));
    child(&mut ops, template, "template", def, vec![t.wrap()]);
    let mut key = OrderKey::first();
    let mut mark = |ops: &mut Vec<Op>, id: NodeId, t: &str, facets: Vec<Facet>| {
        ops.push(Op::Create { id, ty: ty(t), parent: Some(template), order: key.clone(), facets });
        key = OrderKey::after(&key);
    };
    for m in &part.symbol {
        match m {
            Mark::Path(contours) => {
                let mut p = PathShape::default();
                p.d = Slot::Const(Value::Geometry(Arc::new(Geometry::new(contours.clone()))));
                let n = id();
                mark(&mut ops, n, "path", vec![at(0.0, 0.0, 0.0), p.wrap(), stroke(2.0)]);
            }
            Mark::Circle { at: c, r } | Mark::Dot { at: c, r } => {
                let mut rad = Radius::default();
                rad.rx = Slot::Const(Value::Number(*r));
                rad.ry = Slot::Const(Value::Number(*r));
                let look = if matches!(m, Mark::Dot { .. }) { fill("ink") } else { stroke(2.0) };
                let n = id();
                mark(&mut ops, n, "ellipse", vec![at(c[0], c[1], 0.0), rad.wrap(), look]);
            }
            Mark::Label { at: p, text, .. } => {
                let mut words = Text::default();
                words.content = match text.strip_prefix('{').and_then(|t| t.strip_suffix('}')) {
                    Some(e) => Slot::Bound(expr(e)?),
                    None => Slot::Const(Value::Str(text.clone())),
                };
                let mut f = Font::default();
                f.size = Slot::Const(Value::Number(11.0));
                let n = id();
                mark(&mut ops, n, "text", vec![at(p[0], p[1], 0.0), words.wrap(), f.wrap(), fill("ink-2")]);
            }
        }
    }
    for (pin, (_, spec)) in part.pins.iter().zip(&names.pins) {
        let mut a = Anchor::default();
        a.port = Slot::Const(Value::Ref(*spec));
        let n = id();
        mark(&mut ops, n, "empty", vec![at(pin.at[0], pin.at[1], pin.face), a.wrap(), named(&pin.name)]);
    }
    Ok((ops, def))
}

/// The definition of part `key` already copied into `doc`, if any.
pub fn find_in(doc: &crate::document::Document, key: &str) -> Option<NodeId> {
    crate::standard::find(doc, key)
}

/// The next free designator for `part` in `doc`: `R3` after `R1` and `R2`.
pub fn next_designator(doc: &crate::document::Document, part: &Part) -> String {
    let n = doc
        .nodes_of_type(ty("entity"))
        .filter(|&e| doc.is_live(e))
        .filter_map(|e| doc.get::<Name>(e)?.text.constant()?.as_str()?.strip_prefix(part.prefix.as_str())?.parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    format!("{}{}", part.prefix, n + 1)
}

/// What placing a part made.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placed {
    pub entity: NodeId,
    pub figure: NodeId,
}

impl crate::Store {
    /// Place part `key` in `composition` with its origin at `at`, turned by
    /// `rot` radians, as one history entry: its definition copied in on
    /// first use, then a linked instance named by its next designator.
    pub fn place_part(&mut self, key: &str, composition: NodeId, at_: [f64; 2], rot: f64, actor: crate::Actor) -> Result<Placed, String> {
        let part = find(key).ok_or_else(|| format!("no part {key:?}"))?;
        let (ops, placed) = place_ops(&self.doc, part, composition, at_, rot, None)?;
        let label = format!("place {}", part.name.to_lowercase());
        self.apply(crate::TransactionInput { label, actor, ts: 0, idempotency_key: None, ops, exact: false }).map_err(|e| e.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "))?;
        Ok(placed)
    }
}

/// The ops placing `part` (see [`crate::Store::place_part`]), named
/// `name` or by its next designator.
pub fn place_ops(doc: &crate::document::Document, part: &Part, composition: NodeId, at_: [f64; 2], rot: f64, name: Option<&str>) -> Result<(Vec<Op>, Placed), String> {
    use crate::standard::{after_last, root_of};
    let mut ops = Vec::new();
    let mut scratch = doc.clone();
    let stamp = scratch.version().next();
    let push = |scratch: &mut crate::document::Document, ops: &mut Vec<Op>, new: Vec<Op>| -> Result<(), String> {
        for op in &new {
            crate::op::apply_op(scratch, 0, op, stamp).map_err(|e| e.message)?;
        }
        ops.extend(new);
        Ok(())
    };
    let def = match find_in(&scratch, &part.key) {
        Some(d) => d,
        None => {
            let library = match root_of(&scratch, "library") {
                Some(l) => l,
                None => {
                    let l = scratch.next_id();
                    let order = after_last(&scratch, None);
                    push(&mut scratch, &mut ops, vec![Op::Create { id: l, ty: ty("library"), parent: None, order, facets: vec![] }])?;
                    l
                }
            };
            let (def_ops, def) = definition_ops(part, library, after_last(&scratch, Some(library)), scratch.next_id().raw())?;
            push(&mut scratch, &mut ops, def_ops)?;
            def
        }
    };
    let model = match root_of(&scratch, "model") {
        Some(m) => m,
        None => {
            let m = scratch.next_id();
            let order = after_last(&scratch, None);
            push(&mut scratch, &mut ops, vec![Op::Create { id: m, ty: ty("model"), parent: None, order, facets: vec![] }])?;
            m
        }
    };
    let designator = name.map_or_else(|| next_designator(&scratch, part), str::to_owned);
    let last = |p: NodeId| scratch.children(p).iter().filter(|c| !c.is_expanded()).filter_map(|&c| scratch.live(c)).map(|n| n.order.clone()).max();
    let place = crate::definition::Placement { entity_parent: Some(model), entity_after: last(model), figure_parent: Some(composition), figure_after: last(composition) };
    let (entity, mut inst) = crate::definition::instantiate(&scratch, def, NOTATION, &designator, &place)?;
    let mut figure = None;
    for op in inst.iter_mut() {
        let Op::Create { id, ty: t, facets, .. } = op else { continue };
        match t.name() {
            "entity" => {
                for f in facets.iter_mut() {
                    if let Facet::Args(a) = f {
                        a.insert("designator", Slot::Const(Value::Str(designator.clone())));
                    }
                }
            }
            "use" => {
                figure = Some(*id);
                for f in facets.iter_mut() {
                    if let Facet::Transform2d(tr) = f {
                        tr.x = Slot::Const(Value::Number(at_[0]));
                        tr.y = Slot::Const(Value::Number(at_[1]));
                        tr.rot = Slot::Const(Value::Number(rot));
                    }
                }
            }
            _ => {}
        }
    }
    let figure = figure.ok_or("no picture placed")?;
    push(&mut scratch, &mut ops, inst)?;
    // A turned part's labels stay upright: overrides on its picture.
    let quarter = (rot / std::f64::consts::FRAC_PI_2).round().rem_euclid(4.0) as u8;
    if quarter != 0
        && let Some(template) = crate::definition::template_of(&scratch, def, NOTATION)
    {
        let labels: Vec<NodeId> = scratch.children(template).iter().copied().filter(|&c| scratch.live(c).is_some_and(|n| n.ty.name() == "text")).collect();
        let marks = part.symbol.iter().filter_map(|m| match m {
            Mark::Label { at, turned, with, .. } => Some((*at, *turned, *with)),
            _ => None,
        });
        let r = f64::from(quarter) * std::f64::consts::FRAC_PI_2;
        let (s, c) = r.sin_cos();
        let mut overrides = Vec::new();
        let mut key = OrderKey::first();
        for (label, (at, turned, with)) in labels.into_iter().zip(marks) {
            if with {
                continue;
            }
            // Where it goes from the part's origin on the sheet, then back
            // into the part's turned frame.
            let world = if quarter % 2 == 1 { turned.unwrap_or([c * at[0] - s * at[1], s * at[0] + c * at[1]]) } else { at };
            let local = [c * world[0] + s * world[1], -s * world[0] + c * world[1]];
            for (leaf, v) in [("transform2d.x", local[0]), ("transform2d.y", local[1]), ("transform2d.rot", -r)] {
                let mut o = crate::facet::Override::default();
                o.target = Slot::Const(Value::Ref(label));
                o.leaf = Slot::Const(Value::Str(leaf.into()));
                o.value = Slot::Const(Value::Number(v));
                overrides.push((o, key.clone()));
                key = OrderKey::after(&key);
            }
        }
        let ids = scratch.next_ids(overrides.len());
        let new: Vec<Op> = overrides.into_iter().zip(ids).map(|((o, order), id)| Op::Create { id, ty: ty("override"), parent: Some(figure), order, facets: vec![o.wrap()] }).collect();
        push(&mut scratch, &mut ops, new)?;
    }
    Ok((ops, Placed { entity, figure }))
}

/// Ops joining two pins with a connection under `parent` (the model): the
/// connection node and its two `connects` relations. Returns the ops and
/// the connection.
pub fn connect_ops(doc: &crate::document::Document, a: NodeId, b: NodeId, parent: NodeId) -> (Vec<Op>, NodeId) {
    use crate::document::{RelKind, Relation};
    use std::collections::BTreeMap;
    let id = doc.next_id();
    let rels = doc.next_relation_ids(2);
    let order = crate::standard::after_last(doc, Some(parent));
    let mut c = crate::facet::Connection::default();
    c.domain = Slot::Const(Value::Str("electrical".into()));
    let end = |e: &str| BTreeMap::from([("end".to_owned(), Value::Str(e.into()))]);
    let ops = vec![
        Op::Create { id, ty: ty("connection"), parent: Some(parent), order, facets: vec![c.wrap()] },
        Op::Link { rel: Relation { id: rels[0], from: id, to: a, rel: RelKind::Connects, data: end("a") } },
        Op::Link { rel: Relation { id: rels[1], from: id, to: b, rel: RelKind::Connects, data: end("b") } },
    ];
    (ops, id)
}

/// An instance's pin by name: the entity's port.
pub fn pin(doc: &crate::document::Document, entity: NodeId, name: &str) -> Option<NodeId> {
    doc.children(entity).iter().copied().find(|&p| doc.live(p).is_some_and(|n| n.ty.name() == "port") && doc.get::<Name>(p).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(name))
}

/// Where a pin sits on a figure: the anchor in `figure`'s expanded picture
/// that names it (expansion points a copied anchor at the entity's port).
pub fn pin_anchor(doc: &crate::document::Document, figure: NodeId, pin: NodeId) -> Option<NodeId> {
    let mut stack = doc.children(figure).to_vec();
    while let Some(n) = stack.pop() {
        if doc.get::<Anchor>(n).and_then(|a| a.port.constant()).and_then(|v| v.as_ref()) == Some(pin) {
            return Some(n);
        }
        stack.extend(doc.children(n).iter().copied());
    }
    None
}

/// One end of a wire: a pin, and where it sits on the sheet (its anchor
/// in a figure there).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct End {
    pub pin: NodeId,
    pub anchor: NodeId,
}

/// The ops for a wire (schematic.md §4): a connection joining `a` and `b`
/// in the model, and its figure in `sheet`, a path through `points`
/// (sheet px, the ends' positions first and last). The ends bind to the
/// anchors, so the wire follows its parts; a corner next to an end binds
/// the coordinate its leg shares with that end, so a part moved across
/// the leg keeps it straight (rubber-banding). Returns the ops and the
/// path.
pub fn wire_ops(doc: &crate::document::Document, sheet: NodeId, a: End, b: End, points: &[[f64; 2]]) -> Result<(Vec<Op>, NodeId), String> {
    use crate::document::{RelKind, Relation};
    use crate::facet::{Contour, Handles};
    if points.len() < 2 {
        return Err("a wire needs its two ends".into());
    }
    let model = crate::standard::root_of(doc, "model").ok_or("no model to hold the connection")?;
    let (mut ops, connection) = connect_ops(doc, a.pin, b.pin, model);
    let ids = doc.next_ids(1 + 2 + points.len());
    let (path, contour) = (ids[1], ids[2]);
    let rel = doc.next_relation_ids(3)[2];
    let mut s = Stroke::default();
    s.color = token("ink");
    s.width = Slot::Const(Value::Number(2.0));
    ops.push(Op::Create { id: path, ty: ty("path"), parent: Some(sheet), order: crate::standard::after_last(doc, Some(sheet)), facets: vec![at(0.0, 0.0, 0.0), PathShape::default().wrap(), s.wrap(), named("wire")] });
    ops.push(Op::Link { rel: Relation { id: rel, from: path, to: connection, rel: RelKind::Figures, data: Default::default() } });
    ops.push(Op::Create { id: contour, ty: ty("contour"), parent: Some(path), order: OrderKey::first(), facets: vec![Contour::default().wrap()] });
    let bind = |anchor: NodeId, f: &str| Slot::Bound(Expr::parse(&format!("{anchor}.anchor.{f}")).expect("anchor leaf"));
    let num = |v: f64| Slot::Const(Value::Number(v));
    let last = points.len() - 1;
    let mut key = OrderKey::first();
    for (i, p) in points.iter().enumerate() {
        let mut t = Transform2d::default();
        let end = if i == 0 { Some(a.anchor) } else if i == last { Some(b.anchor) } else { None };
        match end {
            Some(anchor) => {
                t.x = bind(anchor, "wx");
                t.y = bind(anchor, "wy");
            }
            None => {
                t.x = num(p[0]);
                t.y = num(p[1]);
                // Beside an end: share its coordinate along the leg.
                for (j, anchor) in [(0, a.anchor), (last, b.anchor)] {
                    if i.abs_diff(j) == 1 {
                        if (points[j][1] - p[1]).abs() < 1e-9 {
                            t.y = bind(anchor, "wy");
                        } else if (points[j][0] - p[0]).abs() < 1e-9 {
                            t.x = bind(anchor, "wx");
                        }
                    }
                }
            }
        }
        ops.push(Op::Create { id: ids[3 + i], ty: ty("point"), parent: Some(contour), order: key.clone(), facets: vec![t.wrap(), Handles::default().wrap()] });
        key = OrderKey::after(&key);
    }
    Ok((ops, path))
}

/// The corners an orthogonal wire takes from `from` to `to`: none when
/// they line up, else one, horizontal first unless `vertical_first`.
pub fn elbow(from: [f64; 2], to: [f64; 2], vertical_first: bool) -> Option<[f64; 2]> {
    if (from[0] - to[0]).abs() < 1e-9 || (from[1] - to[1]).abs() < 1e-9 {
        return None;
    }
    Some(if vertical_first { [from[0], to[1]] } else { [to[0], from[1]] })
}

/// Every pin drawn on `sheet`'s top level: its end and where it is.
pub fn pins_on(store: &crate::Store, sheet: NodeId) -> Vec<(End, [f64; 2])> {
    let doc = store.document();
    let mut out = Vec::new();
    for &f in doc.children(sheet) {
        if doc.live(f).is_none_or(|n| n.ty.name() != "use") {
            continue;
        }
        let mut stack = doc.children(f).to_vec();
        while let Some(n) = stack.pop() {
            if let Some(pin) = doc.get::<Anchor>(n).and_then(|a| a.port.constant()).and_then(|v| v.as_ref())
                && doc.get::<Port>(pin).and_then(|p| p.kind.constant()).and_then(|v| v.as_str()) == Some(PIN_KIND)
            {
                let num = |f: &str| store.resolved(n, format!("anchor.{f}").parse().expect("anchor leaf")).and_then(|v| v.as_f64());
                if let (Some(x), Some(y)) = (num("wx"), num("wy")) {
                    out.push((End { pin, anchor: n }, [x, y]));
                }
            }
            stack.extend(doc.children(n).iter().copied());
        }
    }
    out
}

/// What deleting `figure` removes when its context says `affords.delete =
/// "subject"`: what it figures, and everything that only means something
/// with it. An entity takes every figure of it, and every connection on its
/// ports with their figures (a part's wires); a connection takes its
/// figures. A node that projects content (timeline.md) takes that. A figure
/// of nothing is only itself.
pub fn delete_subject_ops(doc: &crate::document::Document, figure: NodeId) -> Vec<Op> {
    use crate::document::RelKind;
    // A session node that stands for content (a timeline bar `projects` its
    // clip) deletes that; the session view regenerates without it.
    if let Some(subject) = doc.relations_from(figure).find(|r| r.rel == RelKind::Projects).map(|r| r.to).filter(|&s| doc.is_live(s)) {
        return vec![Op::Delete { id: subject }];
    }
    let figures_of = |subject: NodeId| -> Vec<NodeId> { doc.relations_to(subject).filter(|r| r.rel == RelKind::Figures && doc.is_live(r.from)).map(|r| r.from).collect() };
    let Some(subject) = doc.relations_from(figure).find(|r| r.rel == RelKind::Figures).map(|r| r.to).filter(|&s| doc.is_live(s)) else {
        return vec![Op::Delete { id: figure }];
    };
    let mut gone: Vec<NodeId> = Vec::new();
    let add = |id: NodeId, gone: &mut Vec<NodeId>| {
        if !gone.contains(&id) {
            gone.push(id);
        }
    };
    if doc.live(subject).is_some_and(|n| n.ty.name() == "entity") {
        let ports: Vec<NodeId> = doc.children(subject).to_vec();
        for c in doc.nodes_of_type(ty("connection")).filter(|&c| doc.is_live(c)) {
            if crate::port::ends(doc, c).is_some_and(|(a, b)| ports.contains(&a) || ports.contains(&b)) {
                for f in figures_of(c) {
                    add(f, &mut gone);
                }
                add(c, &mut gone);
            }
        }
    }
    for f in figures_of(subject) {
        add(f, &mut gone);
    }
    add(figure, &mut gone);
    add(subject, &mut gone);
    gone.into_iter().map(|id| Op::Delete { id }).collect()
}

/// Where a wire starts or ends: on a pin, or on another wire (a T, made by
/// splitting that wire at a junction).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Terminal {
    Pin(End),
    Wire { path: NodeId, at: [f64; 2] },
}

/// The wire under `at` on `sheet` (a path figuring a connection), within
/// `reach` sheet px, and the nearest point on it.
pub fn wire_at(store: &crate::Store, sheet: NodeId, at: [f64; 2], reach: f64) -> Option<(NodeId, [f64; 2])> {
    let doc = store.document();
    let p = Point::new(at[0], at[1]);
    let mut best: Option<(f64, NodeId, Point)> = None;
    for &path in doc.children(sheet) {
        let Some(points) = wire_points(store, path) else { continue };
        for w in points.windows(2) {
            let (a, b) = (Point::new(w[0][0], w[0][1]), Point::new(w[1][0], w[1][1]));
            let ab = b - a;
            let t = if ab.hypot2() < 1e-12 { 0.0 } else { ((p - a).dot(ab) / ab.hypot2()).clamp(0.0, 1.0) };
            let q = a + ab * t;
            let d = q.distance(p);
            if d <= reach && best.is_none_or(|(bd, ..)| d < bd) {
                best = Some((d, path, q));
            }
        }
    }
    best.map(|(_, path, q)| (path, [q.x, q.y]))
}

/// A wire's points, sheet px, if `path` is a wire (a path figuring a
/// connection, its points in one contour).
fn wire_points(store: &crate::Store, path: NodeId) -> Option<Vec<[f64; 2]>> {
    let doc = store.document();
    doc.live(path).filter(|n| n.ty.name() == "path")?;
    let figured = doc.relations_from(path).find(|r| r.rel == crate::document::RelKind::Figures)?.to;
    doc.live(figured).filter(|n| n.ty.name() == "connection")?;
    let contour = *doc.children(path).first()?;
    let num = |n: NodeId, f: &str| store.resolved(n, format!("transform2d.{f}").parse().expect("declared leaf")).and_then(|v| v.as_f64());
    doc.children(contour).iter().map(|&p| Some([num(p, "x")?, num(p, "y")?])).collect()
}

/// The end a wire's first or last point is bound to.
fn wire_end(doc: &crate::document::Document, path: NodeId, last: bool) -> Option<End> {
    let contour = *doc.children(path).first()?;
    let kids = doc.children(contour);
    let point = if last { *kids.last()? } else { *kids.first()? };
    let t = doc.get::<Transform2d>(point)?;
    let anchor = t.x.expr()?.deps().into_iter().next()?.node;
    let pin = doc.get::<Anchor>(anchor)?.port.constant()?.as_ref()?;
    Some(End { pin, anchor })
}

/// Split `wire` at `at` (on it): a junction placed there, the wire replaced
/// by two through it. Applies to `scratch` and returns the ops and the
/// junction's end.
fn split(scratch: &mut crate::Store, sheet: NodeId, wire: NodeId, at: [f64; 2], ops: &mut Vec<Op>) -> Result<End, String> {
    let apply = |scratch: &mut crate::Store, new: Vec<Op>, ops: &mut Vec<Op>| -> Result<(), String> {
        scratch.apply(crate::TransactionInput { label: "scratch".into(), actor: crate::Actor::System, ts: 0, idempotency_key: None, ops: new.clone(), exact: false }).map_err(|e| e.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "))?;
        ops.extend(new);
        Ok(())
    };
    let points = wire_points(scratch, wire).ok_or("not a wire")?;
    let (a, b) = (wire_end(scratch.document(), wire, false).ok_or("a wire's start is not on a pin")?, wire_end(scratch.document(), wire, true).ok_or("a wire's end is not on a pin")?);
    // The segment `at` lies on.
    let p = Point::new(at[0], at[1]);
    let seg = points.windows(2).position(|w| {
        let (s, e) = (Point::new(w[0][0], w[0][1]), Point::new(w[1][0], w[1][1]));
        (s.distance(p) + p.distance(e) - s.distance(e)).abs() < 1e-6
    }).ok_or("the point is not on the wire")?;
    let junction = find("junction").ok_or("no junction part")?;
    let (place, placed) = place_ops(scratch.document(), junction, sheet, at, 0.0, None)?;
    apply(scratch, place, ops)?;
    let j = pin(scratch.document(), placed.entity, "j").ok_or("the junction has no pin")?;
    let j = End { pin: j, anchor: pin_anchor(scratch.document(), placed.figure, j).ok_or("the junction's pin is not drawn")? };
    apply(scratch, delete_subject_ops(scratch.document(), wire), ops)?;
    let mut first: Vec<[f64; 2]> = points[..=seg].to_vec();
    first.push(at);
    let mut second = vec![at];
    second.extend_from_slice(&points[seg + 1..]);
    for (from, to, pts) in [(a, j, first), (j, b, second)] {
        let pts: Vec<[f64; 2]> = pts.into_iter().fold(Vec::new(), |mut v, q| {
            if v.last().is_none_or(|l: &[f64; 2]| (l[0] - q[0]).abs() > 1e-9 || (l[1] - q[1]).abs() > 1e-9) {
                v.push(q);
            }
            v
        });
        let (wire_ops, _) = wire_ops(scratch.document(), sheet, from, to, &pts)?;
        apply(scratch, wire_ops, ops)?;
    }
    Ok(j)
}

/// The ops for a wire from `from` to `to` through `points` (sheet px, the
/// two ends first and last): a wire ending on a wire splits it at a new
/// junction. One transaction's worth.
pub fn wire_between(store: &crate::Store, sheet: NodeId, from: Terminal, to: Terminal, points: &[[f64; 2]]) -> Result<Vec<Op>, String> {
    let mut scratch = crate::Store::with_document(store.document().clone());
    let mut ops = Vec::new();
    let end = |t: Terminal, scratch: &mut crate::Store, ops: &mut Vec<Op>| match t {
        Terminal::Pin(e) => Ok(e),
        Terminal::Wire { path, at } => split(scratch, sheet, path, at, ops),
    };
    let a = end(from, &mut scratch, &mut ops)?;
    let b = end(to, &mut scratch, &mut ops)?;
    if a.pin == b.pin {
        return Err("a wire joins two different pins".into());
    }
    let (wire, _) = wire_ops(scratch.document(), sheet, a, b, points)?;
    ops.extend(wire);
    Ok(ops)
}

/// The sheet a model names: a composition's id or name.
pub(crate) fn sheet_named(doc: &crate::document::Document, s: &str) -> Result<NodeId, String> {
    if let Some(id) = s.starts_with('#').then(|| s.parse::<NodeId>().ok()).flatten() {
        return doc.live(id).filter(|n| n.ty.name() == "composition").map(|_| id).ok_or_else(|| format!("{s} is not a composition"));
    }
    doc.nodes_of_type(ty("composition"))
        .find(|&c| doc.is_live(c) && doc.get::<Name>(c).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(s))
        .ok_or_else(|| format!("no composition named {s}"))
}

/// A placed part by its designator.
fn entity_named(doc: &crate::document::Document, name: &str) -> Option<NodeId> {
    doc.nodes_of_type(ty("entity")).find(|&e| doc.is_live(e) && doc.get::<Name>(e).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(name))
}

/// The ops for a model's `<circuit>` (schematic.md §5): parts placed and
/// wired on a sheet, as one transaction.
///
/// ```xml
/// <circuit sheet="circuit">
///   <place part="vsource" name="V1" x="120" y="220" V="9 V"/>
///   <place part="resistor" name="R1" x="220" y="160" R="1 kΩ"/>
///   <place part="resistor" name="R2" x="300" y="220" rot="90" R="2 kΩ"/>
///   <place part="ground" x="120" y="300"/>
///   <wire from="V1.p" to="R1.a" via="120,160"/>
///   <wire from="R1.b" to="R2.a"/>
/// </circuit>
/// ```
///
/// A `place` names its part, where its origin goes (on the grid, or it is
/// refused), a turn in degrees (quarter turns), optionally its designator,
/// and any input's value. A `wire` joins two pins, `designator.pin`, of
/// parts placed here or before, through the `via` corners (and one elbow,
/// horizontal first, to reach the end). Returns the ops and a summary.
pub fn circuit_ops(store: &crate::Store, markup: &str) -> Result<(Vec<Op>, String), String> {
    let els = elements(markup)?;
    let [circuit] = &els[..] else { return Err("write one <circuit> at a time".into()) };
    if circuit.name != "circuit" {
        return Err(format!("expected <circuit>, got <{}>", circuit.name));
    }
    let sheet = sheet_named(store.document(), circuit.need("sheet")?)?;
    let mut scratch = crate::Store::with_document(store.document().clone());
    let mut ops: Vec<Op> = Vec::new();
    let apply = |scratch: &mut crate::Store, new: Vec<Op>, ops: &mut Vec<Op>| -> Result<(), String> {
        scratch.apply(crate::TransactionInput { label: "scratch".into(), actor: crate::Actor::Model, ts: 0, idempotency_key: None, ops: new.clone(), exact: false }).map_err(|e| e.iter().map(ToString::to_string).collect::<Vec<_>>().join("; "))?;
        ops.extend(new);
        Ok(())
    };
    let (mut placed, mut wired) = (Vec::new(), 0);
    for (i, el) in circuit.children.iter().enumerate() {
        let at = |m: String| format!("<{}> {}: {m}", el.name, i + 1);
        match el.name.as_str() {
            "place" => {
                let key = el.need("part").map_err(at)?;
                let part = find(key).ok_or_else(|| at(format!("no part {key} ({})", library().iter().map(|p| p.key.as_str()).collect::<Vec<_>>().join(", "))))?;
                let (x, y) = (el.num("x").map_err(at)?, el.num("y").map_err(at)?);
                if x % 10.0 != 0.0 || y % 10.0 != 0.0 {
                    return Err(at(format!("({x}, {y}) is off the 10 px grid; pins land on grid points only when a part's origin does")));
                }
                let deg = el.num("rot").map_err(at)?;
                if deg % 90.0 != 0.0 {
                    return Err(at(format!("rot=\"{deg}\": quarter turns only (0, 90, 180, 270)")));
                }
                let name = el.attr("name");
                if let Some(n) = name
                    && entity_named(scratch.document(), n).is_some()
                {
                    return Err(at(format!("{n} is placed already")));
                }
                let (place, p) = place_ops(scratch.document(), part, sheet, [x, y], deg.to_radians(), name)?;
                apply(&mut scratch, place, &mut ops).map_err(at)?;
                let mut sets = Vec::new();
                for (k, v) in &el.attrs {
                    if matches!(k.as_str(), "part" | "name" | "x" | "y" | "rot") {
                        continue;
                    }
                    if !part.inputs.iter().any(|(n, _)| n == k) {
                        return Err(at(format!("a {} has no input {k} ({})", part.key, part.inputs.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", "))));
                    }
                    // A value, or a binding: `R="{= #s.slider.value * 1 kΩ}"`.
                    let slot = match v.trim().strip_prefix("{=").and_then(|r| r.strip_suffix('}')) {
                        Some(e) => Slot::Bound(Expr::parse(e.trim()).map_err(|e| at(format!("{k}=\"{v}\": {e}")))?),
                        None => Slot::Const(Value::parse_quantity(v).map_err(|e| at(format!("{k}=\"{v}\": {e}")))?),
                    };
                    sets.push(Op::Set { id: p.entity, path: format!("args.{k}").parse().map_err(|e| at(format!("{e:?}")))?, slot });
                }
                if !sets.is_empty() {
                    apply(&mut scratch, sets, &mut ops).map_err(at)?;
                }
                placed.push(scratch.document().get::<Name>(p.entity).and_then(|n| n.text.constant()).and_then(|v| v.as_str()).unwrap_or("?").to_owned());
            }
            "wire" => {
                let pins = pins_on(&scratch, sheet);
                let end = |k: &str| -> Result<(End, [f64; 2]), String> {
                    let s = el.need(k).map_err(at)?;
                    let (d, p) = s.split_once('.').ok_or_else(|| at(format!("{k}=\"{s}\": write designator.pin, as R1.a")))?;
                    let e = entity_named(scratch.document(), d).ok_or_else(|| at(format!("no part {d}")))?;
                    let pin = pin(scratch.document(), e, p).ok_or_else(|| at(format!("{d} has no pin {p}")))?;
                    pins.iter().find(|(x, _)| x.pin == pin).copied().ok_or_else(|| at(format!("{d} is not drawn on this sheet")))
                };
                let ((a, pa), (b, pb)) = (end("from")?, end("to")?);
                let mut pts = vec![pa];
                for c in el.attr("via").unwrap_or("").split_whitespace() {
                    let (x, y) = c.split_once(',').ok_or_else(|| at(format!("via: {c} is not x,y")))?;
                    pts.push([x.trim().parse().map_err(|_| at(format!("via: {c}")))?, y.trim().parse().map_err(|_| at(format!("via: {c}")))?]);
                }
                let last = *pts.last().expect("starts at from");
                let vertical = match el.attr("elbow").unwrap_or("horizontal") {
                    "horizontal" => false,
                    "vertical" => true,
                    e => return Err(at(format!("elbow=\"{e}\": horizontal (the default: across, then up or down) or vertical"))),
                };
                pts.extend(elbow(last, pb, vertical));
                pts.push(pb);
                for w in pts.windows(2) {
                    if (w[0][0] - w[1][0]).abs() > 1e-9 && (w[0][1] - w[1][1]).abs() > 1e-9 {
                        return Err(at(format!("({}, {}) to ({}, {}) is not horizontal or vertical; add a via corner", w[0][0], w[0][1], w[1][0], w[1][1])));
                    }
                }
                let new = wire_between(&scratch, sheet, Terminal::Pin(a), Terminal::Pin(b), &pts).map_err(at)?;
                apply(&mut scratch, new, &mut ops).map_err(at)?;
                wired += 1;
            }
            other => return Err(format!("a <circuit> holds place and wire, not <{other}>")),
        }
    }
    Ok((ops, format!("placed {}; {wired} wire{}", if placed.is_empty() { "nothing".to_owned() } else { placed.join(" ") }, if wired == 1 { "" } else { "s" })))
}

/// A SPICE-like netlist of the parts drawn on `sheet` (schematic.md §5):
/// one line per part, its designator, the net on each pin in its order, and
/// its inputs; the net through any ground is `0`, the rest `N1`, `N2`, …
/// in order of appearance. Grounds and junctions are only nets; a probe is
/// a comment. Each net's solved voltage follows, as comments.
pub fn netlist(store: &crate::Store, sheet: NodeId) -> String {
    use std::collections::BTreeMap;
    let doc = store.document();
    let name_of = |n: NodeId| doc.get::<Name>(n).and_then(|x| x.text.constant()).and_then(|v| v.as_str()).map(str::to_owned).unwrap_or_default();
    // The parts on the sheet, in drawing order, with their part.
    let mut items: Vec<(NodeId, &Part)> = Vec::new();
    for &f in doc.children(sheet) {
        let Some(e) = doc.relations_from(f).find(|r| r.rel == crate::document::RelKind::Figures).map(|r| r.to) else { continue };
        let Some(def) = doc.get::<crate::facet::Instance>(e).and_then(|i| i.def.constant()).and_then(|v| v.as_ref()) else { continue };
        let key = doc.get::<Import>(def).and_then(|i| i.source.constant()).and_then(|v| v.as_str()).and_then(|s| s.strip_prefix("std:")).map(str::to_owned);
        if let Some(part) = key.as_deref().and_then(find) {
            items.push((e, part));
        }
    }
    // Nets over every electrical connection.
    let mut parent: BTreeMap<NodeId, NodeId> = BTreeMap::new();
    fn root(parent: &mut BTreeMap<NodeId, NodeId>, p: NodeId) -> NodeId {
        let mut r = p;
        while let Some(&q) = parent.get(&r).filter(|&&q| q != r) {
            r = q;
        }
        parent.insert(p, r);
        r
    }
    for c in doc.nodes_of_type(ty("connection")).filter(|&c| doc.is_live(c)) {
        if let Some((a, b)) = crate::port::ends(doc, c) {
            let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
            parent.insert(ra, rb);
        }
    }
    let mut names: BTreeMap<NodeId, String> = BTreeMap::new();
    for (e, part) in &items {
        if part.key == "ground" {
            for p in &part.pins {
                if let Some(pin) = pin(doc, *e, &p.name) {
                    let r = root(&mut parent, pin);
                    names.insert(r, "0".into());
                }
            }
        }
    }
    let mut next = 1;
    let mut net = |pin: NodeId, names: &mut BTreeMap<NodeId, String>| {
        let r = root(&mut parent, pin);
        names.entry(r).or_insert_with(|| {
            let n = format!("N{next}");
            next += 1;
            n
        }).clone()
    };
    // The solver's last digits are noise (a ground at 1e-20 V): show them
    // as zero.
    let show = |v: &Value| match v {
        Value::Quantity { si, dim } => crate::units::format(if si.abs() < 1e-15 { 0.0 } else { *si }, *dim),
        other => crate::text::display(other),
    };
    let mut lines = vec![format!("* {} (netlist from Iron)", name_of(sheet))];
    let mut volts: BTreeMap<String, String> = BTreeMap::new();
    for (e, part) in &items {
        let nets: Vec<String> = part.pins.iter().filter_map(|p| pin(doc, *e, &p.name)).map(|p| {
            let n = net(p, &mut names);
            if let Some(v) = store.resolved(p, "port.across".parse().expect("declared leaf")) {
                volts.entry(n.clone()).or_insert_with(|| show(&v));
            }
            n
        }).collect();
        let args = doc.get::<crate::facet::Args>(*e);
        let values: Vec<String> = part.inputs.iter().filter_map(|(n, _)| args.and_then(|a| a.get(n)).and_then(|s| store.resolved(*e, format!("args.{n}").parse().ok()?).or_else(|| s.constant().cloned()))).map(|v| show(&v)).collect();
        match part.key.as_str() {
            "ground" | "junction" => {}
            "probe" => lines.push(format!("* {} probes {}", name_of(*e), nets.join(" "))),
            _ => lines.push(format!("{} {} {}", name_of(*e), nets.join(" "), values.join(" ")).trim_end().to_owned()),
        }
    }
    lines.push(".end".into());
    for (n, v) in volts {
        lines.push(format!("* {n} = {v}"));
    }
    lines.join("\n")
}
