//! Tables (plans/specs/layout-and-tables.md §3): where cells sit, what a
//! table draws under them, and the ops that make and edit one.
//!
//! A `table` node's children are its `row`s, and a row's are its cells,
//! `text` nodes, as HTML's `<tr>` and `<td>`. Columns are the table's
//! `columns` tracks. A merged cell spans (`layout_item.span_x`/`span_y`) and
//! the positions it covers have no node; as in HTML's table-forming
//! algorithm, each cell takes the first column in its row not already
//! covered by a cell above. [`place`] does this; layout (`layout.rs`) hands the result to
//! Taffy as explicit grid areas, and keeps the track positions it gets back
//! as the table's [`Grid`], from which [`decor`] works out the bands and the
//! lines, broken where a merged cell spans across them.

use kurbo::{Line, Rect};

use crate::facet::{Font, LayoutItem, LeafStruct, Name, Rules, Table, Text, Transform2d};
use crate::{Document, NodeId, Op, OrderKey, Slot, Store, TypeId, Value};

/// A cell's area: 0-based column and row, and how many of each it spans.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Area {
    pub id: NodeId,
    pub col: usize,
    pub row: usize,
    pub span_x: usize,
    pub span_y: usize,
}

/// Where a table's cells fell: track positions from the table's top-left,
/// `(start, size)`, and each cell's area.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Grid {
    pub columns: Vec<(f64, f64)>,
    pub rows: Vec<(f64, f64)>,
    pub cells: Vec<Area>,
}

/// How a table's leaves are read: through the store beneath layout's
/// output (`layout::base`), or through the graph, as a view reads.
pub type Read<'a> = &'a dyn Fn(NodeId, &str) -> Option<Value>;

fn read_num(read: Read, id: NodeId, leaf: &str, default: f64) -> f64 {
    read(id, leaf).and_then(|v| v.as_f64()).filter(|v| v.is_finite()).unwrap_or(default)
}

fn read_word(read: Read, id: NodeId, leaf: &str) -> String {
    read(id, leaf).and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default()
}

fn num(store: &Store, id: NodeId, leaf: &str, default: f64) -> f64 {
    read_num(&|n, l| crate::layout::base(store, n, l), id, leaf, default)
}

fn column_count_with(read: Read, table: NodeId) -> usize {
    read_word(read, table, "table.columns").split_whitespace().count().max(1)
}

/// How many columns a table has: one per track in `table.columns`.
pub fn column_count(store: &Store, table: NodeId) -> usize {
    column_count_with(&|n, l| crate::layout::base(store, n, l), table)
}

fn of_type(doc: &Document, parent: NodeId, ty: &str) -> Vec<NodeId> {
    doc.children(parent).iter().copied().filter(|&c| doc.live(c).is_some_and(|n| n.ty.name() == ty)).collect()
}

/// The table's rows, top to bottom.
pub fn rows(doc: &Document, table: NodeId) -> Vec<NodeId> {
    of_type(doc, table, "row")
}

/// A row's cells, left to right.
pub fn row_cells(doc: &Document, row: NodeId) -> Vec<NodeId> {
    of_type(doc, row, "text")
}

/// The table's cells, in reading order.
pub fn cells(doc: &Document, table: NodeId) -> Vec<NodeId> {
    rows(doc, table).into_iter().flat_map(|r| row_cells(doc, r)).collect()
}

/// The table a node is a cell or a row of, if any.
pub fn table_of(doc: &Document, id: NodeId) -> Option<NodeId> {
    let is = |n: NodeId, ty: &str| doc.live(n).is_some_and(|x| x.ty.name() == ty);
    let mut at = id;
    for _ in 0..2 {
        if is(at, "table") {
            return Some(at);
        }
        at = doc.live(at)?.parent?;
    }
    is(at, "table").then_some(at)
}

/// Place a table's cells (HTML's rule): row by row, each cell takes the
/// first column at or after the previous cell's end that no cell above
/// covers; a span runs no further than the table's last row. A cell with
/// no room left in its row goes past the last column, where it shows as an
/// extra column rather than shifting its neighbours. Returns the areas, in
/// reading order, and the number of rows.
pub fn place(store: &Store, table: NodeId) -> (Vec<Area>, usize) {
    place_with(store.document(), &|n, l| crate::layout::base(store, n, l), table)
}

/// [`place`], reading leaves through `read`.
pub fn place_with(doc: &Document, read: Read, table: NodeId) -> (Vec<Area>, usize) {
    let cols = column_count_with(read, table);
    let rows = rows(doc, table);
    let mut taken = vec![vec![false; cols]; rows.len()];
    let mut out = Vec::new();
    for (r, &row) in rows.iter().enumerate() {
        let mut c = 0usize;
        for id in row_cells(doc, row) {
            while c < cols && taken[r][c] {
                c += 1;
            }
            let room = (c..cols).take_while(|&x| !taken[r][x]).count().max(1);
            let span_x = (read_num(read, id, "layout_item.span_x", 1.0).round().max(1.0) as usize).min(room);
            let span_y = (read_num(read, id, "layout_item.span_y", 1.0).round().max(1.0) as usize).min(rows.len() - r);
            for line in taken.iter_mut().skip(r).take(span_y) {
                for x in c..(c + span_x).min(cols) {
                    line[x] = true;
                }
            }
            out.push(Area { id, col: c, row: r, span_x, span_y });
            c += span_x;
        }
    }
    (out, rows.len())
}

/// What a table draws under its cells: filled bands (its background, the
/// header, zebra rows) and lines, each with a width and a colour.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Decor {
    pub bands: Vec<(Rect, [f32; 4])>,
    pub lines: Vec<(Line, f64, [f32; 4])>,
}

fn color(store: &Store, id: NodeId, leaf: &str) -> [f32; 4] {
    match crate::layout::base(store, id, leaf) {
        Some(Value::Color(c)) => c,
        _ => [0.0; 4],
    }
}

/// A table's bands and lines, from its grid and its `table` and `rules`
/// leaves.
pub fn decor(store: &Store, table: NodeId, grid: &Grid) -> Decor {
    let mut d = Decor::default();
    let (Some(&(x0, _)), Some(&(xl, wl)), Some(&(y0, _)), Some(&(yl, hl))) = (grid.columns.first(), grid.columns.last(), grid.rows.first(), grid.rows.last()) else {
        return d;
    };
    let (x1, y1) = (xl + wl, yl + hl);
    let header = (num(store, table, "table.header", 1.0).round().max(0.0) as usize).min(grid.rows.len());
    let header_fill = color(store, table, "table.header_fill");
    if header > 0 && header_fill[3] > 0.0 {
        let (hy, hh) = grid.rows[header - 1];
        d.bands.push((Rect::new(x0, y0, x1, hy + hh), header_fill));
    }
    let zebra = color(store, table, "table.zebra_fill");
    if zebra[3] > 0.0 {
        for (i, &(y, h)) in grid.rows.iter().enumerate().skip(header) {
            if (i - header) % 2 == 1 {
                d.bands.push((Rect::new(x0, y, x1, y + h), zebra));
            }
        }
    }
    let rule = |kind: &str| (num(store, table, &format!("rules.{kind}_width"), 0.0), color(store, table, &format!("rules.{kind}_color")));
    let has_rules = store.document().facet(table, crate::FacetKind::Rules).is_some();
    if !has_rules {
        return d;
    }
    // Whether a cell spans across the boundary before row `r` (or column
    // `c`) at column `at` (or row `at`): no line is drawn through it.
    let across_row = |r: usize, at: usize| grid.cells.iter().any(|a| a.row < r && r < a.row + a.span_y && a.col <= at && at < a.col + a.span_x);
    let across_col = |c: usize, at: usize| grid.cells.iter().any(|a| a.col < c && c < a.col + a.span_x && a.row <= at && at < a.row + a.span_y);
    // Segments along one boundary, joined where neighbours meet.
    let push_runs = |d: &mut Decor, spans: Vec<(f64, f64)>, make: &dyn Fn(f64, f64) -> Line, (w, c): (f64, [f32; 4])| {
        if w <= 0.0 || c[3] <= 0.0 {
            return;
        }
        let mut run: Option<(f64, f64)> = None;
        for (a, b) in spans {
            run = match run {
                Some((s, e)) if (e - a).abs() < 1e-6 => Some((s, b)),
                Some((s, e)) => {
                    d.lines.push((make(s, e), w, c));
                    Some((a, b))
                }
                None => Some((a, b)),
            };
        }
        if let Some((s, e)) = run {
            d.lines.push((make(s, e), w, c));
        }
    };
    for r in 1..grid.rows.len() {
        let y = grid.rows[r].0;
        let spans: Vec<(f64, f64)> = grid.columns.iter().enumerate().filter(|(c, _)| !across_row(r, *c)).map(|(_, &(x, w))| (x, x + w)).collect();
        let kind = if r == header { "header" } else { "rows" };
        push_runs(&mut d, spans, &|a, b| Line::new((a, y), (b, y)), rule(kind));
    }
    for c in 1..grid.columns.len() {
        let x = grid.columns[c].0;
        let spans: Vec<(f64, f64)> = grid.rows.iter().enumerate().filter(|(r, _)| !across_col(c, *r)).map(|(_, &(y, h))| (y, y + h)).collect();
        push_runs(&mut d, spans, &|a, b| Line::new((x, a), (x, b)), rule("columns"));
    }
    let outer = rule("outer");
    for line in [Line::new((x0, y0), (x1, y0)), Line::new((x1, y0), (x1, y1)), Line::new((x1, y1), (x0, y1)), Line::new((x0, y1), (x0, y0))] {
        if outer.0 > 0.0 && outer.1[3] > 0.0 {
            d.lines.push((line, outer.0, outer.1));
        }
    }
    d
}

/// The look a new table, or a restyled one, takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    /// Hairlines between rows, no box, a muted monospace label column
    /// (the reference engineering table).
    Light,
    /// Every cell boxed, a filled header band.
    Boxed,
}

impl Preset {
    pub fn parse(s: &str) -> Preset {
        if s == "boxed" { Preset::Boxed } else { Preset::Light }
    }
    pub fn name(self) -> &'static str {
        match self {
            Preset::Light => "light",
            Preset::Boxed => "boxed",
        }
    }
}

/// How a table is made: its rows of words and its look.
#[derive(Clone, Debug)]
pub struct Spec {
    /// Rows of cells. A cell `<` merges into the cell on its left, `^` into
    /// the one above (§3.4); `\<` and `\^` are those characters.
    pub rows: Vec<Vec<String>>,
    pub preset: Preset,
    /// Header rows.
    pub header: usize,
    /// The first column is a label column (muted, left-aligned).
    pub label_col: bool,
    /// A column (0-based) to emphasise: bold body, accent header.
    pub emphasis: Option<usize>,
    /// Alignments for the first columns (`left`, `center`, `right`); the
    /// rest are right-aligned.
    pub align: Vec<String>,
}

const INK: [f32; 4] = [0.11, 0.13, 0.17, 1.0];
const MUTED: [f32; 4] = [0.45, 0.48, 0.55, 1.0];
const ACCENT: [f32; 4] = [0.85, 0.47, 0.13, 1.0];

/// The style of the cell at (`row`, `col`), as its `font` and fill colour.
pub fn cell_style(spec: &Spec, row: usize, col: usize) -> (Font, [f32; 4]) {
    let mut f = Font::default();
    let header = row < spec.header;
    let label = spec.label_col && col == 0;
    let emph = spec.emphasis == Some(col);
    let align = spec.align.get(col).cloned().unwrap_or_else(|| if label { "left".into() } else { "right".into() });
    f.align = Slot::Const(Value::Enum(align));
    f.valign = Slot::Const(Value::Enum("middle".into()));
    f.tabular = Slot::Const(Value::Bool(true));
    let n = |v: f64| Slot::Const(Value::Number(v));
    let mut ink = INK;
    match spec.preset {
        Preset::Light => {
            f.size = n(15.0);
            if label {
                f.family = Slot::Const(Value::Str("JetBrains Mono".into()));
                f.size = n(14.0);
                ink = MUTED;
            } else if header {
                ink = MUTED;
            } else {
                ink = [0.25, 0.27, 0.32, 1.0];
            }
            if emph {
                f.weight = n(700.0);
                ink = if header { ACCENT } else { INK };
            }
        }
        Preset::Boxed => {
            f.size = n(15.0);
            if header {
                f.weight = n(600.0);
            }
            if label && !header {
                f.weight = n(500.0);
            }
            if emph {
                f.weight = n(700.0);
                if header {
                    ink = ACCENT;
                }
            }
        }
    }
    (f, ink)
}

/// The `table` and `rules` facets a preset gives a table.
pub fn preset_facets(preset: Preset, columns: usize, header: usize) -> (Table, Rules) {
    let mut t = Table::default();
    t.columns = Slot::Const(Value::Str(vec!["auto"; columns.max(1)].join(" ")));
    t.header = Slot::Const(Value::Number(header as f64));
    t.preset = Slot::Const(Value::Enum(preset.name().into()));
    let mut r = Rules::default();
    let n = |v: f64| Slot::Const(Value::Number(v));
    let c = |v: [f32; 4]| Slot::Const(Value::Color(v));
    match preset {
        Preset::Light => {
            t.pad_x = n(14.0);
            t.pad_y = n(9.0);
            r.outer_width = n(0.0);
            r.header_width = n(1.0);
            r.header_color = c([0.0, 0.0, 0.0, 0.12]);
            r.rows_width = n(1.0);
            r.rows_color = c([0.0, 0.0, 0.0, 0.12]);
            r.columns_width = n(0.0);
        }
        Preset::Boxed => {
            t.pad_x = n(12.0);
            t.pad_y = n(8.0);
            t.header_fill = c([0.94, 0.95, 0.97, 1.0]);
            r.outer_width = n(1.0);
            r.outer_color = c([0.0, 0.0, 0.0, 0.3]);
            r.header_width = n(1.5);
            r.header_color = c([0.0, 0.0, 0.0, 0.3]);
            r.rows_width = n(1.0);
            r.rows_color = c([0.0, 0.0, 0.0, 0.15]);
            r.columns_width = n(1.0);
            r.columns_color = c([0.0, 0.0, 0.0, 0.15]);
        }
    }
    (t, r)
}

/// Rows written as text: one per line, cells split by `|` (`\|` is a bar),
/// each trimmed. Blank lines are skipped.
pub fn parse_rows(text: &str) -> Vec<Vec<String>> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let mut cells = vec![String::new()];
            let mut chars = l.chars().peekable();
            while let Some(c) = chars.next() {
                match c {
                    '\\' if chars.peek() == Some(&'|') => {
                        cells.last_mut().expect("one").push('|');
                        chars.next();
                    }
                    '|' => cells.push(String::new()),
                    c => cells.last_mut().expect("one").push(c),
                }
            }
            cells.into_iter().map(|c| c.trim().to_owned()).collect()
        })
        .collect()
}

/// What a table's cells say of its look (§8's restyle and the compact
/// view): its preset and header rows, a label column (monospace, or weight
/// 500 in `boxed`), an emphasised column (every single body cell bold), and
/// each column's alignment (its first single cell's).
pub fn infer(read: Read, table: NodeId, areas: &[Area], rows: usize, cols: usize) -> Spec {
    let header = (read_num(read, table, "table.header", 1.0).round().max(0.0) as usize).min(rows);
    let body = |c: usize| areas.iter().filter(move |a| a.row >= header && a.col == c && a.span_x == 1);
    let weight = |id: NodeId| read_num(read, id, "font.weight", 400.0);
    let label_col = body(0).next().is_some_and(|a| read_word(read, a.id, "font.family") == "JetBrains Mono" || weight(a.id) == 500.0);
    let emphasis = (1..cols).find(|&c| body(c).next().is_some() && body(c).all(|a| weight(a.id) >= 700.0));
    let align = (0..cols).map(|c| areas.iter().find(|a| a.col == c && a.span_x == 1).map(|a| read_word(read, a.id, "font.align")).filter(|w| !w.is_empty()).unwrap_or_else(|| "left".into())).collect();
    Spec { rows: Vec::new(), preset: Preset::parse(&read_word(read, table, "table.preset")), header, label_col, emphasis, align }
}

/// One cell's text as written: a merge marker, or its words.
enum Written {
    Left,
    Up,
    Words(String),
}

fn written(s: &str) -> Written {
    match s.trim() {
        "<" => Written::Left,
        "^" => Written::Up,
        w => Written::Words(w.replace("\\<", "<").replace("\\^", "^").replace("\\n", "\n")),
    }
}

/// Written rows as cells: the number of columns, and each cell (in reading
/// order) with its position, its spans and its content, a constant or a
/// binding for `{= expr}`. A merge must make a rectangle.
type Laid = (usize, Vec<((usize, usize), (usize, usize), Slot)>);

fn lay_out(rows_in: &[Vec<String>]) -> Result<Laid, String> {
    let columns = rows_in.iter().map(|r| r.len()).max().unwrap_or(0);
    if columns == 0 {
        return Err("a table needs at least one cell".into());
    }
    // Which cell each position belongs to: its own, or (by a marker) the
    // one it merges into.
    let rows = rows_in.len();
    let mut owner: Vec<Vec<Option<(usize, usize)>>> = vec![vec![None; columns]; rows];
    for (r, row) in rows_in.iter().enumerate() {
        for c in 0..columns {
            let cell = row.get(c).map(|s| written(s)).unwrap_or(Written::Words(String::new()));
            owner[r][c] = match cell {
                Written::Left if c > 0 => owner[r][c - 1],
                Written::Up if r > 0 => owner[r - 1][c],
                _ => Some((r, c)),
            };
        }
    }
    // A merge must make a rectangle: each owner's positions are counted.
    let mut spans: std::collections::BTreeMap<(usize, usize), (usize, usize)> = Default::default();
    for (r, row) in owner.iter().enumerate() {
        for (c, o) in row.iter().enumerate() {
            let o = o.expect("filled");
            let e = spans.entry(o).or_insert((1, 1));
            e.0 = e.0.max(c - o.1 + 1);
            e.1 = e.1.max(r - o.0 + 1);
        }
    }
    for (&(r0, c0), &(sx, sy)) in &spans {
        for r in r0..r0 + sy {
            for c in c0..c0 + sx {
                if owner[r][c] != Some((r0, c0)) {
                    return Err(format!("the merge at row {} column {} is not a rectangle", r0 + 1, c0 + 1));
                }
            }
        }
    }
    // Reading order: by row, then column (the map's own order).
    let mut cells = Vec::new();
    for (&(r, c), &span) in &spans {
        let words = match rows_in.get(r).and_then(|row| row.get(c)).map(|s| written(s)) {
            Some(Written::Words(w)) => w,
            _ => String::new(),
        };
        let slot = match words.strip_prefix("{=").and_then(|w| w.strip_suffix('}')) {
            Some(e) => Slot::Bound(crate::Expr::parse(e.trim()).map_err(|e| format!("row {} column {}: {e}", r + 1, c + 1))?),
            None => Slot::Const(Value::Str(words)),
        };
        cells.push(((r, c), span, slot));
    }
    Ok((columns, cells))
}

/// A new cell's facets: its content, its preset style, its spans.
fn cell_facets(spec: &Spec, (r, c): (usize, usize), (sx, sy): (usize, usize), content: Slot) -> Vec<crate::Facet> {
    let (font, ink) = cell_style(spec, r, c);
    let mut text = Text::default();
    text.content = content;
    let mut fill = crate::facet::Fill::default();
    fill.color = Slot::Const(Value::Color(ink));
    let mut facets = vec![Transform2d::default().wrap(), text.wrap(), font.wrap(), fill.wrap()];
    if sx > 1 || sy > 1 {
        let mut item = LayoutItem::default();
        item.span_x = Slot::Const(Value::Number(sx as f64));
        item.span_y = Slot::Const(Value::Number(sy as f64));
        facets.push(item.wrap());
    }
    facets
}

/// Rows and their cells under `table`, with ids from `first`.
fn rows_ops(spec: &Spec, table: NodeId, rows: usize, cells: Vec<((usize, usize), (usize, usize), Slot)>, ids: &[NodeId]) -> Vec<Op> {
    let row_ids = &ids[..rows];
    let mut ops = Vec::new();
    let mut key = OrderKey::first();
    for &row in row_ids {
        ops.push(Op::Create { id: row, ty: TypeId::lookup("row").expect("registered"), parent: Some(table), order: key.clone(), facets: vec![] });
        key = OrderKey::after(&key);
    }
    let mut keys: Vec<OrderKey> = vec![OrderKey::first(); rows];
    for (i, (at, span, content)) in cells.into_iter().enumerate() {
        ops.push(Op::Create { id: ids[rows + i], ty: TypeId::lookup("text").expect("registered"), parent: Some(row_ids[at.0]), order: keys[at.0].clone(), facets: cell_facets(spec, at, span, content) });
        keys[at.0] = OrderKey::after(&keys[at.0]);
    }
    ops
}

/// The ops creating a table under `parent` at `at`, with ids from
/// `doc.next_ids`, and the table's id. Merge markers become spans on the
/// cell they merge into; the positions they stood for get no node. A cell
/// written `{= expr}` is bound.
pub fn create_ops(doc: &Document, parent: Option<NodeId>, order: OrderKey, at: [f64; 2], spec: &Spec) -> Result<(Vec<Op>, NodeId), String> {
    create_ops_after(doc, 0, parent, order, at, spec)
}

/// [`create_ops`] in a transaction whose earlier ops create `taken` nodes
/// first: the table's ids follow theirs.
pub fn create_ops_after(doc: &Document, taken: usize, parent: Option<NodeId>, order: OrderKey, at: [f64; 2], spec: &Spec) -> Result<(Vec<Op>, NodeId), String> {
    let (columns, cells) = lay_out(&spec.rows)?;
    let rows = spec.rows.len();
    let ids = doc.next_ids(taken + 1 + rows + cells.len()).split_off(taken);
    let table = ids[0];
    let (t, rules) = preset_facets(spec.preset, columns, spec.header.min(rows));
    let mut place = Transform2d::default();
    place.x = Slot::Const(Value::Number(at[0]));
    place.y = Slot::Const(Value::Number(at[1]));
    let mut name = Name::default();
    name.text = Slot::Const(Value::Str("table".into()));
    let mut ops = vec![Op::Create { id: table, ty: TypeId::lookup("table").expect("registered"), parent, order, facets: vec![place.wrap(), t.wrap(), rules.wrap(), name.wrap()] }];
    ops.extend(rows_ops(spec, table, rows, cells, &ids[1..]));
    Ok((ops, table))
}

/// Replace a table's words (§5.2, `<table id="#T">`): rows as the verb
/// writes them. When the new rows have the table's shape (rows, columns,
/// merges), only cells whose content changed are set, so every cell keeps
/// its id, style and selection. Otherwise its rows are made again in the
/// look the table has now (`infer`), keeping its own leaves.
pub fn replace_ops(store: &Store, table: NodeId, rows_in: &[Vec<String>]) -> Result<Vec<Op>, String> {
    let doc = store.document();
    if doc.live(table).is_none_or(|n| n.ty.name() != "table") {
        return Err(format!("{table} is not a table"));
    }
    let (columns, cells) = lay_out(rows_in)?;
    let rows = rows_in.len();
    let (areas, have_rows) = place(store, table);
    let have_cols = column_count(store, table);
    let content: crate::LeafPath = "text.content".parse().expect("declared leaf");
    let same = rows == have_rows && columns == have_cols && cells.len() == areas.len() && cells.iter().zip(&areas).all(|((at, span, _), a)| *at == (a.row, a.col) && *span == (a.span_x, a.span_y));
    if same {
        return Ok(cells.into_iter().zip(&areas).filter(|((_, _, slot), a)| doc.leaf(a.id, content) != Some(slot)).map(|((_, _, slot), a)| Op::Set { id: a.id, path: content, slot }).collect());
    }
    let read = |n: NodeId, l: &str| crate::layout::base(store, n, l);
    let mut spec = infer(&read, table, &areas, have_rows, have_cols);
    spec.align.resize(columns, "right".into());
    let mut ops: Vec<Op> = self::rows(doc, table).into_iter().map(|id| Op::Delete { id }).collect();
    if columns != have_cols {
        let tracks = read_word(&read, table, "table.columns");
        let mut t: Vec<&str> = tracks.split_whitespace().collect();
        t.resize(columns, "auto");
        ops.push(Op::Set { id: table, path: "table.columns".parse().expect("declared leaf"), slot: Slot::Const(Value::Str(t.join(" "))) });
    }
    let ids = doc.next_ids(rows + cells.len());
    ops.extend(rows_ops(&spec, table, rows, cells, &ids));
    Ok(ops)
}

/// Order keys for new cells inserted before the cells at `at` (indices into
/// `cells`, in reading order; `cells.len()` appends), in the order given.
fn keys_at(doc: &Document, cells: &[NodeId], at: &[usize]) -> Result<Vec<OrderKey>, String> {
    let order = |i: usize| cells.get(i).and_then(|&c| doc.live(c)).map(|n| n.order.clone());
    let mut out: Vec<OrderKey> = Vec::new();
    let mut last: Option<(usize, OrderKey)> = None;
    for &i in at {
        let prev = match &last {
            Some((j, k)) if *j == i => Some(k.clone()),
            _ => i.checked_sub(1).and_then(order),
        };
        let k = OrderKey::between(prev.as_ref(), order(i).as_ref()).map_err(|e| format!("{e:?}"))?;
        last = Some((i, k.clone()));
        out.push(k);
    }
    Ok(out)
}

/// A new empty cell in `row`, styled as `like`.
fn blank_cell(doc: &Document, id: NodeId, row: NodeId, like: Option<NodeId>, order: OrderKey) -> Op {
    let mut facets = vec![Transform2d::default().wrap(), Text::default().wrap()];
    let font = like.and_then(|l| doc.facet(l, crate::FacetKind::Font).cloned()).unwrap_or_else(|| Font::default().wrap());
    facets.push(font);
    if let Some(fill) = like.and_then(|l| doc.facet(l, crate::FacetKind::Fill).cloned()) {
        facets.push(fill);
    }
    Op::Create { id, ty: TypeId::lookup("text").expect("registered"), parent: Some(row), order, facets }
}

/// Set a cell's spans, adding its `layout_item` if it has none.
fn span_ops(doc: &Document, cell: NodeId, span_x: usize, span_y: usize) -> Vec<Op> {
    let leaf = |f: &str| format!("layout_item.{f}").parse().expect("declared leaf");
    if doc.facet(cell, crate::FacetKind::LayoutItem).is_some() {
        vec![
            Op::Set { id: cell, path: leaf("span_x"), slot: Slot::Const(Value::Number(span_x as f64)) },
            Op::Set { id: cell, path: leaf("span_y"), slot: Slot::Const(Value::Number(span_y as f64)) },
        ]
    } else {
        let mut item = LayoutItem::default();
        item.span_x = Slot::Const(Value::Number(span_x as f64));
        item.span_y = Slot::Const(Value::Number(span_y as f64));
        vec![Op::AddFacet { id: cell, facet: item.wrap() }]
    }
}

/// The cell covering (`row`, `col`), if any.
fn covering(areas: &[Area], row: usize, col: usize) -> Option<&Area> {
    areas.iter().find(|a| a.row <= row && row < a.row + a.span_y && a.col <= col && col < a.col + a.span_x)
}

/// Where a new cell at column `col` of row `row` goes among that row's
/// cells: before the first that starts after it.
fn index_in_row(areas: &[Area], row: usize, col: usize) -> usize {
    areas.iter().filter(|a| a.row == row && a.col < col).count()
}

/// An edit to a table's structure (layout-and-tables.md §5.1), as one
/// transaction's ops and its label: `add_row` and `remove_row` (at the
/// bottom), `add_column` and `remove_column` (at the right), `merge` (the
/// `selected` cells, which must make a rectangle: the top-left one keeps
/// the others' words, on new lines, as PowerPoint does), `split` (the one
/// selected merged cell back into single cells).
pub fn edit_ops(store: &Store, table: NodeId, verb: &str, selected: &[NodeId]) -> Result<(Vec<Op>, String), String> {
    let doc = store.document();
    if doc.live(table).is_none_or(|n| n.ty.name() != "table") {
        return Err(format!("{table} is not a table"));
    }
    let (areas, rows) = place(store, table);
    let cols = column_count(store, table);
    let row_nodes = self::rows(doc, table);
    let columns_leaf: crate::LeafPath = "table.columns".parse().expect("declared leaf");
    let tracks = crate::layout::base(store, table, "table.columns").and_then(|v| v.as_str().map(str::to_owned)).unwrap_or_default();
    let mut ops = Vec::new();
    let label = match verb {
        "add_row" => {
            let ids = doc.next_ids(1 + cols);
            let key = keys_at(doc, &row_nodes, &[row_nodes.len()])?.remove(0);
            ops.push(Op::Create { id: ids[0], ty: TypeId::lookup("row").expect("registered"), parent: Some(table), order: key, facets: vec![] });
            let mut k = OrderKey::first();
            for c in 0..cols {
                let like = rows.checked_sub(1).and_then(|r| covering(&areas, r, c)).map(|a| a.id);
                ops.push(blank_cell(doc, ids[1 + c], ids[0], like, k.clone()));
                k = OrderKey::after(&k);
            }
            "add row"
        }
        "remove_row" => {
            if rows < 2 {
                return Err("a table keeps at least one row".into());
            }
            let last = rows - 1;
            ops.push(Op::Delete { id: row_nodes[last] });
            for a in areas.iter().filter(|a| a.row < last && a.row + a.span_y > last) {
                ops.extend(span_ops(doc, a.id, a.span_x, a.span_y - 1));
            }
            "remove row"
        }
        "add_column" => {
            let mut t: Vec<&str> = tracks.split_whitespace().collect();
            t.resize(cols, "auto");
            t.push("auto");
            ops.push(Op::Set { id: table, path: columns_leaf, slot: Slot::Const(Value::Str(t.join(" "))) });
            // One cell at the end of each row: the new column is free in all.
            let ids = doc.next_ids(rows);
            for (r, &row) in row_nodes.iter().enumerate() {
                let here = row_cells(doc, row);
                let key = keys_at(doc, &here, &[here.len()])?.remove(0);
                let like = covering(&areas, r, cols - 1).map(|a| a.id);
                ops.push(blank_cell(doc, ids[r], row, like, key));
            }
            "add column"
        }
        "remove_column" => {
            if cols < 2 {
                return Err("a table keeps at least one column".into());
            }
            let last = cols - 1;
            let mut t: Vec<&str> = tracks.split_whitespace().collect();
            t.resize(cols, "auto");
            t.pop();
            ops.push(Op::Set { id: table, path: columns_leaf, slot: Slot::Const(Value::Str(t.join(" "))) });
            for a in &areas {
                if a.col == last {
                    ops.push(Op::Delete { id: a.id });
                } else if a.col + a.span_x > last {
                    ops.extend(span_ops(doc, a.id, a.span_x - 1, a.span_y));
                }
            }
            "remove column"
        }
        "merge" => {
            let picked: Vec<&Area> = areas.iter().filter(|a| selected.contains(&a.id)).collect();
            if picked.len() < 2 {
                return Err("select two or more cells of one table to merge".into());
            }
            let (r0, c0) = (picked.iter().map(|a| a.row).min().expect("some"), picked.iter().map(|a| a.col).min().expect("some"));
            let (r1, c1) = (picked.iter().map(|a| a.row + a.span_y).max().expect("some"), picked.iter().map(|a| a.col + a.span_x).max().expect("some"));
            let inside = |a: &Area| a.row >= r0 && a.row + a.span_y <= r1 && a.col >= c0 && a.col + a.span_x <= c1;
            let touching = |a: &Area| a.row < r1 && r0 < a.row + a.span_y && a.col < c1 && c0 < a.col + a.span_x;
            let region: Vec<&Area> = areas.iter().filter(|a| touching(a)).collect();
            if region.iter().any(|a| !inside(a)) {
                return Err("those cells do not make a rectangle with the cells around them".into());
            }
            let keep = region[0];
            let words: Vec<String> = region.iter().map(|a| crate::text::content(store, a.id)).filter(|w| !w.trim().is_empty()).collect();
            let content: crate::LeafPath = "text.content".parse().expect("declared leaf");
            ops.push(Op::Set { id: keep.id, path: content, slot: Slot::Const(Value::Str(words.join("\n"))) });
            ops.extend(span_ops(doc, keep.id, c1 - c0, r1 - r0));
            for a in &region[1..] {
                ops.push(Op::Delete { id: a.id });
            }
            "merge cells"
        }
        "split" => {
            let [cell] = selected else { return Err("select one merged cell to split".into()) };
            let a = *areas.iter().find(|a| a.id == *cell).ok_or("that cell is not in this table")?;
            if a.span_x == 1 && a.span_y == 1 {
                return Err("that cell is not merged".into());
            }
            ops.extend(span_ops(doc, a.id, 1, 1));
            // New cells in each row the merge covered, where it stood.
            let ids = doc.next_ids(a.span_x * a.span_y - 1);
            let mut next = ids.into_iter();
            for r in a.row..a.row + a.span_y {
                let here = row_cells(doc, row_nodes[r]);
                let first = if r == a.row { a.col + 1 } else { a.col };
                let count = a.col + a.span_x - first;
                let at = index_in_row(&areas, r, first);
                let keys = keys_at(doc, &here, &vec![at; count])?;
                for k in keys {
                    ops.push(blank_cell(doc, next.next().expect("counted"), row_nodes[r], Some(a.id), k));
                }
            }
            "split cell"
        }
        restyle if restyle.starts_with("preset:") => {
            let preset = Preset::parse(&restyle["preset:".len()..]);
            let mut spec = infer(&|n, l| crate::layout::base(store, n, l), table, &areas, rows, cols);
            spec.preset = preset;
            let header = spec.header;
            let (t, rules) = preset_facets(preset, cols, header);
            for f in ["pad_x", "pad_y", "header_fill", "zebra_fill", "preset"] {
                let path: crate::LeafPath = format!("table.{f}").parse().expect("declared leaf");
                ops.push(Op::Set { id: table, path, slot: t.get(f).expect("declared").clone() });
            }
            if doc.facet(table, crate::FacetKind::Rules).is_some() {
                ops.push(Op::RemoveFacet { id: table, kind: crate::FacetKind::Rules });
            }
            ops.push(Op::AddFacet { id: table, facet: rules.wrap() });
            for a in &areas {
                let (font, ink) = cell_style(&spec, a.row, a.col);
                for (f, _) in <Font as crate::LeafStruct>::FIELDS {
                    let path: crate::LeafPath = format!("font.{f}").parse().expect("declared leaf");
                    ops.push(Op::Set { id: a.id, path, slot: font.get(f).expect("declared").clone() });
                }
                let path: crate::LeafPath = "fill.color".parse().expect("declared leaf");
                ops.push(Op::Set { id: a.id, path, slot: Slot::Const(Value::Color(ink)) });
            }
            "restyle table"
        }
        other => return Err(format!("no table edit {other:?}")),
    };
    Ok((ops, label.to_owned()))
}

/// A cell's words as the compact form writes them: `|` and a lone `<` or
/// `^` escaped, line breaks as `\n`.
fn write_cell(words: &str) -> String {
    match words {
        "<" => "\\<".into(),
        "^" => "\\^".into(),
        w => w.replace('|', "\\|").replace('\n', "\\n"),
    }
}

/// A table's rows as the `<table>` verb writes them: each cell's words (a
/// bound one as `{= expr}`), `<` and `^` where a merge covers a position.
fn written_grid(read: Read, bound: &dyn Fn(NodeId) -> Option<String>, areas: &[Area], rows: usize, cols: usize) -> Vec<Vec<String>> {
    let mut grid = vec![vec![String::new(); cols]; rows];
    for a in areas {
        let words = match bound(a.id) {
            Some(e) => format!("{{= {e}}}"),
            None => write_cell(&read(a.id, "text.content").map(|v| crate::text::display(&v)).unwrap_or_default()),
        };
        for r in a.row..(a.row + a.span_y).min(rows) {
            for c in a.col..(a.col + a.span_x).min(cols) {
                grid[r][c] = if (r, c) == (a.row, a.col) { words.clone() } else if r == a.row { "<".into() } else { "^".into() };
            }
        }
    }
    grid
}

/// Tab-separated text (a spreadsheet's copy) as written rows: one per line,
/// cells split by tabs, each escaped so nothing in it reads as a marker.
pub fn from_tsv(text: &str) -> Vec<Vec<String>> {
    text.replace("\r\n", "\n").lines().filter(|l| !l.trim().is_empty()).map(|l| l.split('\t').map(|c| write_cell(c.trim())).collect()).collect()
}

/// Paste rows into a table (§5.1): from the selected cell (`at`, or the
/// top-left), over what is there, the table growing to fit. A merge the
/// pasted block lands on is undone where words land on it.
pub fn paste_ops(store: &Store, table: NodeId, at: Option<NodeId>, rows_in: &[Vec<String>]) -> Result<Vec<Op>, String> {
    let doc = store.document();
    let read = |n: NodeId, l: &str| crate::layout::base(store, n, l);
    let bound = |cell: NodeId| doc.leaf(cell, "text.content".parse().expect("declared leaf")).and_then(|s| s.expr()).map(|e| e.to_string());
    let (areas, rows) = place(store, table);
    let cols = column_count(store, table);
    let mut grid = written_grid(&read, &bound, &areas, rows, cols);
    let (r0, c0) = at.and_then(|c| areas.iter().find(|a| a.id == c)).map_or((0, 0), |a| (a.row, a.col));
    let width = (c0 + rows_in.iter().map(|r| r.len()).max().unwrap_or(0)).max(cols);
    for line in &mut grid {
        line.resize(width, String::new());
    }
    for (dr, line) in rows_in.iter().enumerate() {
        if grid.len() <= r0 + dr {
            grid.push(vec![String::new(); width]);
        }
        for (dc, words) in line.iter().enumerate() {
            grid[r0 + dr][c0 + dc] = words.clone();
        }
    }
    // A merge whose own cell was written over leaves stray markers: they
    // become empty cells of their own.
    if lay_out(&grid).is_err() {
        for w in grid.iter_mut().flatten() {
            if w == "<" || w == "^" {
                w.clear();
            }
        }
    }
    replace_ops(store, table, &grid)
}

/// A table of `rows` as a paste fragment, centred on `centre`, in the light
/// preset with a header row (§5.1: a spreadsheet's cells pasted on the
/// canvas).
pub fn fragment(rows: Vec<Vec<String>>, centre: [f64; 2]) -> Result<crate::copy::Fragment, String> {
    use crate::{Actor, Affine2, TransactionInput};
    let mut store = Store::new();
    let spec = Spec { rows, preset: Preset::Light, header: 1, label_col: false, emphasis: None, align: Vec::new() };
    let (ops, table) = create_ops(store.document(), None, OrderKey::first(), [0.0, 0.0], &spec)?;
    let tx = |ops| TransactionInput { label: "paste table".into(), actor: Actor::System, ts: 0, idempotency_key: None, ops, exact: false };
    store.apply(tx(ops)).map_err(|e| format!("the table did not apply: {e:?}"))?;
    // Laid out now: centre it by its size.
    let size = |l: &str| store.resolved(table, l.parse().expect("declared leaf")).and_then(|v| v.as_f64()).unwrap_or(0.0);
    let (w, h) = (size("size.w"), size("size.h"));
    let place = [("transform2d.x", (centre[0] - w * 0.5).round()), ("transform2d.y", (centre[1] - h * 0.5).round())];
    let ops = place.iter().map(|(l, v)| Op::Set { id: table, path: l.parse().expect("declared leaf"), slot: Slot::Const(Value::Number(*v)) }).collect();
    store.apply(tx(ops)).map_err(|e| format!("the table did not apply: {e:?}"))?;
    Ok(crate::copy::Fragment {
        doc: store.document().clone(),
        roots: vec![table],
        extras: Vec::new(),
        homes: std::collections::BTreeMap::from([(table, (None, OrderKey::first()))]),
        parent_world: std::collections::BTreeMap::from([(table, Affine2::IDENTITY)]),
        externals: std::collections::BTreeMap::new(),
        origin: "table".into(),
        dropped: Vec::new(),
    })
}

/// The compact form a model reads a table in (layout-and-tables.md §5.2):
/// attributes for the table's element, and the lines inside it. The rows
/// as the `<table>` verb writes them (a bound cell as `{= expr}`); the
/// cells' ids in the same shape; what differs from the preset, the table's
/// leaves as attributes and each cell's as a `<style>` line. `bound` gives a
/// cell's content binding, if it has one.
pub fn compact(doc: &Document, read: Read, bound: &dyn Fn(NodeId) -> Option<String>, table: NodeId) -> (Vec<(String, String)>, Vec<String>) {
    let (areas, rows) = place_with(doc, read, table);
    let cols = column_count_with(read, table).max(areas.iter().map(|a| a.col + a.span_x).max().unwrap_or(0));
    let spec = infer(read, table, &areas, rows, cols);
    let mut attrs: Vec<(String, String)> = vec![("preset".into(), spec.preset.name().into()), ("header".into(), spec.header.to_string()), ("columns".into(), read_word(read, table, "table.columns"))];
    if spec.label_col {
        attrs.push(("label-col".into(), "true".into()));
    }
    if let Some(e) = spec.emphasis {
        attrs.push(("emphasis".into(), (e + 1).to_string()));
    }
    let default_align: Vec<String> = (0..cols).map(|c| if spec.label_col && c == 0 { "left".into() } else { "right".into() }).collect();
    if spec.align != default_align {
        attrs.push(("align".into(), spec.align.join(" ")));
    }
    // The table's own leaves, where they differ from its preset's.
    let (t, r) = preset_facets(spec.preset, cols, spec.header);
    let differs = |leaf: &str, want: Option<&Value>| read(table, leaf).as_ref() != want;
    for f in ["pad_x", "pad_y", "width", "header_fill", "zebra_fill"] {
        let leaf = format!("table.{f}");
        if differs(&leaf, t.get(f).and_then(|s| s.constant())) {
            attrs.push((f.into(), read(table, &leaf).map(|v| v.to_string()).unwrap_or_default()));
        }
    }
    if doc.facet(table, crate::FacetKind::Rules).is_none() {
        attrs.push(("rules".into(), "none".into()));
    } else {
        for (f, _) in <Rules as LeafStruct>::FIELDS {
            let leaf = format!("rules.{f}");
            if differs(&leaf, r.get(f).and_then(|s| s.constant())) {
                attrs.push((leaf.clone(), read(table, &leaf).map(|v| v.to_string()).unwrap_or_default()));
            }
        }
    }
    // The grid, merged positions marked, columns padded to line up.
    let grid = written_grid(read, bound, &areas, rows, cols);
    let widths: Vec<usize> = (0..cols).map(|c| grid.iter().map(|row| row[c].chars().count()).max().unwrap_or(0)).collect();
    let mut lines: Vec<String> = grid
        .iter()
        .map(|row| row.iter().zip(&widths).enumerate().map(|(c, (w, &n))| if c + 1 == cols { w.clone() } else { format!("{w:n$}") }).collect::<Vec<_>>().join(" | ").trim_end().to_owned())
        .collect();
    let ids: Vec<String> = (0..rows).map(|r| areas.iter().filter(|a| a.row == r).map(|a| a.id.to_string()).collect::<Vec<_>>().join(" ")).collect();
    lines.push(format!("<cells>{}</cells>", ids.join(" / ")));
    // Cells styled other than the preset styles them.
    for a in &areas {
        let (font, ink) = cell_style(&spec, a.row, a.col);
        let mut diffs: Vec<String> = Vec::new();
        for (f, _) in <Font as LeafStruct>::FIELDS {
            let have = read(a.id, &format!("font.{f}"));
            if have.as_ref() != font.get(f).and_then(|s| s.constant()) {
                diffs.push(format!("{f}=\"{}\"", have.map(|v| v.to_string()).unwrap_or_default()));
            }
        }
        let color = read(a.id, "fill.color");
        if color != Some(Value::Color(ink)) {
            diffs.push(format!("color=\"{}\"", color.map(|v| v.to_string()).unwrap_or_else(|| "none".into())));
        }
        if !diffs.is_empty() {
            lines.push(format!("<style cell=\"{}\" {}/>", a.id, diffs.join(" ")));
        }
    }
    (attrs, lines)
}
