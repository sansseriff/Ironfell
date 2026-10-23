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
    println!("import {:?}: {} nodes, {} anchors; rss {start:.0} -> {:.0} MB", t.elapsed(), imported.nodes, imported.anchors, rss_mb());
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
