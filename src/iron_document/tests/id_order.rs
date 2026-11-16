//! Assumption A12 (reactive-engine.md §14; handoff §4.1, task 7): nothing
//! that can be seen depends on the numeric order of derived ids. A derived
//! id's number is its slot in a process-wide registry, in order of first
//! use (`id.rs`), so two processes holding the same document can number its
//! derived ids differently. The differential harness cannot see this: its
//! two stores share the registry. So this test runs itself twice as child
//! processes and compares what each prints: the saved text and the full
//! view. The second child first interns every derived id the first one
//! made, in reverse order, so the numeric order of the document's derived
//! ids is reversed between the two.

use iron_document::facet::{Composition, Name};
use iron_document::*;
use std::process::Command;

const CHILD: &str = "IRON_ID_ORDER_CHILD";
const FILE: &str = "IRON_ID_ORDER_FILE";

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

/// Instances with repeats inside (two plots, a meter), and repeats keyed by
/// value and by index with an instance in their items.
fn build() -> String {
    let mut store = Store::new();
    let ids = store.document().next_ids(2);
    let mut c = Composition::default();
    c.dims = Slot::Const(Value::Enum("2d".into()));
    let mut name = Name::default();
    name.text = Slot::Const(Value::Str("sheet".into()));
    let ty = |n: &str| TypeId::lookup(n).unwrap();
    store
        .apply(tx(
            "sheet",
            vec![
                Op::Create { id: ids[0], ty: ty("composition"), parent: None, order: OrderKey::first(), facets: vec![name.wrap(), c.wrap()] },
                Op::Create { id: ids[1], ty: ty("model"), parent: None, order: OrderKey::after(&OrderKey::first()), facets: vec![] },
            ],
        ))
        .unwrap();
    let comp = ids[0];
    store.place_standard("plot", comp, [0.0, 0.0], Actor::Human).unwrap();
    store.place_standard("meter", comp, [800.0, 0.0], Actor::Human).unwrap();
    store.place_standard("plot", comp, [0.0, 500.0], Actor::Human).unwrap();
    let markup = r#"<let id="$lo" value="3"/>
        <group id="$t" name="ticks" repeat.each="{= range($lo, $lo + 30, 4)}" repeat.key="{= t => t}"><text name="tick" x="{= $t.repeat.item}" y="0" content="{= fmt($t.repeat.item)}"/></group>
        <group id="$r" name="rows" repeat.count="3"><rect name="row" x="0" y="{= $r.repeat.index * 10}" w="5" h="5"/></group>"#;
    let built = markup::ops(markup, Some(comp), OrderKey::first(), store.document().next_id()).unwrap_or_else(|e| panic!("{e}"));
    store.apply(tx("draw", built.ops)).unwrap_or_else(|e| panic!("{e:?}"));
    // Move the value-keyed repeat, so items come and go.
    store.apply(tx("slide", vec![Op::Set { id: built.named["lo"], path: "let.value".parse().unwrap(), slot: Slot::Const(Value::Number(11.0)) }])).unwrap();
    let live: Vec<NodeId> = store.document().live_nodes().map(|n| n.id).collect();
    let view = store.view(&ViewQuery { relation: ViewRelation::Tree, scope: None, fidelity: Fidelity::Full, depth: None, expand: live, omit_defaults: false }).unwrap();
    let derived: Vec<String> = store.document().nodes().map(|n| n.id).filter(|id| id.is_expanded()).map(|id| id.to_string()).collect();
    format!("{}\n=====\n{view}\n=====\n{}", store.save(), derived.join("\n"))
}

#[test]
fn derived_id_order_does_not_depend_on_what_the_process_interned_first() {
    if let Ok(mode) = std::env::var(CHILD) {
        if mode == "reversed" {
            let ids = std::fs::read_to_string(std::env::var(FILE).unwrap()).unwrap();
            for id in ids.lines().rev() {
                let _: NodeId = id.parse().unwrap_or_else(|e| panic!("{id}: {e}"));
            }
        }
        print!("{}", build());
        return;
    }
    let file = std::env::temp_dir().join(format!("iron-id-order-{}.txt", std::process::id()));
    let run = |mode: &str| {
        let out = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "derived_id_order_does_not_depend_on_what_the_process_interned_first", "--nocapture", "--test-threads=1"])
            .env(CHILD, mode)
            .env(FILE, &file)
            .output()
            .unwrap();
        assert!(out.status.success(), "child {mode} failed: {}", String::from_utf8_lossy(&out.stderr));
        let text = String::from_utf8(out.stdout).unwrap();
        // The test harness's own lines around what the child printed.
        let start = text.find('{').expect("saved text");
        let end = text.rfind("\ntest ").unwrap_or(text.len());
        let mut parts = text[start..end].split("\n=====\n").map(str::to_owned);
        (parts.next().unwrap() + &parts.next().unwrap(), parts.next().unwrap())
    };
    let (first, derived) = run("first");
    assert!(derived.lines().count() > 100, "the document has derived nodes: {}", derived.lines().count());
    std::fs::write(&file, &derived).unwrap();
    let (reversed, again) = run("reversed");
    std::fs::remove_file(&file).ok();
    assert!(first.len() > 10_000, "the document is substantial: {} bytes", first.len());
    // The same ids, made in the same process order: only their numbers differ.
    let (mut a, mut b): (Vec<&str>, Vec<&str>) = (derived.lines().collect(), again.lines().collect());
    let same_order = a == b;
    a.sort();
    b.sort();
    assert_eq!(a, b, "both processes made the same derived ids");
    assert!(!same_order, "the document's derived ids are numbered in another order in the second process, so the test can see a difference");
    if first != reversed {
        let line = first.lines().zip(reversed.lines()).position(|(a, b)| a != b).unwrap_or(0);
        let (a, b): (Vec<&str>, Vec<&str>) = (first.lines().collect(), reversed.lines().collect());
        panic!("A12 is false: two processes print the same document differently, from line {line}:\n  first:    {}\n  reversed: {}", a.get(line).unwrap_or(&""), b.get(line).unwrap_or(&""));
    }
}
