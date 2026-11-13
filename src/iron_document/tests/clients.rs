//! Several clients editing one scope (id.rs): each mints ids as its own
//! client, so their creates never collide, and a server that sequences both
//! clients' transactions ends with everything each one made.

use iron_document::facet::Name;
use iron_document::*;

fn ty(name: &str) -> TypeId {
    TypeId::lookup(name).unwrap()
}

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::Human, ts: 0, idempotency_key: None, ops, exact: false }
}

fn group(store: &Store, id: NodeId, parent: Option<NodeId>, name: &str) -> Op {
    let mut n = Name::default();
    n.text = Slot::Const(Value::Str(name.into()));
    let order = match parent {
        Some(p) => store.document().children(p).last().and_then(|&c| store.document().live(c)).map_or_else(OrderKey::first, |n| OrderKey::after(&n.order)),
        None => OrderKey::first(),
    };
    Op::Create { id, ty: ty("group"), parent, order, facets: vec![n.wrap()] }
}

/// The last transaction `store` logged.
fn last(store: &Store) -> Transaction {
    let v = store.document().version();
    store.log_since(Version(v.0 - 1)).unwrap()[0].clone()
}

#[test]
fn client_ids_print_and_parse() {
    let mut store = Store::new();
    store.set_client(4);
    let id = store.document().next_id();
    assert_eq!(id.client(), Some(4));
    assert_eq!(id.to_string(), "#4_1");
    assert_eq!("#4_1".parse::<NodeId>().unwrap(), id);
    assert_eq!(store.document().next_relation_id().to_string(), "r4_1");
    // Client 0's ids print as they always have.
    assert_eq!(Store::new().document().next_id().to_string(), "#1");
    // Each id has one spelling: no client 0 written out, no bare number in a
    // client's range, nothing past the counter's bits.
    for bad in ["#0_1", "#_1", "#4_", "#4__1", "#1000000000"] {
        assert!(bad.parse::<NodeId>().is_err(), "{bad} parsed");
    }
    // An expanded path through a client's ids.
    let e = NodeId::expanded(id, "#7_2".parse().unwrap()).unwrap();
    assert_eq!(e.to_string(), "#4_1/7_2");
    assert_eq!("#4_1/7_2".parse::<NodeId>().unwrap(), e);
    // Expressions read the whole id, and print it back.
    let x = Expr::parse("#4_1.transform2d.x * 2 + (#3_b ? 1 : 0)").unwrap();
    let printed = x.to_string();
    assert!(printed.contains("#4_1.transform2d.x") && printed.contains("#3_b"), "{printed}");
}

#[test]
fn two_clients_create_at_once_and_the_server_keeps_both() {
    // The scope as uploaded: made locally, as client 0.
    let mut server = Store::new();
    let root = server.document().next_id();
    server.apply(tx("root", vec![group(&server, root, None, "root")])).unwrap();
    let snapshot = server.save();

    // Two clients open it and each create a node before hearing of the
    // other's: the same counter, different clients.
    let open = |client| {
        let mut s = Store::load(&snapshot).unwrap();
        s.set_client(client);
        s
    };
    let (mut a, mut b) = (open(1), open(2));
    let (ia, ib) = (a.document().next_id(), b.document().next_id());
    assert_ne!(ia, ib);
    a.apply(tx("a", vec![group(&a, ia, Some(root), "from a")])).unwrap();
    b.apply(tx("b", vec![group(&b, ib, Some(root), "from b")])).unwrap();

    // The server sequences a's, then b's; both apply.
    let (ta, tb) = (last(&a), last(&b));
    for t in [&ta, &tb] {
        server.replay(t).unwrap();
    }
    assert!(server.document().is_live(ia) && server.document().is_live(ib));

    // Each client hears the other's and converges on the server's state.
    a.replay(&tb).unwrap();
    b.replay(&ta).unwrap();
    for s in [&a, &b] {
        assert_eq!(s.document().children(root).len(), 2);
    }

    assert_eq!(server.document().clients(), vec![0, 1, 2]);
    assert!(server.document().counted(ia) && !server.document().counted(a.document().next_id()));

    // Each keeps counting in its own range past what it saw of the other's.
    assert_eq!(a.document().next_id().client(), Some(1));
    assert_ne!(a.document().next_id(), ia);
    assert_eq!(b.document().next_id().client(), Some(2));
}

#[test]
fn every_clients_counter_survives_a_save() {
    let mut store = Store::new();
    let root = store.document().next_id();
    store.apply(tx("root", vec![group(&store, root, None, "root")])).unwrap();
    store.set_client(9);
    let ids = store.document().next_ids(2);
    store.apply(tx("two", vec![group(&store, ids[0], Some(root), "x"), group(&store, ids[1], Some(root), "y")])).unwrap();
    // Deleted, but never reused.
    store.apply(tx("delete", vec![Op::Delete { id: ids[1] }])).unwrap();

    let text = store.save();
    assert!(text.contains("\"next_ids\": {\"0\": 2, \"9\": 3}"), "{}", text.lines().next().unwrap());
    let mut loaded = Store::load(&text).unwrap();
    // Who is editing is not saved: a loaded store mints as client 0 until told otherwise.
    assert_eq!(loaded.client(), 0);
    loaded.set_client(9);
    assert_eq!(loaded.document().next_id(), ids[1].offset(1));

    // A file whose node is past its client's counter does not load.
    let bad = text.replace("\"9\": 3", "\"9\": 2");
    assert!(Store::load(&bad).is_err());
}
