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

/// EN-5b hardening: collision-free multi-instance rank allocation (C2) & PutGet
/// over a higher-ranked same-scope patch (H3 / Sol #4).
#[test]
fn translate_avoids_rank_collisions_and_preserves_putget() {
    use iron_document::patch::{Axis, AxisId, AxisKind, AxisOp, Patch, PatchId, PatchKey, PatchOp, PatchTarget, PatchValue, Product, Sel};
    use std::collections::BTreeMap;

    let mut store = StoreCore::new();
    let (tx, plan) = edit::transaction(
        &store,
        r#"<edit>
          <model name="model"/>
          <composition id="$c" name="scene" dims="2d"/>
          <library name="library">
            <definition id="$d" name="d"><template notation="free"><rect id="$r" w="10" h="10"/></template></definition>
          </library>
        </edit>"#,
        "setup",
        Actor::Human,
    )
    .unwrap();
    store.apply(tx).unwrap();
    let (c, d, r) = (plan.aliases["$c"], plan.aliases["$d"], plan.aliases["$r"]);
    let (tx, _) = edit::transaction(
        &store,
        &format!(
            r#"<edit>
              <place def="{d}" in="{c}" x="0" y="0"/>
              <place def="{d}" in="{c}" x="50" y="0"/>
              <place def="{d}" in="{c}" x="100" y="0"/>
              <place def="{d}" in="{c}" x="150" y="0"/>
            </edit>"#
        ),
        "place four",
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
    let (u0, u1, u2, u3) = (uses[0], uses[1], uses[2], uses[3]);
    let w_path: LeafPath = "size.w".parse().unwrap();

    // Seed two patches in layer 0 at ranks "a0" (tail len 1 on u0) and "a1" (tail len 2).
    // Then override u1 and u2 in ONE transaction — previously `OrderKey::after("a0V") == "a1"` collided!
    let inst_ax = AxisId(1);
    let r0 = OrderKey::first();
    let r1 = OrderKey::after(&r0);
    store
        .apply(TransactionInput {
            label: "seed ranks".into(),
            actor: Actor::Human,
            ts: 0,
            idempotency_key: None,
            ops: vec![
                Op::Axis(AxisOp::Add {
                    axis: Axis { id: inst_ax, name: "instance".into(), kind: AxisKind::Instance, owner: None, values: Vec::new() },
                }),
                Op::Patch(PatchOp::Add {
                    patch: Patch {
                        id: PatchId(1),
                        layer: 0,
                        rank: r0.clone(),
                        region: Product::default().with(inst_ax, Sel::Suffix { tail: vec![u0], not: Vec::new() }),
                        anchor: None,
                        writable: true,
                        entries: BTreeMap::from([(PatchTarget { node: r, key: PatchKey::Leaf(w_path) }, PatchValue::Slot { slot: Slot::Const(Value::Number(20.0)) })]),
                    },
                }),
                Op::Patch(PatchOp::Add {
                    patch: Patch {
                        id: PatchId(2),
                        layer: 0,
                        rank: r1.clone(),
                        region: Product::default().with(inst_ax, Sel::Suffix { tail: vec![u0, r], not: Vec::new() }),
                        anchor: None,
                        writable: true,
                        entries: BTreeMap::from([(PatchTarget { node: r, key: PatchKey::Leaf(w_path) }, PatchValue::Slot { slot: Slot::Const(Value::Number(25.0)) })]),
                    },
                }),
                // Stronger same-scope patch on u3 (with empty suffix / All) so editing u3 must outrank it (PutGet)!
                Op::Patch(PatchOp::Add {
                    patch: Patch {
                        id: PatchId(3),
                        layer: 0,
                        rank: OrderKey::after(&r1),
                        region: Product::default().with(inst_ax, Sel::Suffix { tail: vec![u3], not: Vec::new() }),
                        anchor: None,
                        writable: false, // read-only higher-ranked patch on u3
                        entries: BTreeMap::from([(PatchTarget { node: r, key: PatchKey::Leaf(w_path) }, PatchValue::Slot { slot: Slot::Const(Value::Number(99.0)) })]),
                    },
                }),
            ],
            exact: false,
        })
        .unwrap();

    // Override u1, u2, and u3 in a SINGLE transaction!
    store
        .apply(TransactionInput {
            label: "multi override".into(),
            actor: Actor::Human,
            ts: 0,
            idempotency_key: None,
            ops: vec![
                Op::Set { id: NodeId::expanded(u1, r).unwrap(), path: w_path, slot: Slot::Const(Value::Number(31.0)) },
                Op::Set { id: NodeId::expanded(u2, r).unwrap(), path: w_path, slot: Slot::Const(Value::Number(32.0)) },
                Op::Set { id: NodeId::expanded(u3, r).unwrap(), path: w_path, slot: Slot::Const(Value::Number(33.0)) },
            ],
            exact: false,
        })
        .unwrap();

    assert_eq!(store.resolved(NodeId::expanded(u1, r).unwrap(), w_path), Some(Value::Number(31.0)));
    assert_eq!(store.resolved(NodeId::expanded(u2, r).unwrap(), w_path), Some(Value::Number(32.0)));
    assert_eq!(store.resolved(NodeId::expanded(u3, r).unwrap(), w_path), Some(Value::Number(33.0)), "PutGet: must outrank stronger same-scope patch");
    store.check_virtual_views_parity().unwrap();
}

/// EN-5b hardening: `(anchor, layer)` lints & `plan_drag` outer-scope guard (H1),
/// P19 time-axis validation (M5), and P21 strictness (no implicit optional facet
/// synthesis on leaf-only patches, Sol #7 / H4).
#[test]
fn anchor_layer_separation_p19_and_p21_strictness() {
    use iron_document::patch::{
        Axis, AxisId, AxisKind, AxisOp, AxisValue, AxisValueId, Bound, Patch, PatchId, PatchKey, PatchOp, PatchTarget, PatchValue, Point, Product, Sel,
    };
    use std::collections::BTreeMap;

    let mut store = StoreCore::new();
    let (tx, plan) = edit::transaction(
        &store,
        r#"<edit>
          <model name="model"/>
          <composition id="$c" name="scene" dims="2d"/>
          <library name="library">
            <definition id="$d" name="d"><template notation="free"><rect id="$r" w="10" h="10"/></template></definition>
          </library>
        </edit>"#,
        "setup",
        Actor::Human,
    )
    .unwrap();
    store.apply(tx).unwrap();
    let (c, d, r) = (plan.aliases["$c"], plan.aliases["$d"], plan.aliases["$r"]);
    let (tx, _) = edit::transaction(&store, &format!(r#"<edit><place def="{d}" in="{c}" x="0" y="0"/></edit>"#), "place", Actor::Human).unwrap();
    store.apply(tx).unwrap();
    let u = store
        .tree()
        .children(c)
        .iter()
        .copied()
        .find(|&n| !n.is_expanded() && store.tree().live(n).is_some_and(|x| x.ty.name() == "use"))
        .unwrap();
    let copy = NodeId::expanded(u, r).unwrap();

    // 1. P21 strictness: a leaf-only patch on a missing optional facet (`stroke.width`)
    //    without `FacetPresent(Stroke) = true` stays inert in BOTH copies and virtual views!
    let inst_ax = AxisId(1);
    let mode_ax = AxisId(2);
    let time_ax = AxisId(3);
    let v1 = AxisValueId(1);
    let stroke_w: LeafPath = "stroke.width".parse().unwrap();
    let w_path: LeafPath = "size.w".parse().unwrap();
    store
        .apply(TransactionInput {
            label: "axes + leaf-only stroke patch".into(),
            actor: Actor::Human,
            ts: 0,
            idempotency_key: None,
            ops: vec![
                Op::Axis(AxisOp::Add {
                    axis: Axis { id: inst_ax, name: "instance".into(), kind: AxisKind::Instance, owner: None, values: Vec::new() },
                }),
                Op::Axis(AxisOp::Add {
                    axis: Axis { id: mode_ax, name: "mode".into(), kind: AxisKind::Unordered, owner: None, values: vec![AxisValue::new(v1, "m1")] },
                }),
                Op::Axis(AxisOp::Add {
                    axis: Axis { id: time_ax, name: "t".into(), kind: AxisKind::Time, owner: None, values: Vec::new() },
                }),
                Op::Patch(PatchOp::Add {
                    patch: Patch {
                        id: PatchId(1),
                        layer: 0,
                        rank: OrderKey::first(),
                        region: Product::default().with(inst_ax, Sel::Suffix { tail: vec![u], not: Vec::new() }),
                        anchor: None,
                        writable: true,
                        entries: BTreeMap::from([(PatchTarget { node: r, key: PatchKey::Leaf(stroke_w) }, PatchValue::Slot { slot: Slot::Const(Value::Number(5.0)) })]),
                    },
                }),
            ],
            exact: false,
        })
        .unwrap();
    assert!(store.tree().facet(copy, FacetKind::Stroke).is_none(), "P21: leaf-only patch must not synthesize Stroke");
    store.check_virtual_views_parity().unwrap();

    // 2. P19: Time-axis patches reject `Present` and `Slot::Bound`.
    let time_region = Product::default().with(time_ax, Sel::Range { lo: Bound::TimeIncl { t: 0.0 }, hi: Bound::TimeIncl { t: 1.0 } });
    assert!(
        store
            .apply(TransactionInput {
                label: "bad time present".into(),
                actor: Actor::Human,
                ts: 0,
                idempotency_key: None,
                ops: vec![Op::Patch(PatchOp::Add {
                    patch: Patch {
                        id: PatchId(2),
                        layer: 0,
                        rank: OrderKey::after(&OrderKey::first()),
                        region: time_region.clone(),
                        anchor: None,
                        writable: true,
                        entries: BTreeMap::from([(PatchTarget { node: r, key: PatchKey::Present }, PatchValue::Bool { v: false })]),
                    },
                })],
                exact: false,
            })
            .is_err(),
        "P19: time-axis patch must reject Present"
    );
    assert!(
        store
            .apply(TransactionInput {
                label: "bad time binding".into(),
                actor: Actor::Human,
                ts: 0,
                idempotency_key: None,
                ops: vec![Op::Patch(PatchOp::Add {
                    patch: Patch {
                        id: PatchId(3),
                        layer: 0,
                        rank: OrderKey::after(&OrderKey::first()),
                        region: time_region,
                        anchor: None,
                        writable: true,
                        entries: BTreeMap::from([(
                            PatchTarget { node: r, key: PatchKey::Leaf(w_path) },
                            PatchValue::Slot { slot: Slot::Bound(Expr::parse("1 + 2").unwrap()) },
                        )]),
                    },
                })],
                exact: false,
            })
            .is_err(),
        "P19: time-axis patch must reject bindings"
    );

    // 3. (anchor, layer) separation: document-level patch (anchor: None, layer: 0)
    //    and definition-anchored patch (anchor: Some(d), layer: 0) on the same target
    //    produce zero false `dead`/`contradictions` lints, and `plan_drag` in the
    //    anchored scope refuses when an outer document patch wins!
    let mode_region = Product::default().with(mode_ax, Sel::Set { values: [v1].into_iter().collect() });
    store
        .apply(TransactionInput {
            label: "outer + inner same layer 0".into(),
            actor: Actor::Human,
            ts: 0,
            idempotency_key: None,
            ops: vec![
                Op::Patch(PatchOp::Add {
                    patch: Patch {
                        id: PatchId(4),
                        layer: 0,
                        rank: OrderKey::first(), // same rank allowed in different anchor scope!
                        region: mode_region.clone(),
                        anchor: Some(d),
                        writable: true,
                        entries: BTreeMap::from([(PatchTarget { node: r, key: PatchKey::Leaf(w_path) }, PatchValue::Slot { slot: Slot::Const(Value::Number(50.0)) })]),
                    },
                }),
                Op::Patch(PatchOp::Add {
                    patch: Patch {
                        id: PatchId(5),
                        layer: 0,
                        rank: OrderKey::after(&OrderKey::first()),
                        region: mode_region.clone(),
                        anchor: None,
                        writable: true,
                        entries: BTreeMap::from([(PatchTarget { node: r, key: PatchKey::Leaf(w_path) }, PatchValue::Slot { slot: Slot::Const(Value::Number(80.0)) })]),
                    },
                }),
            ],
            exact: false,
        })
        .unwrap();
    let lints = store.patch_lints();
    assert!(lints.dead.is_empty(), "distinct anchors must not mark inner patch dead: {:?}", lints.dead);
    assert!(lints.contradictions.is_empty(), "distinct anchors must not report contradiction: {:?}", lints.contradictions);
    let pt = Point::default().with_value(mode_ax, v1);
    let drag_err = iron_document::patch::plan_drag(
        store.authored().patches(),
        &pt,
        &mode_region,
        Some(d),
        PatchTarget { node: r, key: PatchKey::Leaf(w_path) },
        PatchValue::Slot { slot: Slot::Const(Value::Number(60.0)) },
        0,
        true,
        true,
        None,
    );
    assert!(drag_err.is_err(), "outer document patch must block inner anchored drag: {drag_err:?}");
}

/// EN-5b hardening: live `set_point` updates `repeat.count` via patch, cleans up
/// derived copies when an authored `use` becomes `Present = false`, and routes
/// edits under a notation-patched `main` to the active template (Sol #1 & #2).
#[test]
fn set_point_repeats_absent_use_and_main_notation_routing() {
    use iron_document::patch::{Axis, AxisId, AxisKind, AxisOp, AxisValue, AxisValueId, Patch, PatchId, PatchKey, PatchOp, PatchTarget, PatchValue, Point, Product, Sel};
    use std::collections::BTreeMap;

    let mut store = StoreCore::new();
    let (tx, plan) = edit::transaction(
        &store,
        r#"<edit>
          <model name="model"/>
          <composition id="$c" name="scene" dims="2d">
            <group id="$rep" name="rep" repeat.count="1"><rect id="$cell" w="5" h="5"/></group>
          </composition>
          <library name="library">
            <definition id="$d" name="d">
              <template id="$t_free" notation="free"><rect id="$box" name="box" w="10" h="10"/></template>
              <template id="$t_alt" notation="alt"><ellipse id="$dot" name="dot" w="4" h="4"/></template>
            </definition>
          </library>
        </edit>"#,
        "setup",
        Actor::Human,
    )
    .unwrap();
    store.apply(tx).unwrap();
    let (c, rep, d, t_alt, bx) = (plan.aliases["$c"], plan.aliases["$rep"], plan.aliases["$d"], plan.aliases["$t_alt"], plan.aliases["$box"]);
    let (tx, p2) = edit::transaction(
        &store,
        &format!(
            r#"<edit>
              <place def="{d}" in="{c}" x="0" y="0"/>
              <use id="$main" in="{c}" notation="free" instance.def="{d}" instance.mode="main"/>
            </edit>"#
        ),
        "place + main",
        Actor::Human,
    )
    .unwrap();
    store.apply(tx).unwrap();
    let main_u = p2.aliases["$main"];
    let inst_u = store
        .tree()
        .children(c)
        .iter()
        .copied()
        .find(|&n| !n.is_expanded() && n != main_u && store.tree().live(n).is_some_and(|x| x.ty.name() == "use"))
        .unwrap();

    let mode_ax = AxisId(1);
    let m0 = AxisValueId(1);
    let m1 = AxisValueId(2);
    let m1_region = Product::default().with(mode_ax, Sel::Set { values: [m1].into_iter().collect() });
    store
        .apply(TransactionInput {
            label: "mode patches".into(),
            actor: Actor::Human,
            ts: 0,
            idempotency_key: None,
            ops: vec![
                Op::Axis(AxisOp::Add {
                    axis: Axis {
                        id: mode_ax,
                        name: "mode".into(),
                        kind: AxisKind::Unordered,
                        owner: None,
                        values: vec![AxisValue::new(m0, "m0"), AxisValue::new(m1, "m1")],
                    },
                }),
                Op::Patch(PatchOp::Add {
                    patch: Patch {
                        id: PatchId(1),
                        layer: 0,
                        rank: OrderKey::first(),
                        region: m1_region,
                        anchor: None,
                        writable: true,
                        entries: BTreeMap::from([
                            (PatchTarget { node: rep, key: PatchKey::Leaf("repeat.count".parse().unwrap()) }, PatchValue::Slot { slot: Slot::Const(Value::Number(3.0)) }),
                            (PatchTarget { node: inst_u, key: PatchKey::Present }, PatchValue::Bool { v: false }),
                            (PatchTarget { node: main_u, key: PatchKey::Notation }, PatchValue::Notation { notation: "alt".into() }),
                        ]),
                    },
                }),
            ],
            exact: false,
        })
        .unwrap();

    // At m0: 1 repeat item, inst_u's copy live, main_template is t_free.
    assert!(store.tree().is_live(NodeId::item_scope(rep, "0").unwrap()));
    assert!(!store.tree().is_live(NodeId::item_scope(rep, "2").unwrap()));
    assert!(store.tree().is_live(NodeId::expanded(inst_u, bx).unwrap()));

    // Switch to m1:
    // 1. repeat settles live to 3 items!
    // 2. absent authored `inst_u` cleans up its derived copy!
    // 3. `main_template` routes to `t_alt`!
    store.set_point(Point::default().with_value(mode_ax, m1));
    assert!(store.tree().is_live(NodeId::item_scope(rep, "2").unwrap()), "set_point must settle patched repeat.count");
    assert!(!store.tree().is_live(NodeId::expanded(inst_u, bx).unwrap()), "absent authored use must clean up its derived copies");
    assert_eq!(expand::main_template(store.tree(), main_u), Some(t_alt), "main_template must follow PatchKey::Notation");

    // Creating a child inside `main_u` routes into `t_alt` (not `t_free`)!
    let new_rect = store.authored().next_id();
    store
        .apply(TransactionInput {
            label: "create in alt main".into(),
            actor: Actor::Human,
            ts: 0,
            idempotency_key: None,
            ops: vec![Op::Create {
                id: new_rect,
                ty: TypeId::lookup("rect").unwrap(),
                parent: Some(main_u),
                order: OrderKey::first(),
                facets: vec![FacetKind::Transform2d.default_facet(), FacetKind::Size.default_facet()],
            }],
            exact: false,
        })
        .unwrap();
    assert_eq!(store.tree().live(new_rect).unwrap().parent, Some(t_alt));
}

#[test]
fn repeat_nested_use_view_coord_and_make_component_patch_order() {
    use iron_document::patch::{Axis, AxisId, AxisKind, AxisOp, Patch, PatchId, PatchKey, PatchOp, PatchTarget, PatchValue, Product, Sel};
    use std::collections::BTreeMap;

    // 1. Verify ViewCoord explicit path start offset for an anchored patch inside `t_card`
    //    targeting a nested `use` inside a `repeat` (`outer_u -> rep -> ~0 -> inner_u -> leaf_bx`).
    let mut store = StoreCore::new();
    let ids = apply_xml(
        &mut store,
        r#"<edit>
          <model name="model"/>
          <composition id="$c" name="scene" dims="2d"/>
          <library id="$lib" name="library">
            <definition id="$d_leaf" name="leaf">
              <template id="$t_leaf" notation="free">
                <rect id="$leaf_bx" name="box" x="1" w="10" h="10"/>
              </template>
            </definition>
          </library>
        </edit>"#,
    );
    let (c, lib, d_leaf, leaf_bx) = (ids["$c"], ids["$lib"], ids["$d_leaf"], ids["$leaf_bx"]);
    let ids_card = apply_xml(
        &mut store,
        &format!(
            r#"<edit>
              <definition in="{lib}" id="$d_card" name="card">
                <entity id="$ie" name="ie" instance.def="{d_leaf}" instance.mode="linked"/>
                <template id="$t_card" notation="free">
                  <group id="$rep" name="rep" repeat.count="2">
                    <use id="$inner_u" name="inner" notation="free"/>
                  </group>
                </template>
              </definition>
            </edit>"#
        ),
    );
    let (d_card, ie, rep, inner_u) = (ids_card["$d_card"], ids_card["$ie"], ids_card["$rep"], ids_card["$inner_u"]);
    let rel_id = store.authored().next_relation_id();
    store
        .apply(TransactionInput {
            label: "figure inner".into(),
            actor: Actor::Human,
            ts: 0,
            idempotency_key: None,
            ops: vec![Op::Link { rel: Relation { id: rel_id, from: inner_u, to: ie, rel: RelKind::Figures, data: BTreeMap::new() } }],
            exact: false,
        })
        .unwrap();
    apply_xml(
        &mut store,
        &format!(r#"<edit><place def="{d_card}" in="{c}" x="0" y="0"/></edit>"#),
    );
    let outer_u = store
        .tree()
        .children(c)
        .iter()
        .copied()
        .find(|&n| !n.is_expanded() && store.tree().live(n).is_some_and(|x| x.ty.name() == "use"))
        .unwrap();

    let copied_rep = NodeId::expanded(outer_u, rep).unwrap();
    let pic0 = NodeId::item(copied_rep, "0", inner_u).unwrap();
    let pic1 = NodeId::item(copied_rep, "1", inner_u).unwrap();
    let leaf0 = NodeId::expanded(pic0, leaf_bx).unwrap();
    let leaf1 = NodeId::expanded(pic1, leaf_bx).unwrap();
    let step0: NodeId = "#~0".parse().unwrap();

    let inst_ax = AxisId(1);
    store
        .apply(TransactionInput {
            label: "anchored repeat override".into(),
            actor: Actor::Human,
            ts: 0,
            idempotency_key: None,
            ops: vec![
                Op::Axis(AxisOp::Add { axis: Axis::new(inst_ax, "instance", AxisKind::Instance, None) }),
                Op::Patch(PatchOp::Add {
                    patch: Patch {
                        id: PatchId(1),
                        layer: 0,
                        rank: OrderKey::first(),
                        region: Product::default().with(inst_ax, Sel::Suffix { tail: vec![rep, step0, inner_u], not: vec![] }),
                        anchor: Some(d_card),
                        writable: true,
                        entries: BTreeMap::from([(
                            PatchTarget { node: leaf_bx, key: PatchKey::Leaf("transform2d.x".parse().unwrap()) },
                            PatchValue::Slot { slot: Slot::Const(Value::Number(42.0)) },
                        )]),
                    },
                }),
            ],
            exact: false,
        })
        .unwrap();

    assert_eq!(store.resolved(leaf0, "transform2d.x".parse().unwrap()), Some(Value::Number(42.0)));
    assert_eq!(store.resolved(leaf1, "transform2d.x".parse().unwrap()), Some(Value::Number(1.0)));

    // 2. Verify `make_component` migrating two patches (with `not` exclusion & distinct ranks
    //    ordered opposite to PatchId) preserves winner order and survives undo/redo.
    let mut s2 = StoreCore::new();
    let ids = apply_xml(
        &mut s2,
        r#"<edit>
          <model name="model"/>
          <composition id="$c" name="scene" dims="2d"/>
          <library name="library">
            <definition id="$d_item" name="item">
              <template id="$t_item" notation="free">
                <rect id="$bx" name="box" x="5" w="10" h="10"/>
              </template>
            </definition>
          </library>
        </edit>"#,
    );
    let (comp, d_item, bx) = (ids["$c"], ids["$d_item"], ids["$bx"]);
    apply_xml(
        &mut s2,
        &format!(r#"<edit><place def="{d_item}" in="{comp}" x="0" y="0"/></edit>"#),
    );
    let u = s2
        .tree()
        .children(comp)
        .iter()
        .copied()
        .find(|&n| !n.is_expanded() && s2.tree().live(n).is_some_and(|x| x.ty.name() == "use"))
        .unwrap();
    let copy_bx = NodeId::expanded(u, bx).unwrap();

    let inst = AxisId(1);
    let r1 = OrderKey::first();
    let r2 = OrderKey::after(&r1);
    s2.apply(TransactionInput {
        label: "two patches on placed use".into(),
        actor: Actor::Human,
        ts: 0,
        idempotency_key: None,
        ops: vec![
            Op::Axis(AxisOp::Add { axis: Axis::new(inst, "instance", AxisKind::Instance, None) }),
            // Patch 10 has HIGHER rank (`r2`) and LOWER id than Patch 20 (`r1`).
            Op::Patch(PatchOp::Add {
                patch: Patch {
                    id: PatchId(10),
                    layer: 0,
                    rank: r2,
                    region: Product::default().with(inst, Sel::Suffix { tail: vec![u], not: vec![] }),
                    anchor: None,
                    writable: true,
                    entries: BTreeMap::from([(
                        PatchTarget { node: bx, key: PatchKey::Leaf("transform2d.x".parse().unwrap()) },
                        PatchValue::Slot { slot: Slot::Const(Value::Number(100.0)) },
                    )]),
                },
            }),
            Op::Patch(PatchOp::Add {
                patch: Patch {
                    id: PatchId(20),
                    layer: 0,
                    rank: r1,
                    region: Product::default().with(inst, Sel::Suffix { tail: vec![u], not: vec![vec![comp, u]] }),
                    anchor: None,
                    writable: true,
                    entries: BTreeMap::from([(
                        PatchTarget { node: bx, key: PatchKey::Leaf("transform2d.x".parse().unwrap()) },
                        PatchValue::Slot { slot: Slot::Const(Value::Number(200.0)) },
                    )]),
                },
            }),
        ],
        exact: false,
    })
    .unwrap();

    assert_eq!(s2.resolved(copy_bx, "transform2d.x".parse().unwrap()), Some(Value::Number(100.0)));
    let (made, make_tx) = s2.make_component(&[u], "Card", Actor::Human).unwrap();
    s2.apply(make_tx).unwrap();
    let inner_tpl = s2.authored().children(made.definition).iter().copied().find(|&n| s2.authored().live(n).is_some_and(|x| x.ty.name() == "template")).unwrap();
    let new_u = s2.authored().children(inner_tpl).iter().copied().find(|&n| s2.authored().live(n).is_some_and(|x| x.ty.name() == "use")).unwrap();
    let migrated_bx = NodeId::expanded(NodeId::expanded(made.picture, new_u).unwrap(), bx).unwrap();

    // Patch 10 was stronger than Patch 20 before Make Component; migrated winner must remain 100.0!
    assert_eq!(s2.resolved(migrated_bx, "transform2d.x".parse().unwrap()), Some(Value::Number(100.0)));
    s2.check_virtual_views_parity().unwrap();
    // And `not` exclusion on migrated Patch 20 was remapped from `u` to `new_u`!
    let p_not = s2.authored().patches().patches.values().find(|p| {
        p.anchor == Some(made.definition)
            && matches!(p.region.sel(inst), Sel::Suffix { not, .. } if not == &[vec![comp, new_u]])
    });
    assert!(p_not.is_some(), "make_component must remap `not` exclusions to migrated node ids");

    // Undo / redo roundtrip restores everything cleanly.
    s2.undo(1).unwrap().unwrap();
    assert_eq!(s2.resolved(copy_bx, "transform2d.x".parse().unwrap()), Some(Value::Number(100.0)));
    s2.check_virtual_views_parity().unwrap();
    s2.redo(2).unwrap().unwrap();
    assert_eq!(s2.resolved(migrated_bx, "transform2d.x".parse().unwrap()), Some(Value::Number(100.0)));
    s2.check_virtual_views_parity().unwrap();
}
