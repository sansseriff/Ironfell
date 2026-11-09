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
    assert_eq!(err(r#"<context name="k"><rule match="all" affords.wobble="move"/></context>"#), "unknown field affords.wobble (affords.select, affords.enter, affords.edit, affords.grips, affords.drag, affords.drives, affords.activate, affords.step, affords.axis, affords.delete, affords.cursor)");
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
    let set = Op::Set { id: node, path: "context_def.markup".parse().unwrap(), slot: Slot::Const(Value::Str("<context name=\"k\"><rule match=\"all\" affords.wobble=\"move\"/></context>".into())) };
    let err = store.apply(tx(vec![set])).unwrap_err().remove(0);
    assert_eq!(err.kind, ErrorKind::BadContext);
    assert!(err.message.ends_with("unknown field affords.wobble (affords.select, affords.enter, affords.edit, affords.grips, affords.drag, affords.drives, affords.activate, affords.step, affords.axis, affords.delete, affords.cursor)"), "{}", err.message);
}

#[test]
fn a_document_context_reads_as_its_markup() {
    let mut store = Store::new();
    let (ops, node) = contexts::put_ops(store.document(), CELLS_EDIT).unwrap();
    store.apply(tx(ops)).unwrap();
    let q = view::ViewQuery { omit_defaults: false, relation: Default::default(), scope: None, fidelity: view::Fidelity::Summary, depth: None, expand: vec![] };
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
    assert_eq!(err(r#"<context name="k"><rule match="view" palette.tools="select lasso"/></context>"#), "palette.tools=\"select lasso\": unknown tool lasso (select, pen, rect, ellipse, text, table, port, empty, wire, or place:<part>)");
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
    assert_eq!(contexts::parse(r#"<context name="k"><rule match="all" affords.grips="box handles"/></context>"#).unwrap_err(), "affords.grips=\"box handles\": unknown word handles (box, shape, ends)");
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

fn named_nodes(store: &Store, name: &str) -> Vec<NodeId> {
    let doc = store.document();
    doc.nodes().filter(|n| n.is_live()).map(|n| n.id).filter(|&id| doc.get::<facet::Name>(id).and_then(|n| n.text.constant().and_then(|v| v.as_str().map(str::to_owned))).as_deref() == Some(name)).collect()
}

#[test]
fn a_definitions_part_declares_its_drag_for_every_instance_and_drives_through_its_binding() {
    let mut store = Store::new();
    let comp = composition(&mut store, "bench", "free");
    let a = store.place_standard("meter", comp, [0.0, 0.0], Actor::Human).unwrap();
    let b = store.place_standard("meter", comp, [200.0, 0.0], Actor::Human).unwrap();
    // The definition's own pivot (not an instance's expanded copy).
    let pivots = named_nodes(&store, "pivot");
    let def_pivot = *pivots.iter().find(|p| !p.is_expanded()).expect("the template's pivot");
    let mut f = facet::Affords::default();
    f.style = Slot::Const(Value::Str("drag: ring; drives: transform2d.rot".into()));
    store.apply(tx(vec![Op::AddFacet { id: def_pivot, facet: f.wrap() }])).unwrap();
    // Every instance's pivot now turns, by the definition's rule.
    let r = Resolver::new(store.document(), store.contexts());
    let instance_pivots: Vec<NodeId> = named_nodes(&store, "pivot").into_iter().filter(|p| p.is_expanded()).collect();
    assert_eq!(instance_pivots.len(), 2);
    for &p in &instance_pivots {
        let chain = contexts::chain_of(store.document(), p);
        let v = r.values(&chain, chain.len() - 1);
        assert_eq!((v.get(Field::DRAG), v.get(Field::DRIVES)), ("ring", "transform2d.rot"));
        assert_eq!(r.describe_source(&v, Field::DRAG), format!("inline on {p}"));
    }
    let _ = (a, b);
    // A drive previews and commits the pivot's rotation through its
    // binding: the meter's `in` follows (rot = -π + in / range · π).
    let p = instance_pivots[0];
    let rot: LeafPath = "transform2d.rot".parse().unwrap();
    store.begin_gesture(&[p]);
    store.preview_target(p, rot, Value::Number(-std::f64::consts::FRAC_PI_2)).unwrap();
    store.commit_gesture("turn pivot", Actor::Human, 0).unwrap().unwrap();
    let turned = store.resolved(p, rot).and_then(|v| v.as_f64()).unwrap();
    assert!((turned + std::f64::consts::FRAC_PI_2).abs() < 1e-9, "{turned}");
    // Bad style is refused, with the field.
    let mut bad = facet::Affords::default();
    bad.style = Slot::Const(Value::Str("drag: spin".into()));
    let err = store.apply(tx(vec![Op::RemoveFacet { id: def_pivot, kind: FacetKind::Affords }, Op::AddFacet { id: def_pivot, facet: bad.wrap() }])).unwrap_err().remove(0);
    assert_eq!(err.kind, ErrorKind::BadContext);
}

#[test]
fn interaction_state_drives_looks_only_and_rules_can_match_it() {
    let mut store = Store::new();
    let id = store.document().next_id();
    let mut fill = facet::Fill::default();
    fill.opacity = Slot::Bound(Expr::parse(&format!("{id}.interaction.hovered ? 1 : 0.4")).unwrap());
    store.apply(tx(vec![Op::Create { id, ty: TypeId::lookup("rect").unwrap(), parent: None, order: OrderKey::first(), facets: vec![facet::Transform2d::default().wrap(), facet::Size::default().wrap(), fill.wrap(), facet::Interaction::default().wrap()] }])).unwrap();
    let opacity = |s: &Store| s.resolved(id, "fill.opacity".parse().unwrap()).and_then(|v| v.as_f64()).unwrap();
    assert_eq!(opacity(&store), 0.4);
    // The host publishes the state: the look follows, the document does not change.
    let version = store.document().version();
    let producer = store.register_producer("interaction");
    store.publish(producer, std::collections::BTreeMap::from([(Leaf { node: id, path: "interaction.hovered".parse().unwrap() }, Value::Bool(true))]));
    assert_eq!((opacity(&store), store.document().version()), (1.0, version));
    // State may not drive what the document holds.
    let set = Op::Set { id, path: "transform2d.x".parse().unwrap(), slot: Slot::Bound(Expr::parse(&format!("{id}.interaction.hovered ? 10 : 0")).unwrap()) };
    let err = store.apply(tx(vec![set])).unwrap_err().remove(0);
    assert_eq!(err.kind, ErrorKind::OutOfScope);
    // Rules match a node's state through the store's resolver.
    let (ops, _) = contexts::put_ops(store.document(), r#"<context name="std:free-2d"><rule match="type:rect state:selected" affords.grips="box"/><rule match="type:rect" affords.grips=""/></context>"#).unwrap();
    store.apply(tx(ops)).unwrap();
    // Later rules win: the unconditional one, after, clears it; order the other way.
    let (ops, _) = contexts::put_ops(store.document(), r#"<context name="std:free-2d"><rule match="type:rect" affords.grips=""/><rule match="type:rect state:selected" affords.grips="box shape"/></context>"#).unwrap();
    store.apply(tx(ops)).unwrap();
    assert_eq!(store.resolver().get(&[id], 0, Field::GRIPS), "");
    store.set_interaction(std::collections::BTreeMap::from([(id, vec!["selected"])]));
    assert_eq!(store.resolver().get(&[id], 0, Field::GRIPS), "box shape");
}

#[test]
fn a_views_tolerances_are_context_data() {
    let mut store = Store::new();
    let world = composition(&mut store, "touch", "touch");
    let (ops, _) = contexts::put_ops(store.document(), r#"<context name="touch" extends="std:free-2d"><rule match="view" tolerance.hit="2.5" tolerance.slop="8"/></context>"#).unwrap();
    store.apply(tx(ops)).unwrap();
    let r = store.resolver();
    let v = r.view_values(&Focus { world: Some(world), ..Default::default() });
    assert_eq!((v.get(Field::HIT), v.get(Field::SLOP), v.get(Field::DOUBLE_CLICK)), ("2.5", "8", "400"));
    assert!(contexts::parse(r#"<context name="k"><rule match="view" tolerance.hit="big"/></context>"#).unwrap_err().contains("expected a number"));
}

#[test]
fn a_views_chrome_is_context_data_and_a_part_can_ask_for_its_handle() {
    let mut store = Store::new();
    let world = composition(&mut store, "pitch", "pitch");
    let (ops, _) = contexts::put_ops(store.document(), r##"<context name="pitch" extends="std:free-2d"><rule match="view" chrome.accent="#e8711aff" chrome.grip_shape="round" chrome.hover="off"/></context>"##).unwrap();
    store.apply(tx(ops)).unwrap();
    let id = store.document().next_id();
    let mut a = facet::Affords::default();
    a.style = Slot::Const(Value::Str("drag: ring; chrome.handle: dot; chrome.lit: none".into()));
    store.apply(tx(vec![Op::Create { id, ty: TypeId::lookup("rect").unwrap(), parent: Some(world), order: OrderKey::first(), facets: vec![facet::Transform2d::default().wrap(), facet::Size::default().wrap(), a.wrap()] }])).unwrap();
    let r = store.resolver();
    let v = r.view_values(&Focus { world: Some(world), ..Default::default() });
    assert_eq!((v.get(Field::ACCENT), v.get(Field::GRIP), v.get(Field::GRIP_SHAPE), v.get(Field::HOVER)), ("#e8711aff", "8", "round", "off"));
    assert_eq!((r.get(&[id], 0, Field::HANDLE), r.get(&[id], 0, Field::LIT)), ("dot".into(), "none".into()));
    assert!(contexts::parse(r#"<context name="k"><rule match="view" chrome.accent="orange"/></context>"#).unwrap_err().contains("expected a colour"));
}

#[test]
fn typing_into_a_readout_edits_what_it_reads() {
    let mut store = Store::new();
    let ids = store.document().next_ids(3);
    let (s, cell, power) = (ids[0], ids[1], ids[2]);
    let mut slider = facet::Slider::default();
    slider.value = Slot::Const(Value::Number(0.5));
    let readout = |id: NodeId, e: String| {
        let mut t = facet::Text::default();
        t.content = Slot::Bound(Expr::parse(&e).unwrap());
        Op::Create { id, ty: TypeId::lookup("text").unwrap(), parent: None, order: OrderKey::first(), facets: vec![facet::Transform2d::default().wrap(), t.wrap(), facet::Font::default().wrap()] }
    };
    store.apply(tx(vec![
        Op::Create { id: s, ty: TypeId::lookup("slider").unwrap(), parent: None, order: OrderKey::first(), facets: vec![facet::Transform2d::default().wrap(), slider.wrap()] },
        readout(cell, format!("fmt({s}.slider.value * 100, 0)")),
        readout(power, format!("fmt({s}.slider.value * 20 mW, \"mW\", 1)")),
    ])).unwrap();
    let content: LeafPath = "text.content".parse().unwrap();
    let value = |st: &Store| st.resolved(s, "slider.value".parse().unwrap()).and_then(|v| v.as_f64()).unwrap();
    assert!(!store.handle(cell, content).is_locked());
    store.set_through_handle(cell, content, Value::Str("75".into()), "type", Actor::Human, 0).unwrap();
    assert!((value(&store) - 0.75).abs() < 1e-9);
    // A plain number takes the unit the words are formatted in; a unit may be typed.
    store.set_through_handle(power, content, Value::Str("4".into()), "type", Actor::Human, 0).unwrap();
    assert!((value(&store) - 0.2).abs() < 1e-9, "{}", value(&store));
    store.set_through_handle(power, content, Value::Str("0.012 W".into()), "type", Actor::Human, 0).unwrap();
    assert!((value(&store) - 0.6).abs() < 1e-9, "{}", value(&store));
    assert!(store.set_through_handle(cell, content, Value::Str("lots".into()), "type", Actor::Human, 0).unwrap_err().contains("is not a number"));
}

#[test]
fn a_context_facet_names_a_containers_context() {
    let mut store = Store::new();
    let ids = store.document().next_ids(3);
    let (g, a, b) = (ids[0], ids[1], ids[2]);
    let mut named = facet::ContextOf::default();
    named.name = Slot::Const(Value::Str("palette".into()));
    store.apply(tx(vec![
        Op::Create { id: g, ty: TypeId::lookup("group").unwrap(), parent: None, order: OrderKey::first(), facets: vec![facet::Transform2d::default().wrap(), named.wrap()] },
        Op::Create { id: a, ty: TypeId::lookup("rect").unwrap(), parent: Some(g), order: OrderKey::first(), facets: vec![facet::Transform2d::default().wrap(), facet::Size::default().wrap()] },
        Op::Create { id: b, ty: TypeId::lookup("rect").unwrap(), parent: None, order: OrderKey::after(&OrderKey::first()), facets: vec![facet::Transform2d::default().wrap(), facet::Size::default().wrap()] },
    ])).unwrap();
    let (ops, _) = contexts::put_ops(store.document(), r#"<context name="palette"><rule match="children" affords.drag="none" affords.activate="toggle" affords.drives="appearance.opacity"/></context>"#).unwrap();
    store.apply(tx(ops)).unwrap();
    let r = store.resolver();
    // The group's children take the named context's rules; a rect elsewhere does not.
    assert_eq!((r.get(&[g, a], 1, Field::DRAG), r.get(&[g, a], 1, Field::ACTIVATE)), ("none".into(), "toggle".into()));
    assert_eq!(r.get(&[b], 0, Field::DRAG), "point");
}

#[test]
fn every_field_constant_names_its_field() {
    let named = [
        (Field::SELECT, "affords.select"), (Field::ENTER, "affords.enter"), (Field::EDIT, "affords.edit"), (Field::LIT, "chrome.lit"),
        (Field::SNAP_TARGET, "snap.target"), (Field::GRIPS, "affords.grips"), (Field::DRAG, "affords.drag"), (Field::DRIVES, "affords.drives"),
        (Field::ACTIVATE, "affords.activate"), (Field::STEP, "affords.step"), (Field::AXIS, "affords.axis"), (Field::NAV, "keys.nav"),
        (Field::PLACEMENT, "motion.placement"), (Field::PALETTE, "palette.tools"), (Field::KEYS, "keys.tools"), (Field::SLOP, "tolerance.slop"),
        (Field::DOUBLE_CLICK, "tolerance.double_click"), (Field::HIT, "tolerance.hit"), (Field::SECTIONS, "inspector.sections"),
        (Field::HANDLE, "chrome.handle"), (Field::ACCENT, "chrome.accent"), (Field::GRIP, "chrome.grip"), (Field::GRIP_SHAPE, "chrome.grip_shape"), (Field::HOVER, "chrome.hover"),
        (Field::MATE, "plug.mate"), (Field::GRID, "snap.grid"), (Field::DELETE, "affords.delete"), (Field::COMPONENT, "chrome.component"), (Field::CURSOR, "affords.cursor"),
        (Field::STRIP, "workspace.strip"),
    ];
    for (f, path) in named {
        assert_eq!(f.path(), path);
    }
    assert_eq!(named.len(), contexts::FIELDS.len(), "a field without a constant here");
}

/// The workspace's view selector strip (panel-primitives.md): its views come
/// from `std:workspace`, and a document's override changes them.
#[test]
fn the_workspace_strip_is_context_data() {
    let mut store = Store::new();
    let strip = |s: &Store| Resolver::new(s.document(), s.contexts()).workspace_values().get(Field::STRIP).to_owned();
    assert_eq!(strip(&store), "layers model narratives library");
    let ctx = r#"<context name="std:workspace"><rule match="view" workspace.strip="model layers composition:library"/></context>"#;
    let (ops, _) = contexts::put_ops(store.document(), ctx).unwrap();
    store.apply(tx(ops)).unwrap();
    assert_eq!(strip(&store), "model layers composition:library");
    let bad = r#"<context name="std:workspace"><rule match="view" workspace.strip="layers nonsense"/></context>"#;
    let refused = contexts::put_ops(store.document(), bad).map(|(ops, _)| store.apply(tx(ops)).is_err()).unwrap_or(true);
    assert!(refused, "an unknown view is refused");
}
