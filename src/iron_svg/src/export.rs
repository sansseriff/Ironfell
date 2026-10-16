//! SVG out: a composition, or some of its nodes, from resolved values.
//!
//! One element per node, in painter order, each tagged with Iron's data
//! (see the crate docs). Strokes are drawn as Iron draws them, with round
//! joins and caps; a shadow is an `feDropShadow` on a wrapper `<g>` in the
//! shape's parent space, which is where Iron offsets it.

use crate::{exact, num, Cut, Cuts};
use iron_document::composition::{self, Dims};
use iron_document::contour;
use iron_document::shape;
use iron_document::{Affine2, ComponentKind, Document, NodeId, Store, Value};
use kurbo::{Affine, BezPath, PathEl, Point, Rect, Shape as _, Vec2};
use std::collections::{BTreeMap, BTreeSet};

/// What to export.
#[derive(Clone, Debug)]
pub enum Scope {
    /// A 2D composition's content, in its own space.
    Composition(NodeId),
    /// These nodes (the selection), each where it sits in its composition.
    Nodes(Vec<NodeId>),
}

/// The SVG text, and what it could not carry.
#[derive(Clone, Debug)]
pub struct Export {
    pub svg: String,
    pub cuts: Vec<Cut>,
}

/// Deepest nesting of embeds written, as drawn (`document_bridge::nest`).
const MAX_DEPTH: u32 = 8;

/// Export `scope` from `store` as SVG text.
pub fn export(store: &Store, scope: &Scope) -> Result<Export, String> {
    let doc = store.document();
    let mut w = Writer { store, doc, body: String::new(), defs: Vec::new(), ids: BTreeSet::new(), unnamed: BTreeMap::new(), bounds: None, cuts: Cuts::default(), filters: 0, clips: 0 };
    match scope {
        Scope::Composition(c) => {
            if doc.live(*c).is_none_or(|n| n.ty.name() != "composition") {
                return Err(format!("{c} is not a composition"));
            }
            if composition::dims(doc, *c) == Some(Dims::Three) {
                return Err(format!("{c} is a 3D composition; only 2D compositions export as SVG"));
            }
            for &child in doc.children(*c) {
                w.node(child, Affine::IDENTITY, 1, 0);
            }
        }
        Scope::Nodes(ids) => {
            let order = preorder(doc);
            let mut roots: Vec<NodeId> = ids.iter().copied().filter(|&id| doc.is_live(id)).collect();
            roots.sort_by_key(|r| order.get(r).copied().unwrap_or(usize::MAX));
            roots.dedup();
            let all = roots.clone();
            roots.retain(|&r| !all.iter().any(|&o| o != r && doc.is_ancestor(o, r)));
            if roots.is_empty() {
                return Err("nothing to export".into());
            }
            for r in roots {
                if composition::composition_of(doc, r).is_some_and(|c| composition::dims(doc, c) == Some(Dims::Three)) {
                    w.cuts.add(format!("{r} is in a 3D composition and is left out"));
                    continue;
                }
                let parent = doc.live(r).and_then(|n| n.parent).map_or(Affine2::IDENTITY, |p| store.world_affine(p));
                if parent.max_diff(&Affine2::IDENTITY) < 1e-12 {
                    w.node(r, Affine::IDENTITY, 1, 0);
                } else {
                    // Where its container puts it; import folds this
                    // anonymous group into the node's own transform.
                    let m = parent.0.map(num).join(" ");
                    w.line(1, &format!("<g transform=\"matrix({m})\">"));
                    w.node(r, Affine::new(parent.0), 2, 0);
                    w.line(1, "</g>");
                }
            }
        }
    }
    Ok(w.finish())
}

/// Every live node in preorder: painter order across the document.
fn preorder(doc: &Document) -> BTreeMap<NodeId, usize> {
    let mut out = BTreeMap::new();
    let mut stack: Vec<NodeId> = doc.roots().iter().rev().copied().collect();
    while let Some(id) = stack.pop() {
        out.insert(id, out.len());
        stack.extend(doc.children(id).iter().rev().copied());
    }
    out
}

struct Writer<'a> {
    store: &'a Store,
    doc: &'a Document,
    body: String,
    /// Filters and clip paths, each a finished element.
    defs: Vec<String>,
    /// Element ids written so far, to keep them unique.
    ids: BTreeSet<String>,
    /// The next number for an unnamed node of each type.
    unnamed: BTreeMap<&'static str, usize>,
    /// What is drawn, in export space: outlines, strokes and shadows.
    bounds: Option<Rect>,
    cuts: Cuts,
    filters: usize,
    clips: usize,
}

/// Attribute list of one element, in writing order.
type Attrs = Vec<(&'static str, String)>;

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
    out
}

fn element(tag: &str, attrs: &Attrs, close: bool) -> String {
    let mut s = format!("<{tag}");
    for (k, v) in attrs {
        s.push_str(&format!(" {k}=\"{}\"", escape(v)));
    }
    s.push_str(if close { "/>" } else { ">" });
    s
}

/// `#rrggbb` for an sRGB colour, and its alpha.
pub(crate) fn hex(c: [f32; 4]) -> (String, f64) {
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    (format!("#{:02x}{:02x}{:02x}", b(c[0]), b(c[1]), b(c[2])), c[3] as f64)
}

/// Path data: absolute M, L, C and Z. A line back to the start right
/// before a close is left to the Z.
fn path_d(path: &BezPath) -> String {
    let p = |pt: Point| format!("{} {}", num(pt.x), num(pt.y));
    let els = path.elements();
    let mut out: Vec<String> = Vec::new();
    let mut start = Point::ORIGIN;
    for (i, el) in els.iter().enumerate() {
        match *el {
            PathEl::MoveTo(a) => {
                start = a;
                out.push(format!("M{}", p(a)));
            }
            PathEl::LineTo(a) => {
                if a == start && matches!(els.get(i + 1), Some(PathEl::ClosePath)) {
                    continue;
                }
                out.push(format!("L{}", p(a)));
            }
            PathEl::QuadTo(a, b) => out.push(format!("Q{} {}", p(a), p(b))),
            PathEl::CurveTo(a, b, c) => out.push(format!("C{} {} {}", p(a), p(b), p(c))),
            PathEl::ClosePath => out.push("Z".into()),
        }
    }
    out.join(" ")
}

fn union(a: Option<Rect>, b: Rect) -> Option<Rect> {
    Some(a.map_or(b, |a| a.union(b)))
}

impl Writer<'_> {
    fn line(&mut self, depth: usize, text: &str) {
        for _ in 0..depth {
            self.body.push_str("  ");
        }
        self.body.push_str(text);
        self.body.push('\n');
    }

    fn get(&self, id: NodeId, leaf: &str) -> Option<Value> {
        self.store.resolved(id, leaf.parse().expect("declared leaf"))
    }

    fn f(&self, id: NodeId, leaf: &str, default: f64) -> f64 {
        self.get(id, leaf).and_then(|v| v.as_f64()).unwrap_or(default)
    }

    fn color(&self, id: NodeId, leaf: &str) -> Option<[f32; 4]> {
        self.get(id, leaf).and_then(|v| v.as_color())
    }

    fn name(&self, id: NodeId) -> Option<String> {
        self.get(id, "name.text").and_then(|v| v.as_str().map(str::to_owned)).filter(|s| !s.is_empty())
    }

    /// A unique element id: the name made XML-safe, else the type and a
    /// number. Never the node id, so a re-import exports the same ids.
    fn ident(&mut self, ty: &'static str, name: Option<&str>) -> String {
        let id = match name {
            Some(name) => {
                let mut base: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '-' }).collect();
                if !base.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
                    base.insert_str(0, "n-");
                }
                let mut id = base.clone();
                let mut k = 2;
                while self.ids.contains(&id) {
                    id = format!("{base}-{k}");
                    k += 1;
                }
                id
            }
            None => loop {
                let k = self.unnamed.entry(ty).or_insert(0);
                *k += 1;
                let id = format!("{ty}-{k}");
                if !self.ids.contains(&id) {
                    break id;
                }
            },
        };
        self.ids.insert(id.clone());
        id
    }

    /// `id`, `data-iron-type`, `data-iron-name` and `transform`.
    fn head(&mut self, id: NodeId, ty: &'static str) -> Attrs {
        let name = self.name(id);
        let ident = self.ident(ty, name.as_deref());
        let mut a: Attrs = vec![("id", ident), ("data-iron-type", ty.to_owned())];
        if let Some(n) = name {
            a.push(("data-iron-name", n));
        }
        if let Some(t) = self.transform_attr(id) {
            a.push(("transform", t));
        }
        // Exact resolved optional paint survives an Iron SVG round trip;
        // ordinary SVG attributes beside it remain useful to other editors.
        let kinds = [ComponentKind::Appearance, ComponentKind::Gradient, ComponentKind::GroupClip];
        if kinds.iter().any(|k| self.doc.component(id, *k).is_some()) {
            let components: Vec<_> = kinds.into_iter().chain([ComponentKind::Fill]).filter_map(|kind| {
                let mut c = self.doc.component(id, kind)?.clone();
                for field in c.field_names() {
                    if let Some(v) = self.get(id, &format!("{}.{field}", kind.name())) { let _ = c.set(field, iron_document::Slot::Const(v)); }
                }
                Some(c)
            }).collect();
            a.push(("data-iron-appearance", serde_json::to_string(&components).expect("components serialize")));
        }
        let opacity = self.f(id, "appearance.opacity", 1.0).clamp(0.0, 1.0);
        if opacity != 1.0 { a.push(("opacity", num(opacity))); }
        a
    }

    fn trs(&self, id: NodeId) -> Option<[f64; 5]> {
        self.doc.component(id, ComponentKind::Transform2d)?;
        Some([self.f(id, "transform2d.x", 0.0), self.f(id, "transform2d.y", 0.0), self.f(id, "transform2d.rot", 0.0), self.f(id, "transform2d.sx", 1.0), self.f(id, "transform2d.sy", 1.0)])
    }

    /// `translate(x y) rotate(deg) scale(sx sy)`, leaving out what is the
    /// identity; `None` for no transform at all.
    fn transform_attr(&self, id: NodeId) -> Option<String> {
        let [x, y, rot, sx, sy] = self.trs(id)?;
        let mut parts = Vec::new();
        let (x, y) = (num(x), num(y));
        if x != "0" || y != "0" {
            parts.push(format!("translate({x} {y})"));
        }
        let deg = num(rot.to_degrees());
        if deg != "0" {
            parts.push(format!("rotate({deg})"));
        }
        let (sx, sy) = (num(sx), num(sy));
        if sx != "1" || sy != "1" {
            parts.push(if sx == sy { format!("scale({sx})") } else { format!("scale({sx} {sy})") });
        }
        (!parts.is_empty()).then(|| parts.join(" "))
    }

    fn local(&self, id: NodeId) -> Affine {
        self.trs(id).map_or(Affine::IDENTITY, |[x, y, rot, sx, sy]| Affine::new(Affine2::from_trs(x, y, rot, sx, sy).0))
    }

    /// Note what a node loses besides its geometry: bindings flattened,
    /// clips, figures of entities, and links.
    fn note_losses(&mut self, id: NodeId) {
        let bound = self.doc.components_of(id).map(|c| c.field_names().iter().filter(|f| c.get(f).is_some_and(|s| s.expr().is_some())).count()).sum::<usize>();
        for _ in 0..bound {
            self.cuts.add("a bound value is exported at its current value");
        }
        if self.doc.relations_to(id).any(|r| r.rel == iron_document::RelKind::Animates) {
            self.cuts.add(format!("{id} is animated; the animation is not exported"));
        }
        if composition::figured_by(self.doc, id).is_some() {
            self.cuts.add("a figure of a model entity is exported as a plain shape");
        }
    }

    fn node(&mut self, id: NodeId, parent: Affine, depth: usize, embeds: u32) {
        let Some(n) = self.doc.live(id) else { return };
        let ty = n.ty.name();
        self.note_losses(id);
        match ty {
            "group" => {
                let mut attrs = self.head(id, "group");
                if let Some(frame) = iron_document::appearance::ClipFrame::read(self.store, id) {
                    self.clips += 1;
                    let cid = self.ident_raw(format!("clip-{}", self.clips));
                    let shape = if frame.ellipse {
                        element("ellipse", &vec![("cx", num(frame.w/2.0)), ("cy", num(frame.h/2.0)), ("rx", num(frame.w/2.0)), ("ry", num(frame.h/2.0))], true)
                    } else { element("rect", &vec![("width", num(frame.w)), ("height", num(frame.h)), ("rx", num(frame.radius))], true) };
                    self.defs.push(format!("<clipPath id=\"{cid}\">{shape}</clipPath>"));
                    attrs.push(("clip-path", format!("url(#{cid})")));
                }
                let here = parent * self.local(id);
                let kids = self.doc.children(id).to_vec();
                if kids.is_empty() {
                    self.line(depth, &element("g", &attrs, true));
                } else {
                    self.line(depth, &element("g", &attrs, false));
                    for c in kids {
                        self.node(c, here, depth + 1, embeds);
                    }
                    self.line(depth, "</g>");
                }
            }
            "rect" => self.shape(id, "rect", parent, depth),
            "ellipse" => self.shape(id, "ellipse", parent, depth),
            "path" => self.shape(id, "path", parent, depth),
            "slider" => self.slider(id, parent, depth),
            "embed" => self.embed(id, parent, depth, embeds),
            "constraint" => self.cuts.add("constraints are not exported"),
            other => self.cuts.add(format!("{other} {id} is not exported; only 2D shapes are")),
        }
    }

    fn shape(&mut self, id: NodeId, ty: &'static str, parent: Affine, depth: usize) {
        // Geometry first: a path with nothing to draw takes no id.
        let mut attrs: Attrs = Vec::new();
        let (mut open_filled, mut fill_outline) = (false, None::<BezPath>);
        let (tag, outline) = match ty {
            "rect" => {
                let (w, h) = (self.f(id, "size.w", 100.0), self.f(id, "size.h", 100.0));
                let radii = ["tl", "tr", "br", "bl"].map(|c| self.f(id, &format!("corners.{c}"), 0.0));
                let drawn = shape::clamp_radii(w, h, radii);
                let outline = shape::to_bez(&[shape::rect_outline(w, h, radii)]);
                if w >= 0.0 && h >= 0.0 && drawn.iter().all(|&r| num(r) == num(drawn[0])) {
                    attrs.push(("width", num(w)));
                    attrs.push(("height", num(h)));
                    if num(drawn[0]) != "0" {
                        attrs.push(("rx", num(drawn[0])));
                    }
                    ("rect", outline)
                } else {
                    attrs.push(("d", path_d(&outline)));
                    attrs.push(("data-iron-w", exact(w)));
                    attrs.push(("data-iron-h", exact(h)));
                    attrs.push(("data-iron-corners", radii.map(exact).join(" ")));
                    ("path", outline)
                }
            }
            "ellipse" => {
                let (rx, ry) = (self.f(id, "radius.rx", 50.0), self.f(id, "radius.ry", 50.0));
                let arc = self.doc.component(id, ComponentKind::Arc).map(|_| [self.f(id, "arc.start", 0.0), self.f(id, "arc.sweep", std::f64::consts::TAU), self.f(id, "arc.inner", 0.0)]);
                let outline = shape::to_bez(&shape::ellipse_outline(rx, ry, arc));
                match arc {
                    None if rx >= 0.0 && ry >= 0.0 => {
                        attrs.push(("rx", num(rx)));
                        attrs.push(("ry", num(ry)));
                        ("ellipse", outline)
                    }
                    _ => {
                        attrs.push(("d", path_d(&outline)));
                        attrs.push(("data-iron-rx", exact(rx)));
                        attrs.push(("data-iron-ry", exact(ry)));
                        if let Some(a) = arc {
                            attrs.push(("data-iron-arc", a.map(exact).join(" ")));
                        }
                        ("path", outline)
                    }
                }
            }
            _ => {
                // Each contour a subpath; the fill rule decides the holes.
                let contours = contour::contours(self.store, id);
                if contours.iter().all(|c| c.points.len() < 2) {
                    self.cuts.add(format!("path {id} has fewer than two points and is left out"));
                    return;
                }
                if contour::fill_rule(self.store, id) == contour::FillRule::EvenOdd {
                    attrs.push(("fill-rule", "evenodd".into()));
                }
                let outline = contour::outline(&contours);
                attrs.insert(0, ("d", path_d(&outline)));
                open_filled = contours.iter().any(|c| c.points.len() >= 2 && !contour::draws_closed(c)) && contours.iter().any(contour::draws_closed);
                fill_outline = Some(contour::fill_outline(&contours));
                ("path", outline)
            }
        };
        let mut head = self.head(id, ty);
        head.append(&mut attrs);
        let mut attrs = head;
        // Paint, as `materialize` reads it: no fill component, no fill; an
        // open contour has no inside to fill, though SVG fills it as if
        // closed.
        let open = fill_outline.as_ref().is_some_and(|f| f.elements().is_empty());
        let fill = self.color(id, "fill.color").filter(|_| !open);
        if fill.is_some() && open_filled {
            self.cuts.add(format!("path {id} mixes open and closed outlines; SVG fills the open ones too"));
        }
        if !open && let Some(start) = self.color(id, "gradient.start") {
            let end = self.color(id, "gradient.end").unwrap_or(start);
            let bounds = outline.bounding_box();
            let x0 = bounds.x0 + bounds.width() * self.f(id, "gradient.x0", 0.0);
            let y0 = bounds.y0 + bounds.height() * self.f(id, "gradient.y0", 0.0);
            let x1 = bounds.x0 + bounds.width() * self.f(id, "gradient.x1", 1.0);
            let y1 = bounds.y0 + bounds.height() * self.f(id, "gradient.y1", 1.0);
            let radial = self.get(id, "gradient.kind").and_then(|v| v.as_str().map(str::to_owned)).as_deref() == Some("radial");
            let gid = self.ident_raw(format!("gradient-{}", self.defs.len()+1));
            let mut ga = vec![("id", gid.clone()), ("gradientUnits", "userSpaceOnUse".into())];
            let tag = if radial {
                ga.extend([("cx", num(x0)), ("cy", num(y0)), ("r", num((x1-x0).hypot(y1-y0).max(0.001)))]);
                "radialGradient"
            } else {
                ga.extend([("x1", num(x0)), ("y1", num(y0)), ("x2", num(x1)), ("y2", num(y1))]);
                "linearGradient"
            };
            let stops: Vec<_> = [start, end].into_iter().enumerate().map(|(i,c)| {
                let (h,a) = hex(c);
                element("stop", &vec![("offset", i.to_string()), ("stop-color", h), ("stop-opacity", num(a))], true)
            }).collect();
            self.defs.push(format!("{}{}{} </{tag}>", element(tag, &ga, false), stops[0], stops[1]));
            attrs.push(("fill", format!("url(#{gid})")));
            let opacity = self.f(id, "fill.opacity", 1.0).clamp(0.0, 1.0);
            if opacity != 1.0 { attrs.push(("fill-opacity", num(opacity))); }
        } else {
        match fill {
            Some(c) => {
                let (h, a) = hex(c);
                attrs.push(("fill", h));
                let op = a * self.f(id, "fill.opacity", 1.0);
                if num(op) != "1" {
                    attrs.push(("fill-opacity", num(op)));
                }
            }
            None => attrs.push(("fill", "none".into())),
        }
        }
        let stroke = self.color(id, "stroke.color").map(|c| (c, self.f(id, "stroke.width", 1.0)));
        if let Some((c, width)) = stroke {
            let (h, a) = hex(c);
            attrs.push(("stroke", h));
            attrs.push(("stroke-width", num(width)));
            if num(a) != "1" {
                attrs.push(("stroke-opacity", num(a)));
            }
            let style = |leaf: &str, default: &str| self.get(id, leaf).and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_else(|| default.to_owned());
            let join = style("stroke.join", "round");
            attrs.push(("stroke-linejoin", join.clone()));
            attrs.push(("stroke-linecap", style("stroke.cap", "round")));
            let miter = self.f(id, "stroke.miter", 4.0);
            if join == "miter" && num(miter) != "4" {
                attrs.push(("stroke-miterlimit", num(miter)));
            }
            let dash = crate::dashes(&style("stroke.dash", ""));
            if !dash.is_empty() {
                attrs.push(("stroke-dasharray", dash.iter().map(|d| num(*d)).collect::<Vec<_>>().join(" ")));
                let offset = self.f(id, "stroke.dash_offset", 0.0);
                if num(offset) != "0" {
                    attrs.push(("stroke-dashoffset", num(offset)));
                }
            }
        }

        let local = self.local(id);
        let pad = stroke.map_or(0.0, |(_, w)| w.abs() * 0.5);
        let own = outline.bounding_box().inflate(pad, pad);
        self.bounds = union(self.bounds, (parent * local).transform_rect_bbox(own));

        let shadow = self.color(id, "shadow.color").filter(|c| c[3] > 0.0);
        match shadow {
            None => self.line(depth, &element(tag, &attrs, true)),
            Some(c) => {
                let (dx, dy, blur) = (self.f(id, "shadow.dx", 0.0), self.f(id, "shadow.dy", 4.0), self.f(id, "shadow.blur", 4.0).max(0.0));
                let (h, a) = hex(c);
                attrs.push(("data-iron-shadow", format!("{h} {} {} {} {}", num(a), num(dx), num(dy), num(blur))));
                // In the parent's space, as Iron offsets it (`render.rs`).
                let in_parent = local.transform_rect_bbox(own);
                let sigma = blur * 0.5;
                let region = in_parent.union((in_parent + Vec2::new(dx, dy)).inflate(3.0 * sigma, 3.0 * sigma));
                self.bounds = union(self.bounds, parent.transform_rect_bbox(region));
                self.filters += 1;
                let fid = self.ident_raw(format!("shadow-{}", self.filters));
                let fa: Attrs = vec![
                    ("id", fid.clone()),
                    ("filterUnits", "userSpaceOnUse".into()),
                    ("x", num(region.x0)),
                    ("y", num(region.y0)),
                    ("width", num(region.width())),
                    ("height", num(region.height())),
                    ("color-interpolation-filters", "sRGB".into()),
                ];
                let drop: Attrs = vec![("dx", num(dx)), ("dy", num(dy)), ("stdDeviation", num(sigma)), ("flood-color", h), ("flood-opacity", num(a))];
                self.defs.push(format!("{}\n      {}\n    </filter>", element("filter", &fa, false), element("feDropShadow", &drop, true)));
                self.line(depth, &format!("<g filter=\"url(#{fid})\">"));
                self.line(depth + 1, &element(tag, &attrs, true));
                self.line(depth, "</g>");
            }
        }
    }

    /// An id for a definition, unique among the element ids.
    fn ident_raw(&mut self, base: String) -> String {
        let mut id = base.clone();
        let mut k = 2;
        while self.ids.contains(&id) {
            id = format!("{base}-{k}");
            k += 1;
        }
        self.ids.insert(id.clone());
        id
    }

    /// A slider as it looks (`render.rs`): track, filled part, knob.
    fn slider(&mut self, id: NodeId, parent: Affine, depth: usize) {
        let attrs = self.head(id, "slider");
        let label = self.name(id).unwrap_or_else(|| id.to_string());
        self.cuts.add(format!("slider {label} is exported as a picture of itself; it imports as shapes"));
        let (w, h) = (self.f(id, "size.w", 200.0), self.f(id, "size.h", 24.0));
        let (min, max) = (self.f(id, "slider.min", 0.0), self.f(id, "slider.max", 1.0));
        let value = self.f(id, "slider.value", 0.0);
        let t = if max > min { ((value - min) / (max - min)).clamp(0.0, 1.0) } else { 0.0 };
        let mid = h * 0.5;
        let r = mid.min(12.0);
        self.line(depth, &element("g", &attrs, false));
        let bar = |width: f64, fill: &str| -> Attrs { vec![("y", num(mid - 3.0)), ("width", num(width)), ("height", "6".into()), ("rx", "3".into()), ("fill", fill.into())] };
        self.line(depth + 1, &element("rect", &bar(w, &hex([0.75, 0.76, 0.8, 1.0]).0), true));
        self.line(depth + 1, &element("rect", &bar((w * t).max(6.0), &hex([0.2, 0.5, 0.9, 1.0]).0), true));
        let knob: Attrs = vec![("cx", num(w * t)), ("cy", num(mid)), ("r", num(r)), ("fill", "#ffffff".into()), ("stroke", hex([0.3, 0.3, 0.35, 1.0]).0), ("stroke-width", "1.5".into())];
        self.line(depth + 1, &element("circle", &knob, true));
        self.line(depth, "</g>");
        let own = Rect::new(0.0, mid - 3.0, w, mid + 3.0).union(Rect::new(w * t - r, mid - r, w * t + r, mid + r)).inflate(0.75, 0.75);
        self.bounds = union(self.bounds, (parent * self.local(id)).transform_rect_bbox(own));
    }

    /// Conservative painted bounds of a composition's shapes, in its space: what
    /// an embed without a frame shows (`nest::content_bounds`).
    fn content_bounds(&self, composition: NodeId) -> Option<Rect> {
        let mut out = None;
        for n in self.doc.descendants(composition) {
            let Some(node) = self.doc.live(n) else { continue };
            let bb = match node.ty.name() {
                "rect" | "ellipse" => shape::outline(self.store, n).map(|c| shape::to_bez(&c).bounding_box()),
                "slider" => Some(Rect::new(0.0, 0.0, self.f(n, "size.w", 200.0), self.f(n, "size.h", 24.0))),
                "path" => {
                    let pts: Vec<Point> = contour::contours(self.store, n).into_iter().flat_map(|c| c.points).map(|p| p.1.at).collect();
                    pts.iter().map(|&p| Rect::from_points(p, p)).reduce(|a, b| a.union(b))
                }
                _ => None,
            };
            if let Some(bb) = bb {
                // Match automatic embed frames in the live bridge: strokes,
                // shadows and slider knobs must not be clipped to geometry.
                let mut pad = self.color(n, "stroke.color").map_or(0.0, |_| {
                    self.f(n, "stroke.width", 1.0) * 0.5 * self.f(n, "stroke.miter", 4.0).max(4.0)
                });
                if self.color(n, "shadow.color").is_some() {
                    pad += self.f(n, "shadow.dx", 0.0).abs().max(self.f(n, "shadow.dy", 4.0).abs())
                        + 1.5 * self.f(n, "shadow.blur", 4.0).max(0.0);
                }
                if node.ty.name() == "slider" {
                    pad = pad.max((self.f(n, "size.h", 24.0) * 0.5).min(12.0) + 3.0);
                }
                out = union(out, Affine::new(self.store.world_affine(n).0).transform_rect_bbox(bb.inflate(pad, pad)));
            }
        }
        out
    }

    /// A 2D embed as a group of what it shows, clipped to its frame.
    fn embed(&mut self, id: NodeId, parent: Affine, depth: usize, embeds: u32) {
        let label = self.name(id).unwrap_or_else(|| id.to_string());
        let Some(source) = composition::embedded(self.doc, id) else {
            self.cuts.add(format!("embed {label} shows no composition and is left out"));
            return;
        };
        if composition::dims(self.doc, source) == Some(Dims::Three) {
            self.cuts.add(format!("embed {label} shows a 3D composition and is left out"));
            return;
        }
        if embeds >= MAX_DEPTH {
            self.cuts.add(format!("embed {label} is nested too deep and is left out"));
            return;
        }
        let region = match self.doc.component(id, ComponentKind::Frame) {
            Some(_) => Some(Rect::new(self.f(id, "frame.x0", 0.0), self.f(id, "frame.y0", 0.0), self.f(id, "frame.x1", 400.0), self.f(id, "frame.y1", 300.0))),
            None => self.content_bounds(source),
        };
        let Some(region) = region else {
            return; // An empty composition shows nothing.
        };
        let source_name = self.name(source).unwrap_or_else(|| source.to_string());
        self.cuts.add(format!("embed {label} is exported as a copy of {source_name}; it imports as shapes, not a live embed"));
        let mut attrs = self.head(id, "embed");
        self.clips += 1;
        let cid = self.ident_raw(format!("clip-{}", self.clips));
        let r: Attrs = vec![("x", num(region.x0)), ("y", num(region.y0)), ("width", num(region.width())), ("height", num(region.height()))];
        self.defs.push(format!("<clipPath id=\"{cid}\">\n      {}\n    </clipPath>", element("rect", &r, true)));
        attrs.push(("clip-path", format!("url(#{cid})")));
        let here = parent * self.local(id);
        // What shows is what is inside the frame.
        let outer = self.bounds.take();
        self.line(depth, &element("g", &attrs, false));
        for c in self.doc.children(source).to_vec() {
            self.node(c, here, depth + 1, embeds + 1);
        }
        self.line(depth, "</g>");
        let inner = self.bounds.take().and_then(|b| {
            let shown = b.intersect(here.transform_rect_bbox(region));
            (shown.width() >= 0.0 && shown.height() >= 0.0).then_some(shown)
        });
        self.bounds = match (outer, inner) {
            (Some(a), Some(b)) => Some(a.union(b)),
            (a, b) => a.or(b),
        };
    }

    fn finish(self) -> Export {
        let b = self.bounds.unwrap_or(Rect::new(0.0, 0.0, 1.0, 1.0));
        let (x0, y0) = (b.x0.floor(), b.y0.floor());
        let (w, h) = ((b.x1.ceil() - x0).max(1.0), (b.y1.ceil() - y0).max(1.0));
        let mut svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"{} {} {} {}\" data-iron=\"1\">\n",
            num(w),
            num(h),
            num(x0),
            num(y0),
            num(w),
            num(h)
        );
        if !self.defs.is_empty() {
            svg.push_str("  <defs>\n");
            for d in &self.defs {
                svg.push_str("    ");
                svg.push_str(d);
                svg.push('\n');
            }
            svg.push_str("  </defs>\n");
        }
        svg.push_str(&self.body);
        svg.push_str("</svg>\n");
        Export { svg, cuts: self.cuts.into_lines() }
    }
}
