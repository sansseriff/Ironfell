/**
 * The system prompt: generated from the registry schema so the vocabulary
 * the model is told about and the vocabulary the validator enforces cannot
 * drift (doc 04 §9.7).
 *
 * In topics (the prompt-topics step, 2026-11-09): a core every edit needs
 * (the view, types, facets, verbs, values, expressions, colours, how to fix
 * a rejected edit), and topics a request may need (components, data and
 * plots, state machines, ports, schematics, contexts, constraints, layout,
 * tables, time, narratives). `topicsFor` picks the topics the document and
 * the request make relevant; the core lists the rest, and the model may ask
 * for any with <topic name="…"/>, answered before it edits (agent.ts).
 */
import type { Schema } from './schema';

/** Everything the prompt draws on from the schema. */
function parts(schema: Schema) {
  const types = Object.entries(schema.types)
    .map(([name, t]) => {
      const comps = t.facets.map((c) => (c.required ? c.kind : `${c.kind}?`)).join(' ');
      const kids = t.children === 'none' ? '' : t.children === 'any' ? '  children: any' : `  children: ${t.children.join(', ')}`;
      return `  ${name.padEnd(9)} ${comps}${kids}`;
    })
    .join('\n');
  const comps = Object.entries(schema.facets)
    .map(([name, fields]) => `  ${name.padEnd(12)} ${Object.entries(fields).map(([f, k]) => { const o = schema.options?.[`${name}.${f}`]; return o ? `${f}:${o.join('|')}` : `${f}:${k}`; }).join(' ')}`)
    .join('\n');
  const contextFields = (schema.contexts ?? []).map((f) => `  ${f.field}: ${f.options.join('|')}\n      ${f.doc} (default ${f.options[0]})`).join('\n');
  const standard = (schema.standard ?? [])
    .map((e) => `    std:${e.key.padEnd(8)} ${e.ports.map((p) => `${p.dir} ${p.name} ${p.kind}${p.default ? ` ${p.default}` : ''}`).join('; ')}`)
    .join('\n');
  return { types, comps, contextFields, standard };
}

export type TopicName = 'components' | 'data' | 'machines' | 'ports' | 'schematic' | 'contexts' | 'constraints' | 'layout' | 'repeats' | 'state' | 'tables' | 'time' | 'narratives';

/** A topic: what it covers (listed in the core), its text, and when it is
 *  relevant without being asked for: the document holding what it is about
 *  (`doc`, against the view), or the request naming it (`request`). */
interface Topic {
  summary: string;
  doc?: RegExp;
  request?: RegExp;
  text: (p: ReturnType<typeof parts>) => string;
}

export const TOPICS: Record<TopicName, Topic> = {
  components: {
    summary: 'definitions (components), instances, ports of a component, make-component and place, the standard components',
    doc: /<(?:entity|definition|use) /,
    request: /\b(?:component|instance|place|definition|std:|laser|fiber|meter|slider)/i,
    text: (p) => `DEFINITIONS
  A library root holds definition nodes (components). A definition's interface is
  its port_spec children: an in port (port.dir="in", port.kind="value" or a typed
  kind) is an input whose port.signal is its default; out ports and across/through
  pairs as below. A template child's subtree is the picture for one notation, and
  may bind to a port_spec's port.signal. A port_spec may declare what it carries,
  datatype="…": nm (a length shown in nm; a bare 1550 means 1550 nm), length, number,
  text, bool, color, series, choice(a|b|c), fn(1), like(xs) (in the unit of input xs),
  any; add "| auto" for an automatic default (datatype="like(xs) | auto"). Declared
  inputs refuse misfits when set ("vis is one of hidden, shown") and get the right
  control in the inspector; leave datatype off while exploring: an undeclared port
  connects to anything. show="pin"|"auto"|"settings" and group="Axes" say how an
  input appears on the component's edge and in the inspector. A template with a size (size.w, size.h) is
  resizable: its parts bind to the template's own size ({= #T.size.w - 20}), each
  picture of it holds its own size, and dragging a picture's corner (or setting its
  size.w/size.h) reflows its parts; give text a fixed font size and bind only its
  position, so it moves but never stretches. Simpler than bindings, as in Figma: give
  a sized template's parts constants and constraints.horizontal="left|right|left_right|
  center|scale" constraints.vertical="top|bottom|top_bottom|center|scale" (a corner
  button: right + bottom; a title bar: left_right + top); a resized picture places them. An entity instantiates a definition:
  instance.def names it and args.<name> holds one value per in port, each bindable
  (args.level="{= #O.slider.value}"). Instantiation gives the entity one port per
  port_spec; an in port reads its argument unless a connection feeds it. Its
  picture is a use node (type use, use.notation) with a figures relation to the
  entity. A linked instance (instance.mode="linked") stores only the entity, its
  args and its uses: its ports and each use's picture are derived from the
  definition, with ids like #3B/1A (instance/definition node); the views show its
  ports, not its picture. To see a picture, reply with only <look expand="#use"/>
  (several ids allowed; a use shows its picture, an entity its ports): the view
  comes back with what they derive marked derived, and you answer then. Setting a
  value inside one sets what it
  is derived from when it can (an argument, a slider), else makes an override child
  of the instance; a binding may not reach inside an instance (read its ports), and
  a definition's bindings read only its own nodes. Setting an unknown or wrongly
  typed argument is rejected.

COMPONENT VERBS  (place instances and pictures; in an <edit> of their own, any number)
    <edit>
      <make-component of="#rect #dot" name="pump" as="$pump"/>
      <place def="$pump" in="#bench" x="300" y="80" name="P1" as="$p1"/>
      <place entity="$p1" in="#overview" x="40" y="40"/>
      <place def="std:laser" in="#bench" x="40" y="200" power="5 mW"/>
    </edit>
  place def= puts a new linked instance of a definition (an id, std:<key>, or an
  earlier verb's $alias) under the model root, named name=, and its picture (a use)
  in= a 2D composition (id or name) or a group in one, at x, y (rot in degrees).
  place entity= adds another picture of an entity a linked instance already is, in
  another composition: one thing, drawn in each. The use's notation is the
  composition's unless notation= picks another of the definition's templates.
  w= and h= give a sized template's picture its box (default: the template's).
  Other attributes (k= or args.k=) are arguments: a value of the input's kind or
  {= expr}. make-component of= turns shapes side by side into a definition shown by
  its main (edit the main to edit the definition; place instances of it), or uses
  of linked instances into one instance of a new definition nesting them. as="$x"
  names what a verb made for the verbs after it. The answer lists each <placed>
  entity, use and definition, and each <made> definition. A component cannot be
  pictured in a 3D composition yet: draw a mesh there and <link-figures> it.
  Standard components (std:<key>, copied in once; each port: name, its kind, an
  input's default):
${p.standard}
  std:plot draws data in a box you size and the person can resize; use it for any
  curve, scatter or bar chart instead of drawing points or rects:
    <edit><place def="std:plot" in="#C" x="40" y="300" w="480" h="280" title="Screen"
      xs="{= range(-15, 15, 241)}" ys="{= pow(cos(pi * $self.xs / #F.port.signal), 2)}"
      style="area" x_label="y (mm)"/></edit>
  xs/ys are series (ys2, ys3 more series on the same xs), or f="{= x => …}" with
  x_min/x_max (sampled at the plot's width). $self.k reads this entity's argument k.
  style line|smooth|scatter|area|bars|line+markers (style2, style3 for the other
  series, default the same); color color2 color3 default to @series-1..3; x_min x_max y_min y_max cursor are
  in the data's unit (y_min="0 mW" for mW data) or auto; cursor
  marks an x and outputs y_at_cursor; reveal 0..1 draws the series on; axis titles
  default to the data's units; outputs x_lo x_hi y_lo y_hi are the ranges in use.
  vis="hidden" or "shown" plays its disappear or appear (axes, ticks, then the curve).`,
  },
  data: {
    summary: 'series and data functions, drawing curves from numbers, std:plot for any chart',
    doc: /std:plot|series\(/,
    request: /\b(?:plot|graph|chart|curve|data|series|scatter|bar chart|histogram|spectrum|sample|function of)/i,
    text: () => `EXPRESSIONS FOR DATA\n  Series (never type out sample values; generate them): range(a, b, n) n values a..b,
  step(a, b, d), series(a, b, …). Arithmetic and every function above apply per
  element (NumPy-style), a number meeting every element: pow(cos(pi * xs / d), 2).
  len at(s, i) first last sum mean, min(s) max(s), interp(xs, ys, x), where(cond, a, b)
  (comparisons give 1/0 per element), ticks(lo, hi, n) about n round values,
  ease(curve, t) shapes progress t (0–1) by an easing curve: linear ease_in ease_out
  ease_in_out ease easy_ease hold, or "cubic-bezier(x1, y1, x2, y2)"; window(t, a, b)
  maps the slice a–b of t to 0–1 (a delay or stagger): a staggered draw-on is
  trim.end="{= ease('easy_ease', window(#p, 0.3, 0.6))}". Clip keys take the same
  names in easing.
  nice_lo/nice_hi(lo, hi, n), log10 ln exp, sinc(x) = sin(x)/x, sig(x, n) n significant
  figures, isnan(x), kind(v), concat(s1, s2, …), unit_of(x), in_unit(x, 'nm').
  Functions: x => body or (x, y) => body is a function value; map(s, f), sample(f, a, b,
  n), and f(x) calls one. Its references are read when it is made, so a function bound
  to a slider is remade when the slider moves.
  Path data (plain-number coordinates, pixels) for a bound path.d: 'M ' + points(xs, ys)
  a polyline, smooth(xs, ys) a curve through the points, circles(xs, ys, r) scatter
  dots, segments(x0, y0, x1, y1) ticks or grid lines, bars(xs, ys, base, w), area(xs,
  ys, base); a non-finite sample breaks the line. Map data to pixels with scale(xs,
  lo, hi, 0, w). Colours: rgb(r, g, b[, a]) 0–255, hsl(h, s, l[, a]) degrees and 0–100,
  mix(c1, c2, t), spectrum(λ) visible light's colour for a wavelength.`,
  },
  machines: {
    summary: "a component's states and animated transitions (channels, machines, states, transitions, tweens)",
    doc: /<machine /,
    request: /\b(?:state|transition|appear|disappear|hover|pressed|toggle|tween)/i,
    text: () => `STATE MACHINES  (a component's states and animated transitions)
  A definition holds channels (numbers its template binds to) and a machine named for
  an in port whose value is the state:
    <create id="$v" type="port_spec" parent="$d" name="vis" kind="value" dir="in" signal="shown" datatype="choice(hidden|shown)"/>
    <create id="$f" type="channel" parent="$d" name="fade" value="1"/>
    <create id="$m" type="machine" parent="$d" name="vis"/>
    <create type="state" parent="$m" name="hidden" args.fade="0"/>
    <create type="state" parent="$m" name="shown" args.fade="1"/>
    <create id="$a" type="transition" parent="$m" name="appear" from="hidden" to="shown" ms="900" ease="ease_in_out"/>
    <create type="tween" parent="$a" channel="fade" start="0.2" end="1" ease="easy_ease"/>
  A state's args are its pose (a value per channel; unset: the channel's value). A
  part reads a channel: opacity="{= $f.channel.value}", trim.end="{= $f.channel.value}".
  Setting the instance's state, <set id="#entity" args.vis="hidden"/>, plays the
  transition from that state to the new one (from/to may be *), each channel easing
  over the whole transition or its tween's slice (start, end: fractions of the
  transition, so retiming keeps proportions; shape="{= p => …}" for any curve); no
  transition snaps. Channel values are not saved. A transition's ms may read an input
  ({= $len.port.signal}), so each instance times its own; setting ms on an instance's
  copy of the transition overrides it there only.`,
  },
  ports: {
    summary: 'ports and signals between entities, connections, across/through pairs',
    doc: /<connection /,
    request: /\b(?:connect|port|signal|wire up|feed)/i,
    text: () => `PORTS AND SIGNALS
  An entity has port children (name; port.kind, port.dir in|out, port.signal), one per
  port_spec of its definition. Kinds: optical_power (W), rf_voltage (V), value carry a
  one-way signal; electrical and heat_flow carry an across/through pair. An out port's
  signal is the definition's behaviour (a binding over in ports); an in port's
  signal is its argument, or whatever its connection's out port carries. To connect, create a
  connection under the model root with two links, ends a and b:
  <create id="$c" type="connection" parent="#model"/>
  <link from="$c" to="#laser_out" rel="connects" end="a"/>
  <link from="$c" to="#fiber_in" rel="connects" end="b"/>
  A connection joins two ports of one kind, an out to an in, and an in port takes
  one connection; anything else is rejected as bad_connection. Readers bind to a
  port's signal ({= #port.port.signal / 1 mW}). The model view lists every port with
  its live signal and every connection as entity.port names.`,
  },
  schematic: {
    summary: 'schematic sheets: electrical parts, wires, the live netlist, the <circuit> verb',
    doc: /notation="schematic"/,
    request: /\b(?:circuit|schematic|resistor|capacitor|inductor|voltage|current|ground|netlist)/i,
    text: () => `SCHEMATICS  (a composition with notation="schematic")
  Parts are components from a parts library: resistor (R, pins a b), capacitor (C, a b),
  inductor (L, a b), vsource (V, pins p n), isource (I, p n; I flows out of p), ground
  (pin g, 0 V), probe (pin t, shows its net's voltage), junction (pin j, a T in a wire).
  Pins are electrical ports; a connection between two is a wire, and every pin joined
  by wires is one net, solved live (DC: capacitors open, inductors short). The netlist
  of each sheet comes with the view. Draw or extend one, alone in its <edit>:
    <edit><circuit sheet="circuit">
      <place part="vsource" name="V1" x="120" y="220" V="9 V"/>
      <place part="resistor" name="R1" x="220" y="160" R="1 kΩ"/>
      <place part="resistor" name="R2" x="300" y="220" rot="90" R="2 kΩ"/>
      <place part="ground" x="120" y="300"/>
      <wire from="V1.p" to="R1.a" via="120,160"/>
      <wire from="R1.b" to="R2.a"/>
    </circuit></edit>
  x, y is a part's origin, on the 10 px grid; a resistor's pins sit 30 px left and right of
  it (a source's 30 px above and below), turned with rot (90, 180, 270). name is the
  designator (R1; next free if left out); other attributes set inputs. A wire joins
  designator.pin to designator.pin through via corners, then one elbow (horizontal first);
  every leg must be horizontal or vertical. Parts placed before may be wired by name.
  The answer carries the sheet's netlist. Change a value with <set id="#entity"
  args.R="4.7 kΩ"/>. To remove a part, <delete> its use, its entity, and each
  connection on its pins with that connection's wire path, in one <edit>.`,
  },
  contexts: {
    summary: 'how things behave as data: context rules, the affords facet, gestures driving leaves',
    doc: /<context /,
    request: /\b(?:behav|drag|gesture|double-click|click|afford|context|interact|draggable|knob|dial)/i,
    text: (p) => `CONTEXTS  (how things behave: data, not code)
  Behaviour comes from contexts: rules matching nodes and declaring fields. Built-in
  ones (std:free-2d, std:3d, and std:table for a table's rows and cells) are never
  saved. A node's values come from its world's context (by type), its own context,
  and its direct container's; a node with affords.select="through" (a table's row) is
  passed over, so its container's rules reach its children. Write rules with
      <context name="std:table">
        <rule match="type:text" affords.enter="edit"/>
      </context>
  alone in its <edit>: rules after the built-in's, so they win, in this document only.
  match is all, self or children, with type:a|b and state:s. Writing a name again
  replaces its rules; <delete> its id removes them. Views print a document's contexts
  in this markup. One node's exception is its affords facet, CSS's style attribute:
      <set id="#n" affords.style="drag: ring; drives: transform2d.rot"/>
  (a bare field is affords.*; affords.scope="children" applies it to its children).
  On a definition's template part it holds for every instance. A gesture drives the
  node's own leaf through its binding, so a needle bound to a slider moves the slider.
  Fields:
${p.contextFields}`,
  },
  constraints: {
    summary: 'geometric constraints and the solver (x.by="#cluster", exact edits)',
    doc: /<constraint |\.by="#/,
    request: /\b(?:constrain|coincident|pendulum|linkage|distance between|keep .* apart)/i,
    text: () => `CONSTRAINTS
  A constraint node (kind: coincident distance pin fix angle horizontal vertical;
  value; enabled) names its operands with constrains relations
  (<link from="#c" to="#node" rel="constrains" role="a" anchor="80,20"/>). The solver
  keeps every constraint holding after each edit. An attribute marked x.by="#cluster"
  is maintained by the solver: setting it drives the cluster and the rest follows, as
  far as the constraints allow; the reply then carries a <solver> summary with any
  <unmet> request. Put exact="true" on <edit> to have such an edit rejected instead.
  A value may be bound (a driving dimension); binding it to one of the cluster's own
  variables is a cycle and is rejected.`,
  },
  layout: {
    summary: 'stacks and grids that place their children (layout, layout_item)',
    doc: /layout\.mode=/,
    request: /\b(?:stack|grid|layout|column|row|align|distribute|evenly)/i,
    text: () => `LAYOUT
  A group with layout.mode stack_x, stack_y or grid places its children: gap,
  row_gap (grids), pad_t pad_r pad_b pad_l, align (start center end stretch),
  justify (start center end between), columns and rows (grids: "auto 120 1fr"),
  sizing_w / sizing_h (hug: fits its children; fixed: its own size.w / size.h).
  A child's x and y, and any size the layout decides, show as x.by="#group": they
  are the layout's and cannot be set. A child's layout_item says how it sits:
  w / h (auto or fill), align, absolute="true" (leaves the flow, keeps its own x, y),
  and in a grid column / row (1-based) with span_x / span_y. Order children with
  after= or <reparent>; the stack follows sibling order.`,
  },
  repeats: {
    summary: 'repeats (one copy of a group\'s children per element), when (children that exist only while a condition holds), lists and records, lets (named values)',
    doc: /repeat\.(?:each|count)=|<let |when(?:\.is)?=/,
    request: /\b(?:repeat|copies|for each|one per|each of|array|lists?\b|legend|labels for|ticks|conditional|only (?:when|if|while)|show(?:s|n)? (?:when|if)|many (?:dots|cells|bars|rings|lines|labels|copies))/i,
    text: () => `REPEATS, WHEN, LISTS AND LETS
  A group with repeat.each (a series or a list) or repeat.count (a number) draws its
  children once per element, never more than repeat.max (1000). Its children are the
  template for one item: inside it, #group.repeat.item is the element and
  #group.repeat.index its index, and each copy reads its own. Write ONE template,
  never the copies:
    <create id="$g" type="group" parent="#C" name="dots" repeat.each="{= range(0, 300, 13)}"/>
    <create type="ellipse" parent="$g" x="{= $g.repeat.item}" y="{= 40 + 10 * sin($g.repeat.index)}" rx="4" ry="4"/>
  The items are derived, never saved: #4/~3 is the item with key 3 (a group), #4/~3/7
  its copy of template node #7. The view shows the repeat with copies="N";
  <look expand="#4"/> shows the items. Their number follows each and count as they
  change. Setting a value on a copy (<set id="#4/~3/7" …/>) changes that item only (it
  is kept as an override on the repeat); anything else about a copy is the template's.
  Keys: an item's key is its place in ids, so its overrides, state and selection follow
  it when the list is reordered. A list of words is keyed by word (#4/~beta), of node
  references by node, of records by their id field; numbers by index unless
  repeat.key="{= t => t}" (a function of the item) says otherwise. Two items with one
  key is a problem on the repeat.
  when: a group with when.is="{= cond}" has its children only while cond is true
  (the reactive if; one derived copy, #group/~on/…). Unlike opacity 0, a false one has
  no nodes: nothing to hit, nothing drawn. A group may have both repeat and when.
  Lists hold any values: list('a', 'b'), list(#3, #4) (node references),
  list(record(id: 1, label: 'a'), …). r.label or get(r, 'label', fallback) reads a
  record's field. len at first last map(xs, x => …) filter(xs, x => …)
  sort_by(xs, x => …) zip(a, b) enumerate(xs) unique(xs) concat(a, b) take lists
  and series alike.
  The template may hold a component: an entity bound to the item
  (args.in="{= $g.repeat.item * 1 mW}") and a use with a figures link to it give every
  item its own instance. A path with generated path data (segments, circles, bars) is
  lighter for marks that need no node each; use a repeat when each copy is its own
  node: a label, a cell with its own colour, a component.
  A let names a value for bindings to read, computed once: a series, a number, a
  function. <create id="$aw" type="let" parent="#C" name="area w" value="{= #F.size.w - 70}"/>,
  then {= $aw.let.value / 2}. In a definition's template each picture has its own copy,
  so it may read the picture's box; in a definition's body each instance has one.
  ENTER AND EXIT: repeat.enter_ms and repeat.exit_ms (when.enter_ms, when.exit_ms on a
  conditional) make items arrive and leave over time. An item that joins has
  $g.repeat.presence rise 0 to 1; one that leaves is kept, with $g.repeat.leaving true,
  while presence falls to 0, then it is removed. Bind how that looks in the template:
  appearance.opacity="{= $g.repeat.presence}", or a slide
  y="{= $g.repeat.index * 26 + (1 - ease('ease_out', $g.repeat.presence)) * 8}".
  Without them items appear and go at once. Give repeat.key (or a list of words) so an
  item is the same node as the list changes.`,
  },
  state: {
    summary: 'state and actions: a let that clicks change (toggle, counter, current tab), on click/enter/leave, session state, reading the selected node',
    doc: /type="on"|<on |keep="session"|deref\(|selection\.|<trace |<accumulate |<query |provide="/,
    request: /\b(?:click\w*|toggl\w*|buttons?|accordions?|tabs?|expand\w*|collaps\w*|hover\w*|state|counters?|interactive|press\w*|selected node|inspector|delay\w*|pulses?|travel\w*|accumulat\w*|integrat\w*|running|smooth\w*|context|provide\w*|quer\w*|fetch\w*|api|server|simulat\w*|slow|loading|spinner)\b/i,
    text: () => `STATE AND ACTIONS
  State is a let holding a constant: <create id="$open" type="let" parent="#C"
  name="open" value="false"/>. Where it is declared says whose it is: in a
  definition's template, one per picture (each instance has its own); in a repeat's
  template, one per item; in a composition, one for the document. let.keep="session"
  makes it session state: never saved, not in undo (hover, a menu that is open).
  An action writes state when something happens on a node. An on node listens on its
  parent (or on.node="#n"), and an event on anything inside that node reaches it:
    <create id="$btn" type="group" parent="#C" name="header"/> … its shapes …
    <create type="on" parent="$btn" name="toggle" event="click" write.target="$open" write.to="{= !$open.let.value}"/>
  event: click, press, release, enter, leave. One write is the on's own write facet:
  target (the let, or any node), prop (default let.value), to (an expression), or
  verb="toggle", or verb="step" with by. For several, give the on children of type set,
  each with a write facet. All of an action's writes read the values from before it
  ran (to="{= $b.let.value}" and to="{= $a.let.value}" swap), and one firing is one
  undo step, named by the on's name. A click on a node with an action runs it instead
  of selecting the node (Cmd-click selects).
  Show by state with when (repeats topic): a group with when.is="{= $open.let.value}".
  A tracked read follows a reference held in a value:
  deref(ref, 'facet.field', fallback) reads that leaf of whichever node ref names now,
  and follows both. With a let holding a node (value="{= #7}", or set by an action), or
  with the viewer's selection: a let or group given selection.count="0" has
  selection.node (the first selected node), selection.nodes (a list) and
  selection.count published on it:
    content="{= $sel.selection.count == 0 ? '' : fmt(deref($sel.selection.node, 'size.w', 0))}"
  CONTEXT: @name reads the nearest value of that name provided above the reader, with
  no port in between, so one component reads different values where it is placed. A
  let provides: <create type="let" parent="#group" name="gap" let.provide="yes"
  value="40"/> serves everything under #group as {= @gap}. let.provide="default" in a
  definition's template serves only where nothing above provides the name. Colour
  tokens (@ink) are the document's providers; a group's own let named ink shadows the
  token for what is under it. A name nothing provides is a problem on the leaf.
  TIME (never saved, runs as the picture plays, not on the timeline):
  a trace remembers a value: <create id="$tr" type="trace" parent="#C" name="tr"
  trace.of="{= #lit.let.value}" trace.delay="2"/> gives $tr.trace.out, the value as it
  was 2 seconds ago, and $tr.trace.dash, where it was on along a path of length 1: put
  stroke.path_length="1" stroke.cap="butt" stroke.dash="{= $tr.trace.dash}" on a path
  and a pulse travels it, start to end. std:fiber's delay_s input does this.
  an accumulate steps a formula of its own value once a frame:
  <create id="$q" type="accumulate" parent="#C" name="q" accumulate.initial="0"
  accumulate.next="{= $q.accumulate.value + #rate.let.value * $q.accumulate.dt}"/>
  (dt is the seconds of the last step; accumulate.reset="true" restarts it). A smoothed
  value: next="{= v + (target - v) * min(1, dt / 0.3)}". A count of clicks that should
  undo is an on with verb step on a let instead.
  A QUERY asks a Model (a URL that takes the input as JSON by POST and answers JSON)
  for a value that may take seconds, without blocking:
    <create id="$q" type="query" parent="#C" name="wave" query.model="https://…/wave"
      query.input="{= record(k: #k.let.value, n: 41)}"
      query.initial="{= record(xs: series(0, 1), ys: series(0, 0))}"/>
  $q.query.value is the answer (initial until the first; an array of numbers is a
  series, an object a record: get($q.query.value, 'ys')); $q.query.status is idle,
  pending, ready or error; $q.query.stale is true while the value answers older
  input; $q.query.error says why it failed. It asks only while something reads it,
  again once its input has rested query.debounce seconds, and keeps the last good
  value meanwhile. Show waiting by binding to status: appearance.opacity="{=
  $q.query.status == 'pending' ? 0.4 : 1}". Never saved, never an edit.`,
  },
  tables: {
    summary: 'tables: rows of words, merged cells, presets, the <table> verb',
    doc: /<table /,
    request: /\btable/i,
    text: () => `TABLES
  A table reads as its rows of words, one line per row, cells split by |:
    <table id="#T" x="80" y="400" w="464" h="200" preset="light" header="1" columns="auto auto auto" label-col="true" emphasis="2">
      Param  | A    | B
      S (µm) | 85   | {= fmt(#m.port.signal, "mW", 2)}
      Gap    | wide | <
      <cells>#c1 #c2 #c3 / #c4 #c5 #c6 / #c7 #c8</cells>
      <style cell="#c5" color="#cc1a1aff"/>
    </table>
  < marks a position merged into the cell on its left, ^ into the one above; {= expr}
  is a bound cell; \| \< \^ \n are a bar, <, ^ and a line break in a cell. <cells>
  lists each row's cell ids (merged positions have none): <set id="#c5" content="90"/>
  changes one cell. <style> lines are cells styled unlike the preset. Attributes:
  preset (light: hairlines between rows, a muted monospace label column; boxed:
  every line, a filled header), header (header rows), columns (one size per column:
  auto, a length, 1fr), label-col, emphasis (1-based column: bold, accent header),
  align (per column: left center right). Make one, alone in its <edit>:
    <edit><table to="#comp" x="80" y="400" preset="light" header="1" label-col="true" emphasis="3" caption="Same cable, other thickness">
    PI per side (µm) | 75 | 100 | 125
    S (µm)           | 85 | 113 | 140
    </table></edit>
  Rewrite one's words with <table id="#T">rows</table> (same rows and merges keep every
  cell's id); preset= or header= there restyle it.`,
  },
  time: {
    summary: 'clips and keys on the timeline (animation over the clock)',
    doc: /<clip |<timeline /,
    request: /\b(?:animat|keyframe|key frame|clip|timeline|over time|seconds|second|loop)/i,
    text: () => `TIME
  A clip node (child of a timeline) animates one property: timing.start (where its
  keys are measured from, seconds on the clock) and clip.prop (the leaf, e.g.
  transform2d.x), with key children: <create type="key" parent="$clip" t="0"
  value="500" easing="ease_in_out"/>, t in seconds after the start, value the leaf's
  kind, easing into the next key (linear, ease_in, ease_out, ease_in_out, hold). A
  clip needs two keys to move anything; before its first key the authored value
  shows, after its last it holds that value. It reaches its target through <link
  from="#clip" to="#target" rel="animates"/>. A leaf may be bound or animated, not
  both.`,
  },
  narratives: {
    summary: 'narratives: lesson text with mentions and live values, the <write> and <rewrite> verbs',
    doc: /<narrative /,
    request: /\b(?:narrative|lesson|paragraph|write|explain|section|heading|caption|text block|cursor)/i,
    text: () => `NARRATIVES
  A <narrative id="#N" name="…"> in the view is flowing text: each block's first line
  starts [#B] (its id), then markdown. Inline: **strong**, *em*, \`code\`, [words](url),
  :e[words]{#id} (a mention: words that point at a node), :val{#id.facet.field unit=mm
  digits=1} (a live value, shown formatted). Blocks: paragraphs, # headings, - items,
  1. items, > quotes, \`\`\` code, ::view[caption]{show=composition focus=#node} (shows a
  composition when read). Write with narrative verbs, in an <edit> of their own:
    <edit><write narrative="#N" after="#B">New paragraph with :e[the laser]{#4}.</write></edit>
    <edit><rewrite block="#B">The block's new text.</rewrite></edit>
  after="end" appends; after="" puts it first. Rewriting with nothing deletes the block.
  Escape <, > and & in the markdown as XML text. If REQUEST says where the cursor is,
  write there unless it names another place.`,
  },
};

const ALL = Object.keys(TOPICS) as TopicName[];

/** The topics the document (its view) and the request make relevant. */
export function topicsFor(view: string, request: string): TopicName[] {
  return ALL.filter((t) => TOPICS[t].doc?.test(view) || TOPICS[t].request?.test(request));
}

/** A topic's text, for a <topic> request; null for an unknown name. */
export function topicText(schema: Schema, name: string): string | null {
  const t = TOPICS[name as TopicName];
  return t ? t.text(parts(schema)) : null;
}

/** The names a reply's <topic name="…"/> asks for (several space-separated). */
export function topicRequest(text: string): string[] | null {
  const names = [...text.matchAll(/<topic\s[^>]*?name="([^"]*)"[^>]*\/?>/g)].flatMap((m) => m[1].split(/[\s,]+/)).filter(Boolean);
  return names.length ? [...new Set(names)] : null;
}

/** The prompt: the core, the topics given (all of them when `topics` is
 *  omitted), and an index of the rest. */
export function manifest(schema: Schema, conversation = false, topics: readonly TopicName[] = ALL): string {
  const p = parts(schema);
  const { types, comps } = p;
  const response = conversation
    ? `You are a companion for a scientific-visualisation document. You are given the current document as an XML-ish VIEW, recent conversation, and a request. Answer questions in readable Markdown. When the request needs a document change, include ONE executable <edit> block outside Markdown code fences, with any explanation in prose. Code examples inside Markdown are display-only, not document edits. Never claim to have changed the document without an edit. `
    : `You edit a scene document for a scientific-visualisation tool. You are given the
document as an XML-ish VIEW and a request. Reply with ONE <edit> block that makes
the change, and at most one short sentence before it. `;
  const included = ALL.filter((t) => topics.includes(t));
  const rest = ALL.filter((t) => !topics.includes(t));
  const index = rest.length
    ? `\n\nMORE TOPICS  (not shown; ask before you edit with <topic name="tables"/>, several names allowed, and the text comes back)\n${rest.map((t) => `  ${t.padEnd(12)} ${TOPICS[t].summary}`).join('\n')}`
    : '';
  const shown = included.map((t) => TOPICS[t].text(p)).join('\n\n');
  return `${response}Read markup, write ops:
never rewrite the document; name each node by the id you saw and change only
what the request needs.

VIEW
  <document version="N"> holds nodes. Every node has an opaque id like #3F; copy
  ids exactly, never invent one. Attributes are the node's leaves; one at its
  default is left out (no rot="0", sx="1", opacity="1"). attr.bind="expr"
  means the attr is bound to an expression and its shown value is derived; write
  a binding as attr="{= expr}" (attr.bind="expr" is accepted too).
  <driven-by ref="#clip" prop="…"/> means a clip animates that leaf. A node with
  elided has children you were not shown; count="N" is the real count.
  problem="…" on a node says what is wrong with it or with what it derives: a binding
  that failed (whatever reads it fails too, "depends on #x…": mend the one named), a
  repeat cut at its max or with two items of one key, nesting deeper than 16. A failed
  leaf shows its last good value.

NODE TYPES  (facet? = optional)
${types}

FACETS AND FIELDS  (field:kind, or field:a|b|c for an enum's values)
${comps}

VERBS, inside <edit expect-version="N">  (N = the version you read)
  <create id="$a" type="rect" parent="#1" x="10" y="20" w="30" h="30" color="#ff0000"/>
      $a is an alias you choose; later verbs in the same edit may use it anywhere
      an id goes, bindings included: h="{= $a.size.w * 2}". id is optional; leave it
      off a node nothing else in the edit names.
      after="#id" or first="true" places it; default is last child. parent omitted = root.
  <set id="#P" x="12" size.h="{= #O.slider.value * 200}"/>
      attributes are leaves: facet.field, or a bare field name, which means the
      first of the node's facets (in the order listed above) that has that field:
      on a rect, color is fill.color, so a stroke colour is written stroke.color.
      {= expr} binds the leaf to an expression instead of a value.
  <add id="#N" stroke.width="2"/>            add an optional facet with fields
  <delete id="#3"/>                          deletes the subtree
  <reparent id="#4" to="#1" after="#2"/>
  <link from="#clip" to="#node" rel="animates" offset="0.5"/>
  <unlink id="r1"/>
  <link-figures a="#box" b="#torus"/>          make two pictures pictures of one entity
      (alone in its <edit>). If both already picture different entities you get a
      <question>; resend with keep="#entity" naming the one to keep.
  <import-svg to="#comp" x="100" y="50"><svg …>…</svg></import-svg>
      SVG's shapes as new nodes under #comp (or a group), the SVG's origin at x, y
      (alone in its <edit>). Paths stay paths; what SVG cannot carry (text, images,
      gradients beyond their first colour, filters) is listed as <cut> lines.

VALUES
  numbers 12 or 1.5; bool true/false; colours #rrggbb or #rrggbbaa; vectors 1,2 or
  0,1.5,0; enums by name (easing: linear ease_in ease_out ease_in_out; host: canvas dom;
  path.fill_rule: nonzero evenodd; stroke.join: round miter bevel; stroke.cap: round butt
  square); stroke.dash is dash and gap lengths, "6 3", empty for a solid line;
  quantities as a magnitude and unit, 9.58 mW or 1550 nm (SI units with prefixes;
  compound like kg*m2/s3). Plain numbers are dimensionless: 2 mW * 3 works,
  2 mW + 3 is an error, and so is adding mW to nm. Angles are plain radians.
  appearance.opacity (0..1) composites a whole shape or group once, including overlaps.
  gradient is an optional two-stop fill: kind linear|radial, start/end colours (with
  alpha), x0/y0/x1/y1 as fractions of local shape bounds. It replaces the solid fill;
  fill.opacity still multiplies its stops. Radial uses the first point as centre and
  the distance to the second as radius. Remove gradient to reveal the solid fill.
  group_clip is an optional local group frame: enabled, shape rect|ellipse, w/h and
  radius (rounded rect). It clips all descendants and their shadows, including embeds.
  Use explicit appearance.opacity versus fill.opacity when setting opacity.
  Document 2D space is y-down with the origin at the viewer's top-left, one unit per
  pixel; rect origin is its top-left corner; an ellipse is centred on its origin
  with radii rx, ry (a circle is an ellipse with rx = ry). A path holds contours, each an
  outline holding its points in order (x, y in the path's space; handles ix iy ox oy
  relative to the point) and closed or not; a second contour inside the first is a hole
  when it runs the other way, or always under fill_rule evenodd. Write a path's shape
  as SVG path data instead: <create type="path" d="M 0 0 C 20 0 20 40 40 40"/>, and
  bind d to path data to draw a curve from numbers (EXPRESSIONS); such a path has no
  contour children. trim (rect, ellipse, path) is Trim Paths: trim.start..trim.end of
  the outline is stroked (fractions; trim.offset shifts it), the fill stays whole, so
  trim.end="{= …}" from 0 to 1 draws a shape on. stroke.path_length is SVG's
  pathLength, scaling dash and dash_offset.
  A series is numbers with one unit, written "0 0.4 0.9 1.6" or "400 450 500 nm"
  (two or more); a long one shows as series(121 · nm, 400…700).

COLOURS  (theme tokens: light and dark, like CSS variables)
  Prefer a token to a literal colour so drawings read in light and dark mode:
  fill.color="@series-2", stroke.color="@ink", or inside formulas {= mix(@surface,
  @accent, 0.2)}. Built in: surface ink ink-2 muted grid axis accent series-1 …
  series-8 (series in this order for categories; text in ink, ink-2 or muted, never a
  series colour). The first use adds the theme to the document: a palette root of
  token nodes (token.light, token.dark), which you may edit with <set> or extend:
  <create type="token" parent="#P" name="brand" light="#1d3557" dark="#a8dadc"/>.

EXPRESSIONS (for {= …} bindings)
  #id.facet.field references a leaf; + - * / % < <= > >= == != && || !;
  cond ? a : b; functions abs floor ceil round min(a,b) max(a,b) clamp(x,lo,hi)
  lerp(a,b,t) scale(x,d0,d1,r0,r1) sin cos tan atan2(y,x) sqrt pow(x,y) hypot(x,y);
  the constant pi; angles in radians; quantity literals 9.58 mW. A binding may not
  depend on itself. Inside an attribute write < as &lt; (or flip the comparison).
  Text: '…' strings (single quotes inside an attribute), + joins them; fmt(x) a
  number or quantity as text, fmt(x, d) with d decimals, fmt(q, 'nm', d) converted
  to that unit with it appended, sci(x, d) scientific, str(v) anything. So a text
  node shows a live value: content="{= 'λ = ' + fmt(#L.port.signal, 'nm', 0)}".

MODEL AND COMPOSITIONS
  Two views come first. <model-view> lists entities: what things are, each with its
  definition, its arguments, and a <figure> for every picture of it with the composition
  it is drawn in. The <document> tree follows: a composition root (notation, dims 2d
  or 3d) is one diagram, shown in its own panel; its children are its layout. A node
  with a figures relation pictures an entity; pictures in different compositions of
  one entity are the same thing drawn differently. Local geometry (brackets, posts,
  decoration) has no figures relation. The model root holds entities.
  An embed node shows one composition inside another by reference (embed.source):
  2D in 2D or 3D in 3D places and clips it like a group; a 3D composition in a 2D
  one is a camera view (embed_camera.camera names a camera node of the 3D
  composition; without it, a default view frames everything); a 2D composition in a
  3D one is a plane. frame gives a manual bounding box. Editing the embedded
  composition changes every embed of it. An embed may not loop back to itself.${index}${shown ? `\n\n${shown}` : ''}

If an <edit-failed> block comes back, nothing was applied. Its op="N" counts your
edit's verbs from 0 (ops="…" lists several with the same error). Fix exactly what it
names with a <revise> of that edit, which changes only the verbs you name and keeps
the rest as you wrote it; send a whole new <edit> only when most of it must change:
  <revise>
    <change op="$a" h="{= $b.size.h}"/>     set or add attributes on one verb
    <replace op="14"><create …/></replace>  swap one verb for any number
    <drop op="$c"/>
    <insert after="$a"><create …/></insert> (or before="…")
    <text old="0.5 mw" new="0.5 mW" all="true"/>   exact text, once unless all
  </revise>
op names a verb by the $alias it created or its number in the rejected edit as sent;
<text> runs last. <revise expect-version="N"> updates the version.
The person may edit while you work; your edit is merged with their changes. An error
kind="conflict" means they changed what that op changes, and says to what: <drop op>
keeps their change; an empty <revise/> resends your edit as is, overriding them, which
you do only when the request needs that op.`;
}
