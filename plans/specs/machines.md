# Machines: component states, transitions, and how they meet the timeline and the narrative

**Proposed 2026-11-08.** Waits on the user. The first slice of the state machine that
affordances.md §8.4 promised ("a later spec builds vision 06's state machine … one
animation system, not two"), sized for its first user: [`plot.md`](plot.md)'s appear
and disappear. Built on the recorded decisions it cites; where it goes past them it says
so (§9).

## 0. What is already decided (and where)

| Decision | Source |
|---|---|
| A component's state machine is part of its design: states, and transitions that each hold a **sub-timeline**, Rive-style | narrative note §6.7 (`notes/narrative-layer-and-storage-primitives.md`), replacing vision 05 §4's `<play>` |
| Timelines say *how* a change looks, machines say *when* it plays; signals may be machine inputs | vision 05 §5.1 |
| A transition has a duration but no start: its start is an event | vision 05 §1–2 |
| The current state is an enum value rules and bindings read; progress is a scalar on a track; states are discrete (no dragging between), transitions are scrubbable | affordances.md §8.4, vision 06 §3.2 |
| A pinned state while authoring is editor-local, not a document edit, and loud | vision 06 §3.3 |
| A narrative never reaches inside a component: its move **sets the state**, and the component's transition is the derived sub-move, overridden by exception | narrative note §6.7 |
| State changes in a narrative are **held overrides** with an extent (SMIL `<set>`, Unreal Keep/Restore State); playing forward runs the transition, scrubbing snaps; "if in A, go to B" is not used | narrative note §6.7 |
| Moves' **ends** must be seekable; anything not seekable is event-driven and belongs to the machine | narrative note §6.4 |
| Pedagogical components (hint, quiz) are fine if their states live in the narrative's compositions; a model component's state is design | narrative note §6.7 |

## 1. Decisions at a glance (proposed)

| # | Decision |
|---|---|
| M1 | A machine is a `machine` child of a **definition**: `state`s, `transition`s, and `channel`s, the numbers its states set. Template parts bind to channels, never to "the state" directly for continuous values. |
| M2 | The **current state is an in port** of enum kind, so it is an argument (`args.vis="shown"`): the agent, the inspector, a binding, a clip key and a narrative override all set it the one way arguments are set. Saved and undone like any argument. |
| M3 | A state is a **pose**: a value for every channel. A transition with nothing written **derives** its motion: every channel that differs eases from the old pose to the new one with the transition's duration and easing (the state diff, narrative note §6.2). |
| M4 | A transition may refine that with **tweens** (a channel, a start and a length within the transition, an easing), and for real keyframes, clips. That is the sub-timeline. |
| M5 | Every transition has a **`progress` input**, 0 → 1. The transition's tweens are written against progress, not seconds: the component owns **what** happens; whoever drives progress owns **when and how fast**. (Decided 2026-11-08.) |
| M6 | **Who drives progress is the only difference between cases.** A clip on a clock (the document timeline, a lesson's narrative clock) drives it: the transition scrubs, forwards and backwards, for free. Nothing drives it (a click, the inspector, the agent): the machine plays 0 → 1 itself over the default duration, on presentation time; scrubbing past it shows the end state. Channel values are a **session producer** either way: never saved, never undone; picking and bounds use them. |
| M7 | **One writer per leaf.** A channel belongs to its machine: clips key the *state*, not the channels. A leaf is bound, animated, or machine-driven, never two. |
| M8 | Interrupting a transition **retargets from where things are drawn** (CSS transitions' rule), so reversing halfway is smooth. |

## 2. Markup

As built (phase 1; the names a document uses today):

```xml
<definition name="Plot">
  <port_spec name="vis" kind="value" dir="in" signal="shown"/>   <!-- M2 -->
  <channel name="fade"   value="1"/>        <!-- channels belong to the definition -->
  <channel name="reveal" value="1"/>
  <channel name="lift"   value="0"/>
  <machine name="vis">                      <!-- named for the input it follows -->
    <state name="hidden" fade="0" reveal="0" lift="12"/>
    <state name="shown"  fade="1" reveal="1" lift="0"/>
    <transition name="appear" from="hidden" to="shown" ms="900" ease="ease_out">
      <tween channel="fade"   at="0"   ms="250"/>
      <tween channel="lift"   at="0"   ms="350" ease="ease_out"/>
      <tween channel="reveal" at="250" ms="650" ease="easy_ease"/>
    </transition>
    <transition name="disappear" from="shown" to="hidden" ms="300" ease="ease_in"/>
  </machine>
  <template notation="free">
    <group name="body" appearance.opacity="{= #fade.channel.value}" y="{= #lift.channel.value}">
      <path name="series 1" d="{= 'M ' + points(…)}" trim.end="{= #reveal.channel.value}"/>
    </group>
  </template>
</definition>
```

- `kind="state"` is a port kind whose values are the machine's state names (an enum);
  `signal` is the default state. The port and the machine share a name.
- `disappear` writes no tweens, so it is derived (M3): all three channels ease together
  over 300 ms.
- `from="*"` matches any state; a change with no matching transition **snaps** (and the
  editor lists such pairs, so a missing transition is visible, not silent).
- Channels are numbers in phase 1. Colours and vectors ease the way the interpolator
  will (affordances.md §8.2 `color`), later.

As types: `machine` (`name`, `initial`), children `channel` (`name`, `channel.value`:
the resting value used before any state applies), `state` (`name`, then one field per
channel: an open record like `args`), `transition` (`from`, `to`, `ms`, `ease`), whose
children are `tween`s (`channel`, `at`, `ms`, `ease`) and optionally a `timeline` of
clips for keyframed motion (overshoot, a pulse). A clip in a transition's timeline
animates a channel, with key times in seconds from the transition's start; key values
may be omitted on the first and last key to mean "the from pose" and "the to pose".

Expansion copies a definition's machine for each linked instance as it copies ports:
`#plot1/fade` is plot 1's fade channel. Bindings in the template read
`#fade.channel.value`, which expansion maps to the instance's channel, as it maps port
signals today.

## 3. Running a transition

When an instance's state argument changes from A to B (resolved, so a bound state
argument works too: `args.vis="{= #s.slider.value > 0 ? "shown" : "hidden"}"`):

1. Find the transition A → B (or `*` → B, A → `*`). None: the channels take B's pose
   at once.
2. Find its progress driver. A clip animating `#plot1/appear.progress` (stored or
   derived, §4.2) drives it from the clock. Otherwise the machine's own player starts
   it: progress rises linearly from 0 to 1 over the transition's `ms`, on presentation
   time.
3. Each frame, the machine producer publishes every channel's value from progress `p`:
   each tween is sampled at `(p · ms − at) / tween.ms`, clamped and eased; channels with
   no tween ease by the transition's own `ease` over the whole of `p` (M3). At `p = 1`
   channels rest at B's pose.
4. While any transition runs, the views showing that instance render every frame
   (free-running.md's mechanism), then return to on demand.
5. **Interrupted** (the state changes again mid-way): the new transition's from-pose is
   the channels' **current** values, not A's pose (M8).

Two easings stack and are kept apart: a **tween's** easing is the shape of one channel
inside the transition (the component's), and the **progress clip's** easing is a time
warp of the whole transition at one occurrence (the timeline's). Linear progress plays
the transition exactly as designed.

The producer uses the same easing functions as clips (`time.rs::ease`), and the
interpolator of affordances.md §8.2 becomes a client of the same code when it gains
channels beyond placement: **one animation system**, as decided.

## 4. The timeline

### 4.1 What drives progress

| Source of the state change | Progress driven by | Seekable |
|---|---|---|
| A **clip key** on the document's clock | the derived (or edited) progress clip after that key | yes |
| A **narrative move** (narrative-panel.md, not built) | a progress clip on the narrative clock, by the same rule | yes |
| An **event**: the inspector, the agent, a click, a binding on a free input | the machine's own player | no: scrubbing shows the end pose |

This resolves vision 05's "a transition has no start" without a special case: it has
none of its own, and it is seekable exactly when a clock drives its progress.

### 4.2 The progress band: derived by default, overridden by exception

A state port is an ordinary leaf, so a clip may key `args.vis`, with `hold` easing (an
enum steps; Unreal and Unity key enums the same way). The timeline draws that clip as
**blocks of named state**. After each key that changes state, it **derives** a progress
clip for the transition that change selects: `0` at the key's time, `1` at the key's
time plus the transition's `ms`, linear. It is drawn as a sloped band under the state
row:

```
time            0s        1s        2s        3s        4s
vis             [ hidden ·············][ shown ···················]
appear.progress                       ╱‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾
                                    2.0  2.9
```

Scrubbing to 2.45 s gives progress 0.5: half faded, partly drawn. The derived band is
computed, never saved. **Editing it saves it** as an ordinary clip on
`#plot1/appear.progress`, owned by that key (`key.progress` names it), and changes this
one occurrence only:

| Edit | Effect here |
|---|---|
| drag its end from 2.9 s to 4 s | appears slowly, over 2 s |
| easing `ease_in_out` | starts and ends gently |
| a middle key 0.5 at 3 s, held to 3.5 s | pauses half-drawn while narration explains |
| keys 0 → 1 → 0.6 → 1 | overshoots and settles |

"Reset to default" deletes the stored clip and the derived band returns. If the key
moves, an edited band moves with it (its keys are relative to the key). If the key's
state changes so that another transition is selected, the edited band is kept as stale
and listed, as overrides are.

### 4.3 What stays disallowed (M7)

- A clip animating a channel directly. The channel's writer is its machine; key the
  state, edit the progress band, or override a tween.
- A state port both bound and keyed (the existing rule: a leaf is bound or animated).
- Autokey (timeline.md §12) while a machine is mid-transition writes the *state*, never
  a channel, so moving the playhead never freezes a half-drawn curve into the document.

## 5. The narrative and teaching layer

Recommendations, following the recorded decisions in §0:

1. **The plot's `vis` machine is design state** (the plot makes sense without a lesson),
   so it lives in `std:plot`, and by default a lesson **sets** `vis` and inherits the
   animation. A lesson that needs more (a changed part, a new state or transition) makes
   a **lesson variant** of the component and uses that
   ([`variants-and-imports.md`](variants-and-imports.md)); structure never lives in the
   narrative's text.
2. **A narrative move that shows a plot is a held override** of `args.vis = shown` with an
   extent ("from this paragraph on"); leaving the extent backwards restores `hidden` and,
   when played, runs `disappear`. Jumping (clicking a heading far away) snaps.
3. **Per-lesson timing** ("everything at half speed in this section") is a motion-domain
   context rule held by the section's mode; it needs a `transition:` selector beside
   `state:` (narrative note §6.7), which must pass contexts.md §4.1's admission test.
   Proposed selector: `match="transition:appear"`, fields `motion.scale` (a duration
   multiplier) and `motion.ease`. Not in phase 1.
4. **One change, a different motion** is an edited progress band (§4.2) on the narrative
   clock: the same object as on the document timeline, owned by the move. Retiming,
   pausing mid-transition and overshoot are all edits of that band.
5. **Pedagogical components** (hint, quiz, "try it") use the same `machine` markup, but
   their definitions belong to the narrative's compositions and their state is narrative
   state (§0). Nothing in this spec forks for them.
6. **Learner exploration** (narrative note's scratch layer): if a learner flips a plot to
   `hidden` mid-lesson, the scratch layer holds it; on continue, the rejoin is that
   state's transition, played. No new mechanism.
7. **Steps or moves.** ECM §10 and vision 09 still say "a lesson is steps"; the narrative
   note says steps dissolve into moves. This spec needs only "something sets the state at a
   position", which either supplies; it recommends the moves reading, and that ECM §10
   be amended when the narrative clock is specified, not here.

## 6. Editing and inspecting

- **Inspector, instance selected:** the state port shows as a segmented control of the
  machine's states. Changing it is an event (§4.1): the transition plays in the view and
  the change is one undo step. **Undo snaps** (no reverse animation), as Figma and Keynote
  do; replaying a transition on undo reads as the undo doing something else.
- **Entering a component** (editing its main) switches the timeline panel to the
  component's machines. **One panel, two layouts**, switchable, chosen automatically:

```
Strip (each state has at most one way in and one way out: a chain or a loop):
 vis:  ( hidden ) ──appear──▶ ( shown ) ──disappear──▶ ( hidden )      [graph]
 │ fade     ▬▬▬▬                                                        │
 │ reveal       ▬▬▬▬▬▬▬▬▬▬                                              │

Graph (it branches): a resizable pane left of the tracks
 ┌───────────────────┬──────────────────────────────────────────────────┐
 │ (hidden)          │ appear · 900 ms                        [strip]   │
 │   ↓       ↘       │ fade     ▬▬▬▬                                    │
 │ (shown) → (compare)│ reveal   ▬▬▬▬▬▬▬▬▬▬                              │
 └───────────────────┴──────────────────────────────────────────────────┘
```

  Several machines stack one strip each. In either layout, **clicking a state** pins the
  canvas to it (session-only, with a loud "Previewing: hidden" chip; vision 06 §3.3) and
  the panel lists its pose, editable; a state has no duration. **Clicking a transition**
  gives the tracks that transition's duration and one row per tween; the playhead there
  scrubs its progress on every instance in the view, session-only. Leaving the component
  returns the panel to the document's clock. Editing a part while pinned offers "set this
  channel in *hidden*" when the edited leaf is bound to a channel.
- Panel layout and the switch are workspace state (the session tier), not the document.

## 7. For the agent

```xml
<edit><set id="#plot1" args.vis="hidden"/></edit>        <!-- plays disappear in the user's view -->
<edit>
  <create id="$k" type="clip" parent="#timeline" prop="args.vis" start="0"/>
  <create type="key" parent="$k" t="0" value="hidden" easing="hold"/>
  <create type="key" parent="$k" t="2" value="shown" easing="hold"/>
  <link from="$k" to="#plot1" rel="animates"/>
</edit>                                                   <!-- appears at 2 s, scrubbable -->
```

The view prints a machine compactly, one line per state and transition, and an
instance's state port as `vis="shown"` with `transition="appear 0.4"` while one runs, so
the agent can tell it caught an instance mid-way.

## 8. Phases and exit criteria

| Phase | Builds | Exit |
|---|---|---|
| 1. Machines on events | §2–§3: types, expansion of machines and channels, the `state` port kind, the session producer, derived and tweened transitions, interruption | a test definition fades and draws on when `vis` flips in the inspector; flipping back mid-way reverses smoothly; undo snaps; nothing about channels reaches the saved file |
| 2. On the clock | §4 and §6: hold keys on a state port, the derived progress band, edited bands, the strip and graph layouts | scrubbing across a key shows the transition mid-way, forwards and backwards; dragging the band's end retimes one occurrence; reset restores the default; autokey never writes a channel |
| 3. `std:plot` gains `vis` | plot.md §8 | the plot appears (fade, lift, then draw-on) and disappears; works the same from the inspector, a clip key and the agent |
| 4. Narrative | §5 items 2–4, once narrative moves exist | a paragraph shows the plot on reading forward and hides it reading back |

## 9. Open questions (including where this goes past the recorded plans)

1. **Channels** are new. The notes' sketches animate the component's own leaves from the
   sub-timeline. Channels add one indirection so that template leaves can stay bound (a
   curve's `path.d` is an expression; a sub-timeline cannot key it). Acceptable, or should
   tweens also target ordinary constant template leaves directly?
2. **Per entity or per picture?** M2 makes state an argument, so one plot pictured in two
   compositions appears in both at once. That suits a model component. A picture-level
   override (show it in the overview, not in the detail view) would be a `use`-level
   argument override; leave it until someone needs it?
   **User, 2026-11-08:** per entity is fine for now, but views will need their own
   machines: a phase modulator's diagram animating through states in one view while its
   schematic stays constant in another. The likely form is that `use`-level override of
   the state argument (or of the channels), so one entity has a state per picture.
   Revisit after step 8.
3. **Triggers inside the component** (`on="press"`, `on="hover"`, `on="mount"` from
   vision 05 §4 and vision 06 §2): phase 1 changes state only from outside. A click that
   toggles a component's own state is `affords.activate="cycle"` driving the state port
   (affordances.md §5.3), which may already be enough; `on="mount"` (appear when first
   shown) waits on what "shown" means in a narrative or presentation.
4. **Machines without components.** Can a plain group in a composition have a machine,
   or only definitions? Proposed: definitions only for now; make-component is one step.
5. **Seekable events.** Should an agent's edit that sets a state be recorded with a
   time so a later scrub replays it? Proposed: no; that is what keying the state is for.
6. **Progress ids.** `#plot1/appear.progress` names a transition's progress on an
   instance. Is a transition a node with a `progress` leaf (simplest for clips), or is
   progress a port on the entity (`#plot1.appear`)? Proposed: a leaf on the expanded
   transition node, so clips animate it like any leaf.

7. **Tracks as timing values (direction agreed 2026-11-08).** Tweens and
   `ease(window(progress))` bindings are two spellings of one thing and should
   coalesce. Not by pattern-matching formulas in the timeline: a **timing** becomes a
   value kind, as a colour or path data is, built by `ease(curve)`, `window(a, b, t)`,
   `then(…)`, `keys(…)` and `custom(p => …)`. A transition holds one track per channel
   whose value is a timing; the player computes `from + (to − from) · timing(progress)`.
   The timeline edits a **constant** timing's structure directly (segments, handles);
   a **bound** timing (its window depends on an input) shows its formula, like every
   bound leaf in the inspector. The same rule as everywhere: constant typed values get
   an editor, bindings get a formula. Lambdas and timings are one-way, as tweens were:
   the player writes channels, templates read them, nothing flows back, so no inverse
   is needed. Built with step 8.

8. **Authoring (council 2026-11-08, both members).** §6 covers editing but not
   *creating* a machine; today only markup can. Add to step 8: on a main, **Add state**
   records the current pose (Figma variants, Keynote Magic Move); edits made while a
   state is pinned make a channel for each changed leaf and bind it; transitions are
   derived and their band edited in the strip panel; a loud chip says which state is
   being previewed. Exit: a person makes a two-state draw-on by hand, scrubs it
   backwards, retimes one occurrence, and undoes.
9. **Tweens in progress fractions, not ms** (council): M5 writes tweens against
   `progress`, the code writes `at`/`ms`. With timing values (Q7) a tween is a window of
   progress, so step 8's band is the same object as a clip.
10. **Channels as out ports?** (council, Opus) A channel is a fourth kind of component
    variable beside in, out and port. As an out port driven by the machine, pins, wires
    and the inspector would show it with nothing new. Decide in step 8.
11. **The plot's `reveal`** duplicates `vis` (council, Opus): two ways to draw the
    series on. Proposed: drop `reveal` once the timeline keys state (step 8).

12. **Customising timing from inside and outside (agreed 2026-11-08, step 6).** Four
    layers, like arguments, overrides and clips: (a) the author's default, in the
    definition; (b) timing the author exposes as an input (`ms="{= $self.appear_time}"`:
    the player evaluates transition and tween timing per instance instead of reading
    constants); (c) a per-instance override of any transition, without the author
    exposing it (the inspector lists an instance's transitions with Override and Reset);
    (d) a per-occurrence retime on the timeline (§4.2's band). Poses (each state's
    channel values) take (b) and (c) the same way. None of (b)–(d) is built yet.

## 10. Done

**2026-11-08: phase 1 (machines on events) and phase 3's plot.**

- Node types (registry.rs): `channel` (a definition child, `channel.value` its rest
  value, copied onto each instance like a port: expand.rs `is_body`), `machine` (named
  for the `in` port whose value is the state), `state` (its pose as `args`, one per
  channel), `transition` (`from`, `to` with `*`, `ms`, `ease`) and `tween` (`channel`,
  `at`, `ms`, `ease`). §2's `<machine>`/`<channel>` markup is these nodes; the state
  port is an ordinary `value` input whose default is the initial state.
- The player (`machine.rs`, `Store::advance_machines(now)`, a PreUpdate system beside
  the motion player that keeps picturing compositions rendering while it runs): a
  changed state argument starts the transition (exact match, then `*`), progress runs
  0 → 1 over `ms` on the host clock, and each channel is `from + (to − from) ·
  ease(curve, window(progress, at/ms, (at+ms)/ms))` (wires-and-formulas.md §5). An
  interruption starts from the current values (M8); a change with no transition, an
  instance first seen, and undo or redo snap. Values publish as
  `ProducerId::MACHINES` only where they differ from rest, so resting instances stay
  static to the renderer and nothing is saved.
- An enum input takes text (`args.vis="hidden"` from an edit; `definition::check_kind`).
- `std:plot` gains `vis` (`shown`/`hidden`) and channels `frame`, `axes`, `ticks`,
  `series`: appear (1.4 s) fades the frame and titles in, draws the axes, then ticks,
  grid and labels, then the curve, with Easy Ease; disappear (0.45 s) reverses it. The
  series' `trim.end` is `min(reveal, series)`.
- `tests/machines.rs` (transition, tween slice, interruption, undo snap, nothing
  saved, no transition snaps) and `tests/plot.rs` (the plot's order). Seen in the app
  through `<edit>`.
- Not yet: the `progress` leaf a clip can drive (phase 2, with keys on the timeline,
  the derived band and the strip/graph panel), the inspector's segmented state
  control, a pinned preview state on a main, a machine on a main's own picture (mains
  show channels' rest values). Tweens stay their own nodes; a tween is the binding
  `ease(window(progress))` in meaning, and phase 2's progress leaf lets either form be
  written. In the area style the fill fades in whole while the line draws; clipping
  the fill to the drawn length would read better.


**2026-11-08: step 6, machines on the timeline and made by hand.**

- **Per instance (Q12 b, c).** Each instance gets copies of its component's machines
  (expand.rs, after its ports), so a transition's `ms` may read an input (each
  instance its own length) and an instance may override any transition, tween or pose
  through the existing override mechanism. The player reads the resolved copies.
- **Timing (Q7, Q9).** Tweens are fractions of their transition (`start`, `end`), so
  retiming keeps proportions, and `shape` takes any function of progress
  (`{= p => …}`). One easing default, Easy Ease. Not built as a separate value kind:
  the tween's fields are the timing's structure, and `shape` is its formula form.
- **On the clock (§4, phase 2).** A clip keying a state (hold keys) drives the
  transitions it selects from the clock, from the previous state's pose, so any
  playhead time shows the same picture, forwards or backwards. `key.span` retimes one
  occurrence (auto: the transition's length). The timeline draws a progress band from
  each such key. The inspector keys a choice input with ◇, and a change to an
  animated input keys the playhead (After Effects).
- **By hand (Q8).** On a main, **+ State** makes the machine (a `state` input typed as
  the choice of its states, `default` and `state_2`, one any-to-any transition of
  300 ms Easy Ease) and pins the new state; clicking a state pins it (session only,
  the main shows its pose, a banner says so, a field renames it); number edits made
  while pinned become its pose (`machine::record`: a new channel resting at the old
  value, the part bound to it, the state's value; one undo step). Transitions show
  their length and an **easing field** (curve previews, a cubic-bezier editor).
- Exit met in the app: a line made a component; + State; Trim End 0 recorded into
  `hidden`; an instance's state keyed hidden at 0 s and default at 1 s by hand;
  scrubbing 0.9 / 1.15 / 1.6 s shows nothing, half and all; a 2 s span retimes that
  occurrence; undo restores it. `tests/machines.rs`, `tests/machine_authoring.rs`.
- Not yet (the panel and chip were built 2026-11-09, below): dragging the band's end
  (the drag's handle is ambiguous between the key's time and its span; set Span in the
  key's inspector instead); colour channels
  (channels are numbers); double-click does not enter a main whose only part is an
  unfilled path (Enter does); per-view machines (Q2); per-lesson timing (§5.3).

**2026-11-09: the machine panel, state lanes, the canvas chip (§6).**

- **State lanes.** Every instance of a component with states has a row on the
  document's timeline, keyed or not: its state port's lane shows a named stretch
  per state (from its key to the next; an unkeyed one holds all along, fainter),
  coloured by the state's place in the machine, under the progress bands. The DOM
  column says what the lane is in at the playhead (`hidden`, or `appear 40%` mid-
  transition; `timeline_rows` `lane`, `state`); an unkeyed lane's ◇ starts keying it.
- **The machine panel** (`MachinePanel.svelte`, `machine_view`). The timeline panel
  has two layouts: the clock, and a component's states. Selecting a main or one of
  its parts switches to its states, leaving switches back, and a selected instance
  offers them (a **States** button) without switching. **Strip** (chains and loops,
  `machine::is_strip`): the states in a row, each gap holding both directions'
  transition (name · length); **graph** (it branches): a diagram in a pane left of
  the tracks, an edge per explicit transition, wildcard ones listed. Auto, Strip or
  Graph, a session choice. Clicking a state on a main pins it; on an instance, sets
  its state (the instance's current state is marked).
- **A transition's tracks.** Clicking a transition, or choosing From and To, shows
  that pair's transition: length and easing (shared ones say so; **Own timing**
  gives the pair its own, copying length, easing and tweens:
  `machine::add_transition_ops`), a ruler in ms, and a row per channel: its tween's
  slice as a bar to drag (body moves, ends resize; one undo step on release) with
  its curve drawn in, an easing field, × to follow the transition again; or **+** to
  give the channel its own slice (`add_tween_ops`). The playhead scrubs the
  transition's progress on the main (`Pinned::scrub`, session only), ▶ plays it; an
  instance mid-transition shows where it is.
- **The canvas chip.** While a state is pinned or a transition scrubbed, every 2D
  view shows "Previewing *hidden* · component" (or "*a → b* at 40%") with a stop
  button.
- `tests/machine_authoring.rs` (scrub, own timing, a slice, an unkeyed lane).
- **Council fixes, the same day** (Opus and Sol reviewed the panel in the app):
  - **Scrubbing records nothing.** A scrubbed transition is a moment between two
    states, so edits then are plain edits (`machine::record` passes them through);
    the panel's banner says so and the inspector shows no state as previewed.
  - **Stopping a preview stops playback** (the chip, the banner's Stop previewing,
    clicking a state or another pair); ▶ turns into ■ while it plays.
  - **Leaving a main unpins it**, so later edits there are not recorded into a state
    you can no longer see; a pin whose state is gone (deleted, a rename undone) lets
    go; a rename moves the pin and a scrub's ends, within its machine.
  - **The panel follows the component**: entering a main shows its states; selecting
    another component or nothing shows the clock (it stuck before).
  - **Live status for events too**: the player keeps what it ran last
    (`Store::running_machines`, `Running.from`/`to`), so a transition started by an
    event shows in the lane (`hidden → default 50%`) and the panel, which follows an
    instance's running pair.
  - **Shared timing says so**: tracks on a shared transition read "Shared · any
    change: edits change every pair using it" with **Customize this pair** (was Own
    timing); a pair's own reads "This pair" with **Use shared** to remove it. Shared
    gaps in the strip are italic; the graph draws shared pairs dashed and selectable.
  - **The strip follows the chain** (`machine::strip_order`) and closes a loop.
  - **Rename and Edit main in the panel**: the preview banner holds the state's name
    to rename; an instance's view has Edit main. Graph states and edges take the
    keyboard; the graph draws at its real size; a lane shows the state held before its
    first key; a formula length is read-only; the States button sits off the ruler.
  - `tests/machine_authoring.rs` (`scrubbing_records_nothing_and_events_report_their_states`).
- Not yet: the inspector's States section and the panel overlap (both pin, add and
  rename); the council suggested one home, the panel. Per-instance overrides edited from the panel (instances are read-only
  there; edit the main); the panel's scrub on every instance in the view (it shows on
  the main); adding or removing explicit graph edges by dragging.

**2026-11-09: the state editor as a drill-in (two council rounds, design).**

The rule (the user's): what you need most of the time lives in panels that stay in
view; what belongs to one flow (configuring a component's behaviour) lives in a mode you
enter and leave on purpose. Both rounds of the council chose the timeline drill-in over a
panel on the right and over one on the left: the editor must leave Layers, the canvas,
the inspector and the Model chat in view while a state is shaped, and timing is
horizontal (rows against milliseconds).

- **Always in view:** each instance's state over time (lanes; the row's readout), an
  instance's state as an input value with ◇ (inspector), what the canvas is showing
  (the chip). Retiming one occurrence is ordinary timeline work: a state key's
  inspector (Span), never the editor.
- **The editor** (`StateEditor.svelte`) is the timeline panel drilled into one
  component, at its own size; it never resizes the dock. Entered from the inspector
  (**Edit states…**, or **+ Add state** on a main with none), a double-click on a
  lane's stretch (that state) or a transition band (that pair), a lane's readout, or a
  state key's inspector (**Edit transition a → b…**, **Edit state b…**). Left with
  **Done** or the breadcrumb (the preview ends, the clock is back). Selection never
  switches it; selecting another component offers **Edit its states**.
- **One header row:** `‹ Timeline › component`, the states as pills (click: edit its
  look; double-click or ⋯: rename; ⋯: make default, delete; a house marks the default)
  with each pair's transitions as two small arrows between them (dashed: shared, faint:
  none), + (add a state), Graph (a pane at the left, automatic when the machine
  branches), Done.
- **A state shows the pose table:** a row per channel, a column per state, the edited
  state's column editable; edits on the canvas record into it ("Editing *hidden*",
  orange, in the pill, the table, the chip and a strip at the top of the inspector:
  "Number edits go into *hidden*; other edits (colour, text) change every state").
  Selecting outside the main pauses the recording ("Recording into *hidden* paused ·
  Return to main"); it never resumes silently.
- **A transition shows the timeline's grammar:** in the corner the pair, ▶, length and
  easing; above the ruler its scope (Shared · any change, with **Customize this pair**;
  or This pair's own, with **Use shared**); a row per channel with its from → to
  values, its slice as a bar to drag, and a popover (⋯) with exact start and end in
  ms, its easing, and Follow the transition. Scrubbing or playing previews only
  (blue chip, "nothing records").
- **Double-click opens** (contexts.md): `affords.enter="open"` makes a double-click
  send the shell an `Open` event naming the node and what it projects; the timeline's
  tracks and keys declare it, and a state lane's stretches now stand for their keys
  (a click selects that occurrence, a drag moves it).
- Engine: `state_key` (a key's component, machine, state and the one before),
  `machine_view` by definition with each state's pose, `delete_state_ops`,
  `set_default_state_ops`. `tests/machine_authoring.rs`
  (`a_state_is_made_default_and_deleted`). In the app: + Add state, shape (Trim End 0
  recorded), rename, pause and return, a pair's tracks, Done; a band's double-click
  opens its pair and a stretch's its state.
- Not yet: several machines on one component show one at a time with no chooser yet;
  "Duplicate state"; an instance's own timing from the editor.

**2026-11-09: the layout council's editor changes.** A third council round reviewed the
whole workspace and the editor's use of space. In the editor:

- **The graph is the state map when it shows:** a resizable pane (300 px, drawing
  scaled to it), each direction of a pair its own curve with its own hit area and its
  length as a label, the selected pair in the accent colour, no focus box; the
  header then drops its neighbour-only arrows (the pills stay, for menus and renaming).
- **A strip's loop is labelled** (`↺ idle`).
- **A state's view uses the width:** the compact pose table (channel column 200 px,
  the rest value greyed where a state does not set it) and, at the right, the
  transitions out of that state (`→ hover · 300 ms · own|shared`), each opening its
  tracks. "Editing" is said by the pill, the table column, the chip and the inspector
  strip only. Empty states span the panel and say what to do (or offer Return to main).
- **A transition's controls are one toolbar row** (pair, ▶, length, easing, then its
  scope: Shared timing with Customize this pair, or This pair with Use shared), the
  ruler and the rows below, the channel column 200 px.
- **Several machines** get a chooser in the header.
- **A focus on a state or pair that is gone** (deleted, renamed) falls back.
- Not done: the dock's default sizes (a taller timeline, under the canvas only) wait,
  because the smoke scripts drag at absolute window coordinates and all move with it.

### Parents over children (2026-11-10)

Asked for with the reactivity work (reactivity.md §13): a plot with a legend, labels
and two lines, each able to appear alone, must not become one machine with every
combination as a state. Statecharts' answer is parallel regions; here that is one
machine per part, and two small additions.

- **Each part keeps its own machine; the parent forwards an input.** Already possible:
  an inner instance's state argument is a binding on the parent's ports
  (`{= $legend ? $vis : 'hidden'}`), so a part is shown only when the parent and its
  own input both say so, and toggling the input plays that part's transition alone.
- **`machine.at`** (the `machine` facet, optional on a machine): at 0 or more it is a
  position along the machine's states in order, and the machine shows that instead of
  following its state input: between states `i` and `i + 1`, their transition at the
  fraction (the downward one when the position is falling). A pure function of the
  number, so it scrubs. A definition opts in with a number input bound to it.
- **`machine.running`** is published true while one of a machine's transitions is
  under way (its own player's, or a clocked one).
- **Sequencing:** the parent has a channel per part, tweened over that part's slice
  of the parent's transition, and gives the part
  `vis_at="{= #M.machine.running ? #p.channel.value : -1}"`. The parent composes
  when; the part owns what its appearing looks like (M5). **Handover:** when the
  parent rests the position goes below 0, and the part resumes from where it is drawn
  in the state nearest its position, so nothing replays (M8).
- `Store::advance_machines` goes round until nothing more moves, so a driven child is
  never a frame behind its parent.
- Test: `tests/machines.rs` `a_parents_transition_sequences_its_childrens_own` (two
  cards nested in a pair: staggered appear, hand back at rest, one card hidden alone).
- **Not yet:** a state row cannot name a child's state directly (it sets a channel a
  binding reads); `std:plot` is still one machine, not split into legend, labels and
  series parts; the machine panel shows none of this.

### Fixes from a three-state walkthrough (2026-11-11)

Found building a water-phases component by hand (ice, water, steam, with all six
legs given their own timing and slices):

- **Recording through a bound part.** With a state pinned, an edit to a part already
  bound to a channel inverts the binding and lands on the channel's `channel.value`,
  its rest value, which moved every state that does not set the channel; only the
  first state recorded came out right. `machine::record` now takes that edit into
  the pinned state's pose. Test:
  `tests/machine_authoring.rs`
  `an_edit_to_a_part_bound_to_a_channel_records_into_the_pinned_state`.
- **Opacity inside instances.** `record_instance` drew parts that render themselves
  (`whole` false) without their `appearance.opacity`; it now wraps them as `branches`
  does. Checked by screenshot only.
- **Not yet, from the same walkthrough:** transitions cannot be named in the UI;
  tween markup prints `shape="NaN"` with no shape; + Add state drops the selection;
  graph edge labels overlap; an edit equal to the value already shown records
  nothing.

### The state editor, redesigned with a council (2026-11-13)

A council review (Claude Opus 5.5, GPT-6.1 Sol) of the walkthrough above graded the
authoring flows 5.5–6/10. This round answers its findings.

- **One timeline at two levels.** The transition's tracks are no longer DOM: while
  the editor previews a transition (the pin carries a scrub), the engine's
  generated timeline composition is that transition's (`timeline_view::inside_plan`,
  `inside_wants`). Its rows are the component's parts and, under each, the
  properties that read a channel (`machine::readers`, `property_label`). Its ruler
  is the transition's own time, and its playhead is `axis.cursor`, which drives the
  preview each frame (`sync_themed`). A property's slice is a bar between two key
  diamonds:
  - the bar projects its tween with `with: tween.end`, so a drag moves start and end
    together (`handle::rigid`, `Store::preview_target`); pushed against an end, it
    shortens there;
  - a diamond trims its end, held between 0 and the other end (`handle::bounds`).

  Marquee, multi-select and dragging several bars are the canvas's own. A pair on
  the default timing draws its bars but does not move them. The editor's header,
  navigator and a state's look are DOM around it.
- **Values other than numbers.** `channel.value` is `Any`: a state holds numbers,
  colours (blended through a transition) and a text's words (switched half way
  through their slice, so the slice places the switch) (`machine::mix`,
  `recordable`). A part's leaf bound straight to a channel edits it whatever its
  kind (`Handle::Alias`), and a channel the player publishes while a state is
  pinned is still editable (`handle`: MACHINES-driven leaves are free).
- **Timing is copy on write.** `edit_pair` gives a pair its own transition when it
  used the default, with a slice for every channel, and applies the edit in the
  same transaction. A slice with no easing of its own follows the transition's
  (`tween.ease` empty). `copy_timing` copies another transition's timing, mirrored
  for a reverse leg (`machine::copied_timing`, `mirror_ease`). `copy_pose` and
  `reset_value` are the look table's menu and ↺.
- **Previewing refuses part edits** (`record` errs): a moment between two states
  belongs to neither.
- **Named on the way in.** `add_state` takes `first` and `name`. The first Add state
  opens the editor's form ("Its look now", "A new state").
- **The editor** (`StateEditor.svelte`):
  - a navigator tree: each state, its outgoing transitions under it (named, length,
    or *default*), and a Default timing row; or the same as a graph;
  - for a transition, a header with the transport, length, easing and a timing menu;
  - a state's look as a table by part and property;
  - selected slices in the Inspector (`SliceSection.svelte`): start, end, easing,
    and Stagger;
  - the document timeline's view is restored on Done.

  The instance Inspector shows one State control. Document-timeline state lanes name
  the transition into each keyed state ("→ water · melting 1.6s").
- **Round 3** (after the council's second grades, Opus 7, GPT Sol 7.8):
  - parts are folded by default: one row and one bar per part, and dragging the bar
    moves all its slices (`timeline_view`, part bars); selected bars are emphasised
    and each bar sketches its easing curve;
  - easings `overshoot` and `spring` (`time.rs`);
  - the navigator unfolds only the focused state's transitions and pins Default
    timing; the graph is a large overlay with every edge labelled;
  - the header names its mode ("State look", "Transition animation"), and Back names
    where it returns;
  - the look table has a Drawing column, folding parts, a "selected parts only"
    filter, and explicit inherited cells (–); an Inspector field that a state
    cannot hold is marked "all states" while a state is edited;
  - the Slice section follows the transition's easing on request, and Stagger keeps
    lengths (offering to lengthen the transition) or fits them in it;
  - the timeline panel grows while the editor is open and is restored on Done, as
    is the selection;
  - the instance Inspector shows the State control with "ice → water · 56%" and
    where the state is keyed; the document's length includes transition tails.

  Not done: onion skin of another state, and keys in the middle of a slice.
- Tests:
  - `tests/machine_authoring.rs`
    `states_hold_colours_and_words_and_pairs_take_their_own_timing_on_edit`;
  - `tests/timeline_view.rs`
    `inside_a_transition_the_timeline_shows_and_moves_its_slices`;
  - the walkthrough's driver script, by screenshot (`screenshots/tri4/`).

### The editor as a place (2026-11-13)

A person walking the flow by hand could not tell whether states existed, which one
was being edited, or how to reach a transition; the two-step form, the navigator
tree and the sentences explaining each screen were the problem. This replaces the
navigator and the first-state form of the redesign above. The rule for this
editor: a screen that needs a sentence to explain it is wrong; controls carry at
most a name and a tooltip.

- **Add state acts at once.** On a component with no states it makes two
  (`state_1`, the look now, and `state_2`), opens the editor and edits `state_2`.
  There is no form and no Done (`machines.svelte.ts` `open`).
- **The states bar is on the canvas** (`StatesBar.svelte`): a chip per state, the
  edited one in the record colour, each with the number of properties it sets; a
  click edits that state, a double-click renames, the edited chip's menu has
  Rename, Make initial, Copy the look of, Delete; "+" adds a state at once; Close
  leaves. While a transition is previewed its two ends are marked. The bar also
  shows when a main with states is selected, as the way in.
- **Inside means inside.** While a state is pinned, its main is open: a press
  selects the part under it (`contexts::Resolver.editing`, `interaction::target`).
  Opening the editor with the main selected clears the selection, so a drag moves
  a part and not the component. Selecting something else on the canvas leaves the
  editor (it used to pause recording and offer "Return to main").
- **The panel is the machine.** It opens on the graph: a node per state, an arrow
  per transition labelled with its length (dashed on the default timing, solid
  with its own), and the default timing's length and easing beside it. A click on
  a node edits that state; on an arrow, opens that transition's tracks, full
  width (`INSIDE_INSET` is 0), with its name, transport, length, easing and timing
  in the header. The table of what each state sets is the panel's second page;
  values a state does not set show the drawing's, greyed.
- **An edit a state cannot hold says so when it happens.** A part made, deleted or
  re-parented, or a value that is not a number, colour or text's words (a path's
  points, a font), changes every state: the store counts these while a state is
  pinned (`machine::shared_edit`, `Store::shared_edits`), `machine_view` reports
  the count, and the bar shows "Changed in every state" for a few seconds.
- **Make component** is a labelled button in the Inspector's title for plain
  shapes.
- Removed: the first-state form, the navigator tree, the Default timing page, the
  mode chip, the "every instance" note, the recording banners in the Inspector and
  the panel, the canvas's "Editing … edits go into this state" pill, and the
  toast about own timing.

After the council's fourth round (7.6 and 7.5), the same day:

- **A state has one colour**, by its place among the machine's states
  (`timeline_view.rs` `LANE`, `machines.svelte.ts` `STATE_HUES`: change them
  together): its swatch on the bar's chip, the graph's node and the table's
  column, and its stretch on the document's timeline.
- **A state lane has two pieces per change**: the transition (the new state's
  hue, pale, rising to full over the transition's length; named for the
  transition, "melting · 1.6 s") and then the hold (the hue, named for the
  state). A key that comes before the transition ends cuts it short. Stretches
  are opaque, so a label never runs into the next piece. A lane has no grey
  summary bar on its object row.
- **A selected slice is outlined; the others stay as they are** (they used to
  fade).
- **Opening a transition clears the selection**, so the Inspector shows the
  transition. With a part of the edited main selected, the Inspector says which
  state is edited ("Editing water"). The main's Inspector has **Create
  instance**; it does not list the machine's own port.
- **An instance is numbered** after its main ("card 2", `copy.rs`), and placed
  beside it. Its State control always shows its key diamond.
- The transition's header is a breadcrumb (Timeline › component › a → b), each
  step a link; the timing menu has a chevron. Graph labels lie along their own
  arrows. Times read in seconds everywhere. **Stagger** is one button whose label
  is the outcome ("Stagger", "Stagger and lengthen to 2.4 s", "Staggered").

After the fifth round's first review (8.1), the same day:

- **The parts whose slices are selected are outlined on the canvas**
  (`SelectionState.marked`, set by `narrative_probe` `mark` from the timeline's
  column), so a row can be told from the picture. A part's folded bar has a grip
  at each end (the ends trim).
- **An instance's States heading has a labelled key button**: "Key state", then
  "Add key at 3.5 s" or "Key at 3.5 s" once the state is animated.
- A slice's easing is locked while it follows the transition's. Stagger's order
  is by row ("First row first"). The initial state carries a flag on its chip and
  its graph node. A transition's label on the graph is drawn as a button.
- A lane's label that does not fit its piece is left out, and a line marks where
  a transition ends.

After the fifth round's second review (7.9):

- **The states bar shows only while editing**; its presence is what says so. The
  way in is the Inspector's Add state or Edit states….
- **The edited state is marked in its own colour** (chip, node, table column),
  not the record orange.
- While a component's states are edited, the Inspector for the whole main is the
  state's (never Create instance, Input or Resizable).
- A transition's length, easing and timing live in the panel's header only; the
  Inspector has its name.
- **An instance's Inspector** is "Instance of card" with one State row (key
  button, Edit states…, the choice); its progress line names the transition
  ("melting · 56%"). Its canvas tag is its own name. A state input is not counted
  among an instance's folded inputs ("+1 more").
- **Stagger reads the bars**: parts already one after another show that order and
  step, and "Staggered" instead of an offer to stagger again.
- Graph labels are level, beside their own arrow. A state lane's stretch fills
  its row. The timeline's options button sits at the bottom, clear of the rows.
  The Inspector's "Alt + drag" footer is gone.
- Known gaps: converting a shape whose corners are bound shows a dialog naming
  channel ids; the Inspector is empty on entry until a part is clicked; a
  component's `state` input still draws as a port on the main.
- Tests: `tests/authoring.rs`
  `a_pinned_state_opens_its_main_and_counts_edits_it_cannot_hold`; the flow by
  hand through the driver, by screenshot (`screenshots/tri5/`).
