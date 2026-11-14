//! The edit language (plans/specs/markup.md): one `<edit>`, planned in
//! Rust (`edit::plan`), is one transaction; nodes are node markup, and the
//! verbs name what exists or what the edit made.

use iron_document::edit::{self, Failure, Plan};
use iron_document::facet::{Name, Slider};
use iron_document::*;

fn tx(label: &str, ops: Vec<Op>) -> TransactionInput {
    TransactionInput { label: label.into(), actor: Actor::System, ts: 0, idempotency_key: None, ops, exact: false }
}

/// A store holding one 2D composition, `scene`, with a slider in it.
fn desk() -> (Store, NodeId, NodeId) {
    let mut store = Store::new();
    let src = r#"<edit>
      <model name="model"/>
      <composition id="$c" name="scene" dims="2d">
        <slider id="$s" name="level" x="10" y="10" w="200" h="24" value="0.4"/>
      </composition>
    </edit>"#;
    let plan = edit::plan(&store, src, None).unwrap_or_else(|f| panic!("{}", f.message()));
    let (c, s) = (plan.aliases["$c"], plan.aliases["$s"]);
    store.apply(tx("desk", plan.ops)).unwrap();
    (store, c, s)
}

fn plan(store: &Store, xml: &str) -> Plan {
    edit::plan(store, xml, None).unwrap_or_else(|f| panic!("{}", f.message()))
}

fn errors(store: &Store, xml: &str) -> Vec<edit::VerbError> {
    match edit::plan(store, xml, None) {
        Err(Failure::Errors(es)) => es,
        Err(Failure::Markup(m)) => panic!("a question: {m}"),
        Ok(_) => panic!("planned"),
    }
}

fn apply(store: &mut Store, xml: &str) -> Plan {
    let p = plan(store, xml);
    store.apply(tx("edit", p.ops.clone())).unwrap_or_else(|e| panic!("{e:?}"));
    p
}

fn named(store: &Store, name: &str) -> NodeId {
    let doc = store.document();
    doc.roots().iter().flat_map(|&r| std::iter::once(r).chain(doc.descendants(r))).collect::<Vec<_>>().into_iter().find(|&n| !n.is_expanded() && doc.is_live(n) && doc.get::<Name>(n).and_then(|x| x.text.constant()).and_then(|v| v.as_str()) == Some(name)).unwrap_or_else(|| panic!("no node {name}"))
}

fn number(store: &Store, id: NodeId, leaf: &str) -> f64 {
    store.resolved(id, leaf.parse().unwrap()).and_then(|v| v.as_f64()).unwrap_or_else(|| panic!("{id}.{leaf}"))
}

#[test]
fn a_node_is_created_in_a_node_with_its_children_and_any_order_of_names() {
    let (mut store, c, s) = desk();
    // `$w` is read before it is written: every node is named first.
    let p = apply(&mut store, &format!(r#"<edit>
      <group in="{c}" id="$g" name="readout" x="40" y="300">
        <text name="value" content="{{= fmt($w, 2)}}"/>
        <rect name="bar" w="{{= {s}.slider.value * 100}}" h="10" fill.color="@ink"/>
      </group>
      <let in="{c}" id="$w" name="w" value="{{= {s}.slider.value * 2}}"/>
    </edit>"#));
    let doc = store.document();
    let g = p.aliases["$g"];
    assert_eq!(doc.live(g).unwrap().parent, Some(c));
    assert_eq!(doc.children(g).len(), 2);
    assert_eq!(number(&store, named(&store, "bar"), "size.w"), 40.0);
    let text = store.resolved(named(&store, "value"), "text.content".parse().unwrap()).unwrap();
    assert_eq!(text.as_str(), Some("0.80"));
    // Both are the last of the composition's children, in the order written.
    let kids = doc.children(c);
    assert_eq!(&kids[kids.len() - 2..], &[g, p.aliases["$w"]]);
    assert!(p.summary.iter().any(|l| l.contains("alias=\"$g\"")));
}

#[test]
fn after_and_first_place_a_node_among_what_exists() {
    let (mut store, c, s) = desk();
    apply(&mut store, &format!(r#"<edit><rect in="{c}" name="front" first="true" w="5" h="5"/><rect in="{c}" name="next" after="{s}" w="5" h="5"/></edit>"#));
    let doc = store.document();
    let kids = doc.children(c);
    assert_eq!(kids[0], named(&store, "front"));
    assert_eq!(kids[2], named(&store, "next"));
}

#[test]
fn values_take_their_leafs_kind_and_bind_is_a_binding() {
    let (mut store, c, s) = desk();
    // A slider's value bound on creation; a key's value a constant; x.bind,
    // the form a view prints, is a binding and wins over the value beside it.
    apply(&mut store, &format!(r#"<edit>
      <slider in="{c}" name="copy" value="{{= {s}.slider.value}}" max="9"/>
      <rect in="{c}" name="follower" x="3" x.bind="{s}.transform2d.y + 1" w="5" h="5"/>
    </edit>"#));
    assert_eq!(number(&store, named(&store, "copy"), "slider.value"), 0.4);
    assert_eq!(number(&store, named(&store, "copy"), "slider.max"), 9.0);
    assert_eq!(number(&store, named(&store, "follower"), "transform2d.x"), 11.0);
}

#[test]
fn set_takes_a_bare_field_against_the_nodes_type_or_path_and_value() {
    let (mut store, c, s) = desk();
    let p = apply(&mut store, &format!(r#"<edit><set id="{s}" value="0.7"/><set id="{s}" path="slider.min" value="0.1"/><rect in="{c}" id="$r" w="5" h="5"/><add id="$r" stroke.width="2"/></edit>"#));
    let doc = store.document();
    let slider = doc.get::<Slider>(s).unwrap();
    assert_eq!(slider.value.constant(), Some(&Value::Number(0.7)));
    assert_eq!(slider.min.constant(), Some(&Value::Number(0.1)));
    assert_eq!(number(&store, p.aliases["$r"], "stroke.width"), 2.0);
    // <set> of a facet the node lacks adds it; on an entity a bare input
    // name is its argument.
    let p = apply(&mut store, &format!(r#"<edit><rect in="{c}" id="$q" w="5" h="5"/><set id="$q" appearance.opacity="0.5" stroke.width="3"/><place def="std:laser" in="{c}" id="$l"/></edit>"#));
    assert_eq!(number(&store, p.aliases["$q"], "appearance.opacity"), 0.5);
    let laser = composition::figure_of(store.document(), p.aliases["$l"]).map(|(_, e)| e).unwrap();
    apply(&mut store, &format!(r#"<edit><set id="{laser}" power="7 mW"/></edit>"#));
    assert_eq!(store.resolved(laser, "args.power".parse().unwrap()), Some(Value::parse_quantity("7 mW").unwrap()));
}

#[test]
fn errors_name_the_verb_as_written() {
    let (store, c, s) = desk();
    let es = errors(&store, &format!(r#"<edit><rect in="{c}" id="$r" w="5"/><set id="{s}" nope="1"/></edit>"#));
    assert_eq!(es.len(), 1);
    assert_eq!(es[0].op, Some(1));
    assert_eq!(es[0].verb.as_deref(), Some(&*format!("set {s}")));
    assert!(es[0].message.contains("nope"), "{}", es[0].message);
    // Every plain verb's mistake at once: a missing node, a cycle.
    let es = errors(&store, &format!(r##"<edit><set id="#zz" x="1"/><set id="{s}" value="{{= {s}.slider.value + 1}}"/></edit>"##));
    assert_eq!(es.iter().map(|e| (e.op, e.kind.as_str())).collect::<Vec<_>>(), vec![(Some(0), "unknown_node"), (Some(1), "cycle")], "{es:?}");
    let es = errors(&store, r#"<edit><frobnicate/></edit>"#);
    assert!(es[0].message.contains("not a verb or a node type"));
    let es = errors(&store, &format!(r#"<edit><rect in="{c}" id="{s}"/></edit>"#));
    assert!(es[0].message.contains("<set id="), "{}", es[0].message);
    let es = errors(&store, &format!(r#"<edit><rect in="{c}" w="{{= $nobody * 2}}"/></edit>"#));
    assert!(es[0].message.contains("$nobody"), "{}", es[0].message);
}

#[test]
fn links_reparents_and_deletes_name_what_the_edit_made() {
    let (mut store, c, s) = desk();
    apply(&mut store, "<edit><timeline name=\"timeline\"/></edit>");
    let tl = named(&store, "timeline");
    let p = apply(&mut store, &format!(r#"<edit>
      <clip in="{tl}" id="$clip" name="play" start="0" prop="slider.value">
        <key t="0" value="0"/>
        <key t="2" value="1"/>
      </clip>
      <link from="$clip" to="{s}" rel="animates" offset="0.5"/>
      <group in="{c}" id="$g" name="holder"/>
      <reparent id="{s}" in="$g"/>
    </edit>"#));
    let doc = store.document();
    let clip = p.aliases["$clip"];
    let r = doc.relations_from(clip).find(|r| r.rel == document::RelKind::Animates).expect("a link");
    assert_eq!(r.to, s);
    assert_eq!(r.data.get("offset"), Some(&Value::Number(0.5)));
    assert_eq!(doc.live(s).unwrap().parent, Some(p.aliases["$g"]));
    // A relation's id unlinks it.
    let rid = r.id;
    apply(&mut store, &format!(r#"<edit><unlink id="{rid}"/><delete id="{clip}"/></edit>"#));
    assert!(!store.document().is_live(clip));
}

#[test]
fn a_place_is_read_by_its_name_and_its_ports_by_theirs() {
    let (mut store, c, s) = desk();
    // A mark on a plot: its axis ports, before and after it is placed.
    let p = apply(&mut store, &format!(r#"<edit>
      <group in="{c}" name="chart">
        <ellipse name="before" x="{{= $plot.transform2d.x + 10}}" y="{{= $plot.x_hi}}" w="4" h="4"/>
        <place def="std:plot" id="$plot" x="100" y="50" w="300" h="200" xs="0 1 2" ys="{{= $self.xs * {s}.slider.value}}" x_min="0" x_max="2"/>
        <ellipse name="after" x="{{= $plot.transform2d.x}}" w="4" h="4"/>
      </group>
    </edit>"#));
    assert_eq!(number(&store, named(&store, "before"), "transform2d.x"), 110.0);
    assert_eq!(number(&store, named(&store, "before"), "transform2d.y"), 2.0);
    // Siblings keep the order written: the mark after the plot paints over it.
    let doc = store.document();
    let chart = named(&store, "chart");
    let kids = doc.children(chart).iter().copied().filter(|k| !k.is_expanded()).collect::<Vec<_>>();
    assert_eq!(kids, vec![named(&store, "before"), p.aliases["$plot"], named(&store, "after")]);
    // An instance the document has: `#use.port`.
    let plot = p.aliases["$plot"];
    apply(&mut store, &format!(r#"<edit><rect in="{c}" name="reads" x="{{= {plot}.x_hi * 10}}" w="5" h="5"/></edit>"#));
    assert_eq!(number(&store, named(&store, "reads"), "transform2d.x"), 20.0);
}

#[test]
fn a_definition_made_here_is_placed_by_its_name() {
    let (mut store, c, _) = desk();
    let p = apply(&mut store, &format!(r#"<edit>
      <library name="library">
        <definition id="$d" name="pip">
          <port_spec name="r" kind="value" dir="in" signal="5"/>
          <template notation="free">
            <ellipse w="10" h="10"/>
          </template>
        </definition>
      </library>
      <place def="$d" in="{c}" id="$pip" x="30" y="40" r="7"/>
    </edit>"#));
    assert!(p.aliases.contains_key("$pip"));
    assert!(p.summary.iter().any(|l| l.contains("<placed")), "{:?}", p.summary);
    // Every op is someone's: the library is verb 0, the place verb 1.
    assert_eq!(p.op_verb.len(), p.ops.len());
    assert_eq!(*p.op_verb.last().unwrap(), 1);
}

#[test]
fn a_circuit_and_a_table_share_an_edit_with_nodes() {
    let (mut store, c, s) = desk();
    let p = apply(&mut store, &format!(r#"<edit>
      <composition id="$sheet" name="sheet" notation="schematic"/>
      <circuit sheet="$sheet">
        <place part="vsource" name="V9" x="120" y="220" V="{{= {s}.slider.value * 10 V}}"/>
        <place part="resistor" name="R9" x="220" y="160" R="1 kΩ"/>
        <place part="ground" name="G9" x="120" y="300"/>
        <wire from="V9.p" to="R9.a" elbow="vertical"/>
        <wire from="R9.b" to="V9.n" via="300,160 300,220"/>
        <wire from="V9.n" to="G9.g"/>
      </circuit>
      <table in="{c}" x="40" y="400" preset="boxed">a | b
      1 | {{= {s}.slider.value}}</table>
    </edit>"#));
    assert!(p.summary.iter().any(|l| l.contains("<netlist")), "{:?}", p.summary);
    assert!(p.summary.iter().any(|l| l.contains("<table id=")), "{:?}", p.summary);
    let doc = store.document();
    let wires = doc.nodes_of_type(TypeId::lookup("connection").unwrap()).filter(|&w| doc.is_live(w)).count();
    assert_eq!(wires, 3);
}

#[test]
fn narrative_verbs_write_markdown_into_a_narrative() {
    let (mut store, _, _) = desk();
    apply(&mut store, "<edit><narrative name=\"notes\"/></edit>");
    let p = apply(&mut store, "<edit><write narrative=\"notes\">\n# Title\n\nSome *words* &amp; more.\n</write></edit>");
    let n = named(&store, "notes");
    assert_eq!(store.document().children(n).len(), 2);
    assert!(p.summary.iter().any(|l| l.starts_with("  <write blocks=")));
}

#[test]
fn single_quotes_bare_attributes_and_comments_parse() {
    let (mut store, c, _) = desk();
    apply(&mut store, &format!("<edit><!-- a note --><rect in='{c}' name='q' first w='5' h='5'/></edit>"));
    assert_eq!(store.document().children(c)[0], named(&store, "q"));
}

#[test]
fn expect_version_and_exact_are_read() {
    let (store, _, s) = desk();
    let p = plan(&store, &format!(r#"<edit expect-version="7" exact="true"><set id="{s}" value="1"/></edit>"#));
    assert_eq!((p.expect_version, p.exact), (7, true));
    assert_eq!(p.verbs, vec![format!("set {s}")]);
}

/// The store checks an edit whole: a connection and its two links are
/// planned together, not link by link, and so are the instances' ports
/// they name (`$p.port`).
#[test]
fn a_connection_and_its_two_links_are_one_edit() {
    let (mut store, c, _) = desk();
    let model = named(&store, "model");
    let placed = apply(&mut store, &format!(r#"<edit>
      <place def="std:laser" in="{c}" id="$l" x="40" y="40" power="12 mW"/>
      <place def="std:fiber" in="{c}" id="$f" x="230" y="75"/>
    </edit>"#));
    let doc = store.document();
    let port = |owner: &str, name: &str| {
        let u = placed.aliases[owner];
        let e = composition::figure_of(doc, u).map(|(_, e)| e).unwrap();
        doc.children(e).iter().copied().find(|&p| doc.get::<Name>(p).and_then(|n| n.text.constant()).and_then(|v| v.as_str()) == Some(name)).unwrap()
    };
    let (out, inp) = (port("$l", "out"), port("$f", "in"));
    let p = apply(&mut store, &format!(r#"<edit>
      <connection in="{model}" id="$w">
        <link from="$w" to="{out}" rel="connects" end="a"/>
        <link from="$w" to="{inp}" rel="connects" end="b"/>
      </connection>
    </edit>"#));
    let ends = store.document().relations_from(p.aliases["$w"]).filter(|r| r.rel == document::RelKind::Connects).count();
    assert_eq!(ends, 2);
    // A store error found while planning names this edit's nodes as it did.
    let es = errors(&store, &format!(r#"<edit><connection in="{model}" id="$v"><link from="$v" to="{out}" rel="connects" end="a"/></connection></edit>"#));
    assert!(es[0].message.starts_with("$v "), "{es:?}");
}

// Council round 1 (2026-11-14): each of these failed or panicked before.

#[test]
fn a_name_deleted_earlier_in_the_edit_is_an_error_not_a_panic() {
    let (store, c, _) = desk();
    let es = errors(&store, &format!(r#"<edit>
      <rect in="{c}" id="$r" w="5" h="5"/>
      <delete id="$r"/>
      <table in="{c}">a | b</table>
      <set id="$r" x="1"/>
    </edit>"#));
    assert!(es.iter().any(|e| e.op == Some(3) && e.kind == "unknown_node"), "{es:?}");
}

#[test]
fn reparents_in_one_edit_see_each_others_moves() {
    let (mut store, c, _) = desk();
    let p = apply(&mut store, &format!(r#"<edit><group in="{c}" id="$g"><rect id="$a" name="a"/><rect id="$b" name="b"/><rect id="$c" name="c"/></group></edit>"#));
    let (g, a, b, cc) = (p.aliases["$g"], p.aliases["$a"], p.aliases["$b"], p.aliases["$c"]);
    apply(&mut store, &format!(r#"<edit><reparent id="{a}" in="{g}" after="{cc}"/><reparent id="{b}" in="{g}" after="{a}"/></edit>"#));
    assert_eq!(store.document().children(g), &[cc, a, b]);
}

#[test]
fn compound_verbs_read_the_edits_names() {
    let (mut store, c, _) = desk();
    // A place's argument reads an earlier place's port.
    let p = apply(&mut store, &format!(r#"<edit>
      <place def="std:plot" in="{c}" id="$p1" x="0" y="0" xs="0 1 2" ys="0 1 2" x_min="0" x_max="2"/>
      <place def="std:plot" in="{c}" id="$p2" x="0" y="300" xs="0 1 2" ys="0 1 2" x_min="0" x_max="{{= $p1.x_hi * 2}}"/>
      <rect in="{c}" name="reads" x="{{= $p2.x_hi}}" w="1" h="1"/>
    </edit>"#));
    assert!(p.aliases.contains_key("$p2"));
    assert_eq!(number(&store, named(&store, "reads"), "transform2d.x"), 4.0);
    // A table's cell and a narrative read names the edit gave.
    let p = apply(&mut store, &format!(r#"<edit>
      <let in="{c}" id="$v" name="v" value="7"/>
      <table in="{c}">a | b
      1 | {{= $v}}</table>
      <narrative id="$n" name="notes"/>
      <write narrative="$n">Hello.</write>
    </edit>"#));
    assert!(!serde_json::to_string(&p.ops).unwrap().contains("$v"));
    assert_eq!(store.document().children(p.aliases["$n"]).len(), 1);
}

#[test]
fn words_outside_a_binding_stay_as_written() {
    let (mut store, c, _) = desk();
    let p = apply(&mut store, &format!(r#"<edit>
      <let in="{c}" id="$v" name="v" value="1"/>
      <place def="std:plot" in="{c}" id="$p" xs="0 1" ys="0 1" title="Cost $v°"/>
    </edit>"#));
    assert!(serde_json::to_string(&p.ops).unwrap().contains("Cost $v°"));
}

#[test]
fn a_bind_attribute_may_read_a_place() {
    let (mut store, c, _) = desk();
    apply(&mut store, &format!(r#"<edit>
      <group in="{c}">
        <place def="std:plot" id="$plot" x="100" y="50" xs="0 1" ys="0 1"/>
        <rect name="mark" x.bind="$plot.transform2d.x + 1" w="1" h="1"/>
      </group>
    </edit>"#));
    assert_eq!(number(&store, named(&store, "mark"), "transform2d.x"), 101.0);
}

#[test]
fn top_level_places_and_nodes_keep_the_order_written() {
    let (mut store, c, _) = desk();
    let p = apply(&mut store, &format!(r#"<edit>
      <place def="std:plot" in="{c}" id="$p" xs="0 1" ys="0 1"/>
      <rect in="{c}" id="$over" w="5" h="5"/>
    </edit>"#));
    let kids: Vec<NodeId> = store.document().children(c).iter().copied().filter(|k| !k.is_expanded()).collect();
    assert_eq!(&kids[kids.len() - 2..], &[p.aliases["$p"], p.aliases["$over"]]);
}

#[test]
fn import_svg_hands_the_svg_over_as_written() {
    let (store, c, _) = desk();
    let seen = std::cell::RefCell::new(String::new());
    let svg = |s: &str| -> Result<(iron_document::copy::Fragment, Vec<String>), String> {
        *seen.borrow_mut() = s.to_owned();
        Err("stop".into())
    };
    let xml = format!(r#"<edit><import-svg in="{c}" x="0" y="0"><svg xmlns="http://www.w3.org/2000/svg"><rect id="A&amp;B" width="1" height="1"/></svg></import-svg></edit>"#);
    assert!(edit::plan(&store, &xml, Some(&svg)).is_err());
    assert!(seen.borrow().contains(r#"id="A&amp;B""#), "{}", seen.borrow());
}

#[test]
fn text_between_a_nodes_tags_is_refused() {
    let (store, c, _) = desk();
    let es = errors(&store, &format!(r#"<edit><text in="{c}" name="t">Hello world</text></edit>"#));
    assert!(es[0].message.contains("content="), "{es:?}");
}

#[test]
fn markdown_keeps_cdata_and_decodes_numeric_entities() {
    let (mut store, _, _) = desk();
    apply(&mut store, "<edit><narrative name=\"notes\"/></edit>");
    let p = apply(&mut store, "<edit><write narrative=\"notes\">It&#39;s <![CDATA[a<b]]> &#x41;.</write></edit>");
    let ops = serde_json::to_string(&p.ops).unwrap();
    assert!(ops.contains("It's a<b A."), "{ops}");
}

#[test]
fn malformed_names_and_places_are_refused_with_a_reason() {
    let (store, c, _) = desk();
    // An attribute given twice.
    let es = errors(&store, &format!(r#"<edit><rect in="{c}" x="1" x="2"/></edit>"#));
    assert!(es[0].message.contains("given twice"), "{es:?}");
    // A place inside a node goes in that node.
    let es = errors(&store, &format!(r#"<edit><group in="{c}"><place def="std:plot" in="{c}" xs="0 1" ys="0 1"/></group></edit>"#));
    assert!(es[0].message.contains("goes in that node"), "{es:?}");
    // `$__…` is the planner's own.
    let es = errors(&store, &format!(r#"<edit><rect in="{c}" id="$__node_0"/></edit>"#));
    assert!(es[0].message.contains("two _"), "{es:?}");
}

#[test]
fn a_dollar_inside_a_string_is_text() {
    let (mut store, c, _) = desk();
    apply(&mut store, &format!(r#"<edit><let in="{c}" id="$v" name="v" value="3"/><text in="{c}" name="cost" content='{{= "cost $" + fmt($v, 0)}}'/></edit>"#));
    let text = store.resolved(named(&store, "cost"), "text.content".parse().unwrap()).unwrap();
    assert_eq!(text.as_str(), Some("cost $3"));
}

#[test]
fn rewrite_replaces_a_block() {
    let (mut store, _, _) = desk();
    apply(&mut store, "<edit><narrative name=\"notes\"/></edit>");
    apply(&mut store, "<edit><write narrative=\"notes\">First.</write></edit>");
    let n = named(&store, "notes");
    let block = store.document().children(n)[0];
    let p = apply(&mut store, &format!("<edit><rewrite block=\"{block}\">Changed.\n\nAnd more.</rewrite></edit>"));
    assert_eq!(store.document().children(n).len(), 2);
    assert!(p.summary.iter().any(|l| l.starts_with("  <rewrite blocks=")));
}

#[test]
fn a_store_error_at_a_batch_of_plain_verbs_names_its_own_verb() {
    let (store, c, s) = desk();
    // The reparent makes a cycle; the set before it is fine.
    let es = errors(&store, &format!(r#"<edit><set id="{s}" x="1"/><reparent id="{c}" in="{s}"/></edit>"#));
    assert!(es.iter().all(|e| e.op == Some(1)), "{es:?}");
}

// Council round 2 (2026-11-14).

#[test]
fn siblings_put_back_in_order_never_share_a_key() {
    let (mut store, c, _) = desk();
    let p = apply(&mut store, &format!(r#"<edit>
      <group in="{c}" id="$g"><rect id="$a"/><place def="std:plot" id="$p" xs="0 1" ys="0 1"/><rect id="$b"/></group>
      <rect in="$g" id="$z"/>
    </edit>"#));
    let doc = store.document();
    let g = p.aliases["$g"];
    let kids: Vec<NodeId> = doc.children(g).iter().copied().filter(|k| !k.is_expanded()).collect();
    assert_eq!(kids, vec![p.aliases["$a"], p.aliases["$p"], p.aliases["$b"], p.aliases["$z"]]);
    let mut keys: Vec<_> = kids.iter().map(|&k| doc.live(k).unwrap().order.clone()).collect();
    keys.dedup();
    assert_eq!(keys.len(), 4, "distinct order keys");
    // A later edit can place after any of them.
    apply(&mut store, &format!(r#"<edit><rect in="{g}" after="{}"/></edit>"#, p.aliases["$b"]));
}

#[test]
fn top_level_siblings_keep_order_however_in_is_spelled() {
    let (mut store, c, _) = desk();
    let p = apply(&mut store, &format!(r#"<edit>
      <place def="std:plot" in="scene" id="$p" xs="0 1" ys="0 1"/>
      <rect in="{c}" id="$over" w="5" h="5"/>
    </edit>"#));
    let kids: Vec<NodeId> = store.document().children(c).iter().copied().filter(|k| !k.is_expanded()).collect();
    assert_eq!(&kids[kids.len() - 2..], &[p.aliases["$p"], p.aliases["$over"]]);
}

#[test]
fn a_comment_in_a_circuit_does_not_hide_its_names() {
    let (mut store, _, s) = desk();
    let p = apply(&mut store, &format!(r#"<edit>
      <composition id="$sheet" name="sheet" notation="schematic"/>
      <let in="$sheet" id="$k" name="k" value="2"/>
      <circuit sheet="$sheet">
        <!-- don't forget the ground -->
        <place part="vsource" name="V9" x="120" y="220" V="{{= {s}.slider.value * $k * 1 V}}"/>
        <place part="ground" name="G9" x="120" y="300"/>
        <wire from="V9.n" to="G9.g"/>
      </circuit>
    </edit>"#));
    assert!(p.summary.iter().any(|l| l.contains("<netlist")), "{:?}", p.summary);
}

#[test]
fn a_dollar_is_text_outside_bindings_and_id_attributes() {
    let (mut store, c, _) = desk();
    // On a place: a title, not an id.
    let p = apply(&mut store, &format!(r#"<edit>
      <let in="{c}" id="$v" name="v" value="1"/>
      <group in="{c}"><place def="std:plot" id="$plot" xs="0 1" ys="0 1" title="$v"/><text name="note" content="see $plot above"/></group>
    </edit>"#));
    let ops = serde_json::to_string(&p.ops).unwrap();
    assert!(ops.contains(r#""$v""#), "{ops}");
    let note = store.resolved(named(&store, "note"), "text.content".parse().unwrap()).unwrap();
    assert_eq!(note.as_str(), Some("see $plot above"));
    // A name for a node and a place is refused as that.
    let es = errors(&store, &format!(r#"<edit><group in="{c}"><rect id="$x"/><place def="std:plot" id="$x" xs="0 1" ys="0 1"/></group></edit>"#));
    assert!(es[0].message.contains("names a node and a place"), "{es:?}");
}

#[test]
fn what_a_verb_does_not_read_is_refused() {
    let (store, c, s) = desk();
    let es = errors(&store, &format!(r#"<edit><set id="{s}" x="3"><rect w="1" h="1"/></set></edit>"#));
    assert!(es[0].message.contains("holds nothing"), "{es:?}");
    let es = errors(&store, &format!(r#"<edit>Here is my change: <rect in="{c}"/></edit>"#));
    assert!(es[0].message.contains("not read"), "{es:?}");
    let es = errors(&store, &format!(r#"<edit><rect in="{c}" x="1" transform2d.x="2"/></edit>"#));
    assert!(es[0].message.contains("name one leaf"), "{es:?}");
    let es = errors(&store, &format!(r#"<edit><rect in="{c}" id="$self"/></edit>"#));
    assert!(es[0].message.contains("planner's own"), "{es:?}");
}

#[test]
fn a_missing_parent_is_an_unknown_node() {
    let (store, _, _) = desk();
    let es = errors(&store, r##"<edit><rect in="#zz9"/></edit>"##);
    assert_eq!(es[0].kind, "unknown_node", "{es:?}");
    let es = errors(&store, r##"<edit><place def="std:plot" in="#zz9" xs="0 1" ys="0 1"/></edit>"##);
    assert_eq!(es[0].kind, "unknown_node", "{es:?}");
}

#[test]
fn a_brace_inside_a_string_does_not_end_a_binding() {
    let (mut store, c, _) = desk();
    let p = apply(&mut store, &format!(r#"<edit>
      <let in="{c}" id="$v" name="v" value="7"/>
      <table in="{c}">a
      {{= "}}" + fmt($v, 0)}}</table>
    </edit>"#));
    assert!(!serde_json::to_string(&p.ops).unwrap().contains("$v"));
}

#[test]
fn a_node_id_that_is_not_a_name_is_an_error_not_a_panic() {
    let (store, c, _) = desk();
    let es = errors(&store, &format!(r##"<edit><group in="{c}"><place def="std:plot" xs="0 1" ys="0 1"/><rect id="#1"/></group></edit>"##));
    assert!(es[0].message.contains("<set"), "{es:?}");
}

#[test]
fn a_node_moved_twice_is_where_its_last_move_put_it() {
    let (mut store, c, _) = desk();
    let p = apply(&mut store, &format!(r#"<edit><group in="{c}" id="$g"><rect id="$a"/><rect id="$b"/><rect id="$c"/></group></edit>"#));
    let (g, a, b, cc) = (p.aliases["$g"], p.aliases["$a"], p.aliases["$b"], p.aliases["$c"]);
    apply(&mut store, &format!(r#"<edit><reparent id="{a}" in="{g}" after="{b}"/><reparent id="{a}" in="{g}" after="{cc}"/><reparent id="{b}" in="{g}" after="{a}"/></edit>"#));
    assert_eq!(store.document().children(g), &[cc, a, b]);
}

#[test]
fn a_bind_beside_a_value_wins_even_when_the_value_reads_a_place() {
    let (mut store, c, _) = desk();
    apply(&mut store, &format!(r#"<edit><group in="{c}"><place def="std:plot" id="$p" xs="0 1" ys="0 1" x_min="0" x_max="3"/><rect name="m" x="{{= $p.x_hi}}" x.bind="5" w="1" h="1"/></group></edit>"#));
    assert_eq!(number(&store, named(&store, "m"), "transform2d.x"), 5.0);
}

#[test]
fn strings_in_bindings_follow_the_expression_lexer() {
    let (mut store, c, _) = desk();
    apply(&mut store, &format!(r#"<edit><let in="{c}" id="$cash" name="cash" value="1"/><text in="{c}" name="say" content='{{= "say \"$cash\"" }}'/></edit>"#));
    let text = store.resolved(named(&store, "say"), "text.content".parse().unwrap()).unwrap();
    assert_eq!(text.as_str(), Some("say \"$cash\""));
}

#[test]
fn several_cdata_sections_and_entities_make_one_text() {
    let (mut store, _, _) = desk();
    apply(&mut store, "<edit><narrative name=\"notes\"/></edit>");
    let p = apply(&mut store, "<edit><write narrative=\"notes\"><![CDATA[A]]>&amp;<![CDATA[B]]></write></edit>");
    assert!(serde_json::to_string(&p.ops).unwrap().contains("A&B"));
}

#[test]
fn a_contexts_entities_survive_planning() {
    let (store, _, _) = desk();
    plan(&store, r#"<edit><context name="A&gt;B"><rule match="all" affords.select="target"/></context></edit>"#);
}
