//! Writes `public/fonts/previews.json`: each catalog family's name drawn
//! in its own face, as an SVG path, for the font picker
//! (plans/specs/text.md §4). The picker shows every family without
//! fetching any font file; the paths come from the same layout and outline
//! code the canvas draws with.
//!
//!     cargo run -p iron_text --bin font_previews --target aarch64-apple-darwin
//!
//! Rerun after adding a family to `catalog::FAMILIES`.

use iron_text::catalog::FAMILIES;
use std::path::PathBuf;

/// Preview text height, px; the picker scales the SVG.
const SIZE: f32 = 20.0;

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../public/fonts");
    for f in FAMILIES {
        let bytes = std::fs::read(root.join(f.file)).unwrap_or_else(|e| panic!("{}: {e}", f.file));
        let names = iron_text::engine().register(bytes);
        assert!(names.iter().any(|n| n == f.name), "{} registered as {names:?}", f.name);
    }
    let names: Vec<String> = FAMILIES.iter().map(|f| f.name.to_owned()).collect();
    let json = serde_json::json!({ "size": SIZE, "families": iron_text::previews_json(&names, SIZE) });
    let path = root.join("previews.json");
    std::fs::write(&path, serde_json::to_string(&json).unwrap()).unwrap();
    println!("wrote {} ({} families, {} bytes)", path.display(), FAMILIES.len(), std::fs::metadata(&path).unwrap().len());
}
