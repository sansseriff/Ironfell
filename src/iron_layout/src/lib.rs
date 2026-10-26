//! Stacks and grids for 2D compositions (plans/specs/layout-and-tables.md §2).
//!
//! The store describes the laid-out part of its tree in this crate's own
//! vocabulary ([`Container`], [`Item`]), with a measure callback for leaves
//! whose size comes from their content (text), and gets back a [`Placed`]
//! box per node and the track positions of every grid. Taffy does the work;
//! only this crate names its types, so the store's facets never depend on
//! Taffy's style model.
//!
//! The tree is the caller's: [`Tree`] holds nodes in a flat `Vec` with child
//! indices, keyed by the caller's ids. Taffy's traits are implemented over it
//! directly (its low-level API), with no copy into Taffy's own tree.

use std::collections::HashMap;

use taffy::style_helpers::{line, span};
use taffy::{
    AlignContent, AlignItems, AvailableSpace, Cache, CacheTree, Dimension, Display, FlexDirection, GridPlacement, GridTemplateComponent, JustifyContent, Layout, LayoutInput, LayoutOutput,
    LayoutPartialTree, LengthPercentage, LengthPercentageAuto, Line, MaxTrackSizingFunction, MinTrackSizingFunction, TrackSizingFunction, NodeId as TaffyId, Rect, Size, Style, TraversePartialTree, compute_cached_layout, compute_flexbox_layout,
    compute_grid_layout, compute_leaf_layout, compute_root_layout,
};

/// How a container places its children.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    /// Left to right.
    #[default]
    StackX,
    /// Top to bottom.
    StackY,
    /// Rows and columns from `columns` and `rows`.
    Grid,
}

/// Where items sit across a stack, or within a grid cell.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum Align {
    #[default]
    Start,
    Center,
    End,
    Stretch,
}

/// How a stack spreads its items along its axis.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum Justify {
    #[default]
    Start,
    Center,
    End,
    /// The first item at the start, the last at the end, equal gaps between.
    Between,
}

/// A grid track's size.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Track {
    /// As wide as its widest content.
    Auto,
    Px(f32),
    /// A share of the space left after the other tracks.
    Fr(f32),
}

/// One dimension of a box.
#[derive(Copy, Clone, Debug, PartialEq, Default)]
pub enum Dim {
    /// A container hugs its content; a leaf takes its measured size.
    #[default]
    Auto,
    Px(f32),
    /// Fills what its container gives it: the rest of a stack's axis, the
    /// whole of its cross axis or its grid area.
    Fill,
}

/// A node's own layout, as a container.
#[derive(Clone, Debug, Default)]
pub struct Container {
    pub mode: Mode,
    /// Space between items (stacks), or between columns and rows (grids).
    pub gap: [f32; 2],
    /// Top, right, bottom, left.
    pub pad: [f32; 4],
    pub align: Align,
    pub justify: Justify,
    pub columns: Vec<Track>,
    pub rows: Vec<Track>,
}

/// A grid item's area: 0-based start and span, as a table places a cell.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Area {
    pub start: u16,
    pub span: u16,
}

/// A node's layout as an item of its container (and the root's own size).
#[derive(Clone, Debug, Default)]
pub struct Item {
    pub w: Dim,
    pub h: Dim,
    /// Overrides the container's `align` for this item.
    pub align_self: Option<Align>,
    /// Grid placement; `None` flows into the next free cell.
    pub column: Option<Area>,
    pub row: Option<Area>,
}

/// One node of the tree to lay out.
#[derive(Clone, Debug)]
pub struct Node {
    /// The caller's id, given back with its result.
    pub key: u64,
    pub item: Item,
    /// `Some` for a container; `None` for a leaf.
    pub container: Option<Container>,
    /// Indices into [`Tree::nodes`], in order.
    pub children: Vec<usize>,
    /// A leaf whose size comes from its content: the measure callback is
    /// asked for it.
    pub measured: bool,
}

/// A laid-out node: its box relative to its container's top-left (the
/// root's is at the origin).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Placed {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Where a grid's tracks fell: `(start, size)` from the container's top-left.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tracks {
    pub columns: Vec<(f32, f32)>,
    pub rows: Vec<(f32, f32)>,
}

/// What a measured leaf is asked: the width or height already decided, if
/// any, and the space on offer (`None`: as much as it wants, which is how
/// a container asks for max-content; `Some(0)` for min-content).
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Ask {
    pub known: [Option<f32>; 2],
    pub available: [Option<f32>; 2],
    /// The narrowest the content can be (its longest word), not a width.
    pub min_content: bool,
}

/// The result of a layout.
#[derive(Clone, Debug, Default)]
pub struct Output {
    pub placed: HashMap<u64, Placed>,
    pub tracks: HashMap<u64, Tracks>,
}

/// A tree to lay out; `nodes[root]` is its root.
#[derive(Clone, Debug, Default)]
pub struct Tree {
    pub nodes: Vec<Node>,
    pub root: usize,
}

impl Tree {
    /// Lay the tree out, asking `measure` the size of each measured leaf.
    /// The root's `Fill` dimensions take the given size; its `Auto` ones
    /// hug its content.
    pub fn layout(&self, available: [Option<f32>; 2], measure: &mut dyn FnMut(u64, Ask) -> [f32; 2]) -> Output {
        let styles = self.nodes.iter().map(|n| style(n, n.container.as_ref())).collect();
        let mut run = Run { tree: self, styles, caches: vec![Cache::new(); self.nodes.len()], layouts: vec![Layout::new(); self.nodes.len()], tracks: HashMap::new(), measure };
        run.fill_items();
        let space = |a: Option<f32>| a.map_or(AvailableSpace::MaxContent, AvailableSpace::Definite);
        compute_root_layout(&mut run, TaffyId::from(self.root), Size { width: space(available[0]), height: space(available[1]) });
        let placed = self
            .nodes
            .iter()
            .zip(&run.layouts)
            .map(|(n, l)| (n.key, Placed { x: l.location.x, y: l.location.y, w: l.size.width, h: l.size.height }))
            .collect();
        let tracks = run.tracks.into_iter().map(|(i, t)| (self.nodes[i].key, t)).collect();
        Output { placed, tracks }
    }
}

fn style(n: &Node, c: Option<&Container>) -> Style {
    let dim = |d: Dim| match d {
        Dim::Px(v) => Dimension::length(v),
        Dim::Auto | Dim::Fill => Dimension::auto(),
    };
    let mut s = Style { size: Size { width: dim(n.item.w), height: dim(n.item.h) }, ..Style::default() };
    // A `Fill` dimension stretches across; along a stack it grows (below,
    // where the container's axis is known).
    s.align_self = n.item.align_self.map(align_items);
    if let Some(a) = n.item.column {
        s.grid_column = area(a);
    }
    if let Some(a) = n.item.row {
        s.grid_row = area(a);
    }
    let Some(c) = c else {
        return s;
    };
    s.padding = Rect { top: LengthPercentage::length(c.pad[0]), right: LengthPercentage::length(c.pad[1]), bottom: LengthPercentage::length(c.pad[2]), left: LengthPercentage::length(c.pad[3]) };
    s.gap = Size { width: LengthPercentage::length(c.gap[0]), height: LengthPercentage::length(c.gap[1]) };
    match c.mode {
        Mode::StackX | Mode::StackY => {
            s.display = Display::Flex;
            s.flex_direction = if c.mode == Mode::StackX { FlexDirection::Row } else { FlexDirection::Column };
            s.align_items = Some(align_items(c.align));
            s.justify_content = Some(match c.justify {
                Justify::Start => JustifyContent::START,
                Justify::Center => JustifyContent::CENTER,
                Justify::End => JustifyContent::END,
                Justify::Between => JustifyContent::SPACE_BETWEEN,
            });
        }
        Mode::Grid => {
            s.display = Display::Grid;
            s.grid_template_columns = c.columns.iter().map(|t| track(*t)).collect();
            s.grid_template_rows = c.rows.iter().map(|t| track(*t)).collect();
            // Items fill their cells unless they say otherwise; `align`
            // places them within it.
            s.align_items = Some(align_items(c.align));
            s.justify_items = Some(align_items(c.align));
            s.align_content = Some(AlignContent::START);
            s.justify_content = Some(AlignContent::START);
        }
    }
    s
}

fn align_items(a: Align) -> AlignItems {
    match a {
        Align::Start => AlignItems::START,
        Align::Center => AlignItems::CENTER,
        Align::End => AlignItems::END,
        Align::Stretch => AlignItems::STRETCH,
    }
}

fn area(a: Area) -> Line<GridPlacement<String>> {
    Line { start: line(a.start as i16 + 1), end: span(a.span.max(1)) }
}

fn track(t: Track) -> GridTemplateComponent<String> {
    match t {
        // An auto track takes no leftover space: the grid's content is
        // justified to the start, so it is as wide as its widest item.
        Track::Auto => GridTemplateComponent::Single(TrackSizingFunction { min: MinTrackSizingFunction::auto(), max: MaxTrackSizingFunction::auto() }),
        Track::Px(v) => GridTemplateComponent::Single(TrackSizingFunction { min: MinTrackSizingFunction::length(v), max: MaxTrackSizingFunction::length(v) }),
        Track::Fr(f) => GridTemplateComponent::Single(TrackSizingFunction { min: MinTrackSizingFunction::auto(), max: MaxTrackSizingFunction::fr(f) }),
    }
}

/// One layout pass: the tree, its Taffy styles and per-node state.
struct Run<'a> {
    tree: &'a Tree,
    styles: Vec<Style>,
    caches: Vec<Cache>,
    layouts: Vec<Layout>,
    tracks: HashMap<usize, Tracks>,
    measure: &'a mut dyn FnMut(u64, Ask) -> [f32; 2],
}

impl Run<'_> {
    /// A `Fill` item grows along its stack's axis and stretches across it,
    /// or stretches over its grid area; set here, where the parent is known.
    fn fill_items(&mut self) {
        for n in &self.tree.nodes {
            let Some(c) = &n.container else { continue };
            if c.mode == Mode::Grid {
                // In a grid, `Fill` stretches across its area.
                for &k in &n.children {
                    let item = &self.tree.nodes[k].item;
                    let s = &mut self.styles[k];
                    if item.w == Dim::Fill {
                        s.justify_self = Some(AlignItems::STRETCH);
                    }
                    if item.h == Dim::Fill {
                        s.align_self = Some(AlignItems::STRETCH);
                    }
                }
                continue;
            }
            for &k in &n.children {
                let item = &self.tree.nodes[k].item;
                let (main, cross) = if c.mode == Mode::StackX { (item.w, item.h) } else { (item.h, item.w) };
                let s = &mut self.styles[k];
                if main == Dim::Fill {
                    s.flex_grow = 1.0;
                    s.flex_basis = Dimension::length(0.0);
                    if c.mode == Mode::StackX {
                        s.min_size.width = LengthPercentageAuto::length(0.0);
                    } else {
                        s.min_size.height = LengthPercentageAuto::length(0.0);
                    }
                }
                if cross == Dim::Fill && item.align_self.is_none() {
                    s.align_self = Some(AlignItems::STRETCH);
                }
            }
        }
    }
}

pub struct Children(std::vec::IntoIter<TaffyId>);
impl Iterator for Children {
    type Item = TaffyId;
    fn next(&mut self) -> Option<TaffyId> {
        self.0.next()
    }
}

impl TraversePartialTree for Run<'_> {
    type ChildIter<'b>
        = Children
    where
        Self: 'b;

    fn child_ids(&self, node: TaffyId) -> Children {
        Children(self.tree.nodes[usize::from(node)].children.iter().map(|&c| TaffyId::from(c)).collect::<Vec<_>>().into_iter())
    }

    fn child_count(&self, node: TaffyId) -> usize {
        self.tree.nodes[usize::from(node)].children.len()
    }

    fn get_child_id(&self, node: TaffyId, index: usize) -> TaffyId {
        TaffyId::from(self.tree.nodes[usize::from(node)].children[index])
    }
}

impl LayoutPartialTree for Run<'_> {
    type CoreContainerStyle<'b>
        = &'b Style
    where
        Self: 'b;

    type CustomIdent = String;

    fn get_core_container_style(&self, node: TaffyId) -> &Style {
        &self.styles[usize::from(node)]
    }

    fn set_unrounded_layout(&mut self, node: TaffyId, layout: &Layout) {
        self.layouts[usize::from(node)] = *layout;
    }

    fn compute_child_layout(&mut self, node: TaffyId, inputs: LayoutInput) -> LayoutOutput {
        compute_cached_layout(self, node, inputs, |run, node, inputs| {
            let i = usize::from(node);
            let n = &run.tree.nodes[i];
            match &n.container {
                Some(c) if c.mode == Mode::Grid => compute_grid_layout(run, node, inputs),
                Some(_) => compute_flexbox_layout(run, node, inputs),
                None => {
                    let key = n.key;
                    let measured = n.measured;
                    let style = run.styles[i].clone();
                    let measure = &mut run.measure;
                    compute_leaf_layout(inputs, &style, |_, _| 0.0, |known, available| {
                        if !measured {
                            return Size::ZERO;
                        }
                        let space = |a: AvailableSpace| match a {
                            AvailableSpace::Definite(v) => Some(v),
                            AvailableSpace::MinContent => Some(0.0),
                            AvailableSpace::MaxContent => None,
                        };
                        let min_content = matches!(available.width, AvailableSpace::MinContent);
                        let [w, h] = measure(key, Ask { known: [known.width, known.height], available: [space(available.width), space(available.height)], min_content });
                        Size { width: w, height: h }
                    })
                }
            }
        })
    }
}

impl CacheTree for Run<'_> {
    fn cache_get(&mut self, node: TaffyId, input: &LayoutInput) -> Option<LayoutOutput> {
        self.caches[usize::from(node)].get(input)
    }

    fn cache_store(&mut self, node: TaffyId, input: &LayoutInput, output: LayoutOutput) {
        self.caches[usize::from(node)].store(input, output)
    }

    fn cache_clear(&mut self, node: TaffyId) {
        self.caches[usize::from(node)].clear();
    }
}

impl taffy::LayoutFlexboxContainer for Run<'_> {
    type FlexboxContainerStyle<'b>
        = &'b Style
    where
        Self: 'b;

    type FlexboxItemStyle<'b>
        = &'b Style
    where
        Self: 'b;

    fn get_flexbox_container_style(&self, node: TaffyId) -> &Style {
        &self.styles[usize::from(node)]
    }

    fn get_flexbox_child_style(&self, child: TaffyId) -> &Style {
        &self.styles[usize::from(child)]
    }
}

impl taffy::LayoutGridContainer for Run<'_> {
    type GridContainerStyle<'b>
        = &'b Style
    where
        Self: 'b;

    type GridItemStyle<'b>
        = &'b Style
    where
        Self: 'b;

    fn get_grid_container_style(&self, node: TaffyId) -> &Style {
        &self.styles[usize::from(node)]
    }

    fn get_grid_child_style(&self, child: TaffyId) -> &Style {
        &self.styles[usize::from(child)]
    }

    fn set_detailed_grid_info(&mut self, node: TaffyId, info: taffy::DetailedGridInfo<String>) {
        let spans = |t: &taffy::DetailedGridTracksInfo<String>| t.positions.iter().map(|l| (l.start, l.end - l.start)).collect();
        self.tracks.insert(usize::from(node), Tracks { columns: spans(&info.columns), rows: spans(&info.rows) });
    }
}
