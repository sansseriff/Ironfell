//! Grids and stacks against hand calculations (layout-and-tables.md §7,
//! phase 0). Text is faked: 8 units a character, 16 a line, wrapping at
//! spaces when given a width.

use iron_layout::{Align, Area, Ask, Container, Dim, Item, Justify, Mode, Node, Track, Tree};

fn text(words: &str, ask: Ask) -> [f32; 2] {
    let longest = words.split(' ').map(|w| w.len()).max().unwrap_or(0) as f32 * 8.0;
    let full = words.len() as f32 * 8.0;
    let width = match (ask.known[0], ask.available[0]) {
        (Some(w), _) => w,
        _ if ask.min_content => longest,
        (None, Some(a)) => full.min(a.max(longest)),
        (None, None) => full,
    };
    // Greedy wrap at `width`.
    let mut lines = 1;
    let mut line = 0.0;
    for w in words.split(' ') {
        let len = w.len() as f32 * 8.0;
        let add = if line == 0.0 { len } else { line + 8.0 + len };
        if add > width + 0.01 && line > 0.0 {
            lines += 1;
            line = len;
        } else {
            line = add;
        }
    }
    [width, ask.known[1].unwrap_or(lines as f32 * 16.0)]
}

struct Build {
    tree: Tree,
    words: Vec<(u64, String)>,
}

impl Build {
    fn new() -> Self {
        Build { tree: Tree::default(), words: Vec::new() }
    }
    fn node(&mut self, key: u64, item: Item, container: Option<Container>) -> usize {
        self.tree.nodes.push(Node { key, item, container, children: Vec::new(), measured: false });
        self.tree.nodes.len() - 1
    }
    fn text(&mut self, parent: usize, key: u64, words: &str, item: Item) -> usize {
        let i = self.node(key, item, None);
        self.tree.nodes[i].measured = true;
        self.tree.nodes[parent].children.push(i);
        self.words.push((key, words.to_owned()));
        i
    }
    fn run(&self) -> iron_layout::Output {
        let words = self.words.clone();
        self.tree.layout([None, None], &mut |key, ask| text(&words.iter().find(|(k, _)| *k == key).unwrap().1, ask))
    }
}

fn grid(columns: Vec<Track>, rows: usize, pad: f32, gap: f32) -> Container {
    Container { mode: Mode::Grid, gap: [gap, gap], pad: [pad; 4], align: Align::Stretch, justify: Justify::Start, columns, rows: vec![Track::Auto; rows] }
}

#[test]
fn a_grid_of_text_sizes_its_tracks_to_the_widest_cell() {
    let mut b = Build::new();
    let g = b.node(1, Item::default(), Some(grid(vec![Track::Auto; 3], 3, 10.0, 4.0)));
    let cells = [["Gap W", "153", "138"], ["Loss 2 GHz", "1.10", "0.89"], ["S", "85", "113"]];
    let mut key = 10;
    for row in cells {
        for c in row {
            b.text(g, key, c, Item::default());
            key += 1;
        }
    }
    let out = b.run();
    let t = &out.tracks[&1];
    // Column widths: "Loss 2 GHz" = 10 chars, "1.10" = 4, "0.89" = 4.
    let widths: Vec<f32> = t.columns.iter().map(|c| c.1).collect();
    assert_eq!(widths, vec![80.0, 32.0, 32.0]);
    // Starts after the padding, then each width plus the gap.
    let starts: Vec<f32> = t.columns.iter().map(|c| c.0).collect();
    assert_eq!(starts, vec![10.0, 94.0, 130.0]);
    assert_eq!(t.rows.iter().map(|r| (r.0, r.1)).collect::<Vec<_>>(), vec![(10.0, 16.0), (30.0, 16.0), (50.0, 16.0)]);
    // The grid hugs: 10 + 80 + 4 + 32 + 4 + 32 + 10.
    let g = out.placed[&1];
    assert_eq!((g.w, g.h), (172.0, 76.0));
    // A cell stretches to its column: "S" is as wide as "Loss 2 GHz".
    let s = out.placed[&16];
    assert_eq!((s.x, s.y, s.w, s.h), (10.0, 50.0, 80.0, 16.0));
}

#[test]
fn a_merged_cell_spans_its_tracks() {
    let mut b = Build::new();
    let g = b.node(1, Item::default(), Some(grid(vec![Track::Px(50.0), Track::Px(30.0)], 2, 0.0, 0.0)));
    let wide = Item { column: Some(Area { start: 0, span: 2 }), row: Some(Area { start: 0, span: 1 }), ..Item::default() };
    b.text(g, 10, "header", wide);
    b.text(g, 11, "a", Item { column: Some(Area { start: 0, span: 1 }), row: Some(Area { start: 1, span: 1 }), ..Item::default() });
    b.text(g, 12, "b", Item { column: Some(Area { start: 1, span: 1 }), row: Some(Area { start: 1, span: 1 }), ..Item::default() });
    let out = b.run();
    let h = out.placed[&10];
    assert_eq!((h.x, h.w), (0.0, 80.0));
    let bb = out.placed[&12];
    assert_eq!((bb.x, bb.y, bb.w), (50.0, 16.0, 30.0));
}

#[test]
fn a_fixed_column_wraps_its_text() {
    let mut b = Build::new();
    let g = b.node(1, Item::default(), Some(grid(vec![Track::Px(48.0)], 1, 0.0, 0.0)));
    b.text(g, 10, "one two three", Item::default());
    let out = b.run();
    // "one two" is 56 wide, so each word takes a line at 48.
    assert_eq!(out.placed[&10].h, 48.0);
}

#[test]
fn a_stack_places_items_one_after_another() {
    let mut b = Build::new();
    let c = Container { mode: Mode::StackY, gap: [0.0, 8.0], pad: [12.0, 16.0, 12.0, 16.0], align: Align::Start, justify: Justify::Start, columns: vec![], rows: vec![] };
    let s = b.node(1, Item::default(), Some(c));
    b.text(s, 10, "Title", Item::default());
    b.text(s, 11, "A longer body", Item::default());
    let out = b.run();
    let (t, body) = (out.placed[&10], out.placed[&11]);
    assert_eq!((t.x, t.y, t.w), (16.0, 12.0, 40.0));
    assert_eq!((body.x, body.y, body.w), (16.0, 36.0, 104.0));
    let s = out.placed[&1];
    assert_eq!((s.w, s.h), (136.0, 64.0));
}

#[test]
fn fill_takes_the_rest_of_a_stack() {
    let mut b = Build::new();
    let c = Container { mode: Mode::StackX, gap: [10.0, 0.0], pad: [0.0; 4], align: Align::Start, justify: Justify::Start, columns: vec![], rows: vec![] };
    let s = b.node(1, Item { w: Dim::Px(300.0), ..Item::default() }, Some(c));
    b.text(s, 10, "Label", Item::default());
    b.text(s, 11, "x", Item { w: Dim::Fill, h: Dim::Fill, ..Item::default() });
    let out = b.run();
    let f = out.placed[&11];
    // 300 − "Label" (40) − the gap (10).
    assert_eq!((f.x, f.w), (50.0, 250.0));
}
