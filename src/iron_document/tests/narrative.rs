//! plans/specs/narrative-panel.md phase 1: narratives as nodes. Block edits
//! become one transaction that undo reverses; content survives save and
//! load; the span index finds mentions and marks them stale when their
//! target goes; the model's view reads the narrative as markdown.

use iron_document::narrative::{self, BlockContent, Edit};
use iron_document::*;

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn texts(store: &Store, n: NodeId) -> Vec<String> {
    narrative::blocks(store.document(), n).into_iter().map(|(_, c)| c.text).collect()
}

/// A rect to mention, and a narrative of three blocks about it.
fn scene() -> (Store, NodeId, NodeId) {
    let mut store = Store::new();
    let rect = store.document().next_id();
    let mut t = facet::Transform2d::default();
    t.x = Slot::Const(Value::Number(10.0));
    let mut s = facet::Size::default();
    s.w = Slot::Const(Value::Number(40.0));
    store
        .apply(tx("add rect", vec![Op::Create { id: rect, ty: TypeId::lookup("rect").unwrap(), parent: None, order: OrderKey::first(), facets: vec![t.wrap(), s.wrap()] }]))
        .unwrap();
    let blocks = narrative::parse_markdown(&format!("# A lesson\n\nThe :e[box]{{{rect}}} is :val{{{rect}.size.w digits=0}} wide.\n\n- a point"));
    let (n, ops) = narrative::create_ops(store.document(), "lesson", &blocks);
    store.apply(tx("add narrative", ops)).unwrap();
    (store, n, rect)
}

#[test]
fn a_narrative_is_blocks_in_order() {
    let (store, n, rect) = scene();
    let doc = store.document();
    assert_eq!(narrative::narratives(doc), vec![n]);
    assert_eq!(narrative::find(doc, "lesson"), Some(n));
    assert_eq!(narrative::find(doc, &n.to_string()), Some(n));
    let b = narrative::blocks(doc, n);
    assert_eq!(b.iter().map(|(_, c)| c.kind.as_str()).collect::<Vec<_>>(), ["heading", "paragraph", "bullet"]);
    assert_eq!(b[1].1.marks[0].target.as_deref(), Some(rect.to_string().as_str()));
}

#[test]
fn edits_are_one_transaction_and_undo_reverses_them() {
    let (mut store, n, _) = scene();
    let ids: Vec<String> = narrative::blocks(store.document(), n).into_iter().map(|(id, _)| id.to_string()).collect();
    let edits = vec![
        Edit::Insert { key: "k1".into(), after: Some(ids[0].clone()), block: BlockContent::paragraph("Inserted after the heading.") },
        Edit::Insert { key: "k2".into(), after: Some("k1".into()), block: BlockContent::paragraph("And after that.") },
        Edit::Set { id: ids[2].clone(), block: BlockContent { kind: "bullet".into(), text: "a changed point".into(), ..Default::default() } },
        Edit::Move { id: ids[0].clone(), after: Some(ids[2].clone()) },
        Edit::Delete { id: ids[1].clone() },
    ];
    let before = texts(&store, n);
    let planned = narrative::plan(store.document(), n, &edits).unwrap();
    assert_eq!(planned.minted.len(), 2);
    store.apply(tx("edit narrative", planned.ops)).unwrap();
    assert_eq!(texts(&store, n), ["Inserted after the heading.", "And after that.", "a changed point", "A lesson"]);
    store.undo(0).unwrap();
    assert_eq!(texts(&store, n), before, "one undo reverses every edit");
    store.redo(0).unwrap();
    assert_eq!(texts(&store, n).len(), 4);

    // Content survives the canonical file.
    let loaded = Store::load(&store.save()).unwrap();
    assert_eq!(texts(&loaded, n), texts(&store, n));
    assert_eq!(narrative::blocks(loaded.document(), n), narrative::blocks(store.document(), n));
}

#[test]
fn bad_edits_are_refused_with_a_reason() {
    let (store, n, rect) = scene();
    let doc = store.document();
    let err = |e: Edit| narrative::plan(doc, n, &[e]).unwrap_err();
    assert!(err(Edit::Delete { id: rect.to_string() }).contains("not a block"));
    assert!(err(Edit::Insert { key: "k".into(), after: None, block: BlockContent { kind: "table".into(), ..Default::default() } }).contains("unknown block kind"));
    let mut b = BlockContent::paragraph("ab");
    b.marks.push(narrative::Mark { kind: "strong".into(), start: 1, end: 9, ..Default::default() });
    assert!(err(Edit::Insert { key: "k".into(), after: None, block: b }).contains("outside the text"));
    assert!(narrative::plan(doc, rect, &[]).unwrap_err().contains("not a live narrative"));
}

#[test]
fn the_span_index_finds_mentions_and_marks_them_stale() {
    let (mut store, n, rect) = scene();
    let refs = narrative::references_to(store.document(), rect);
    assert_eq!(refs.iter().map(|r| r.kind.as_str()).collect::<Vec<_>>(), ["mention", "value"]);
    assert!(refs.iter().all(|r| r.narrative == n && !r.stale));
    store.apply(tx("delete rect", vec![Op::Delete { id: rect }])).unwrap();
    assert!(narrative::references_to(store.document(), rect).iter().all(|r| r.stale));
}

#[test]
fn the_model_reads_markdown_with_block_ids() {
    let (store, n, rect) = scene();
    let view = store.view(&ViewQuery { relation: Default::default(), scope: None, fidelity: Fidelity::Full, depth: None, expand: vec![] }).unwrap();
    let first = narrative::blocks(store.document(), n)[0].0;
    assert!(view.contains(&format!("<narrative id=\"{n}\" name=\"lesson\" count=\"3\">")), "{view}");
    assert!(view.contains(&format!("[{first}] # A lesson")), "{view}");
    assert!(view.contains(&format!("The :e[box]{{{rect}}} is :val{{{rect}.size.w digits=0}} wide.")), "{view}");
    // And the value it shows evaluates through the reactive graph.
    let m = &narrative::blocks(store.document(), n)[1].1.marks[1];
    let e = Expr::parse(&narrative::value_expr(m).unwrap()).unwrap();
    assert_eq!(e.eval(&|l| store.resolved(l.node, l.path)).unwrap(), Value::Str("40".into()));
}
