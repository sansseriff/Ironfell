//! A model's reply read in Rust (iron_document::reply): its blocks, the
//! stream the chat shows, and `<revise>` of a rejected edit.

use iron_document::reply::{read, revise, stream, Verb};

const REJECTED: &str = r##"<edit expect-version="14">
  <definition in="#L" id="$d"/>
  <rect in="$d" id="$a" x="0" y="0"/>
  <rect in="$d" x="1" y="{= a > b ? 1 : 2}"/>
  <set id="#P" x="12"/>
</edit>"##;

#[test]
fn revise_changes_replaces_drops_and_inserts_verbs_by_handle_as_sent() {
    let r = revise(
        REJECTED,
        r##"<revise expect-version="15">
    <change op="$a" x="5" size.w="9"/>
    <drop op="2"/>
    <insert after="$a"><rect in="$d" id="$b" x="2" y="2"/></insert>
    <insert before="0"><definition in="#L" id="$z"/></insert>
    <replace op="3"><set id="#P" x="13"/>
<set id="#Q" x="1"/></replace>
  </revise>"##,
    );
    assert_eq!(
        r.unwrap(),
        r##"<edit expect-version="15">
  <definition in="#L" id="$z"/>
  <definition in="#L" id="$d"/>
  <rect in="$d" id="$a" x="5" y="0" size.w="9"/>
  <rect in="$d" id="$b" x="2" y="2"/>
  <set id="#P" x="13"/>
<set id="#Q" x="1"/>
</edit>"##
    );
}

#[test]
fn revise_text_replaces_exact_text_once_or_everywhere_with_all() {
    let once = revise(REJECTED, r##"<revise><text old="a &gt; b" new="a &lt; b"/></revise>"##).unwrap();
    assert!(once.contains(r##"y="{= a < b ? 1 : 2}""##), "{once}");
    let ambiguous = revise(REJECTED, r##"<revise><text old="in=&quot;$d&quot;" new="in=&quot;$e&quot;"/></revise>"##).unwrap_err();
    assert!(ambiguous.contains(r##"occurs 2 times; quote more of it, or add all="true""##), "{ambiguous}");
    let all = revise(REJECTED, r##"<revise><text old="in=&quot;$d&quot;" new="in=&quot;$e&quot;" all="true"/></revise>"##).unwrap();
    assert_eq!(all.matches(r##"in="$e""##).count(), 2);
}

#[test]
fn revise_errors_say_which_handle_or_step_is_wrong() {
    let err = |patch: &str| revise(REJECTED, patch).unwrap_err();
    assert!(err(r##"<revise><drop op="$nope"/></revise>"##).contains(r##"no op of the rejected edit has id "$nope""##));
    assert!(err(r##"<revise><drop op="9"/></revise>"##).contains("there is no op 9; the edit had ops 0 to 3"));
    assert!(err(r##"<revise><drop op="1"/><change op="1" x="1"/></revise>"##).contains("op 1 was dropped earlier"));
    assert!(err(r##"<revise><text old="missing" new="x"/></revise>"##).contains(r##""missing" is not in the rejected edit"##));
    assert!(err(r##"<revise><frob op="1"/></revise>"##).contains("unknown revise step <frob>"));
    assert!(err(r##"<revise><drop op="$p"/></revise>"##).starts_with("revise step 0 <drop>:"));
    assert_eq!(revise(REJECTED, "<revise/>").unwrap(), REJECTED);
}

#[test]
fn revise_counts_verbs_as_the_planner_does() {
    // Comments and declarations are skipped; raw bodies are text.
    let edit = r##"<edit><?xml-ish?><!-- a note --><write narrative="#N"><b>not a verb</b> &lt;</write><![CDATA[ ignored ]]><set id="#P" x="1"/></edit>"##;
    let r = revise(edit, r##"<revise><change op="1" x="2"/></revise>"##).unwrap();
    assert!(r.contains(r##"<set id="#P" x="2"/>"##), "{r}");
    assert!(r.contains(r##"<write narrative="#N"><b>not a verb</b> &lt;</write>"##), "{r}");
}

#[test]
fn a_change_reads_the_verb_as_earlier_steps_left_it() {
    let shorter = revise(REJECTED, r##"<revise><change op="2" y="1"/><change op="2" x="7"/></revise>"##).unwrap();
    assert!(shorter.contains(r##"<rect in="$d" x="7" y="1"/>"##), "{shorter}");
    let longer = revise(REJECTED, r##"<revise><change op="$a" x="1234567"/><change op="$a" y="2"/></revise>"##).unwrap();
    assert!(longer.contains(r##"<rect in="$d" id="$a" x="1234567" y="2"/>"##), "{longer}");
    let replaced = revise(REJECTED, r##"<revise><replace op="3"><set id="#P"/></replace><change op="3" y="4"/></revise>"##).unwrap();
    assert!(replaced.contains(r##"<set id="#P" y="4"/>"##), "{replaced}");
}

#[test]
fn a_changed_value_is_written_escaped_once() {
    // The revise's value is read once and written once, so the planner reads
    // what the model meant: `<` and `&` stay text, quotes stay inside.
    let r = revise(REJECTED, r##"<revise><change op="$a" name="a &lt; b &amp; &quot;c&quot;"/></revise>"##).unwrap();
    assert!(r.contains(r##"name="a &lt; b &amp; &quot;c&quot;""##), "{r}");
}

#[test]
fn a_reply_names_its_edit_revise_looks_topics_and_continue() {
    let r = read("Making it.\n<edit><rect in=\"#1\"/></edit>\n<look picture=\"Main\"/>\n<continue/>");
    assert_eq!(r.edit.as_deref(), Some("<edit><rect in=\"#1\"/></edit>"));
    assert_eq!(r.picture.as_deref(), Some("Main"));
    assert!(r.continues);
    assert_eq!(r.note, "Making it.\n\n\n<continue/>");
    assert_eq!(r.bare, "Making it.\n<edit><rect in=\"#1\"/></edit>\n\n<continue/>");
    let r = read("Fixing it.\n<revise><drop op=\"1\"/></revise>");
    assert_eq!((r.edit, r.revise.as_deref(), r.note.as_str()), (None, Some("<revise><drop op=\"1\"/></revise>"), "Fixing it."));
    let r = read("<look expand=\"u e\"/> <topic name=\"tables, circuits\"/><topic name=\"tables\"/>");
    assert_eq!(r.expand, ["#u", "#e"]);
    assert_eq!(r.topics, ["tables", "circuits"]);
    assert!(r.picture.is_none() && !r.continues);
    // An edit's own markup is not the reply's: a <look> written in a
    // narrative it writes is text.
    let r = read("<edit><write narrative=\"#N\">see <look picture=\"x\"/></write></edit>");
    assert!(r.picture.is_none());
    // An edit never closed goes to the planner whole, which says what is wrong.
    let r = read("<edit><rect in=\"#1\"></edit>");
    assert_eq!(r.edit.as_deref(), Some("<edit><rect in=\"#1\"></edit>"));
    assert!(read("<edit><rect").edit.is_some());
    assert!(read("<look picture=\"a\"b\"/>").picture.is_none());
}

fn verb(name: &str, attrs: &[(&str, &str)], closed: bool) -> Verb {
    Verb { name: name.into(), attrs: attrs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(), closed }
}

#[test]
fn a_streaming_reply_shows_prose_holds_back_half_written_markup_and_lists_verbs() {
    let s = stream("Making a pump.\n<ed");
    assert_eq!((s.prose.as_str(), s.edit, s.verbs.len()), ("Making a pump.\n", None, 0));
    assert_eq!(stream("See this:\n\n::vie").prose, "See this:\n\n");
    let s = stream(r##"Making it.
<edit><rect id="$a" name="pump body" x="1"/><set id="#4" x="2" fill.color="#f00"/><import-svg in="#1"><svg><g><path d="M0 0"/></g></svg></import-svg><group id="$g"><rect/><text id="$b" na"##);
    assert_eq!(s.prose, "Making it.\n");
    assert!(s.edit.as_deref().is_some_and(|e| e.starts_with("<edit>")));
    assert_eq!(
        s.verbs,
        [
            verb("rect", &[("id", "$a"), ("name", "pump body"), ("x", "1")], true),
            verb("set", &[("id", "#4"), ("x", "2"), ("fill.color", "#f00")], true),
            verb("import-svg", &[("in", "#1")], true),
            verb("group", &[("id", "$g")], false),
        ]
    );
    let s = stream("<edit><delete id=\"#3\"/></edit>\nDone.");
    assert_eq!((s.prose.as_str(), s.verbs.len()), ("\nDone.", 1));
    assert_eq!(stream("Like this:\n```xml\n<edit><delete id=\"#3\"/></edit>\n```\n").edit, None);
    let s = stream("<look expand=\"#u\"/>");
    assert_eq!((s.prose.as_str(), s.edit), ("", None));
    let s = stream("Fixing the binding.\n<revise>\n  <change op=\"$a\" x=\"1\"/>\n  <drop op=\"4\"/>\n</revise>");
    assert_eq!(s.prose.trim(), "Fixing the binding.");
    assert_eq!(s.verbs.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(), ["change", "drop"]);
    assert_eq!(stream("Fixing.\n<revi").prose, "Fixing.\n");
}
