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
  whole edit. `$x` stands for the id anywhere an id goes; in a binding a bare `$x` reads
  the value a `let`, `port_spec`, `channel`, `trace`, `accumulate` or `query` holds. A
  place's `$p.facet.field` is its picture's leaf and `$p.port` its instance's port;
  `#use.port` does the same for an instance the document has. An `id="#…"` on a new
  node is refused ("change it with `<set>`").
- **Order.** Every new node is numbered first, so any name may be read before the node
  that has it is written. Nodes are made, then the component verbs (`<place>`,
  `<make-component>`, wherever written: inside a node they go `in=` it, and siblings are
  put back in the order written, so a mark after a plot paints over it), then the leaves
  that read a place, then every other verb in the order written, each seeing what came
  before it.
- **Inside a node** every element is a node, except `<place>`, `<make-component>` and
  `<link>`. At the top level `set`, `table` and `context` are verbs (they share names
  with node types: `set` is one of an action's writes, always inside an `<on>`).
- **No verb stands alone.** Every verb is ops in the one transaction; a circuit can be
  drawn on a sheet the same edit makes, its parts bound to sliders the edit makes.
- **XML.** Single or double quotes, `&#10;` for a newline, comments, CDATA; `<write>`,
  `<rewrite>` and `<import-svg>` hold text (markdown, SVG) that is not read as markup.

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
`edit::transaction` does the same for Rust callers (the demo's gallery).

Removed with the TypeScript parser: `schema.ts`'s `coerce` and `resolveLeaf`,
fast-xml-parser, and the engine calls only it used (`compose`, `circuit`, `context`,
`place`, `next_relation_ids`, `import_svg_at`, `narrative_write`, `narrative_rewrite`).

## 5. Breaking changes

- `<create type="rect" parent="#1">` is gone: write `<rect in="#1">`.
- `as="$p"` on `<place>`/`<make-component>` is `id="$p"`; `<revise>` names verbs by
  `id` or number.
- `to=` on `<table>`, `<import-svg>` and `<reparent>` is `in=`.
- Node markup names new nodes `id="$x"`, not `id="x"` (`std/*.xml`, the gallery).
- Edits that were alone in their `<edit>` now share it, and are one undo step.
