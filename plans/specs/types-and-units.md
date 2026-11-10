# Types and units: a typed component boundary, units as first-class values

**Proposed 2026-11-08.** Strict at a component's boundary, loose inside formulas: a
component's inputs and outputs declare what they carry (a length shown in nm, a choice
of `hidden | shown`, a colour, a function of one argument), and that declaration drives
the inspector's widget, connection checks, pin visibility and the agent's errors.
Formulas inside a definition stay dynamically typed, as they are now. Builds on
entity-composition-model.md §6 (ports), component-model.md (inputs as ports),
wires-and-formulas.md §4a (collapsing pins, the drop menu) and plot.md §1 (series).

## 0. Why

Today an input's type is whatever its default happens to be. `std:plot`'s `vis` is a
`value` port whose default is the word `shown`; nothing says `hidden` is the only other
choice, so the inspector shows a text box and an agent can write `vis="hiden"` without
an error until nothing happens. Quantities are checked in arithmetic but not at
connections. A wavelength edits as `0.00000155 m` unless the user types a unit, and a
formula written as `400 nm` prints back as `0.00000040000000000000003 m`. Two loosenings
made the last phases work (`definition::check_kind`: a NaN default accepts any number
or quantity; text and enum are interchangeable). This spec replaces them with declared
types.

## 1. Decisions at a glance

| # | Decision |
|---|---|
| T1 | A port spec gains `port.datatype` (`datatype=` in markup: `type=` already names a node's type), one text leaf in a small grammar (§2). `port.kind` stays the **domain** (signal vs. pair flow, `optical_power`, `electrical`); the domain kinds imply their type (`optical_power` is `power`). |
| T2 | Types: `number`, a **quantity** by dimension or unit (`length`, `nm`, `W/m2`), `series(<dim>)`, `choice(a\|b\|c)`, `bool`, `color`, `text`, `fn(n)`, `any`; any of them `\| auto`. |
| T3 | A quantity type names a **display unit** (`quantity(nm)` or `length in nm`). The inspector shows and edits in it, accepts any compatible unit, and a context can override it (a lesson in µm). |
| T4 | **Literals keep their unit as written.** `400 nm` in a formula prints as `400 nm`; a constant quantity saves as `[si, canonical, written]`. Values at run time stay SI plus dimension; the hint lives on literals and on types, not on computed values. |
| T5 | **Checks at the boundary only:** setting an argument, connecting two ports, and an output's value each check against the declared type. Inside formulas nothing changes. |
| T6 | **Coercions are part of the type, not exceptions:** a `choice` accepts text that is one of its options; `\| auto` accepts `auto`; `number` accepts a dimensionless quantity. Everything else is an error that names the expected type. |
| T7 | **Undeclared ports infer a loose type** from their default at load (today's behaviour, unchanged); Make component and the agent's `<port>` declare. |
| T8 | **Pins:** `port.show` (`auto`, `pin`, `settings`) and `port.group`, with the author-override flow in §6. |
| T10 | **Gradual typing** (user, 2026-11-08): a person playing with ideas connects things before deciding what they are. An undeclared port is **loose** (`any`), never an obstacle: §3.5. |
| T11 | `auto` is stored as NaN (files already carry it, `{"num": "NaN"}`) and shown and typed as `auto`. |
| T9 | Later: an angle dimension (`deg`, `rad`) as an eighth base; affine units (°C, °F) at input and display only; structured function types (`fn(length -> power)`). |

## 2. The type grammar

```
type     := base ( "|" "auto" )?
base     := "number" | "bool" | "color" | "text" | "any"
          | quantity
          | "series" ( "(" dim ")" )?
          | "choice" "(" name ( "|" name )* ")"
          | "fn" "(" int ")"
quantity := dim ( " in " unit )?   // "length in nm"
          | unit                   // "nm": dimension and display unit at once
dim      := "length" | "time" | "mass" | "power" | "voltage" | "current" | "frequency"
          | "energy" | "temperature" | … | unit   // any unit names its dimension
```

Examples: `vis` is `choice(hidden|shown)`; `xs` is `series(any)`; `x_min` is
`like(xs) | auto` and `cursor` `like(xs) | auto`: a quantity whose dimension must be
the data's (`like(input)` names another input's dimension; council 2026-11-08 found a
plain `0` accepted for a range in metres, breaking the plot with errors nowhere near
the input); `f` is `fn(1)`; `color` is `color`; a laser's wavelength is `nm`. Bound
arguments are checked too, when their value is computed, not only constants.

**One easing type** (council 2026-11-08): `key.easing` (an enum without
`cubic-bezier`), `transition.ease` and `tween.ease` (free text) become one `easing`
type: a curve name or `cubic-bezier(x1, y1, x2, y2)`, checked when set, one default
(Easy Ease).

Named dimensions are the `Dim` table's names (`units.rs`), plus the coherent derived
units' quantities (`power` = W, `voltage` = V). `series(any)` takes a series of any
dimension; the series' own unit labels the axis, as today.

## 3. What the type does

### 3.1 The inspector

The widget follows the type, replacing `PropertyField`'s text box for ports:

| Type | Widget |
|---|---|
| `choice` with ≤ 4 options | a segmented control (Bits UI toggle group) |
| `choice` with more | a select (`src-ui/lib/ui/Select.svelte`) |
| `bool` | a switch |
| quantity | a number field in the display unit, scrubbable, with the unit as a suffix; typing `1.55 µm` stores 1550 nm's SI value and keeps showing nm |
| `number` | a number field |
| `color` | the colour picker, with the document's tokens (themes spec) |
| `series` | a one-line preview (`61 · nm, 400…700`), expanding to text |
| `fn(n)` | the formula field |
| `text` | a text field |
| `\| auto` | an "Auto" toggle beside the widget; off shows the value auto would pick, greyed, to start from |

A bound argument shows its formula, as every bound leaf does; the type still checks
the result.

### 3.2 Connections and the drop menu

A connection from an out port to an in port is allowed when the out type fits the in
type: equal dimensions, a choice whose options are a subset, `any` on either side,
`number` into `number | auto`. The link drag and the drop-on-component menu
(wires-and-formulas.md §4a) list only the inputs that fit; a refused connection says
why (`cursor wants a length; slider gives a time`).

### 3.3 Outputs and checks

An output port's declared type is checked when its binding evaluates; a mismatch is a
binding error on that port (the existing `reactive().errors`), not a crash. Setting an
argument through an op checks it (replacing `check_kind`'s loosenings) and refuses
with `LeafError::BadValue` naming the type, so the agent's edit fails at once.

### 3.4 Units

- **Literal hint.** `Expr::Lit` gains the unit text as written. Printing a binding
  prints the literal as written (`range(400 nm, 700 nm, 61)`). Constant quantities save
  as `{"q":[1.55e-6,"m","nm"]}`; the third element is optional (a quantity with no written unit omits it), and
  the value is exactly the SI number, so nothing round-trips lossily.
- **Display.** A value with no hint shows in its type's display unit, else in the
  engineering-prefixed canonical unit (`units::display_unit`, as now).
- **Conversion.** `in_unit(x, 'nm')`, `fmt(x, 'nm', 2)` and the inspector all go
  through `units::parse_unit`; a unit that does not match the dimension is an error
  naming both.
- **Contexts.** A context rule may set display units by dimension
  (`units.length = µm`) for a lesson or a notation (contexts.md); the type's own unit
  wins over the context's only when the author pinned it.

### 3.5 Gradual typing: connecting before types are decided

Typing must never get in the way of trying things. The rules, from loosest to
strictest:

| Connecting | What happens |
|---|---|
| loose → loose | Always allowed; nothing is checked (today's behaviour). |
| loose → typed | Allowed. The typed input checks each value that **arrives**; a misfit is an error on that input (a red pin, the message under its inspector field), the way a formula error shows, not a refused connection. Values are coerced where the type says how (text naming a choice, a number into `number`). |
| typed → loose | Allowed. The loose input takes whatever comes. |
| typed → typed, fits | Allowed (same dimension; a choice whose options are a subset; `any` either side). |
| typed → typed, misfits | Refused, with the reason ("cursor takes a length; time gives a duration"). |

And two ways to firm up later, both one click, both undoable:

- **Adopt the type.** When a loose port meets a typed one, the inspector's port row
  offers "Make `level` a length (mm)": the loose port takes the type it is connected
  to. Blender's sockets and TypeScript's inference work this way.
- **Infer from the value.** An undeclared port's Data type field offers the type its
  current value suggests ("number", "length in nm", "choice of 'shown'…"); choosing it
  declares it.

Make component and the agent's `<port>` may still declare types up front. Nothing is
required: a document with no declared types behaves exactly as today.

## 4. Markup

```xml
<port name="vis" dir="in" type="choice(hidden|shown)" default="shown"/>
<port name="wavelength" dir="in" type="nm" default="1550 nm"/>
<port name="x_min" dir="in" type="number | auto" default="auto"/>
<port name="f" dir="in" type="fn(1)"/>
```

The view prints `type` and omits defaults that equal the type's default (the
default-omission work). `std:plot` declares all its inputs.

## 5. Model changes

| Where | Change |
|---|---|
| `facet.rs` `Port` | `type` (Str leaf, `any` default), `show` (enum `auto`/`pin`/`settings`), `group` (Str) |
| `types.rs` (new) | `Type` parsed from the grammar; `fits(&Value)`, `coerce(Value) -> Result<Value>`, `accepts(&Type)` for connections; `display_unit()` |
| `definition.rs` | `check_kind` becomes `Type::coerce`; the NaN and enum/text exceptions go |
| `port.rs` | connect checks `accepts` |
| `expr.rs` | `Lit` keeps the written unit; `Display` prints it |
| `value.rs` | `q` takes an optional written unit |
| `standard.rs` | `std:plot` declares types; `auto` replaces NaN |
| `schema.ts`, the inspector | the widget table of §3.1 |
| `manifest.ts` | the `type=` attribute and its grammar |

Schema version bumps once for `port.datatype`/`show`/`group` and the `q` triple.

## 6. Pins: what shows, and who decides (T8)

- **Default `auto`:** a pin shows when the input is connected, bound, or set away from
  its default; otherwise it folds into the "+ N more" pill (wires-and-formulas.md §4a).
- **The author overrides per port**, on the main, in the inspector's Port section:
  "Show as pin: Auto / Always / In settings only", and a Group field (ports sharing a
  group fold together under its name, e.g. *Axes*, *Colours*).
- **On an instance**, the pill opens a list of hidden inputs, grouped; drag a wire from
  a row, or set a value inline. The instance's own visibility is not saved: what shows
  there follows `auto`.
- **When editing the definition**, each row in that list has a pin toggle: the
  shortcut that sets the author's choice without opening the Port section.
- `std:plot` declares `xs`, `ys`, `f` and `cursor` `pin`, the colours and labels
  `settings`, the rest `auto`, with groups *Data*, *Axes*, *Labels*, *Colours*,
  *Motion*.

## 7. Phases and exit criteria

| Phase | Builds | Exit |
|---|---|---|
| 1 | `Type` and its parser; `port.datatype`; coercion at set and at connect; loose inference for undeclared ports; `std:plot` declared; `auto`; `like(input)`; the `easing` type. **With phase 3's widgets in the same delivery** (council: a parser-only milestone leaves the workflow blocked) | `vis="hiden"` refused with "vis is one of hidden, shown"; `y_min="0"` on metre data refused at the input; connecting a time into a length refused; `vis` is a segmented toggle and `x_min` has an Auto toggle; all existing tests and the Library bake pass |
| 2 | Literal unit hints; the `q` triple; display units on types and in contexts | `400 nm` round-trips as written (formulas no longer print `0.015 m` for `15 mm`); a wavelength edits in nm and accepts `1.55 µm` |
| 3 | The remaining widgets (§3.1): series preview, colour tokens | — |
| 4 | Pins: `show`, `group`, the pill, the Port section controls (§6) | selecting the plot shows its four pins and "+ 15 more" grouped |
| later | Angles as a dimension; affine units; structured `fn` types | — |

## 8. Open questions

1. **Colour tokens as a type.** Does `color` accept a token (`@series-2`) everywhere, or
   is `token` its own type? Proposed: `color` accepts both; the themes spec decides the
   resolution.
2. **Choice outputs.** A machine's state as an output (`state` out port) would let one
   component drive another's state; its type is the machine's states as a choice.
   Worth adding with the types, or wait?
3. **Series length.** Should `series(nm, 61)` constrain length? Proposed: no, until a
   case needs it.

## 9. Done

**2026-11-08: phase 1 with its widgets, and phase 4 (pins).**

- `types.rs`: the grammar (`nm`, `length in nm`, `number`, `series(length)`,
  `choice(a|b)`, `bool`, `color`, `text`, `fn(n)`, `like(input)`, `any`, `| auto`),
  `Type::check` (with coercions: a choice from text, a bare number into a typed unit,
  `auto` as NaN), `Type::feeds` (connections), `Type::of_value` (inference).
  `like(x)` enforces a unit only when `x`'s data has one (a function plot's range sets
  its own beside the default sample).
- `port.datatype`, `port.show`, `port.group` (the field is `datatype`: `type=` already
  names a node's type in markup). A datatype must parse when set.
- Checks: a constant argument when set (`definition::validate_entity`; `like` reads
  the other input's constant, or evaluates its formula over the document's
  constants); a computed argument or an arriving port signal in the graph, as an error
  on that leaf naming the input; a connection between two declared ports
  (`port::validate_connection`). Loose ends always connect (§3.5).
- `std:plot` declares every input, its pin visibility and its group (Data, Look,
  Axes, Labels, Motion).
- Inspector: a choice of up to four options is a segmented control, more a select; an
  `| auto` input shows "Auto" when automatic and an Auto button to go back; a unit
  input takes bare numbers in its unit; inputs grouped under their headings. The Port
  section has Data type (with "Use …" suggestions from the value and from typed ports
  its instances connect to), Show as pin, and Group.
- Pins: an instance shows `pin` inputs, and `auto` ones that are connected, bound or
  changed; the rest fold into a "+N more" pill. A line being drawn shows every input
  of the components it may land on, filtered by type fit. The main shows all.
- Not yet: phase 2 (literals keep their unit), the easing type (moved to step 6 with
  timing values), the pill as a menu (wires-and-formulas phase 3's drop menu).
