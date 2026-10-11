# Time, machines, and scenarios

**Status:** Note — an idea waiting for the system to be ready, not a plan. Nothing here
should be built until a plan adopts it.
**Date:** 2026-10-10
**Context:** A design conversation after the roadmap was written. It generalises vision
doc 05 (three kinds of time) and revises vision doc 09 §9 (the lesson layer). It needs
no change to roadmap stages A–C: everything it depends on is already planned (ports,
one owner per value, a runtime layer for simulation state).

---

## 1. Everything is a machine with ports

Using vision 09's ports, any behaviour is a box with input ports, output ports, and
maybe internal state. What it *is* depends on two questions: are its inputs all driven
by the clock, and does it remember anything?

| | **No memory** (output depends only on current inputs) | **Has memory** (output depends on history) |
|---|---|---|
| **Inputs free** | A reactive projection: a binding (doc 05 "reactive") | A state machine or open dynamical system: waits for inputs |
| **Inputs all driven by the clock** | A scripted timeline: a pure function of t, so you can jump to any time | A simulation: must run forward from the start |

- A timeline is a machine whose inputs are all wired to one master clock.
- A machine with free inputs that are not driven by the clock is a state machine (if it
  has memory) or a live projection (if it doesn't).
- **Scrubbing** is the practical consequence. A memoryless timeline can jump to t = 7 s
  directly. A clock-driven machine *with* memory cannot: reaching t = 7 s means running
  from 0, so scrubbing needs recorded snapshots. This is the other face of
  `constraint-system.md` open question U6 (recording simulation runs into clips).

This is the "open dynamical systems" idea from applied category theory (David Jaz
Myers' categorical systems theory; the AlgebraicDynamics library): boxes with ports
and state, composed by wiring; wiring every input to a clock "closes" the system into a
trajectory.

## 2. Drivers

Whatever attaches to a free input port is a **driver**. The machine never changes; only
its drivers do:

- a **schedule** (a scenario track: "RF drive on at 2 s", "pulse fires at 3 s");
- a **human hand** (a slider, a click, a drag);
- a **simulation clock** (free-running);
- an **external Model** or **live instrument data**.

Doc 05's three kinds of time become kinds of driver rather than three mechanisms:
scripted means schedule-driven; reactive means driven by other values; transitional
means event-driven, through a state machine.

## 3. Lessons are scenario plus observation

Vision 09 §9 treats a lesson as "what to show". The better model splits it in two:

1. **Scenario (the driver half):** schedules attached to the model's input ports over
   lesson time.
2. **Observation (the observer half):** which compositions are open, camera framing,
   highlights, annotations, and the **time map** (below).

Consequences:

- A looping animation is a looping scenario with a fixed observation.
- A guided lesson is a scenario with observation that changes by step.
- Free exploration is no scenario: the viewer is the driver.
- The model describes the machine, never the lesson. Schedules live in the lesson
  layer and connect to model input ports, so one modulator model serves many lessons
  and stays explorable on its own (vision 09's owner/target rule, extended to time).

**The time map.** Optics happens in picoseconds; a lesson happens in seconds. The
observation carries a 1D mapping from lesson time to physical time ("slow down
10¹²× here, speed up there"), itself editable on the timeline.

**Signals across views come free.** Signals live on entity ports and every composition
reads them at the same clock time, so a pulse travelling laser → fiber → detector is
synchronised in the cartoon, the schematic, and the 3D view. A fiber is a time delay on
a signal; a pulse's position along its depicted path is a binding on
(time since emission ÷ delay).

## 4. One timeline: declarative and imperative

Doc 04 §4.2 and §6.5 already describe the timeline in flex vocabulary (`seq`, `par`,
`dur`, `grow`, `min`/`max`) solved by "the same solver on the time axis". Not built:
today's clips (`time.rs`) have absolute start times.

**The temporal solver reuses the constraint system** with one-dimensional variables
(start and end times):

- *required*: order within `seq`, alignment within `par`;
- *strong*: explicit durations and anchors ("finale ends at 30 s");
- *weak*: `grow` items absorb slack; everything prefers to stay put.

Solve on structural change; the resolved times are derived, like world transforms.
Dragging a clip edge in the timeline is a finger on that 1D cluster: interaction-time
causality on the time axis.

**Manim-style scripts are a text view of the same tree**, not a second runtime:

| Manim | Iron |
|---|---|
| `self.play(A); self.play(B)` | `seq [A, B]` |
| `self.wait(1)` | a fixed-duration gap |
| `AnimationGroup(A, B)` | `par [A, B]` |
| `LaggedStart(…, lag_ratio)` | stagger offsets on `animates` relations (built) |
| `ValueTracker` animated | a clip on a 0→1 parameter (doc 05 §2.1's rule) |
| `add_updater`, `always_redraw` | bindings to that parameter (built) |
| `run_time=2` | a fixed `dur` |

Straight-line play/wait code maps both ways to a seq/par tree, which is doc 02's idea of
surface text as one encoding of the document, and it is the form an LLM writes well.
Loops expand into repeated clips unless they map onto doc 02's planned `Each` node.

## 5. UI sketches

- **State-machine editor**: states and transitions as a graph (doc 02's planned
  state-machine node; doc 06's enumerated choice and "pin a state").
- **Scenario timeline whose tracks are input ports.** Driver tracks are editable;
  output tracks (states visited, signal values) are read-only bands and sparklines
  (doc 05 §5.5).
- **Free inputs made visible.** A machine shows its unconnected inputs; for each, the
  editor offers: put it on a track, give it a slider, or leave it to the viewer. When
  every input is on a track, the scenario is closed: scrubbable if memoryless, "record
  to scrub" if not.
- **Record a live session** of a human driving inputs into scenario tracks (like After
  Effects' motion sketch), turning exploration into a lesson.

## 6. Prior art

Simulink and Modelica (source blocks drive, scope blocks observe, the model unchanged);
Rive (timelines plus state machines with declared inputs); Harel statecharts; Bret
Victor, "Up and Down the Ladder of Abstraction"; manim; After Effects.

## 7. When to promote this to a plan

After roadmap stages D (IK demo) and F (ports and signals with time series) exist. At
that point the questions become concrete: does the scenario/observation split hold up
for a real modulator lesson, and is the temporal solver worth it over absolute times?
Likely outputs: a vision doc 10, a revision of vision 09 §9, and a roadmap stage.
