use iron_document::{Actor, NodeId, Store, TransactionInput, Value, edit};
use iron_svg::{Scope, export, import_math};

const SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="800" height="600" viewBox="0 -300 800 600">
  <g transform="scale(1,-1)">
    <g id="iron-part-num" transform="translate(100,200)">
      <path fill-rule="evenodd" d="M0 0H200V100H0Z M25 25H175V75H25Z"/>
      <rect x="250" y="0" width="75" height="100"/>
    </g>
    <rect x="0" y="-10" width="800" height="20"/>
    <g id="iron-part-den"><rect x="100" y="-300" width="200" height="200"/></g>
  </g>
</svg>"#;

fn apply(store: &mut Store, xml: &str) -> edit::Plan {
    let math = |_, _: &str, _: &str, size| import_math(SVG, size);
    let plan = edit::plan_with_math(store, xml, None, Some(&math))
        .unwrap_or_else(|failure| panic!("{}", failure.message()));
    store
        .apply(TransactionInput {
            label: "math test".into(),
            actor: Actor::Model,
            ts: 0,
            idempotency_key: None,
            ops: plan.ops.clone(),
            exact: true,
        })
        .unwrap();
    plan
}

fn read(store: &Store, node: NodeId, path: &str) -> Value {
    store
        .resolved(node, path.parse().unwrap())
        .unwrap_or_else(|| panic!("unresolved {node}.{path}"))
}

fn path_child(store: &Store, group: NodeId) -> NodeId {
    let doc = store.tree();
    doc.children(group)
        .iter()
        .copied()
        .find(|&id| doc.live(id).unwrap().ty.name() == "path")
        .unwrap()
}

#[test]
fn named_math_paths_animate_and_keep_their_group_identity_after_revision() {
    let mut store = Store::new();
    let initial = apply(
        &mut store,
        r##"<edit>
      <composition id="$scene" name="scene" dims="2d"/>
      <timeline id="$timeline" name="timeline"/>
      <let in="$scene" id="$write" value="0"/>
      <typeset-math in="$scene" id="$equation" x="80" y="100" size="50"
        source="\frac{\ironpart{num}{x+1}}{\ironpart{den}{x-1}}">
        <part key="num" id="$num" fill.color="#ff0000" draw.bind="$write"/>
        <part key="den" id="$den" fill.color="#0000ff"/>
      </typeset-math>
      <clip in="$timeline" id="$clip" start="0" prop="let.value">
        <key t="0" value="0"/><key t="1" value="1"/><key t="2" value="1"/><key t="3" value="0"/>
      </clip>
      <clip in="$timeline" id="$move" start="0" prop="transform2d.y">
        <key t="0" value="0"/><key t="3" value="40"/>
      </clip>
      <link from="$clip" to="$write" rel="animates"/>
      <link from="$move" to="$den" rel="animates"/>
    </edit>"##,
    );
    let (eq, num, den) = (
        initial.aliases["$equation"],
        initial.aliases["$num"],
        initial.aliases["$den"],
    );
    let glyph = path_child(&store, num);
    let denominator = path_child(&store, den);
    let doc = store.tree();
    let outlines = doc.live(num).unwrap().parent.unwrap();
    let rule = path_child(&store, outlines);
    assert!(read(&store, rule, "fill.color").as_color().is_some());
    let bounds = iron_document::contour::geometry(&store, glyph).bounds();
    assert_eq!(
        (bounds.x0, bounds.y0, bounds.width(), bounds.height()),
        (5.0, 0.0, 10.0, 5.0)
    );
    assert_eq!(
        read(&store, glyph, "path.fill_rule").as_str(),
        Some("evenodd")
    );
    assert_eq!(
        read(&store, glyph, "fill.color").as_color(),
        Some([1.0, 0.0, 0.0, 1.0])
    );
    assert_eq!(
        read(&store, denominator, "fill.color").as_color(),
        Some([0.0, 0.0, 1.0, 1.0])
    );
    for (t, trim, fill) in [
        (0.0, 0.0, 0.0),
        (1.0, 1.0, 1.0),
        (3.0, 0.0, 0.0),
        (1.0, 1.0, 1.0),
    ] {
        store.set_time(t);
        assert!(store.time_errors().is_empty(), "{:?}", store.time_errors());
        assert_eq!(read(&store, glyph, "trim.end").as_f64(), Some(trim));
        assert_eq!(read(&store, glyph, "fill.opacity").as_f64(), Some(fill));
        assert_eq!(
            read(&store, denominator, "fill.opacity").as_f64(),
            Some(1.0)
        );
    }
    assert!(
        export(&store, &Scope::Composition(initial.aliases["$scene"]))
            .unwrap()
            .svg
            .contains("#ff0000")
    );

    let mut store = Store::load(&store.save()).unwrap();
    let updated = apply(
        &mut store,
        &format!(
            r#"<edit>
      <typeset-math id="{eq}" source="\frac{{\ironpart{{num}}{{x+2}}}}{{\ironpart{{den}}{{x-1}}}}" size="100">
        <part key="num" id="$again"/>
      </typeset-math>
    </edit>"#
        ),
    );
    assert_eq!(updated.aliases["$again"], num);
    assert!(!store.tree().is_live(glyph));
    let new_glyph = path_child(&store, num);
    assert_eq!(
        iron_document::contour::geometry(&store, new_glyph)
            .bounds()
            .width(),
        20.0
    );
    assert_eq!(
        read(&store, new_glyph, "fill.color").as_color(),
        Some([1.0, 0.0, 0.0, 1.0])
    );
    store.set_time(3.0);
    assert_eq!(read(&store, new_glyph, "trim.end").as_f64(), Some(0.0));
    assert_eq!(read(&store, den, "transform2d.y").as_f64(), Some(40.0));
    store.undo(0).unwrap().unwrap();
    assert!(store.tree().is_live(glyph));
    assert!(!store.tree().is_live(new_glyph));
}

#[test]
fn duplicate_and_nested_parts_are_rejected() {
    for (svg, message) in [
        (SVG.replace("iron-part-den", "iron-part-num"), "duplicate"),
        (
            SVG.replace(
                "<rect x=\"250\"",
                "<g id=\"iron-part-inner\"><rect x=\"250\"",
            )
            .replace(
                "width=\"75\" height=\"100\"/>",
                "width=\"75\" height=\"100\"/></g>",
            ),
            "nested",
        ),
    ] {
        assert!(import_math(&svg, 48.0).err().unwrap().contains(message));
    }
}
