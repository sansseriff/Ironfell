//! Contexts (plans/specs/contexts.md, phase F) and the selection they drive
//! (affordances.md phase A1): the built-in library parses and prints back,
//! bad markup names what is wrong, and resolution is local.

use iron_document::contexts::{self, Field, Focus, Library, Resolver};
use iron_document::table::{self, Preset, Spec};
use iron_document::*;

fn tx(ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: "t".into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn table(store: &mut Store) -> NodeId {
    table_in(store, None)
}

fn table_in(store: &mut Store, parent: Option<NodeId>) -> NodeId {
    let rows = vec![vec!["a".to_owned(), "b".to_owned()], vec!["c".to_owned(), "d".to_owned()]];
    let spec = Spec { rows, preset: Preset::Light, header: 1, label_col: false, emphasis: None, align: vec![] };
    let (ops, t) = table::create_ops(store.document(), parent, OrderKey::first(), [0.0, 0.0], &spec).unwrap();
    store.apply(tx(ops)).unwrap();
    t
}

#[test]
fn the_builtin_library_prints_back_to_itself() {
    let lib = Library::builtin();
    assert!(lib.get("std:free-2d").is_some() && lib.get("std:table").is_some());
    let printed = lib.print();
    let again = Library { contexts: contexts::parse(&printed).unwrap() };
    assert_eq!(&again, lib);
    assert!(printed.contains("<rule match=\"type:row\" affords.select=\"through\"/>"), "{printed}");
}

#[test]
fn bad_markup_says_what_is_wrong() {
    let err = |s: &str| contexts::parse(s).unwrap_err();
    assert_eq!(err(r#"<context name="k"><rule match="all" affords.cursor="move"/></context>"#), "unknown field affords.cursor (affords.select, affords.enter, affords.edit, affords.grips, affords.drag)");
    assert!(err(r#"<context name="k"><rule match="all" sound.x="1"/></context>"#).starts_with("unknown domain in sound.x"));
    assert_eq!(err(r#"<context name="k"><rule match="all" affords.select="maybe"/></context>"#), "affords.select=\"maybe\": expected one of none, target, through, closed, window, inert");
    assert_eq!(err(r#"<context name="k"><rule match="type:widget" affords.select="none"/></context>"#), "unknown node type widget");
    assert_eq!(err(r#"<context name="k"><rule match="nearby"/></context>"#), "match=\"nearby\": unknown word nearby (all, self, children, view, type:<t>, state:<s>)");
    assert_eq!(err(r#"<context name="k">"#), "<context name=\"k\"> is not closed");
}

#[test]
fn a_tables_rows_are_passed_through_to_its_cells() {
    let mut store = Store::new();
    let t = table(&mut store);
    let doc = store.document();
    let row = doc.children(t)[1];
    let cell = doc.children(row)[0];
    let chain = [t, row, cell];
    let r = Resolver::new(doc, Library::builtin());
    assert_eq!(r.get(&chain, 0, Field::SELECT), "target");
    assert_eq!(r.get(&chain, 1, Field::SELECT), "through");
    assert_eq!(r.get(&chain, 2, Field::SELECT), "target");
    assert_eq!(r.get(&chain, 2, Field::EDIT), "text");
    // A press selects the table; with the table selected, its cell.
    assert_eq!(interaction::target(&r, &chain, &|_| false, false), 0);
    assert_eq!(interaction::target(&r, &chain, &|i| i == 0, false), 2);
    // A double-click on the table enters past the row; Esc leaves past it.
    assert_eq!(interaction::enter(&r, &chain, 0), 2);
    assert_eq!(interaction::up(&r, &chain), 1);
    // The computed view names each value's rule.
    let computed = r.computed(&chain, 1);
    assert!(computed.contains(&("affords.select".into(), "through".into(), "std:table rule 1".into())), "{computed:?}");
    assert!(computed.contains(&("chrome.lit".into(), "shape".into(), "default".into())), "{computed:?}");
}

#[test]
fn a_row_outside_a_table_is_not_passed_through() {
    // Locality: `through` comes from the table's context, so a row whose
    // chain does not reach it by its table is an ordinary node.
    let mut store = Store::new();
    let t = table(&mut store);
    let doc = store.document();
    let row = doc.children(t)[0];
    let r = Resolver::new(doc, Library::builtin());
    assert_eq!(r.get(&[row], 0, Field::SELECT), "none");
}

const CELLS_EDIT: &str = r#"<context name="std:table">
  <rule match="type:text" affords.enter="edit"/>
</context>"#;

#[test]
fn a_document_context_overrides_a_builtin_one_and_says_so() {
    let mut store = Store::new();
    let t = table(&mut store);
    let (row, cell) = {
        let doc = store.document();
        let row = doc.children(t)[1];
        (row, doc.children(row)[0])
    };
    let chain = [t, row, cell];
    let enter = |store: &Store| Resolver::new(store.document(), store.contexts()).get(&chain, 2, Field::ENTER);
    let enter = |store: &Store| -> &'static str { if enter(store) == "edit" { "edit" } else { "select" } };
    assert_eq!(enter(&store), "select");
    let (ops, node) = contexts::put_ops(store.document(), CELLS_EDIT).unwrap();
    store.apply(tx(ops)).unwrap();
    assert_eq!(enter(&store), "edit");
    // The computed view names the document's rule.
    let r = Resolver::new(store.document(), store.contexts());
    let computed = r.computed(&chain, 2);
    assert!(computed.contains(&("affords.enter".into(), "edit".into(), format!("std:table rule 1 from {node}"))), "{computed:?}");
    // The built-in rules still hold beside it.
    assert_eq!(r.get(&chain, 1, Field::SELECT), "through");
    // Writing the same context again replaces its markup: one op.
    let (ops, again) = contexts::put_ops(store.document(), "<context name=\"std:table\"></context>").unwrap();
    assert_eq!((ops.len(), again), (1, node));
    store.apply(tx(ops)).unwrap();
    assert_eq!(enter(&store), "select");
    // Undo brings the override back; undo again removes it.
    store.undo(0).unwrap().unwrap();
    assert_eq!(enter(&store), "edit");
    store.undo(0).unwrap().unwrap();
    assert_eq!(enter(&store), "select");
}

#[test]
fn bad_context_markup_is_refused() {
    let mut store = Store::new();
    let (ops, node) = contexts::put_ops(store.document(), CELLS_EDIT).unwrap();
    store.apply(tx(ops)).unwrap();
    let set = Op::Set { id: node, path: "context_def.markup".parse().unwrap(), slot: Slot::Const(Value::Str("<context name=\"k\"><rule match=\"all\" affords.cursor=\"move\"/></context>".into())) };
    let err = store.apply(tx(vec![set])).unwrap_err().remove(0);
    assert_eq!(err.kind, ErrorKind::BadContext);
    assert!(err.message.ends_with("unknown field affords.cursor (affords.select, affords.enter, affords.edit, affords.grips, affords.drag)"), "{}", err.message);
}

#[test]
fn a_document_context_reads_as_its_markup() {
    let mut store = Store::new();
    let (ops, node) = contexts::put_ops(store.document(), CELLS_EDIT).unwrap();
    store.apply(tx(ops)).unwrap();
    let q = view::ViewQuery { relation: Default::default(), scope: None, fidelity: view::Fidelity::Summary, depth: None, expand: vec![] };
    let v = store.view(&q).unwrap();
    let want = format!("<context id=\"{node}\" name=\"std:table\">\n      <rule match=\"type:text\" affords.enter=\"edit\"/>\n    </context>");
    assert!(v.contains(&want), "{v}");
}


fn composition(store: &mut Store, name: &str, notation: &str) -> NodeId {
    let id = store.document().next_id();
    let mut c = facet::Composition::default();
    c.notation = Slot::Const(Value::Str(notation.into()));
    let named = facet::Name { text: Slot::Const(Value::Str(name.into())) };
    store.apply(tx(vec![Op::Create { id, ty: TypeId::lookup("composition").unwrap(), parent: None, order: OrderKey::first(), facets: vec![c.wrap(), named.wrap()] }])).unwrap();
    id
}

#[test]
fn a_views_palette_and_keys_follow_its_focus() {
    let mut store = Store::new();
    let world = composition(&mut store, "slide", "free");
    let r = Resolver::new(store.document(), store.contexts());
    let free = Focus { world: Some(world), ..Default::default() };
    let v = r.view_values(&free);
    assert_eq!(v.get(Field::PALETTE), "select rect|ellipse|table|empty pen text");
    assert_eq!(contexts::keymap(v.get(Field::KEYS)), vec![("v", "select"), ("p", "pen"), ("r", "rect"), ("o", "ellipse"), ("t", "text")]);
    assert_eq!(contexts::palette(v.get(Field::PALETTE))[1], vec!["rect", "ellipse", "table", "empty"]);
    // Where a main is, the port tool joins the shapes.
    let main = Focus { states: vec!["main"], ..free.clone() };
    assert_eq!(r.view_values(&main).get(Field::PALETTE), "select rect|ellipse|table|port|empty pen text");
    let shown = r.computed_view_markup("slide", &main);
    assert!(shown.contains("palette.tools=\"select rect|ellipse|table|port|empty pen text\" palette.tools.by=\"std:free-2d rule 10\""), "{shown}");
    assert!(shown.contains("keys.tools=\"v:select p:pen r:rect o:ellipse t:text\" keys.tools.by=\"std:free-2d rule 9\""), "{shown}");
}

#[test]
fn a_notation_named_by_a_document_context_changes_the_palette_and_keeps_the_rest() {
    let mut store = Store::new();
    let deck = composition(&mut store, "deck", "deck");
    let free = composition(&mut store, "free", "free");
    let (ops, _) = contexts::put_ops(store.document(), r#"<context name="deck" extends="std:free-2d">
  <rule match="view" palette.tools="select text table" keys.tools="v:select t:text b:table" inspector.sections="position fill"/>
</context>"#).unwrap();
    store.apply(tx(ops)).unwrap();
    let t = table_in(&mut store, Some(deck));
    let r = Resolver::new(store.document(), store.contexts());
    assert_eq!(r.view_values(&Focus { world: Some(deck), ..Default::default() }).get(Field::PALETTE), "select text table");
    assert_eq!(r.view_values(&Focus { world: Some(free), ..Default::default() }).get(Field::PALETTE), "select rect|ellipse|table|empty pen text");
    // The inspector's sections follow the view too.
    assert_eq!(r.view_values(&Focus { world: Some(deck), ..Default::default() }).get(Field::SECTIONS), "position fill");
    assert_eq!(r.view_values(&Focus { world: Some(free), ..Default::default() }).get(Field::SECTIONS), contexts::SECTIONS.join(" "));
    assert_eq!(contexts::parse(r#"<context name="k"><rule match="view" inspector.sections="position colour"/></context>"#).unwrap_err(), "inspector.sections=\"position colour\": unknown word colour (main, port, component, table, position, text, layout, appearance, fill, stroke, effects, other, add)");
    // The deck's table still selects as std:free-2d says: deck extends it.
    let computed = r.computed(&[t], 0);
    assert!(computed.contains(&("affords.select".into(), "target".into(), "std:free-2d rule 1".into())), "{computed:?}");
}

#[test]
fn a_container_in_focus_adds_its_view_rules() {
    let mut store = Store::new();
    let world = composition(&mut store, "slide", "free");
    let t = table(&mut store);
    let (row, cell) = {
        let doc = store.document();
        let row = doc.children(t)[1];
        (row, doc.children(row)[0])
    };
    let (ops, _) = contexts::put_ops(store.document(), r#"<context name="std:table">
  <rule match="view" palette.tools="select text"/>
</context>"#).unwrap();
    store.apply(tx(ops)).unwrap();
    let r = Resolver::new(store.document(), store.contexts());
    // With a cell selected, the open scope is the table (through its row).
    let inside = Focus { world: Some(world), scope: vec![t, row], states: vec![] };
    let v = r.view_values(&inside);
    assert_eq!(v.get(Field::PALETTE), "select text");
    // The notation's keys still hold: the table's rule did not set them.
    assert_eq!(v.get(Field::KEYS), "v:select p:pen r:rect o:ellipse t:text");
    let _ = cell;
    let outside = Focus { world: Some(world), scope: vec![], states: vec![] };
    assert_eq!(r.view_values(&outside).get(Field::PALETTE), "select rect|ellipse|table|empty pen text");
}

#[test]
fn view_fields_and_node_fields_stay_apart() {
    let err = |s: &str| contexts::parse(s).unwrap_err();
    assert_eq!(err(r#"<context name="k"><rule match="all" palette.tools="select"/></context>"#), "palette.tools is a view's: write it in a match=\"view\" rule");
    assert_eq!(err(r#"<context name="k"><rule match="view" affords.select="none"/></context>"#), "affords.select is a node's: a match=\"view\" rule cannot declare it");
    assert_eq!(err(r#"<context name="k"><rule match="view" palette.tools="select lasso"/></context>"#), "palette.tools=\"select lasso\": unknown tool lasso (select, pen, rect, ellipse, text, table, port, empty)");
    assert_eq!(err(r#"<context name="k"><rule match="view" keys.tools="rr:rect"/></context>"#), "keys.tools=\"rr:rect\": key rr is not one letter or digit");
    assert_eq!(err(r#"<context name="k"><rule match="view state:sleepy"/></context>"#), "match=\"view state:sleepy\": unknown state sleepy (hovered, selected, focused, pressed, dragged, drop-target, editing-text, editing-points, main)");
}

#[test]
fn grips_and_drags_come_from_the_context() {
    let mut store = Store::new();
    let t = table(&mut store);
    let (row, cell) = {
        let doc = store.document();
        let row = doc.children(t)[1];
        (row, doc.children(row)[0])
    };
    let r = Resolver::new(store.document(), store.contexts());
    let v = |chain: &[NodeId]| r.values(chain, chain.len() - 1);
    // A table and its cells offer their boxes; a row, passed through, nothing.
    assert!(v(&[t]).has(Field::GRIPS, "box") && !v(&[t]).has(Field::GRIPS, "shape"));
    assert_eq!(v(&[t, row]).get(Field::GRIPS), "");
    assert_eq!(v(&[t, row, cell]).get(Field::DRAG), "point");
    // A document can take a table's box grips away.
    let (ops, _) = contexts::put_ops(store.document(), r#"<context name="std:free-2d">
  <rule match="type:table" affords.grips=""/>
</context>"#).unwrap();
    store.apply(tx(ops)).unwrap();
    let r = Resolver::new(store.document(), store.contexts());
    assert_eq!(r.values(&[t], 0).get(Field::GRIPS), "");
    assert_eq!(contexts::parse(r#"<context name="k"><rule match="all" affords.grips="box handles"/></context>"#).unwrap_err(), "affords.grips=\"box handles\": unknown word handles (box, shape)");
}

#[test]
fn a_tables_cells_navigate_as_a_grid() {
    let mut store = Store::new();
    let t = table(&mut store);
    let doc = store.document();
    let rows: Vec<NodeId> = doc.children(t).to_vec();
    let cell = |r: usize, c: usize| doc.children(rows[r])[c];
    let r = Resolver::new(doc, store.contexts());
    let areas = |t: NodeId| store.table_grid(t).map(|g| g.cells.clone());
    let key = |at: (usize, usize), k: &str, shift: bool| interaction::key(&r, &[t, rows[at.0], cell(at.0, at.1)], k, shift, false, &areas);
    use interaction::KeyAction::*;
    assert_eq!(key((0, 0), "arrowright", false), Some(Select(vec![t, rows[0], cell(0, 1)])));
    assert_eq!(key((0, 1), "arrowdown", false), Some(Select(vec![t, rows[1], cell(1, 1)])));
    assert_eq!(key((0, 0), "arrowleft", false), Some(Stay));
    assert_eq!(key((0, 1), "tab", false), Some(Select(vec![t, rows[1], cell(1, 0)])));
    assert_eq!(key((1, 0), "tab", true), Some(Select(vec![t, rows[0], cell(0, 1)])));
    assert_eq!(key((1, 1), "tab", false), Some(AddRow(t)));
    assert_eq!(key((1, 1), "enter", false), Some(Edit(vec![t, rows[1], cell(1, 1)])));
    // The table itself is not inside a grid: the keys keep their meaning.
    assert_eq!(interaction::key(&r, &[t], "arrowright", false, false, &areas), None);
}

#[test]
fn a_stacks_items_navigate_as_a_list_and_alt_moves_one() {
    let mut store = Store::new();
    let ids = store.document().next_ids(4);
    let (g, items) = (ids[0], &ids[1..]);
    let mut layout = facet::Layout::default();
    layout.mode = Slot::Const(Value::Enum("stack_y".into()));
    let mut ops = vec![Op::Create { id: g, ty: TypeId::lookup("group").unwrap(), parent: None, order: OrderKey::first(), facets: vec![facet::Transform2d::default().wrap(), layout.wrap()] }];
    let mut key = OrderKey::first();
    for &id in items {
        ops.push(Op::Create { id, ty: TypeId::lookup("rect").unwrap(), parent: Some(g), order: key.clone(), facets: vec![facet::Transform2d::default().wrap(), facet::Size::default().wrap()] });
        key = OrderKey::after(&key);
    }
    store.apply(tx(ops)).unwrap();
    let r = Resolver::new(store.document(), store.contexts());
    let none = |_: NodeId| None;
    use interaction::KeyAction::*;
    let k = |i: usize, key: &str, alt: bool| interaction::key(&r, &[g, items[i]], key, false, alt, &none);
    assert_eq!(k(0, "arrowdown", false), Some(Select(vec![g, items[1]])));
    assert_eq!(k(0, "arrowup", false), Some(Stay));
    assert_eq!(k(1, "arrowright", false), Some(Stay));
    assert_eq!(k(1, "enter", false), None);
    // Alt+Down moves the first item past the second.
    let Some(Reorder { id, order }) = k(0, "arrowdown", true) else { panic!("a reorder") };
    let doc = store.document();
    let order_of = |n: NodeId| doc.live(n).unwrap().order.clone();
    assert!(id == items[0] && order > order_of(items[1]) && order < order_of(items[2]));
    // Computed: the group is a column.
    assert!(r.computed(&[g], 0).contains(&("keys.nav".into(), "list-y".into(), "std:column rule 1".into())));
}

#[test]
fn a_previewed_order_lays_the_stack_out_and_floats_the_dragged_item() {
    let mut store = Store::new();
    let ids = store.document().next_ids(4);
    let (g, items) = (ids[0], &ids[1..]);
    let mut layout = facet::Layout::default();
    layout.mode = Slot::Const(Value::Enum("stack_y".into()));
    let mut ops = vec![Op::Create { id: g, ty: TypeId::lookup("group").unwrap(), parent: None, order: OrderKey::first(), facets: vec![facet::Transform2d::default().wrap(), layout.wrap()] }];
    let mut key = OrderKey::first();
    for &id in items {
        let mut size = facet::Size::default();
        size.h = Slot::Const(Value::Number(10.0));
        ops.push(Op::Create { id, ty: TypeId::lookup("rect").unwrap(), parent: Some(g), order: key.clone(), facets: vec![facet::Transform2d::default().wrap(), size.wrap()] });
        key = OrderKey::after(&key);
    }
    store.apply(tx(ops)).unwrap();
    let y = |s: &Store, n: NodeId| s.resolved(n, "transform2d.y".parse().unwrap()).and_then(|v| v.as_f64()).unwrap();
    assert_eq!(items.iter().map(|&n| y(&store, n)).collect::<Vec<_>>(), vec![0.0, 10.0, 20.0]);
    // The children of a stack drag in sequence.
    let r = Resolver::new(store.document(), store.contexts());
    assert_eq!(r.get(&[g, items[0]], 1, Field::DRAG), "sequence");
    // The first dragged below the second: the second moves up into its
    // place, the first floats at the pointer.
    let version = store.document().version();
    store.set_layout_preview(Some(layout::Preview { container: g, order: vec![items[1], items[0], items[2]], floating: Some((items[0], [3.0, 14.0])) }));
    assert_eq!((y(&store, items[1]), y(&store, items[0]), y(&store, items[2])), (0.0, 14.0, 20.0));
    // A preview is not an edit.
    assert_eq!(store.document().version(), version);
    store.set_layout_preview(None);
    assert_eq!(items.iter().map(|&n| y(&store, n)).collect::<Vec<_>>(), vec![0.0, 10.0, 20.0]);
}

#[test]
fn a_stacks_items_glide_when_layout_moves_them_and_only_with_a_clock() {
    let build = |motion: bool| {
        let mut store = Store::new();
        store.set_motion(motion);
        let ids = store.document().next_ids(4);
        let (g, items) = (ids[0], ids[1..].to_vec());
        let mut layout = facet::Layout::default();
        layout.mode = Slot::Const(Value::Enum("stack_y".into()));
        let mut ops = vec![Op::Create { id: g, ty: TypeId::lookup("group").unwrap(), parent: None, order: OrderKey::first(), facets: vec![facet::Transform2d::default().wrap(), layout.wrap()] }];
        let mut key = OrderKey::first();
        for &id in &items {
            let mut size = facet::Size::default();
            size.h = Slot::Const(Value::Number(10.0));
            ops.push(Op::Create { id, ty: TypeId::lookup("rect").unwrap(), parent: Some(g), order: key.clone(), facets: vec![facet::Transform2d::default().wrap(), size.wrap()] });
            key = OrderKey::after(&key);
        }
        store.apply(tx(ops)).unwrap();
        (store, items)
    };
    let y = |s: &Store, n: NodeId| s.resolved(n, "transform2d.y".parse().unwrap()).and_then(|v| v.as_f64()).unwrap();
    // The last moved first: the first two move down by its height.
    let reorder = |s: &mut Store, items: &[NodeId]| {
        let first = s.document().live(items[0]).unwrap().order.clone();
        s.apply(tx(vec![Op::Reorder { id: items[2], order: OrderKey::before(&first) }])).unwrap();
    };
    let (mut store, items) = build(true);
    assert_eq!(store.advance_motion(0.0), Vec::<NodeId>::new());
    reorder(&mut store, &items);
    // Where they were, at first; part-way at 80 ms; in place after 160.
    assert_eq!((y(&store, items[0]), y(&store, items[2])), (0.0, 20.0));
    let moving = store.advance_motion(0.08);
    assert_eq!(moving.len(), 3);
    let mid = y(&store, items[0]);
    assert!(mid > 5.0 && mid < 10.0, "ease out passes half-way early: {mid}");
    store.advance_motion(0.2);
    assert_eq!((y(&store, items[0]), y(&store, items[1]), y(&store, items[2])), (10.0, 20.0, 0.0));
    assert_eq!(store.advance_motion(0.3), Vec::<NodeId>::new());
    // Without a clock (motion off), layout lands at once.
    let (mut store, items) = build(false);
    reorder(&mut store, &items);
    assert_eq!((y(&store, items[0]), y(&store, items[2])), (10.0, 0.0));
    assert!(contexts::parse(r#"<context name="k"><rule match="all" motion.placement="out fast"/></context>"#).unwrap_err().starts_with("motion.placement=\"out fast\": expected none"));
}
