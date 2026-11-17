# Markup: one language for edits, components and pages

**Status:** Built 2026-11-14 (branch `t3/demo-refresh`).

Iron had three XML languages and four parsers. This spec is the one language that
replaced them, what it reads, and what stayed separate. Owning code:
`src/iron_document/src/edit.rs` (the planner), `markup.rs` (nodes and values),
`parts.rs` (`elements`, the XML reader), `src-ui/model/edit.ts` (applying a plan as the
model).

## 1. What there was

| Format | Purpose | Parsed by |
|---|---|---|
| Saved JSON (`serial.rs`) | the document on disk | serde, both ways |
| View XML (`view.rs`) | what the model and the Model panel read | written in Rust, never read back |
| Edit XML | what the model wrote: `<create type="rect" parent="#1" …>`, `<set>`, `<link>` | **TypeScript** (`edit.ts`, fast-xml-parser), handing `<place>`, `<circuit>`, `<table>`, `<context>`, `<import-svg>`, narrative verbs to separate Rust calls, most "alone in their edit" |
| Node markup (`markup.rs`) | standard components, the gallery: `<rect x="…"/>`, nested | **Rust** |

Leaf names (`x` is `transform2d.x`) and values (numbers, colours, units, series) were
read twice, in `schema.ts` and in `markup.rs`/`compose.rs`. What one format could write
the other could not: node markup had no `<link>` (the wave packet's clip and the RC
circuit's wires were built in Rust, demo.md §5), the edit format could read a placed
plot's ports only by a raw expanded id (`#95/5z.port.signal`).

## 2. What there is

- **Saved JSON stays.** Its job is an exact copy nobody writes by hand, read and written
  by one library.
- **One markup language for writing**, read only in Rust: node markup plus verbs, in an
  `<edit>`. The model's edits, the standard components (`std/*.xml`, which hold only
  nodes) and the gallery pages (`gallery/*.xml`, whole edits) are all it.
- **The view** prints nodes the same way (element = type, attributes = leaves, children
  nested), with ids, values beside their bindings (`x="40" x.bind="…"`, which the
  reader takes as the binding) and what it elides marked. What the model reads it can
  write back.

## 3. The language

```xml
<edit expect-version="7" exact="false">
  <group in="#C" id="$g" name="readout" x="40" y="300">      <!-- a new node, nested -->
    <text content="{= fmt($v, 2)}"/>
    <place def="std:plot" id="$plot" x="0" y="40" w="300" h="200" ys="{= $self.xs * 2}" marks_x="{= $v}"/>
    <text x="{= $plot.transform2d.x}" y="250" content="{= fmt($plot.x_hi, 1)}"/>
    <link from="$clip" to="$g" rel="references"/>
  </group>
  <let in="#C" id="$v" name="v" value="0.5"/>
  <clip in="#timeline" id="$clip" start="0" prop="slider.value">
    <key t="0" value="0"/><key t="8" value="8"/>
  </clip>
  <set id="#P" x="12"/>  <add id="#N" stroke.width="2"/>  <delete id="#3"/>
  <reparent id="#4" in="#1" first="true"/>
  <link from="$clip" to="#S" rel="animates"/>  <unlink id="r1"/>
  <place def="$d" in="#C" id="$p"/>  <make-component of="#a #b" id="$m"/>
  <circuit sheet="$s">…</circuit>  <table in="#C">a | b</table>
  <context name="…">…</context>  <import-svg in="#C"><svg>…</svg></import-svg>
  <typeset-math in="#C" source="E=mc^2" x="80" y="100"/>
  <link-figures a="#l" b="#Q"/>  <write narrative="#N">markdown</write>  <rewrite block="#B">…</rewrite>
</edit>
```

- **Nodes.** Any top-level element not a verb is a new node: the element is its type,
  attributes its leaves (`facet.field`, or a bare field: the first of the type's facets
  that has it; `name`; `k.bind="e"` for `k="{= e}"`), children its children. A
  top-level node goes `in=` a node that exists, one this edit makes (written earlier),
  or a composition by name; none makes a root. `after="#x"` or `first="true"` places it
  among the parent's children (existing and earlier in this edit), else last.
- **Names.** `id="$x"` names a new node, `id="$p"` a component verb's result, for the
  whole edit (`$self` and names starting `__` are the planner's own). `$x` stands for
  the id in an attribute that names nodes (`in`, `def`, `entity`, `of="$a $b"`,
  `sheet`, `narrative`, `block`, `after`, `from`, `to`, `a`, `b`, `keep`); in a binding a bare `$x` reads the value a
  `let`, `port_spec`, `channel`, `trace`, `accumulate` or `query` holds. A place's
  `$p.facet.field` is its picture's leaf and `$p.port` its instance's port, readable by
  nodes, a later `<place>`'s arguments, circuit parts and table cells; `#use.port` does
  the same for an instance the document has. Outside a binding or an id position a `$`
  is text (`title="Cost $v"`). An `id="#…"` on a new node is refused ("change it with
  `<set>`").
- **Order.** Every new node is numbered first, so any name may be read before the node
  that has it is written. Nodes are made, then the component verbs (`<place>`,
  `<make-component>`, wherever written: inside a node they go `in=` it, and siblings are
  put back in the order written, between the siblings around them, so a mark after a
  plot paints over it and no two siblings share a key; so are top-level nodes and
  places appended to one parent, however its `in=` is spelled), then the leaves that read a place (`k="{= …}"`
  or `k.bind="…"`), then every other verb in the order written, each seeing what came
  before it. The plain verbs (`set`, `add`, `delete`, `reparent`, `link`, `unlink`) are
  planned together until the next verb that reads the document, so a connection's two
  links are checked as a pair and a `<reparent after=…>` sees the moves before it.
- **Inside a node** every element is a node, except `<place>`, `<make-component>` and
  `<link>`; a component verb there goes in that node (an `in=` on it is refused).
  Every value is an attribute: text between a node's tags is refused, not dropped
  (`<text content="Hi"/>`, not `<text>Hi</text>`); so are elements or text inside a
  verb that reads none (`set`, `add`, `delete`, `reparent`, `link`, `unlink`,
  `link-figures`), prose inside `<edit>`, and one leaf given under two spellings
  (`x` and `transform2d.x`). At the top level `set`, `table` and `context` are verbs (they share names
  with node types: `set` is one of an action's writes, always inside an `<on>`).
- **No verb stands alone.** Every verb is ops in the one transaction; a circuit can be
  drawn on a sheet the same edit makes, its parts bound to sliders the edit makes.
- **XML.** Single or double quotes, the five named entities and numeric ones (`&#10;`
  for a newline, `&#x41;`), comments, CDATA; an attribute given twice is refused; a
  `$` or `#` inside a string in a binding (`'…'` or `"…"`, `\` escaping) is text, and
  so is a `}`; `<write>`,
  `<rewrite>` and `<import-svg>` hold text (markdown, SVG) that is not read as markup.
  Markdown's entities are decoded; SVG goes to its parser as written, which decodes its
  own.

### LaTeX in 2D compositions

**Built 2026-11-17.** The top-level `<typeset-math>` verb typesets one TeX
expression in display style. Give it a `source` attribute without math
delimiters. Escape XML characters like `&amp;` for an alignment tab. The
`size` is the em size in composition pixels (default 48). The equation uses
the usual group transform and defaults to the theme's `@ink` color.
Use `font="fira"` for MathJax's Fira and Fira Math variant, or `font="tex"`
for the default TeX font.

```xml
<edit>
  <let in="#C" id="$write" value="0"/>
  <typeset-math in="#C" id="$eq" x="80" y="120" size="48"
    source="\frac{\ironpart{num}{x+1}}{\ironpart{den}{x-1}}">
    <part key="num" id="$num" fill.color="#47a8ff" draw.bind="$write"/>
    <part key="den" id="$den" fill.color="#f4a261"/>
  </typeset-math>
  <clip in="#T" id="$clip" start="0" prop="let.value">
    <key t="0" value="0"/><key t="1" value="1"/>
    <key t="2" value="1"/><key t="3" value="0"/>
  </clip>
  <link from="$clip" to="$write" rel="animates"/>
</edit>
```

`\ironpart{key}{term}` labels a balanced subexpression within the same
typesetting pass. Keys are unique ASCII letters, digits, underscores and
hyphens. Each visible mark becomes a group. Marks can share a line or sit
within a fraction, but cannot nest. Optional `<part>` elements give the
group an alias and override its `fill.color`, `draw`, or group properties.
The equation and part aliases are available to subsequent verbs in the edit.

`draw` accepts a value or binding in `0..1` on the equation or a part.
The paths stagger within each part. They trace their contours, then fade
in the complete fills while fading out the tracing strokes. Decreasing
progress reverses the effect. The named groups also accept ordinary
animation clips for location, rotation, scale and opacity.

To revise the source, use `<typeset-math id="#equation" source="…"/>`.
It replaces the generated paths in one transaction, retains the equation
group and existing named part groups, and preserves their controls and
animation links. To change only the font, use
`<typeset-math id="#equation" font="fira"/>`. A missing mark leaves its group empty so a later
revision can reuse it. Use the same verb with a `<part key="…">` to
update color and progress controls. Use `<set>` to change an existing
group's transform or opacity.

The shared web and desktop model API loads MathJax and the chosen
bundled font on demand. It gives the Rust planner a restricted SVG, which
`iron_svg` converts to packed paths. The document saves the source in
an ordinary `let` alongside its baked outlines. Playback and export
use normal groups, paths and timeline values. Unsupported glyphs and
invalid TeX produce an edit diagnostic and leave the document unchanged.
The built-in agent's `math` topic gives the same authoring example.

## 4. Planning and applying

`edit::plan(store, xml, svg)` returns a `Plan`: the ops, the verb (top-level element,
from 0) that wrote each, the edit's names, a short description of each verb, and
summary lines (`<created alias id/>`, `<placed …/>`, `<table id/>`, `<circuit>` with the
sheet's netlist, `<imported>`, `<write blocks/>`). Nothing is applied: each verb is
planned on a scratch copy holding everything before it. `svg` is how SVG becomes a
fragment (iron_svg, which iron_document does not depend on).

The engine's `edit_plan` call answers the plan as JSON, or errors already counted by
verb, or a question (`<link-figures>` asking which entity to keep). `applyEdit`
(`edit.ts`) applies the plan's ops with the model as actor through `api.apply`, so the
agent's merge with the person's changes (merge.ts) sees every op, now including those
of verbs that used to be "store-built"; then it reads the store's errors back in the
edit's terms (op → verb, minted id → `$name`) and wraps the summary in `<applied>`.
If the document moved between planning and applying (the person, mid-turn), the
agent's merging API refuses the plan as stale and `applyEdit` plans again, so new ids,
order keys and compound verbs see the change; a node the person deleted is reported
as "the person deleted #P while you worked", at plan time as at apply time.
`edit::transaction` does the same for Rust callers (the demo's gallery).

**The model's reply is read in Rust too** (`iron_document::reply`), in the same reader
(`parts::elements`, whose elements carry their source spans):

- `reply::read` finds what a reply asks for: its `<edit>` (or a `<revise>` of the
  rejected edit), `<look picture>`/`<look expand>`, `<topic name>` and `<continue/>`,
  and its note (the reply without the edit and picture looks). An edit never closed
  goes to the planner whole, which says what is wrong. Markdown stays in TypeScript:
  `executableReply` drops code examples before a chat reply's edit is looked for.
- `reply::revise` applies a `<revise>` to the rejected edit as written, counting its
  verbs as the planner does (the TypeScript `revise.ts`, a second reader that had to
  agree with this one by hand, is gone). A changed value is written escaped once.
- `reply::stream` reads a reply as it streams, for the chat: its prose with half-written
  markup held back, the edit or revise forming, and its verbs whose opening tags have
  ended (`closed` when written to their end). `parts::partial` reads markup still being
  written as far as it goes. The agent sends each view as a `stream` event after the
  text it read, one engine read at a time; drawing an edit as it forms can start from it.

The engine calls are `reply_read`, `reply_stream` and `edit_revise`, with `version` and
`netlists` (every schematic sheet's), so the agent reads none of the engine's XML either.
A rejected edit's errors reach the chat as data (`EditOutcome.errors`), not parsed back
out of its `<edit-failed>` markup. The bun tests' fakes read no markup: each test
scripts a reply with what the engine reads in it and an edit with its plan
(`tests/fixtures/plan.ts`); the reader and planner are tested in Rust (`tests/reply.rs`,
`tests/edit.rs`).

Removed with the TypeScript parser: `schema.ts`'s `coerce` and `resolveLeaf`,
fast-xml-parser, and the engine calls only it used (`compose`, `circuit`, `context`,
`place`, `next_relation_ids`, `import_svg_at`, `narrative_write`, `narrative_rewrite`).
`compose::compose_ops` and `compose_after`, the component verbs' own entry with
`as="$p"`, went in the council's first round: tests place components through
`edit::plan` too.

## 5. Breaking changes

- `<create type="rect" parent="#1">` is gone: write `<rect in="#1">`.
- `as="$p"` on `<place>`/`<make-component>` is `id="$p"`; `<revise>` names verbs by
  `id` or number.
- `to=` on `<table>`, `<import-svg>` and `<reparent>` is `in=`.
- Node markup names new nodes `id="$x"`, not `id="x"` (`std/*.xml`, the gallery).
- Edits that were alone in their `<edit>` now share it, and are one undo step.
