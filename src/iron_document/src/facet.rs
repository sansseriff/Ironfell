//! Typed authored facets and leaf paths.
//!
//! Facets are Rust structs, not JSON bags, because the reconciler, the
//! floor and the layout solver all need typed access. Every field is a
//! [`Slot`], so any field can later become bound or animated without changing
//! its address. A leaf is addressed as `facet.field`, e.g. `transform2d.x`
//! (plans/specs/document-spine.md §5.3).
//!
//! The file format stays generic JSON regardless: each facet serialises as
//! an object of its fields.
//!
//! One facet, `args`, has fields known only at run time: the `in` ports a
//! definition declares (component-model.md §4). Its field names
//! are interned (`sym.rs`) so a `LeafPath` into it is as `Copy` as any other.

use crate::sym::intern;
use crate::value::{Slot, Value, ValueKind};
use std::collections::BTreeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

/// Implemented by every facet struct. `KIND` and `FIELDS` are what the
/// registry, the validator and the view renderer read.
pub trait LeafStruct: Sized + Default + Clone {
    const KIND: FacetKind;
    const FIELDS: &'static [(&'static str, ValueKind)];
    fn get(&self, field: &str) -> Option<&Slot>;
    fn get_mut(&mut self, field: &str) -> Option<&mut Slot>;
    fn wrap(self) -> Facet;
    fn unwrap_ref(c: &Facet) -> Option<&Self>;
}

macro_rules! facets {
    ( $( $(#[$m:meta])* $variant:ident / $json:literal => $name:ident {
          $( $field:ident : $vk:ident = $default:expr ),* $(,)?
      } )*
      @dynamic { $( $(#[$dm:meta])* $dvariant:ident / $djson:literal => $dname:ident )* }
    ) => {
        $(
            $(#[$m])*
            #[derive(Clone, Debug, PartialEq)]
            pub struct $name { $( pub $field: Slot, )* }

            impl Default for $name {
                fn default() -> Self {
                    Self { $( $field: Slot::Const($default), )* }
                }
            }

            impl LeafStruct for $name {
                const KIND: FacetKind = FacetKind::$variant;
                const FIELDS: &'static [(&'static str, ValueKind)] =
                    &[ $( (stringify!($field), ValueKind::$vk), )* ];
                fn get(&self, field: &str) -> Option<&Slot> {
                    match field { $( stringify!($field) => Some(&self.$field), )* _ => None }
                }
                fn get_mut(&mut self, field: &str) -> Option<&mut Slot> {
                    match field { $( stringify!($field) => Some(&mut self.$field), )* _ => None }
                }
                fn wrap(self) -> Facet { Facet::$variant(self) }
                fn unwrap_ref(c: &Facet) -> Option<&Self> {
                    match c { Facet::$variant(v) => Some(v), _ => None }
                }
            }
        )*

        /// The kind of a facet, i.e. which struct it is.
        #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum FacetKind { $( $variant, )* $( $dvariant, )* }

        impl FacetKind {
            pub const ALL: &'static [FacetKind] = &[ $( FacetKind::$variant, )* $( FacetKind::$dvariant, )* ];

            pub fn name(self) -> &'static str {
                match self { $( FacetKind::$variant => $json, )* $( FacetKind::$dvariant => $djson, )* }
            }

            pub fn from_name(s: &str) -> Option<FacetKind> {
                match s { $( $json => Some(FacetKind::$variant), )* $( $djson => Some(FacetKind::$dvariant), )* _ => None }
            }

            /// The compile-time fields. Empty for a dynamic facet, whose
            /// fields are per instance: see [`Facet::field_names`].
            pub fn fields(self) -> &'static [(&'static str, ValueKind)] {
                match self { $( FacetKind::$variant => <$name as LeafStruct>::FIELDS, )* $( FacetKind::$dvariant => &[], )* }
            }

            /// Whether this facet's fields come from the document rather
            /// than from this table.
            pub fn is_dynamic(self) -> bool {
                match self { $( FacetKind::$variant => false, )* $( FacetKind::$dvariant => true, )* }
            }

            pub fn default_facet(self) -> Facet {
                match self {
                    $( FacetKind::$variant => Facet::$variant($name::default()), )*
                    $( FacetKind::$dvariant => Facet::$dvariant($dname::default()), )*
                }
            }
        }

        /// One authored facet of a node.
        #[derive(Clone, Debug, PartialEq)]
        pub enum Facet { $( $variant($name), )* $( $dvariant($dname), )* }

        impl Facet {
            pub fn kind(&self) -> FacetKind {
                match self { $( Facet::$variant(_) => FacetKind::$variant, )* $( Facet::$dvariant(_) => FacetKind::$dvariant, )* }
            }

            pub fn get(&self, field: &str) -> Option<&Slot> {
                match self { $( Facet::$variant(v) => v.get(field), )* $( Facet::$dvariant(v) => v.get(field), )* }
            }

            fn get_mut(&mut self, field: &str) -> Option<&mut Slot> {
                match self { $( Facet::$variant(v) => v.get_mut(field), )* $( Facet::$dvariant(v) => v.get_mut(field), )* }
            }

            /// This facet's fields, in declaration (or, for a dynamic
            /// facet, name) order.
            pub fn field_names(&self) -> Vec<&'static str> {
                match self {
                    $( Facet::$variant(_) => <$name as LeafStruct>::FIELDS.iter().map(|(n, _)| *n).collect(), )*
                    $( Facet::$dvariant(v) => v.field_names(), )*
                }
            }
        }
    };
}

facets! {
    /// A human-readable name. Rendered as the `name` attribute in views.
    Name / "name" => Name { text: Str = Value::Str(String::new()) }

    /// Which renderer materialises this node: `canvas` or `dom`
    /// (plans/specs/document-spine.md §4). Absent means the type's default.
    Host / "host" => Host { mode: Enum = Value::Enum("canvas".to_owned()) }

    /// Document 2D space: y-down, origin top-left, matching SVG and the
    /// DisplayList seam. Scalars, not a vector, because they are the leaves a
    /// binding or a slider most often addresses individually.
    Transform2d / "transform2d" => Transform2d {
        x: Number = Value::Number(0.0),
        y: Number = Value::Number(0.0),
        rot: Number = Value::Number(0.0),
        sx: Number = Value::Number(1.0),
        sy: Number = Value::Number(1.0),
    }

    /// 3D placement as vectors: a viewport drag writes one leaf.
    Transform3d / "transform3d" => Transform3d {
        pos: Vec3 = Value::Vec3([0.0, 0.0, 0.0]),
        rot: Vec3 = Value::Vec3([0.0, 0.0, 0.0]),
        scale: Vec3 = Value::Vec3([1.0, 1.0, 1.0]),
    }

    Size / "size" => Size {
        w: Number = Value::Number(100.0),
        h: Number = Value::Number(100.0),
    }

    /// An ellipse's radii along its own x and y; equal for a circle. The
    /// ellipse is centred on its origin (SVG's `rx`, `ry`).
    Radius / "radius" => Radius {
        rx: Number = Value::Number(50.0),
        ry: Number = Value::Number(50.0),
    }

    /// A rect's corner radii, clockwise from the top-left. Each is clamped
    /// to half the shorter side when drawn, as in Figma.
    Corners / "corners" => Corners {
        tl: Number = Value::Number(0.0),
        tr: Number = Value::Number(0.0),
        br: Number = Value::Number(0.0),
        bl: Number = Value::Number(0.0),
    }

    /// Part of an ellipse (Figma's arc): from `start` through `sweep`
    /// radians, clockwise on screen from the +x axis, as angles of the
    /// ellipse's parameter; `inner` is the hole's radius as a fraction of
    /// the outer (0 a pie, above 0 a ring or a ring segment).
    Arc / "arc" => Arc {
        start: Number = Value::Number(0.0),
        sweep: Number = Value::Number(std::f64::consts::TAU),
        inner: Number = Value::Number(0.0),
    }

    /// A drop shadow under the shape (Figma's defaults): offset `dx`, `dy`
    /// in the parent's space, so it does not turn with the shape, and a
    /// `blur` radius (twice the Gaussian's standard deviation, as in CSS).
    Shadow / "shadow" => Shadow {
        color: Color = Value::Color([0.0, 0.0, 0.0, 0.25]),
        dx: Number = Value::Number(0.0),
        dy: Number = Value::Number(4.0),
        blur: Number = Value::Number(4.0),
    }

    /// A path (roadmap stage E; plans/specs/svg-import-export.md phase 3):
    /// its `contour` children, each an outline, filled together. Where
    /// outlines overlap, `fill_rule` decides what is inside, as in SVG:
    /// `nonzero` (an outline running the other way cuts a hole) or `evenodd`
    /// (every enclosed outline cuts a hole).
    PathShape / "path" => PathShape { fill_rule: Enum = Value::Enum("nonzero".to_owned()) }

    /// One outline of a path: its `point` children in sibling order, joined
    /// by lines or curves, closed back to the first or not. Only closed
    /// contours are filled; every contour is stroked.
    Contour / "contour" => Contour { closed: Bool = Value::Bool(false) }

    /// A path point's Bézier handles, relative to the point: `i` leads into
    /// it, `o` out of it. Zero handles make a corner. Leaves on the point,
    /// not child nodes (roadmap §7 item 4), so each can be bound.
    Handles / "handles" => Handles {
        ix: Number = Value::Number(0.0),
        iy: Number = Value::Number(0.0),
        ox: Number = Value::Number(0.0),
        oy: Number = Value::Number(0.0),
    }

    /// Isolated compositing: applied after a shape (fill, stroke, shadow),
    /// or the whole group's children, have been painted.
    Appearance / "appearance" => Appearance {
        opacity: Number = Value::Number(1.0),
    }

    /// Initial two-stop fill. Coordinates are fractions of the local bounds;
    /// radial uses (x0,y0) as centre and the distance to (x1,y1) as radius.
    /// The solid fill remains the fallback when this facet is removed.
    Gradient / "gradient" => Gradient {
        kind: Enum = Value::Enum("linear".to_owned()),
        start: Color = Value::Color([0.35, 0.3, 0.95, 1.0]),
        end: Color = Value::Color([1.0, 0.45, 0.65, 1.0]),
        x0: Number = Value::Number(0.0),
        y0: Number = Value::Number(0.0),
        x1: Number = Value::Number(1.0),
        y1: Number = Value::Number(1.0),
    }

    /// A group's local clip frame. Rectangles support a uniform corner
    /// radius; ellipses occupy the same (0,0)-(w,h) frame.
    GroupClip / "group_clip" => GroupClip {
        enabled: Bool = Value::Bool(true),
        shape: Enum = Value::Enum("rect".to_owned()),
        w: Number = Value::Number(200.0),
        h: Number = Value::Number(200.0),
        radius: Number = Value::Number(0.0),
    }

    /// A text node's words and how its box sizes (plans/specs/text.md §2).
    /// `content` is an ordinary leaf: typed, bound to an expression
    /// (`"P = " + fmt(p, "kPa", 1)`), or driven by a runtime signal.
    /// `sizing` is Figma's: `auto_width` (one line per paragraph, the box
    /// fits it), `auto_height` (wraps at `size.w`, the height fits), or
    /// `fixed` (`size.w` by `size.h`; text may overflow).
    Text / "text" => Text {
        content: Str = Value::Str(String::new()),
        sizing: Enum = Value::Enum("auto_width".to_owned()),
        // Styled parts (§2.1): JSON, `[{"start":0,"end":5,"weight":700}]`,
        // byte ranges of `content` and the properties they override.
        runs: Str = Value::Str(String::new()),
    }

    /// One style for a whole text node. `line_height` and `letter_spacing`
    /// are percents of `size`, as Figma shows them; a `line_height` of 0
    /// is Auto (the font's own metrics). `tabular` turns on tabular
    /// figures, for readouts whose digits should not jitter. `underline`
    /// and `strike` decorate the whole node; runs may override them.
    Font / "font" => Font {
        family: Str = Value::Str("Inter".to_owned()),
        size: Number = Value::Number(24.0),
        weight: Number = Value::Number(400.0),
        italic: Bool = Value::Bool(false),
        line_height: Number = Value::Number(0.0),
        letter_spacing: Number = Value::Number(0.0),
        align: Enum = Value::Enum("left".to_owned()),
        valign: Enum = Value::Enum("top".to_owned()),
        tabular: Bool = Value::Bool(false),
        underline: Bool = Value::Bool(false),
        strike: Bool = Value::Bool(false),
    }

    Fill / "fill" => Fill {
        color: Color = Value::Color([0.5, 0.5, 0.5, 1.0]),
        opacity: Number = Value::Number(1.0),
    }

    /// How an outline is stroked, as SVG says it: `join` (`round`,
    /// `miter`, `bevel`) with its `miter` limit, `cap` (`round`, `butt`,
    /// `square`), and `dash`, dash and gap lengths separated by spaces
    /// (SVG's `stroke-dasharray`; empty for a solid line), started
    /// `dash_offset` along. Round joins and caps were Iron's only style
    /// before schema 5 and stay the default.
    Stroke / "stroke" => Stroke {
        color: Color = Value::Color([0.0, 0.0, 0.0, 1.0]),
        width: Number = Value::Number(1.0),
        join: Enum = Value::Enum("round".to_owned()),
        cap: Enum = Value::Enum("round".to_owned()),
        miter: Number = Value::Number(4.0),
        dash: Str = Value::Str(String::new()),
        dash_offset: Number = Value::Number(0.0),
    }

    /// Which mesh asset a `mesh` node shows.
    Mesh / "mesh" => Mesh { asset: Str = Value::Str("torus".to_owned()) }

    /// A scalar-on-a-track control (doc 02 §9.3). `value` is the leaf other
    /// properties will bind to.
    Slider / "slider" => Slider {
        min: Number = Value::Number(0.0),
        max: Number = Value::Number(1.0),
        step: Number = Value::Number(0.01),
        value: Number = Value::Number(0.5),
    }

    /// Scripted time (doc 05 §2): a start and a duration on a track.
    Timing / "timing" => Timing {
        start: Number = Value::Number(0.0),
        dur: Number = Value::Number(1.0),
        easing: Enum = Value::Enum("linear".to_owned()),
    }

    /// What a clip does to its target. Endpoints stay literal (doc 05 §2.1)
    /// and take the target leaf's kind. The target itself is a relation, not
    /// a field (doc 05 §3).
    Clip / "clip" => Clip {
        prop: Str = Value::Str(String::new()),
        from: Any = Value::Number(0.0),
        to: Any = Value::Number(1.0),
    }

    /// A geometric relationship with no direction (constraint-system.md §4.1).
    /// Operands are `constrains` relations. `value` is a distance or angle
    /// for the kinds that use one, and may be bound: a driving dimension.
    Constraint / "constraint" => Constraint {
        kind: Enum = Value::Enum("coincident".to_owned()),
        value: Number = Value::Number(0.0),
        enabled: Bool = Value::Bool(true),
    }

    /// Which definition an `entity` instantiates (ECM §4.3), and how
    /// (component-model.md §5.2): `linked`, its ports and pictures expanded
    /// from the definition and never stored; or `detached`, plain authored
    /// nodes that only remember where they came from.
    Instance / "instance" => Instance {
        def: Ref = Value::Ref(crate::id::NodeId::from_raw(0)),
        mode: Enum = Value::Enum("linked".to_owned()),
    }

    /// A per-instance change to one leaf of what an instance derives
    /// (component-model.md §5.3): `target` is the definition node, `leaf`
    /// the path in it (`fill.color`), `value` the constant that replaces the
    /// definition's. An `override` node is a child of the instance (its
    /// entity, or a `use`); expansion applies it after copying the template.
    Override / "override" => Override {
        target: Ref = Value::Ref(crate::id::NodeId::from_raw(0)),
        leaf: Str = Value::Str(String::new()),
        value: Any = Value::Number(0.0),
    }

    /// Where a port sits on a picture (component-model.md, anchors): an
    /// `empty` carrying this marks `port`. `wx`, `wy` are its position in its
    /// composition, derived by the graph from its ancestors' transforms
    /// (`anchor.rs`); readers bind to them. A `follow` anchor is placed at
    /// the anchor of whatever its port is connected to, in the same
    /// composition: a fiber's ends follow what they plug into.
    Anchor / "anchor" => Anchor {
        port: Ref = Value::Ref(crate::id::NodeId::from_raw(0)),
        follow: Bool = Value::Bool(false),
        wx: Number = Value::Number(0.0),
        wy: Number = Value::Number(0.0),
        wrot: Number = Value::Number(0.0),
    }

    /// A placed picture of a component in one composition
    /// (component-model.md §5.1): a `use` node figures its entity, and its
    /// children are the definition's template for `notation`, expanded.
    Use / "use" => Use { notation: Str = Value::Str("free".to_owned()) }

    /// A definition's template for one notation (ECM §4.2). Its children are
    /// the subtree an instance copies; their bindings may read the
    /// definition's `port_spec`s (`#spec.port.signal`), which instantiation
    /// rewrites to the instance's ports.
    Template / "template" => Template { notation: Str = Value::Str("free".to_owned()) }

    /// A composition shown inside another (ECM §7.4): a live reference, not a
    /// copy. 2D in 2D or 3D in 3D places and clips it like a group; 3D in 2D
    /// is a camera view; 2D in 3D a plane.
    Embed / "embed" => Embed { source: Ref = Value::Ref(crate::id::NodeId::from_raw(0)) }

    /// Which camera of the embedded 3D composition the embed looks through.
    /// Absent: a derived default view framing all of its content.
    EmbedCamera / "embed_camera" => EmbedCamera { camera: Ref = Value::Ref(crate::id::NodeId::from_raw(0)) }

    /// A manual bounding box for an embed, in the embedded composition's own
    /// space, as corners (names no other facet uses, so a bare `x` on
    /// an embed still means its position). Absent: the bounds of everything
    /// inside.
    Frame / "frame" => Frame {
        x0: Number = Value::Number(0.0),
        y0: Number = Value::Number(0.0),
        x1: Number = Value::Number(400.0),
        y1: Number = Value::Number(300.0),
    }

    /// A camera in a 3D composition (ECM §7.4). `fov` is vertical, in
    /// radians; `aspect` (width / height) sets the shape of an embed that
    /// looks through it.
    Camera / "camera" => Camera {
        fov: Number = Value::Number(0.8),
        aspect: Number = Value::Number(1.5),
        near: Number = Value::Number(0.1),
        far: Number = Value::Number(1000.0),
    }

    /// A port on an entity, or a definition's spec for one (ECM §6.1). `kind`
    /// names a port kind (`port::KINDS`), which declares its variables: a
    /// one-way `signal`, or an `across`/`through` pair. An out-port's signal
    /// is the definition's behaviour, a binding; an in-port's is its
    /// connection partner's, derived by the graph, never stored.
    Port / "port" => Port {
        kind: Str = Value::Str("value".to_owned()),
        dir: Enum = Value::Enum("in".to_owned()),
        signal: Any = Value::Number(0.0),
        across: Any = Value::Number(0.0),
        through: Any = Value::Number(0.0),
    }

    /// A behaviour equation of a definition or entity (ECM §6.1): `lhs` and
    /// `rhs` are expressions over its ports' across and through values and
    /// its parameters, solved with the connections' equations, never in a
    /// fixed direction.
    Equation / "equation" => Equation {
        lhs: Any = Value::Number(0.0),
        rhs: Any = Value::Number(0.0),
    }

    /// A connection between two ports (ECM §6): its ends are two `connects`
    /// relations. `domain` is informational; the ports' kinds decide.
    Connection / "connection" => Connection { domain: Str = Value::Str(String::new()) }

    /// A composition root (ECM §5): one diagram in one notation, 2D or 3D.
    /// Its subtree is layout, figures and composition-local geometry.
    Composition / "composition" => Composition {
        notation: Str = Value::Str("free".to_owned()),
        dims: Enum = Value::Enum("2d".to_owned()),
    }

    @dynamic {
        /// An instance's arguments: one slot per `in` port its definition
        /// declares (component-model.md §4), each a constant or a binding in
        /// the parent scope. The instance's `in` port reads its argument
        /// unless a connection feeds it. Fields are validated against the
        /// definition at apply time, not here.
        Args / "args" => Args
    }
}

/// The run-time facet: a map from interned argument name to slot.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Args {
    pub slots: BTreeMap<&'static str, Slot>,
}

impl Args {
    pub fn field_names(&self) -> Vec<&'static str> {
        self.slots.keys().copied().collect()
    }

    /// Add or replace an argument.
    pub fn insert(&mut self, name: &str, slot: Slot) {
        self.slots.insert(intern(name), slot);
    }
}

impl LeafStruct for Args {
    const KIND: FacetKind = FacetKind::Args;
    const FIELDS: &'static [(&'static str, ValueKind)] = &[];
    fn get(&self, field: &str) -> Option<&Slot> {
        self.slots.get(field)
    }
    fn get_mut(&mut self, field: &str) -> Option<&mut Slot> {
        self.slots.get_mut(field)
    }
    fn wrap(self) -> Facet {
        Facet::Args(self)
    }
    fn unwrap_ref(c: &Facet) -> Option<&Self> {
        match c {
            Facet::Args(v) => Some(v),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeafError {
    UnknownFacet(String),
    UnknownField {
        facet: FacetKind,
        field: String,
    },
    TypeMismatch {
        path: LeafPath,
        expected: ValueKind,
        got: ValueKind,
    },
}

impl fmt::Display for LeafError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LeafError::UnknownFacet(c) => write!(f, "unknown component {c:?}"),
            LeafError::UnknownField { facet, field } => {
                if facet.is_dynamic() {
                    return write!(f, "{} has no argument {:?} (arguments come from the definition's in ports)", facet.name(), field);
                }
                let available: Vec<&str> = facet.fields().iter().map(|(n, _)| *n).collect();
                write!(
                    f,
                    "component {:?} has no field {:?}; available: {}",
                    facet.name(),
                    field,
                    available.join(", ")
                )
            }
            LeafError::TypeMismatch {
                path,
                expected,
                got,
            } => {
                write!(
                    f,
                    "{path} expects {} but got {}",
                    expected.name(),
                    got.name()
                )
            }
        }
    }
}
impl std::error::Error for LeafError {}

impl Facet {
    /// The compile-time fields; empty for `args`. Prefer
    /// [`Facet::field_names`] to enumerate an instance's leaves.
    pub fn fields(&self) -> &'static [(&'static str, ValueKind)] {
        self.kind().fields()
    }

    /// The declared kind of a field this instance has: the table's for a
    /// static facet, `Any` for an argument (the definition decides).
    pub fn field_kind(&self, field: &str) -> Option<ValueKind> {
        if self.kind().is_dynamic() {
            return self.get(field).map(|_| ValueKind::Any);
        }
        self.kind().fields().iter().find(|(n, _)| *n == field).map(|(_, k)| *k)
    }

    /// Replace one field's slot, checking the value kind. Returns the previous
    /// slot, which is what an inverse op needs. An argument must already
    /// exist: arguments are created with the facet (so an inverse is
    /// always a `Set` back), and validated against the definition by the
    /// applier.
    pub fn set(&mut self, field: &str, slot: Slot) -> Result<Slot, LeafError> {
        let kind = self.kind();
        let expected = self.field_kind(field).ok_or_else(|| LeafError::UnknownField {
            facet: kind,
            field: field.to_owned(),
        })?;
        // A binding's kind is only known when it is evaluated; the graph
        // reports a mismatch then. Constants are checked here.
        if let Some(v) = slot.constant()
            && expected != ValueKind::Any
            && v.kind() != expected
        {
            let path = LeafPath::new(kind, field).expect("field checked above");
            return Err(LeafError::TypeMismatch {
                path,
                expected,
                got: v.kind(),
            });
        }
        let target = self.get_mut(field).expect("field checked above");
        Ok(std::mem::replace(target, slot))
    }

    /// Object of `field: slot`, the file and op shape.
    pub fn to_json(&self) -> serde_json::Value {
        let mut m = serde_json::Map::new();
        for name in self.field_names() {
            let slot = self.get(name).expect("declared field");
            m.insert(name.to_owned(), serde_json::to_value(slot).expect("slot serialises"));
        }
        serde_json::Value::Object(m)
    }

    /// Inverse of [`Facet::to_json`]. Missing fields take defaults;
    /// unknown fields are an error rather than silently dropped. An `args`
    /// object defines its fields.
    pub fn from_json(kind: FacetKind, v: &serde_json::Value) -> Result<Facet, String> {
        let obj = v
            .as_object()
            .ok_or_else(|| format!("component {:?} must be an object", kind.name()))?;
        let mut c = kind.default_facet();
        for (field, raw) in obj {
            // Through `Slot`, not `Value`, so `{"bind": …}` becomes a binding.
            let slot: Slot = serde_json::from_value(raw.clone())
                .map_err(|e| format!("{}.{field}: {e}", kind.name()))?;
            match &mut c {
                Facet::Args(p) => p.insert(field, slot),
                _ => {
                    c.set(field, slot).map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(c)
    }
}

impl Serialize for FacetKind {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.name())
    }
}

impl<'de> Deserialize<'de> for FacetKind {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        FacetKind::from_name(&s)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown component kind {s:?}")))
    }
}

/// A facet in an op serialises as `{"<kind>": {fields}}`.
impl Serialize for Facet {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut m = s.serialize_map(Some(1))?;
        m.serialize_entry(self.kind().name(), &self.to_json())?;
        m.end()
    }
}

impl<'de> Deserialize<'de> for Facet {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = serde_json::Value::deserialize(d)?;
        let obj = raw
            .as_object()
            .filter(|o| o.len() == 1)
            .ok_or_else(|| serde::de::Error::custom("component must be a single-key object"))?;
        let (k, v) = obj.iter().next().expect("len 1");
        let kind = FacetKind::from_name(k)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown component kind {k:?}")))?;
        Facet::from_json(kind, v).map_err(serde::de::Error::custom)
    }
}

/// `facet.field`, the address of one editable value.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LeafPath {
    pub facet: FacetKind,
    pub field: &'static str,
}

impl LeafPath {
    /// A path into a declared field, or into any argument of a dynamic
    /// facet (whether a node has that parameter is checked at use).
    pub fn new(facet: FacetKind, field: &str) -> Result<LeafPath, LeafError> {
        if facet.is_dynamic() {
            if field.is_empty() {
                return Err(LeafError::UnknownField { facet, field: field.to_owned() });
            }
            return Ok(LeafPath { facet, field: intern(field) });
        }
        facet
            .fields()
            .iter()
            .find(|(n, _)| *n == field)
            .map(|(n, _)| LeafPath {
                facet,
                field: n,
            })
            .ok_or_else(|| LeafError::UnknownField {
                facet,
                field: field.to_owned(),
            })
    }

    /// The declared kind; `Any` for a parameter.
    pub fn kind(&self) -> ValueKind {
        if self.facet.is_dynamic() {
            return ValueKind::Any;
        }
        self.facet
            .fields()
            .iter()
            .find(|(n, _)| *n == self.field)
            .map(|(_, k)| *k)
            .expect("constructed from a declared field")
    }
}

impl FromStr for LeafPath {
    type Err = LeafError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (c, f) = s
            .split_once('.')
            .ok_or_else(|| LeafError::UnknownFacet(s.to_owned()))?;
        let facet =
            FacetKind::from_name(c).ok_or_else(|| LeafError::UnknownFacet(c.to_owned()))?;
        LeafPath::new(facet, f)
    }
}

impl fmt::Display for LeafPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.facet.name(), self.field)
    }
}

impl fmt::Debug for LeafPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl Serialize for LeafPath {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for LeafPath {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_checks_kind_and_returns_old() {
        let mut t = Transform2d::default().wrap();
        let old = t.set("x", Slot::Const(Value::Number(4.0))).unwrap();
        assert_eq!(old, Slot::Const(Value::Number(0.0)));
        assert_eq!(t.get("x"), Some(&Slot::Const(Value::Number(4.0))));
        let err = t.set("x", Slot::Const(Value::Bool(true))).unwrap_err();
        assert!(matches!(err, LeafError::TypeMismatch { .. }));
        assert!(matches!(
            t.set("nope", Slot::Const(Value::Number(1.0))),
            Err(LeafError::UnknownField { .. })
        ));
    }

    #[test]
    fn json_roundtrip() {
        let mut f = Fill::default().wrap();
        f.set("opacity", Value::Number(0.25).into()).unwrap();
        let j = serde_json::to_string(&f).unwrap();
        assert_eq!(
            j,
            r#"{"fill":{"color":{"rgba":[0.5,0.5,0.5,1.0]},"opacity":0.25}}"#
        );
        let back: Facet = serde_json::from_str(&j).unwrap();
        assert_eq!(back, f);
        assert!(
            Facet::from_json(FacetKind::Fill, &serde_json::json!({"nope": 1})).is_err()
        );
    }

    #[test]
    fn leaf_path_parses() {
        let p: LeafPath = "transform2d.x".parse().unwrap();
        assert_eq!(p.facet, FacetKind::Transform2d);
        assert_eq!(p.kind(), ValueKind::Number);
        assert_eq!(p.to_string(), "transform2d.x");
        assert!("transform2d.q".parse::<LeafPath>().is_err());
        assert!("nope.x".parse::<LeafPath>().is_err());
        assert!("x".parse::<LeafPath>().is_err());
        // Arguments are run-time fields: any name is a path, kind Any.
        let p: LeafPath = "args.waist".parse().unwrap();
        assert_eq!((p.facet, p.field, p.kind()), (FacetKind::Args, "waist", ValueKind::Any));
        assert_eq!(p, LeafPath::new(FacetKind::Args, &String::from("waist")).unwrap());
        assert!("args.".parse::<LeafPath>().is_err());
    }

    #[test]
    fn args_are_a_dynamic_facet() {
        let mut p = Args::default();
        p.insert("level", Slot::Const(Value::Number(0.4)));
        p.insert("power", Slot::Const(Value::parse_quantity("9.58 mW").unwrap()));
        let mut c = p.wrap();
        assert_eq!(c.field_names(), vec!["level", "power"]);
        assert_eq!(c.field_kind("level"), Some(ValueKind::Any));
        assert_eq!(c.field_kind("nope"), None);
        // Set replaces an existing argument, any kind, and returns the old slot.
        let old = c.set("level", Slot::Const(Value::Number(0.9))).unwrap();
        assert_eq!(old, Slot::Const(Value::Number(0.4)));
        // Set on an argument the facet lacks is an error, not a create.
        assert!(matches!(c.set("nope", Slot::Const(Value::Number(1.0))), Err(LeafError::UnknownField { .. })));
        let j = serde_json::to_string(&c).unwrap();
        assert_eq!(j, r#"{"args":{"level":0.9,"power":{"q":[0.00958,"W"]}}}"#);
        let back: Facet = serde_json::from_str(&j).unwrap();
        assert_eq!(back, c);
        assert_eq!(FacetKind::Args.fields(), &[]);
        assert!(FacetKind::Args.is_dynamic() && !FacetKind::Size.is_dynamic());
    }
}
