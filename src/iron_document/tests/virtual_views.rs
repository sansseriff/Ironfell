//! EN-5a integration tests: materialized-view parity oracle across standard
//! components, shared entities with multiple notations, sized vs unsized
//! pictures, mains, nested components, overrides, and demo compositions.

use iron_document::*;

fn apply_xml(store: &mut StoreCore, xml: &str) -> std::collections::BTreeMap<String, NodeId> {
    let plan = edit::plan(store, xml, None).unwrap_or_else(|f| panic!("{}", f.message()));
    let aliases = plan.aliases.clone();
    store
        .apply(TransactionInput {
            label: "edit".into(),
            actor: Actor::Human,
            ts: 0,
            idempotency_key: None,
            ops: plan.ops,
            exact: false,
        })
        .unwrap_or_else(|e| panic!("apply failed: {e:?}"));
    aliases
}

#[test]
fn parity_across_all_standard_components_and_multi_picture_entity() {
    let mut store = StoreCore::new();
    let ids = apply_xml(
        &mut store,
        r#"<edit>
          <composition id="$c" name="Sheet" dims="2d"/>
        </edit>"#,
    );
    let comp = ids["$c"];

    // Place standard components into the composition.
    let meter_use = store.place_standard("meter", comp, [0.0, 0.0], Actor::Human).unwrap();
    let meter_entity = expand::use_entity(store.tree(), meter_use).expect("meter figures its entity");
    let meter_def = expand::linked_definition(store.tree(), meter_entity).expect("meter definition");
    let _laser = store.place_standard("laser", comp, [180.0, 0.0], Actor::Human).unwrap();
    let _slider = store.place_standard("slider", comp, [360.0, 0.0], Actor::Human).unwrap();
    let _fiber = store.place_standard("fiber", comp, [540.0, 0.0], Actor::Human).unwrap();
    let _readout = store.place_standard("readout", comp, [720.0, 0.0], Actor::Human).unwrap();

    // Add a "schematic" template to meter's definition AND a second picture of
    // the same meter entity using that notation!
    apply_xml(
        &mut store,
        &format!(
            r#"<edit>
              <template id="$ms" in="{meter_def}" notation="schematic">
                <ellipse id="$ms_ring" name="ring" w="32" h="32"/>
              </template>
            </edit>"#
        ),
    );
    let extra_use = store.authored().next_id();
    let extra_rel = store.authored().next_relation_id();
    let mut u = facet::Use::default();
    u.notation = Slot::Const(Value::Str("schematic".into()));
    let mut tr = facet::Transform2d::default();
    tr.x = Slot::Const(Value::Number(0.0));
    tr.y = Slot::Const(Value::Number(220.0));
    store
        .apply(TransactionInput {
            label: "second picture".into(),
            actor: Actor::Human,
            ts: 1,
            idempotency_key: None,
            ops: vec![
                Op::Create {
                    id: extra_use,
                    ty: TypeId::lookup("use").unwrap(),
                    parent: Some(comp),
                    order: OrderKey::after(&OrderKey::first()),
                    facets: vec![u.wrap(), tr.wrap()],
                },
                Op::Link {
                    rel: Relation {
                        id: extra_rel,
                        from: extra_use,
                        to: meter_entity,
                        rel: RelKind::Figures,
                        data: Default::default(),
                    },
                },
            ],
            exact: false,
        })
        .unwrap();

    store.check_virtual_views_parity().expect("virtual views must match Docs::copies bit-for-bit");

    // Verify sparse borrowing: pure constant template nodes borrow straight from Arc<Template> with 0 clones!
    let (views, vnodes, resolved, borrowed) = store.virtual_views_stats();
    assert!(views >= 11, "expected views for 5 entities + 6 uses, got {views}");
    assert!(vnodes > 20, "expected virtual nodes across standard components, got {vnodes}");
    assert!(borrowed > 0, "unpatched constant template nodes must borrow directly (resolved={resolved}, borrowed={borrowed})");
}

#[test]
fn parity_for_nested_components_mains_sized_pictures_and_overrides() {
    let mut store = StoreCore::new();
    let ids = apply_xml(
        &mut store,
        r##"<edit>
          <library id="$lib">
            <definition id="$leaf_def" name="Leaf">
              <port_spec id="$lp" name="level" kind="value" dir="in" signal="0.5"/>
              <template id="$lt" notation="free" w="100" h="40">
                <rect id="$bg" name="bg" w.bind="$lt.size.w" h.bind="$lt.size.h" fill.color="#223344"/>
                <rect id="$static_badge" name="badge" x="4" y="4" w="8" h="8" fill.color="#ffffff"/>
                <rect id="$bar" name="bar" w.bind="$lp.port.signal * $lt.size.w" h="20" fill.color="#00cc88"/>
              </template>
            </definition>
            <definition id="$box_def" name="Box">
              <port_spec id="$bp" name="level" kind="value" dir="in" signal="0.25"/>
              <entity id="$inner_e" name="inner" instance.def="$leaf_def" instance.mode="linked" args.level.bind="$bp.port.signal"/>
              <template id="$bt" notation="free" w="200" h="80">
                <rect id="$frame" name="frame" w="200" h="80" fill.color="#111111"/>
                <use id="$inner_u" notation="free" w="120" h="50" x="10" y="10"/>
              </template>
            </definition>
          </library>
          <composition id="$comp" name="MainSheet" dims="2d">
            <entity id="$e1" name="e1" instance.def="$box_def" instance.mode="linked" args.level="0.75"/>
            <use id="$u1_unsized" notation="free" x="0" y="0"/>
            <use id="$u1_sized" notation="free" x="250" y="0" w="300" h="100"/>
            <use id="$main_u" notation="free" x="0" y="150" instance.def="$leaf_def" instance.mode="main"/>
          </composition>
          <link from="$inner_u" to="$inner_e" rel="figures"/>
          <link from="$u1_unsized" to="$e1" rel="figures"/>
          <link from="$u1_sized" to="$e1" rel="figures"/>
        </edit>"##,
    );

    // Also add a nested override targeting the inner static_badge inside u1_unsized!
    let copy_badge = NodeId::from_path(&[ids["$u1_unsized"], ids["$inner_u"], ids["$static_badge"]]).unwrap();
    store
        .apply(TransactionInput {
            label: "override nested badge".into(),
            actor: Actor::Human,
            ts: 1,
            idempotency_key: None,
            ops: vec![Op::Set {
                id: copy_badge,
                path: "size.w".parse().unwrap(),
                slot: Slot::Const(Value::Number(16.0)),
            }],
            exact: false,
        })
        .unwrap();

    store
        .check_virtual_views_parity()
        .expect("nested components, sized vs unsized pictures, mains, and overrides must match bit-for-bit");

    // Edit the definition and re-check parity (verifies recorded-read TemplateCache invalidation + parity!).
    store
        .apply(TransactionInput {
            label: "edit leaf template".into(),
            actor: Actor::Human,
            ts: 2,
            idempotency_key: None,
            ops: vec![Op::Set {
                id: ids["$static_badge"],
                path: "size.h".parse().unwrap(),
                slot: Slot::Const(Value::Number(12.0)),
            }],
            exact: false,
        })
        .unwrap();

    store
        .check_virtual_views_parity()
        .expect("parity must hold after definition edit via recorded-read TemplateCache");
}

#[test]
fn parity_across_all_ten_gallery_pages() {
    const PAGES: [(&str, &str); 10] = [
        ("oscillator", include_str!("../../document_bridge/gallery/oscillator.xml")),
        ("refraction", include_str!("../../document_bridge/gallery/refraction.xml")),
        ("interference", include_str!("../../document_bridge/gallery/interference.xml")),
        ("wave packet", include_str!("../../document_bridge/gallery/wavepacket.xml")),
        ("fourier", include_str!("../../document_bridge/gallery/fourier.xml")),
        ("kepler", include_str!("../../document_bridge/gallery/kepler.xml")),
        ("lorenz", include_str!("../../document_bridge/gallery/lorenz.xml")),
        ("blackbody", include_str!("../../document_bridge/gallery/blackbody.xml")),
        ("enzyme", include_str!("../../document_bridge/gallery/enzyme.xml")),
        ("RC charging", include_str!("../../document_bridge/gallery/rc.xml")),
    ];
    for (name, raw) in PAGES {
        let mut store = StoreCore::new();
        let ids = store.authored().next_ids(2);
        let mut mname = facet::Name::default();
        mname.text = Slot::Const(Value::Str("model".into()));
        let mut tname = facet::Name::default();
        tname.text = Slot::Const(Value::Str("timeline".into()));
        store
            .apply(TransactionInput {
                label: "desk".into(),
                actor: Actor::System,
                ts: 0,
                idempotency_key: None,
                ops: vec![
                    Op::Create { id: ids[0], ty: TypeId::lookup("model").unwrap(), parent: None, order: OrderKey::first(), facets: vec![mname.wrap()] },
                    Op::Create { id: ids[1], ty: TypeId::lookup("timeline").unwrap(), parent: None, order: OrderKey::after(&OrderKey::first()), facets: vec![tname.wrap()] },
                ],
                exact: false,
            })
            .unwrap();
        let src = raw.replace("#TIMELINE", &ids[1].to_string());
        let (tx, _) = edit::transaction(&store, &src, name, Actor::System).unwrap_or_else(|e| panic!("{name}: {e}"));
        store.apply(tx).unwrap_or_else(|e| panic!("{name} apply failed: {e:?}"));
        store
            .check_virtual_views_parity()
            .unwrap_or_else(|e| panic!("gallery page {name:?} failed virtual views parity: {e}"));
    }
}

/// EN-5b: Variants as scope-chain layers (`anchor: Some(def)`), `PatchKey::Notation`,
/// `set_point` live switching across instances, outer instance override precedence,
/// and virtual-view parity at every point.
#[test]
fn variants_as_scope_chain_layers_and_live_point_switching() {
    use iron_document::patch::{
        Axis, AxisId, AxisKind, AxisOp, AxisValue, AxisValueId, Patch, PatchId, PatchKey, PatchOp, PatchTarget, PatchValue, Point, Product, Sel,
    };
    use std::collections::BTreeMap;

    let mut store = StoreCore::new();
    let (tx, plan) = edit::transaction(
        &store,
        r#"<edit>
          <model name="model"/>
          <composition id="$c" name="scene" dims="2d"/>
          <library name="library">
            <definition id="$cell" name="cell">
              <template notation="free"><rect id="$box" name="box" w="10" h="4"/></template>
              <template notation="alt"><ellipse id="$dot" name="dot" w="3" h="3"/></template>
            </definition>
          </library>
        </edit>"#,
        "setup cell",
        Actor::Human,
    )
    .unwrap();
    store.apply(tx).unwrap();
    let aliases = plan.aliases;
    let cell = aliases["$cell"];
    let library = store.tree().live(cell).unwrap().parent.unwrap();
    let (tx, p2) = edit::transaction(
        &store,
        &format!(
            r#"<edit>
              <definition in="{library}" id="$card" name="card">
                <entity id="$ie" name="inner" instance.def="{cell}" instance.mode="linked"/>
                <template notation="free">
                  <rect id="$frame" name="frame" w="40" h="20"/>
                  <use id="$iu" name="iu" notation="free" x="5"/>
                </template>
              </definition>
            </edit>"#
        ),
        "setup card",
        Actor::Human,
    )
    .unwrap();
    store.apply(tx).unwrap();
    let a2 = p2.aliases;
    let (card, frame, iu, ie) = (a2["$card"], a2["$frame"], a2["$iu"], a2["$ie"]);
    let rel = store.authored().next_relation_id();
    store
        .apply(TransactionInput {
            label: "figure".into(),
            actor: Actor::Human,
            ts: 0,
            idempotency_key: None,
            ops: vec![Op::Link { rel: Relation { id: rel, from: iu, to: ie, rel: RelKind::Figures, data: BTreeMap::new() } }],
            exact: false,
        })
        .unwrap();

    // Place two instances of `card`.
    let c = aliases["$c"];
    let (tx, _) = edit::transaction(
        &store,
        &format!(r#"<edit><place def="{card}" in="{c}" x="0" y="0"/><place def="{card}" in="{c}" x="100" y="0"/></edit>"#),
        "place two cards",
        Actor::Human,
    )
    .unwrap();
    store.apply(tx).unwrap();
    let uses: Vec<NodeId> = store
        .tree()
        .children(c)
        .iter()
        .copied()
        .filter(|&n| !n.is_expanded() && store.tree().live(n).is_some_and(|x| x.ty.name() == "use"))
        .collect();
    assert_eq!(uses.len(), 2);
    let (u1, u2) = (uses[0], uses[1]);

    // Add variant axis `Size` (`sm`, `lg`) owned by `card`, and an anchored variant
    // patch (`anchor: Some(card)`, `Size = lg`) that widens `$frame` to 120 and
    // switches `$iu`'s notation via `PatchKey::Notation` to `"alt"`.
    let ax_id = AxisId(1);
    let sm = AxisValueId(1);
    let lg = AxisValueId(2);
    let variant_pid = PatchId(1);
    let region = Product::default().with(ax_id, Sel::Set { values: [lg].into_iter().collect() });
    let mut entries = BTreeMap::new();
    entries.insert(
        PatchTarget { node: frame, key: PatchKey::Leaf("size.w".parse().unwrap()) },
        PatchValue::Slot { slot: Slot::Const(Value::Number(120.0)) },
    );
    entries.insert(
        PatchTarget { node: iu, key: PatchKey::Notation },
        PatchValue::Notation { notation: "alt".into() },
    );
    store
        .apply(TransactionInput {
            label: "add variant axis and patch".into(),
            actor: Actor::Human,
            ts: 0,
            idempotency_key: None,
            ops: vec![
                Op::Axis(AxisOp::Add {
                    axis: Axis {
                        id: ax_id,
                        name: "Size".into(),
                        kind: AxisKind::Unordered,
                        owner: Some(card),
                        values: vec![AxisValue::new(sm, "sm"), AxisValue::new(lg, "lg")],
                    },
                }),
                Op::Patch(PatchOp::Add {
                    patch: Patch { id: variant_pid, layer: 0, rank: OrderKey::first(), region, anchor: Some(card), writable: true, entries },
                }),
            ],
            exact: false,
        })
        .unwrap();

    // Override `u1`'s frame width via normal `Op::Set` (translated to document-level
    // layer-0 instance-axis patch).
    let u1_frame = NodeId::expanded(u1, frame).unwrap();
    let u2_frame = NodeId::expanded(u2, frame).unwrap();
    store
        .apply(TransactionInput {
            label: "override u1 frame".into(),
            actor: Actor::Human,
            ts: 0,
            idempotency_key: None,
            ops: vec![Op::Set { id: u1_frame, path: "size.w".parse().unwrap(), slot: Slot::Const(Value::Number(200.0)) }],
            exact: false,
        })
        .unwrap();

    let (bx, dot) = (aliases["$box"], aliases["$dot"]);
    let w_of = |s: &StoreCore, n: NodeId| s.resolved(n, "size.w".parse().unwrap()).and_then(|v| v.as_f64()).unwrap();

    // At `sm` (default): u1=200 (override), u2=40 (base), both show `box` (`free`).
    assert_eq!(w_of(&store, u1_frame), 200.0);
    assert_eq!(w_of(&store, u2_frame), 40.0);
    assert!(store.tree().is_live(NodeId::from_path(&[u1, iu, bx]).unwrap()));
    assert!(store.tree().is_live(NodeId::from_path(&[u2, iu, bx]).unwrap()));
    store.check_virtual_views_parity().expect("parity at sm");

    // Switch live to `lg`: outer document-level override on u1 (layer 0) still beats
    // anchored variant (layer 1); u2 widens to 120; BOTH switch inner notation to `alt`!
    store.set_point(Point::default().with_value(ax_id, lg));
    assert_eq!(w_of(&store, u1_frame), 200.0, "outer instance override beats inner variant layer");
    assert_eq!(w_of(&store, u2_frame), 120.0, "variant patch widens unoverridden instance");
    assert!(!store.tree().is_live(NodeId::from_path(&[u1, iu, bx]).unwrap()));
    assert!(store.tree().is_live(NodeId::from_path(&[u1, iu, dot]).unwrap()));
    assert!(!store.tree().is_live(NodeId::from_path(&[u2, iu, bx]).unwrap()));
    assert!(store.tree().is_live(NodeId::from_path(&[u2, iu, dot]).unwrap()));
    store.check_virtual_views_parity().expect("parity at lg");

    // Switch back to `sm`: reverts cleanly.
    store.set_point(Point::default().with_value(ax_id, sm));
    assert_eq!(w_of(&store, u1_frame), 200.0);
    assert_eq!(w_of(&store, u2_frame), 40.0);
    assert!(store.tree().is_live(NodeId::from_path(&[u1, iu, bx]).unwrap()));
    assert!(store.tree().is_live(NodeId::from_path(&[u2, iu, bx]).unwrap()));
    store.check_virtual_views_parity().expect("parity back at sm");
}

/// EN-5b: `make_component` migrates document-level instance override patches onto
/// `anchor: Some(new_def)` patches, preserves effective values, and lets outer
/// overrides beat the migrated inner patch.
#[test]
fn make_component_migrates_override_patches_to_anchored_definition_patches() {
    let mut store = StoreCore::new();
    let (tx, plan) = edit::transaction(
        &store,
        r#"<edit><model name="model"/><composition id="$c" name="scene" dims="2d"/></edit>"#,
        "desk",
        Actor::Human,
    )
    .unwrap();
    store.apply(tx).unwrap();
    let c = plan.aliases["$c"];
    let laser_pic = store.place_standard("laser", c, [0.0, 0.0], Actor::Human).unwrap();
    let laser_box = store.tree().children(laser_pic)[0];
    let (_, template_box) = laser_box.expanded_parts().unwrap();
    let w_path: LeafPath = "size.w".parse().unwrap();

    // Override laser's box width on the placed instance.
    store
        .apply(TransactionInput {
            label: "override laser width".into(),
            actor: Actor::Human,
            ts: 0,
            idempotency_key: None,
            ops: vec![Op::Set { id: laser_box, path: w_path, slot: Slot::Const(Value::Number(140.0)) }],
            exact: false,
        })
        .unwrap();
    assert_eq!(store.resolved(laser_box, w_path), Some(Value::Number(140.0)));
    assert_eq!(store.tree().patches().patches.len(), 1);
    assert!(store.tree().patches().patches.values().next().unwrap().anchor.is_none());

    // Make a component wrapping the overridden laser.
    let (made, tx) = store.make_component(&[laser_pic], "wrapped", Actor::Human).unwrap();
    store.apply(tx).unwrap();
    let patches: Vec<_> = store.tree().patches().patches.values().collect();
    assert_eq!(patches.len(), 1, "old document-level patch replaced by one anchored patch");
    assert_eq!(patches[0].anchor, Some(made.definition));

    // The nested copy inside the new wrapped instance still resolves to 140.0!
    let inner_pic = store.tree().children(made.picture)[0];
    let nested_box = NodeId::expanded(inner_pic, template_box).unwrap();
    assert_eq!(store.resolved(nested_box, w_path), Some(Value::Number(140.0)));

    // An outer override on the wrapped instance beats the inner anchored patch.
    store
        .apply(TransactionInput {
            label: "outer override".into(),
            actor: Actor::Human,
            ts: 0,
            idempotency_key: None,
            ops: vec![Op::Set { id: nested_box, path: w_path, slot: Slot::Const(Value::Number(210.0)) }],
            exact: false,
        })
        .unwrap();
    assert_eq!(store.resolved(nested_box, w_path), Some(Value::Number(210.0)));
    store.check_virtual_views_parity().expect("parity with migrated + outer override");

    // Undo restores inner anchored value, then undo make_component restores original.
    store.undo(0).unwrap().unwrap();
    assert_eq!(store.resolved(nested_box, w_path), Some(Value::Number(140.0)));
    store.undo(0).unwrap().unwrap();
    assert_eq!(store.resolved(laser_box, w_path), Some(Value::Number(140.0)));
}

/// EN-5b: `Owners` recorded-read invalidation survives a rolled-back transaction
/// and still wakes the owner on the next committed edit.
#[test]
fn owners_recorded_read_invalidation_survives_rollback() {
    let mut store = StoreCore::new();
    let (tx, plan) = edit::transaction(
        &store,
        r#"<edit>
          <model name="model"/>
          <composition id="$c" name="scene" dims="2d"/>
          <library name="library">
            <definition id="$d1" name="d1"><template notation="free"><rect id="$r1" w="10" h="10"/></template></definition>
            <definition id="$d2" name="d2"><template notation="free"><rect id="$r2" w="20" h="20"/></template></definition>
          </library>
        </edit>"#,
        "setup",
        Actor::Human,
    )
    .unwrap();
    store.apply(tx).unwrap();
    let aliases = plan.aliases;
    let (c, d1, d2, r1, r2) = (aliases["$c"], aliases["$d1"], aliases["$d2"], aliases["$r1"], aliases["$r2"]);
    let (tx, _) = edit::transaction(
        &store,
        &format!(r#"<edit><place def="{d1}" in="{c}" x="0" y="0"/><place def="{d2}" in="{c}" x="50" y="0"/></edit>"#),
        "place both",
        Actor::Human,
    )
    .unwrap();
    store.apply(tx).unwrap();
    let uses: Vec<NodeId> = store
        .tree()
        .children(c)
        .iter()
        .copied()
        .filter(|&n| !n.is_expanded() && store.tree().live(n).is_some_and(|x| x.ty.name() == "use"))
        .collect();
    let (u1, u2) = (uses[0], uses[1]);
    let w_path: LeafPath = "size.w".parse().unwrap();

    // Trigger a rolled-back transaction that mutates `$r1` alongside an invalid op.
    let bad = store.apply(TransactionInput {
        label: "rolled back".into(),
        actor: Actor::Human,
        ts: 0,
        idempotency_key: None,
        ops: vec![
            Op::Set { id: r1, path: w_path, slot: Slot::Const(Value::Number(99.0)) },
            Op::Delete { id: NodeId::expanded(u1, r1).unwrap() },
        ],
        exact: false,
    });
    assert!(bad.is_err());
    assert_eq!(store.resolved(NodeId::expanded(u1, r1).unwrap(), w_path), Some(Value::Number(10.0)));

    // Subsequent valid edit to `$r1` must still wake `u1` and leave `u2` intact.
    store
        .apply(TransactionInput {
            label: "widen r1".into(),
            actor: Actor::Human,
            ts: 0,
            idempotency_key: None,
            ops: vec![Op::Set { id: r1, path: w_path, slot: Slot::Const(Value::Number(55.0)) }],
            exact: false,
        })
        .unwrap();
    assert_eq!(store.resolved(NodeId::expanded(u1, r1).unwrap(), w_path), Some(Value::Number(55.0)));
    assert_eq!(store.resolved(NodeId::expanded(u2, r2).unwrap(), w_path), Some(Value::Number(20.0)));
}
