# Time-domain signals: questions for a future plan

**Status:** Note, 2026-10-18. Not adopted; do not build from it. Written when the
component model's demos (`specs/component-model.md`) were scoped to steady-state
signals, to record why time was left out and what a later plan must settle.

ECM §3.3 reserves `Signal::Series` and `Signal::Spectrum`, and ECM §12 phase 5 sketches
time-dependent sources and the modulator demo. Neither is designed. A pulsed laser
feeding a fiber with a propagation delay, proposed as a component-model demo, showed
that time-domain signals raise questions large enough for their own plan.

## 1. Delay and propagation

- A fiber delays light by `n·L/c`: about 10 µs for 2 km. Nothing a person can see.
  Showing it needs simulation time slowed relative to wall time (vision 05's three kinds
  of time), or an exaggerated propagation speed offered as a teaching control. Which
  one, and where the rate lives (the playhead, the composition, a lesson step), is open.
- Is a delay a property of a port connection, of a component's behaviour (`out(t) =
  in(t − τ)`), or a primitive of its own? General delays (dispersion, filters, echoes)
  suggest behaviour, built from primitives.
- What a delayed signal is at runtime: a sampled `Series` with a history buffer, a list
  of events (pulses as records with an emission time), or both. Pulses suit events;
  analogue waveforms suit samples.
- How a figure shows a signal in flight: pulses travelling along the fiber's cartoon are
  dynamic parts in the component model's render analysis (§11), redrawn every frame.

## 2. Constraints and two-way editing across time

Constraints and delays may not be compatible at all. A constraint holds at an instant; a
delayed relation links values at different instants.

- Dragging a meter's reading back to a laser's power across a delay: one option inverts
  only the steady-state relation (where the delay drops out), showing a ghost "target"
  needle at the finger while the real needle catches up as the pulses already in flight
  arrive. Whether that is honest enough, or whether drags across a delay should simply
  be locked, is open.
- A solver cluster that contains a delayed edge is no longer an algebraic system. Either
  delays cut clusters (each side solved at its own instant), or such loops are refused,
  or they become simulations (vision 09 §7: "a configuration that truly loops… needs a
  simulation, not a binding").
- Handle inversion (`handle.rs`) would need to treat a delay as identity in steady
  state, or refuse it.

## 3. Primitives a time-domain plan needs

- **Integrators and filters**: a meter's dial should show average power with a response
  time, not flicker with every pulse. That is a low-pass filter or integrator over the
  input signal: stateful, so it does not fit the pure floor functions of vision 02 §6.
- **State**: any filter, integrator or delay line carries state between frames. Where it
  lives (runtime only, like producers' signals; never in the file), how it resets on
  scrub or undo, and how it interacts with `Store::freeze`.
- **Sources**: pulse trains, waveforms from parameters (amplitude, frequency, shape), as
  ECM §3.3 says: authored parameters, computed samples.
- **Clocks**: which time drives a signal (lesson playhead, simulation time, wall time),
  and what pausing, scrubbing and slowing down mean for state.
- **Readouts**: peak, average, RMS; scope traces (the modulator demo's fourth view).

## 4. Related

- `vision/05_three-kinds-of-time.md`: playhead, simulation and wall time.
- `vision/10_engine-boundary.md` §2: producers publish runtime signals; a time-domain
  evaluator may be one.
- `notes/bevy-events-and-runtime-drivers.md`, `notes/time-machines-and-scenarios.md`.
- `specs/constraint-system.md` §11: live simulation with grab (XPBD), the nearest
  existing treatment of time and constraints together.
