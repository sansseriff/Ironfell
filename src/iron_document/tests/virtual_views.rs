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
