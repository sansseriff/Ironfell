//! Tables (plans/specs/layout-and-tables.md §7, phase 2): the reference
//! table laid out from its cells, merged cells and the lines drawn around
//! them, each preset's lines, and a bound cell relaying the table.

use iron_document::table::{self, Preset, Spec};
use iron_document::*;
use std::sync::OnceLock;

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn path(s: &str) -> LeafPath {
    s.parse().unwrap()
}

fn fonts() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        for f in ["Inter.ttf", "JetBrainsMono.ttf"] {
            let bytes = std::fs::read(format!("{}/../../public/fonts/{f}", env!("CARGO_MANIFEST_DIR"))).unwrap();
            iron_text::engine().register(bytes);
        }
    });
}

fn num(store: &Store, id: NodeId, leaf: &str) -> f64 {
    store.resolved(id, path(leaf)).and_then(|v| v.as_f64()).unwrap()
}

fn rows(text: &str) -> Vec<Vec<String>> {
    text.lines().map(|l| l.split('|').map(|c| c.trim().to_owned()).collect()).collect()
}

fn make(store: &mut Store, text: &str, preset: Preset) -> NodeId {
    let spec = Spec { rows: rows(text), preset, header: 1, label_col: true, emphasis: Some(2), align: vec![] };
    let (ops, t) = table::create_ops(store.document(), None, OrderKey::first(), [40.0, 30.0], &spec).unwrap();
    store.apply(tx("add table", ops)).unwrap();
    t
}

const REFERENCE: &str = "PI per side (µm) | 75 | 100 | 125 | 150 | 200
S (µm) | 85 | 113 | 140 | 163 | 198
Gap W (µm) | 153 | 138 | 125 | 113 | 96
Loss 2 GHz (dB) | 1.10 | 0.89 | 0.79 | 0.74 | 0.74
W / 1000 lines | 0.40 | 0.40 | 0.41 | 0.42 | 0.43";

#[test]
fn the_reference_table_lays_out_from_its_cells() {
    fonts();
    let mut store = Store::new();
    let t = make(&mut store, REFERENCE, Preset::Light);
    let cells = table::cells(store.document(), t);
    assert_eq!(cells.len(), 30);
    let grid = store.table_grid(t).unwrap().clone();
    assert_eq!((grid.columns.len(), grid.rows.len()), (6, 5));
    // The label column is the widest: its longest label plus the padding.
    let label = grid.columns[0].1;
    assert!(grid.columns[1..].iter().all(|c| c.1 < label), "{:?}", grid.columns);
    // Tracks tile the table, from its top-left.
    assert_eq!(grid.columns[0].0, 0.0);
    let (lx, lw) = *grid.columns.last().unwrap();
    assert!((num(&store, t, "size.w") - (lx + lw)).abs() < 1e-6);
    // Each cell's box is its whole cell: fixed, at its track.
    let c = cells[2 * 6 + 3]; // row 3, column 4: "125"
    assert_eq!(store.resolved(c, path("text.sizing")), Some(Value::Enum("fixed".into())));
    assert_eq!(num(&store, c, "transform2d.x"), grid.columns[3].0);
    assert_eq!(num(&store, c, "transform2d.y"), grid.rows[2].0);
    assert_eq!(num(&store, c, "size.w"), grid.columns[3].1);
    let b = store.local_bounds(c).unwrap().geometry;
    assert_eq!((b.width(), b.height()), (grid.columns[3].1, grid.rows[2].1));
    // Light: a hairline under every row but the last, no verticals, no box.
    let d = table::decor(&store, t, &grid);
    assert_eq!(d.lines.len(), 4);
    assert!(d.lines.iter().all(|(l, w, _)| l.p0.y == l.p1.y && *w == 1.0 && l.p0.x == 0.0 && (l.p1.x - (lx + lw)).abs() < 1e-6));
    // One transaction: one undo removes it all.
    store.undo(0).unwrap().unwrap();
    assert!(!store.document().is_live(t));
}

#[test]
fn boxed_draws_every_line() {
    fonts();
    let mut store = Store::new();
    let t = make(&mut store, "a | b | c\n1 | 2 | 3\n4 | 5 | 6", Preset::Boxed);
    let grid = store.table_grid(t).unwrap().clone();
    let d = table::decor(&store, t, &grid);
    // Two row boundaries, two column boundaries, four sides; and the header band.
    assert_eq!(d.lines.len(), 8);
    assert_eq!(d.bands.len(), 1);
    assert_eq!(d.bands[0].0.y1, grid.rows[0].1);
}

#[test]
fn merged_cells_span_and_lines_go_around_them() {
    fonts();
    let mut store = Store::new();
    // A header spanning columns 2–3, and a label spanning two rows.
    let t = make(&mut store, "x | wide | <\nlabel | 1 | 2\n^ | 3 | 4", Preset::Boxed);
    let cells = table::cells(store.document(), t);
    assert_eq!(cells.len(), 7);
    let grid = store.table_grid(t).unwrap().clone();
    let area = |id: NodeId| *grid.cells.iter().find(|a| a.id == id).unwrap();
    let wide = area(cells[1]);
    assert_eq!((wide.col, wide.row, wide.span_x, wide.span_y), (1, 0, 2, 1));
    let label = area(cells[2]);
    assert_eq!((label.col, label.row, label.span_x, label.span_y), (0, 1, 1, 2));
    // "3" flows past the label's second row into column 2.
    assert_eq!((area(cells[5]).col, area(cells[5]).row), (1, 2));
    // The merged cell's box spans both columns.
    assert_eq!(num(&store, cells[1], "size.w"), grid.columns[1].1 + grid.columns[2].1);
    let d = table::decor(&store, t, &grid);
    let (y2, x2) = (grid.rows[2].0, grid.columns[2].0);
    // The line between rows 2 and 3 stops short of the label column.
    let h = d.lines.iter().find(|(l, _, _)| l.p0.y == y2 && l.p1.y == y2).unwrap().0;
    assert_eq!(h.p0.x, grid.columns[1].0);
    // The line between columns 2 and 3 starts below the wide header.
    let v = d.lines.iter().find(|(l, _, _)| l.p0.x == x2 && l.p1.x == x2).unwrap().0;
    assert_eq!(v.p0.y, grid.rows[1].0);
    // A merge that is not a rectangle is refused.
    let spec = Spec { rows: rows("a | b\n^ | <"), preset: Preset::Light, header: 1, label_col: false, emphasis: None, align: vec![] };
    assert!(table::create_ops(store.document(), None, OrderKey::first(), [0.0, 0.0], &spec).is_err());
}

#[test]
fn a_bound_cell_relays_the_table() {
    fonts();
    let mut store = Store::new();
    let t = make(&mut store, "name | value\nlaser | 1", Preset::Light);
    let cell = table::cells(store.document(), t)[3];
    let w = num(&store, t, "size.w");
    let e = Expr::parse("\"a much longer reading than before\"").unwrap();
    store.apply(tx("bind", vec![Op::Set { id: cell, path: path("text.content"), slot: Slot::Bound(e) }])).unwrap();
    assert!(num(&store, t, "size.w") > w + 100.0);
}

fn edit(store: &mut Store, t: NodeId, verb: &str, cells: &[NodeId]) {
    let (ops, label) = table::edit_ops(store, t, verb, cells).unwrap();
    store.apply(tx(&label, ops)).unwrap();
}

fn shape(store: &Store, t: NodeId) -> (usize, usize, usize) {
    let g = store.table_grid(t).unwrap();
    (g.columns.len(), g.rows.len(), table::cells(store.document(), t).len())
}

#[test]
fn rows_and_columns_are_added_and_removed_in_one_step_each() {
    fonts();
    let mut store = Store::new();
    let t = make(&mut store, "a | b | c\n1 | 2 | 3\n4 | 5 | 6", Preset::Light);
    edit(&mut store, t, "add_row", &[]);
    assert_eq!(shape(&store, t), (3, 4, 12));
    // A new empty row still has a line's height.
    let g = store.table_grid(t).unwrap();
    assert!((g.rows[3].1 - g.rows[2].1).abs() < 1.0, "{:?}", g.rows);
    edit(&mut store, t, "add_column", &[]);
    assert_eq!(shape(&store, t), (4, 4, 16));
    // The new column's cells sit at the end of each row.
    let cells = table::cells(store.document(), t);
    let area = |id: NodeId| *store.table_grid(t).unwrap().cells.iter().find(|a| a.id == id).unwrap();
    assert_eq!(table::cells(store.document(), t).iter().filter(|&&c| area(c).col == 3).count(), 4);
    assert_eq!((area(cells[1]).row, area(cells[1]).col), (0, 1));
    edit(&mut store, t, "remove_column", &[]);
    edit(&mut store, t, "remove_row", &[]);
    assert_eq!(shape(&store, t), (3, 3, 9));
    store.undo(0).unwrap().unwrap();
    store.undo(0).unwrap().unwrap();
    assert_eq!(shape(&store, t), (4, 4, 16));
}

#[test]
fn merge_keeps_the_words_and_split_restores_the_cells() {
    fonts();
    let mut store = Store::new();
    let t = make(&mut store, "a | b | c\n1 | 2 | 3\n4 | 5 | 6", Preset::Light);
    let cells = table::cells(store.document(), t);
    // "2", "3", "5", "6": a 2 × 2 block.
    edit(&mut store, t, "merge", &[cells[4], cells[8]]);
    assert_eq!(shape(&store, t), (3, 3, 6));
    assert_eq!(store.resolved(cells[4], path("text.content")), Some(Value::Str("2\n3\n5\n6".into())));
    let a = *store.table_grid(t).unwrap().cells.iter().find(|a| a.id == cells[4]).unwrap();
    assert_eq!((a.span_x, a.span_y), (2, 2));
    // "b" with the block makes a rectangle; "b" with "1" cuts through it.
    assert!(table::edit_ops(&store, t, "merge", &[cells[1], cells[4]]).is_ok());
    assert!(table::edit_ops(&store, t, "merge", &[cells[1], cells[3]]).is_err());
    edit(&mut store, t, "split", &[cells[4]]);
    assert_eq!(shape(&store, t), (3, 3, 9));
    // The new cells fill the block in reading order: row 2 then row 3.
    let g = store.table_grid(t).unwrap().clone();
    let now = table::cells(store.document(), t);
    let pos: Vec<(usize, usize)> = now.iter().map(|&c| { let a = g.cells.iter().find(|a| a.id == c).unwrap(); (a.row, a.col) }).collect();
    assert_eq!(pos, vec![(0, 0), (0, 1), (0, 2), (1, 0), (1, 1), (1, 2), (2, 0), (2, 1), (2, 2)]);
    store.undo(0).unwrap().unwrap();
    store.undo(0).unwrap().unwrap();
    assert_eq!(store.resolved(cells[4], path("text.content")), Some(Value::Str("2".into())));
}

#[test]
fn a_preset_restyles_the_table_and_keeps_its_emphasis() {
    fonts();
    let mut store = Store::new();
    let t = make(&mut store, REFERENCE, Preset::Light);
    let cells = table::cells(store.document(), t);
    edit(&mut store, t, "preset:boxed", &[]);
    let grid = store.table_grid(t).unwrap().clone();
    // Boxed: every row and column boundary and the four sides.
    assert_eq!(table::decor(&store, t, &grid).lines.len(), 4 + 5 + 4);
    // Column 3 ("100") stays emphasised; the label column loses the mono face.
    assert_eq!(num(&store, cells[6 + 2], "font.weight"), 700.0);
    assert_eq!(num(&store, cells[6 + 3], "font.weight"), 400.0);
    assert_eq!(store.resolved(cells[6], path("font.family")), Some(Value::Str("Inter".into())));
    assert_eq!(num(&store, cells[6], "font.weight"), 500.0);
    // And back: the label column is found again by its weight.
    edit(&mut store, t, "preset:light", &[]);
    assert_eq!(store.resolved(cells[6], path("font.family")), Some(Value::Str("JetBrains Mono".into())));
    store.undo(0).unwrap().unwrap();
    store.undo(0).unwrap().unwrap();
    assert_eq!(table::decor(&store, t, store.table_grid(t).unwrap()).lines.len(), 4);
}

#[test]
fn deleting_a_cell_disturbs_only_its_row() {
    fonts();
    let mut store = Store::new();
    let t = make(&mut store, "a | b | c\n1 | 2 | 3\n4 | 5 | 6", Preset::Light);
    let cells = table::cells(store.document(), t);
    store.apply(tx("delete", vec![Op::Delete { id: cells[4] }])).unwrap();
    let g = store.table_grid(t).unwrap().clone();
    let at = |id: NodeId| g.cells.iter().find(|a| a.id == id).map(|a| (a.row, a.col)).unwrap();
    // "3" closes up into the gap; "4", "5", "6" stay in row 3.
    assert_eq!(at(cells[5]), (1, 1));
    assert_eq!([at(cells[6]), at(cells[7]), at(cells[8])], [(2, 0), (2, 1), (2, 2)]);
    assert_eq!(table::rows(store.document(), t).len(), 3);
}

fn view(store: &Store) -> String {
    store.view(&ViewQuery { relation: ViewRelation::Tree, scope: None, fidelity: Fidelity::Full, depth: None, expand: Vec::new() }).unwrap()
}

#[test]
fn a_model_reads_a_table_as_its_rows() {
    fonts();
    let mut store = Store::new();
    let t = make(&mut store, REFERENCE, Preset::Light);
    let cells = table::cells(store.document(), t);
    // One cell restyled by hand, one merged pair, one bound cell.
    store.apply(tx("red", vec![Op::Set { id: cells[9], path: path("fill.color"), slot: Slot::Const(Value::Color([0.8, 0.1, 0.1, 1.0])) }])).unwrap();
    let e = Expr::parse("fmt(0.5, \"dB\", 2)").unwrap();
    store.apply(tx("bind", vec![Op::Set { id: cells[22], path: path("text.content"), slot: Slot::Bound(e) }])).unwrap();
    edit(&mut store, t, "merge", &[cells[28], cells[29]]);
    let v = view(&store);
    println!("{v}");
    let squeezed: Vec<String> = v.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
    let body: Vec<&str> = squeezed.iter().map(String::as_str).collect();
    assert!(body.iter().any(|l| l.starts_with(&format!("<table id=\"{t}\" name=\"table\" x=\"40\" y=\"30\"")) && l.contains("preset=\"light\" header=\"1\" columns=\"auto auto auto auto auto auto\" label-col=\"true\" emphasis=\"3\"")), "{v}");
    assert!(body.contains(&"PI per side (µm) | 75 | 100 | 125 | 150 | 200"), "{v}");
    assert!(body.contains(&"Loss 2 GHz (dB) | 1.10 | 0.89 | 0.79 | {= fmt(0.5, \"dB\", 2)} | 0.74"), "{v}");
    assert!(body.contains(&"W / 1000 lines | 0.40 | 0.40 | 0.41 | 0.42\\n0.43 | <"), "{v}");
    assert!(body.iter().any(|l| l.starts_with("<cells>") && l.contains(" / ")));
    assert!(body.contains(&format!("<style cell=\"{}\" color=\"#cc1a1aff\"/>", cells[9]).as_str()), "{v}");
    // No cell element is shown: the rows are the table.
    assert!(!v.contains("<text "));
}

#[test]
fn replacing_the_words_keeps_ids_when_the_shape_holds() {
    fonts();
    let mut store = Store::new();
    let t = make(&mut store, "a | b | c\n1 | 2 | 3", Preset::Light);
    let before = table::cells(store.document(), t);
    // Same shape: one cell changes, one is bound, and nothing else is touched.
    let ops = table::replace_ops(&store, t, &table::parse_rows("a | b | c\n1 | 20 | {= fmt(3, 1)}")).unwrap();
    assert_eq!(ops.len(), 2);
    store.apply(tx("replace", ops)).unwrap();
    assert_eq!(table::cells(store.document(), t), before);
    assert_eq!(store.resolved(before[4], path("text.content")), Some(Value::Str("20".into())));
    assert_eq!(store.resolved(before[5], path("text.content")), Some(Value::Str("3.0".into())));
    // A new shape: rows made again, in the same look, with a merge.
    let ops = table::replace_ops(&store, t, &table::parse_rows("a | wide | <\n1 | 2 | 3\n4 | 5 | 6")).unwrap();
    store.apply(tx("replace", ops)).unwrap();
    let g = store.table_grid(t).unwrap();
    assert_eq!((g.columns.len(), g.rows.len(), g.cells.len()), (3, 3, 8));
    let first = table::cells(store.document(), t)[0];
    assert_eq!(store.resolved(first, path("font.family")), Some(Value::Str("JetBrains Mono".into())));
    // One undo puts the old rows back.
    store.undo(0).unwrap().unwrap();
    assert_eq!(table::cells(store.document(), t), before);
}

#[test]
fn spreadsheet_cells_paste_into_a_table_from_the_selected_cell() {
    fonts();
    let mut store = Store::new();
    let t = make(&mut store, "a | b | c\n1 | 2 | 3", Preset::Light);
    let cells = table::cells(store.document(), t);
    // A 2 × 2 block from "2": overwrites 2, 3, and grows a row and a column.
    let rows = table::from_tsv("x\ty\tz\r\n<\t^\tw\r\n");
    let ops = table::paste_ops(&store, t, Some(cells[4]), &rows).unwrap();
    store.apply(tx("paste", ops)).unwrap();
    let g = store.table_grid(t).unwrap();
    assert_eq!((g.columns.len(), g.rows.len()), (4, 3));
    let words: Vec<String> = table::cells(store.document(), t).iter().map(|&c| store.resolved(c, path("text.content")).map(|v| text::display(&v)).unwrap()).collect();
    // The pasted "<" and "^" are words, not merges.
    assert_eq!(words, ["a", "b", "c", "", "1", "x", "y", "z", "", "<", "^", "w"]);
    // Pasted on the canvas: a table centred where it lands.
    let frag = table::fragment(rows, [300.0, 200.0]).unwrap();
    assert_eq!(frag.roots.len(), 1);
}
