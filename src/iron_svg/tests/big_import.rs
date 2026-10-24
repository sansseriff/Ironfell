//! A large SVG's import, measured (plans/specs/packed-paths.md §8 phase 0):
//! nodes by type, anchors, one document clone and peak memory. Ignored; run
//! with a file:
//!
//! ```sh
//! BIG_SVG=path/to/file.svg cargo test -p iron_svg --target aarch64-apple-darwin \
//!     --release --test big_import -- --ignored --nocapture
//! ```

use std::collections::BTreeMap;
use std::time::Instant;

/// This process's resident memory, MB, as `ps` reports it.
fn rss_mb() -> f64 {
    let out = std::process::Command::new("ps").args(["-o", "rss=", "-p", &std::process::id().to_string()]).output().expect("ps runs");
    String::from_utf8_lossy(&out.stdout).trim().parse::<f64>().unwrap_or(0.0) / 1024.0
}

#[test]
#[ignore]
fn big_import() {
    let file = std::env::var("BIG_SVG").expect("BIG_SVG names an SVG file");
    let text = std::fs::read_to_string(&file).expect("readable");
    let start = rss_mb();
    let t = Instant::now();
    let imported = iron_svg::import(&text).expect("imports");
    println!("import {:?}: {} nodes, {} anchors, {} definitions, {} references; rss {start:.0} -> {:.0} MB", t.elapsed(), imported.nodes, imported.anchors, imported.definitions, imported.references, rss_mb());
    let doc = &imported.fragment.doc;
    let mut by_type: BTreeMap<&str, usize> = BTreeMap::new();
    for n in doc.nodes() {
        *by_type.entry(n.ty.name()).or_default() += 1;
    }
    println!("by type: {by_type:?}");
    // What every transaction pays (`Store::run` applies to a copy).
    let before = rss_mb();
    let t = Instant::now();
    let copy = doc.clone();
    println!("one document clone: {:?}, +{:.0} MB", t.elapsed(), rss_mb() - before);
    drop(copy);
    // The drop's paste, as `engine_api::document_import_svg_at` makes it.
    let mut store = iron_document::Store::new();
    let t = Instant::now();
    let pasted = iron_document::copy::paste(&store, &imported.fragment, &Default::default(), "import").expect("pastes");
    let input = iron_document::TransactionInput { label: "import SVG".into(), actor: iron_document::Actor::Human, ts: 0, idempotency_key: None, ops: pasted.ops, exact: false };
    store.apply(input).expect("applies");
    println!("paste and apply {:?}: {} live nodes; rss {:.0} MB", t.elapsed(), store.document().nodes().filter(|n| n.is_live()).count(), rss_mb());
}

/// The same file imported with and without references draws the same
/// (instance-cache.md phase D): walking both side by side, a reference
/// standing for its piece placed where the reference is, every node has the
/// same type, placement and local bounds. Ignored; `BIG_SVG` as above.
#[test]
#[ignore]
fn references_draw_what_copies_draw() {
    use iron_document::LeafStruct;
    let file = std::env::var("BIG_SVG").expect("BIG_SVG names an SVG file");
    let text = std::fs::read_to_string(&file).expect("readable");
    let load = |share: bool| -> (iron_document::Store, iron_document::NodeId) {
        let imported = iron_svg::import_with(&text, share).expect("imports");
        let mut store = iron_document::Store::new();
        let ids = store.document().next_ids(1);
        let mut c = iron_document::facet::Composition::default();
        c.dims = iron_document::Slot::Const(iron_document::Value::Enum("2d".into()));
        let mut name = iron_document::facet::Name::default();
        name.text = iron_document::Slot::Const(iron_document::Value::Str("target".into()));
        store.apply(iron_document::TransactionInput { label: "c".into(), actor: iron_document::Actor::Human, ts: 0, idempotency_key: None, ops: vec![iron_document::Op::Create { id: ids[0], ty: iron_document::TypeId::lookup("composition").unwrap(), parent: None, order: iron_document::OrderKey::first(), facets: vec![name.wrap(), c.wrap()] }], exact: false }).unwrap();
        let place = iron_document::copy::Placement { parent: Some(ids[0]), ..Default::default() };
        let pasted = iron_document::copy::paste(&store, &imported.fragment, &place, "import").expect("pastes");
        store.apply(iron_document::TransactionInput { label: "import SVG".into(), actor: iron_document::Actor::Human, ts: 0, idempotency_key: None, ops: pasted.ops, exact: false }).expect("applies");
        (store, ids[0])
    };
    let (fs, fc) = load(false);
    let (ss, sc) = load(true);
    let label = |s: &iron_document::Store, n: iron_document::NodeId| s.document().get::<iron_document::facet::Name>(n).and_then(|x| x.text.constant().and_then(|v| v.as_str().map(str::to_owned))).unwrap_or_default();
    let aff = |s: &iron_document::Store, n: iron_document::NodeId| kurbo::Affine::new(s.local_affine(n).0);
    // A shared node as the flat one it stands for: a reference becomes its
    // piece, placed by the reference.
    let resolve = |n: iron_document::NodeId| -> (iron_document::NodeId, kurbo::Affine) {
        let doc = ss.document();
        match iron_document::expand::reference_template(doc, n) {
            Some(t) => {
                let piece = doc.children(t)[0];
                (piece, aff(&ss, n) * aff(&ss, piece))
            }
            None => (n, aff(&ss, n)),
        }
    };
    fn close(a: kurbo::Affine, b: kurbo::Affine) -> bool {
        a.as_coeffs().iter().zip(b.as_coeffs()).all(|(x, y)| (x - y).abs() < 1e-3)
    }
    let mut stack: Vec<(Vec<iron_document::NodeId>, Vec<iron_document::NodeId>, Vec<String>)> = vec![(fs.document().children(fc).to_vec(), ss.document().children(sc).to_vec(), Vec::new())];
    while let Some((fk, sk, path)) = stack.pop() {
        assert_eq!(fk.len(), sk.len(), "child count differs at {path:#?}");
        for (f, s) in fk.into_iter().zip(sk) {
            let (piece, sa) = resolve(s);
            let fa = aff(&fs, f);
            let (ft, st) = (fs.document().live(f).unwrap().ty.name(), ss.document().live(piece).unwrap().ty.name());
            let mut here = path.clone();
            here.push(format!("{ft} {} / {} {}", label(&fs, f), if piece != s { "ref→" } else { "" }, label(&ss, piece)));
            let lb = |st: &iron_document::Store, n: iron_document::NodeId| st.local_bounds(n).map(|b| [b.geometry.x0, b.geometry.y0, b.geometry.x1, b.geometry.y1].map(|v| (v * 1000.0).round() as i64));
            assert!(ft == st && close(fa, sa) && lb(&fs, f) == lb(&ss, piece), "differs at {here:#?}: flat {:?} {:?}, shared {:?} {:?}", fa.as_coeffs(), lb(&fs, f), sa.as_coeffs(), lb(&ss, piece));
            stack.push((fs.document().children(f).to_vec(), ss.document().children(piece).to_vec(), here));
        }
    }
    println!("flat and shared imports draw alike");
}
