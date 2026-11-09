# Where the plot system goes: lessons from matplotlib and friends

**Note, 2026-11-08. Not adopted; do not build from it.** `std:plot` (specs/plot.md)
stays the standard plot for now. This note records where it should grow, so later
specs start from the same picture. Prior art: matplotlib (Figure / Axes / Artist),
the grammar of graphics (ggplot2, Vega-Lite, Observable Plot), d3's scales, Manim's
animated axes, Desmos.

## 1. What `std:plot` fixes in code today

- Exactly three series (`ys`, `ys2`, `ys3`) and one `f`.
- One `style` for all series; per-series colours as three inputs.
- Eight tick-label slots per axis (a repeat node lifts this; specs/repeat.md).
- Linear axes only; no log, time, category or polar scales.
- No legend, no annotations, no secondary axis.
- The frame's look, margins and fonts are constants in `standard.rs`.
- One cursor; no regions, error bars or bands.

## 2. The ideas worth taking

**Figure, Axes, Artists (matplotlib).** The Axes owns the coordinate system: scales,
limits, ticks, grid, labels, legend. Artists are what is drawn in it, each added by its
own function (`plot`, `scatter`, `bar`, `hist`, `boxplot`, `fill_between`, `errorbar`,
`imshow`, `contour`, `quiver`, `text`, `annotate`). You pick a different function when
the **data's shape** changes (samples, distributions, grids, vectors), not for looks.
For Iron: an **axes component** whose template is the frame and whose outputs are its
mapping (`x_to_px`, `y_to_px` as functions); **series components** placed inside it
that read the mapping. A plot is an axes with children.

**Scales are functions (d3, grammar of graphics).** A scale maps a data domain to a
range and knows its ticks and its inverse. Linear, log, symlog, time, band
(categories), sqrt, and colour scales are one interface. Iron already has the pieces:
`ticks`, `nice_lo`/`nice_hi`, lambdas. A scale is a value (`scale('log', 1 nm, 1 m)`)
the axes publish and the series consume, and its inverse turns a pointer position
into data (the cursor drag).

**Encodings (Vega-Lite).** Position, colour, size, shape and opacity are channels
mapped from data by scales. A scatter with colour by temperature is a series with a
`color` encoding through a colour scale, plus a colour bar legend.

**Layering and facets.** Several series in one axes; small multiples (one axes per
group) with shared scales. A repeat node over axes gives facets.

**Legends and annotations derive from the series.** Each series names itself; the
legend is a repeat over the axes' children. Annotations (an arrow and label at a data
point) are text placed in data coordinates.

**Animated axes (Manim).** Axes that rescale smoothly when the data changes, ticks that
fade in and out as the range moves, a curve that morphs from one function to another.
Iron's machines and tracks give the timing; the repeat node keyed by tick value gives
ticks that enter and exit.

**Direct manipulation (Desmos).** Drag a point on a curve, drag a slider bound to a
parameter, read values at the cursor. Iron's bindings and affordances already do the
slider half.

## 3. Switching between plot types

Moving between a time series and a box-and-whisker plot is a change of **artist** on
the same axes, not of the axes. With series as components inside an axes, a
lesson can animate between them as a transition: the box plot's layer appears while
the line's disappears, and the y scale animates if the domains differ. Plots that
change the coordinate system (Cartesian to polar) are different axes, crossfaded.
So: one axes, many artist types, and machines to move between them. A single
component with a `kind` switch over unrelated artists is the thing to avoid; it grows
into matplotlib's `plot()` signature.

## 4. A path from here

1. Theme tokens and per-series styles in `std:plot` (now; the themes spec).
2. The repeat node: any number of series, ticks and legend rows.
3. Scales as values; log and category axes.
4. Split into `std:axes` and series components (`std:line`, `std:scatter`, `std:bars`,
   `std:area`, `std:band`); `std:plot` becomes a preset: axes plus a line, kept for
   the agent's one-call case.
5. Distributions (`std:hist`, `std:box`), grids (`std:heatmap`, `std:contour`), vectors.
6. Facets; secondary axes; polar and 3D axes.

## 5. Questions to settle then

- Is the mapping an output of the axes (functions on ports) or a context the children
  read implicitly, like CSS inheritance?
- Do series components keep their data on their own ports, or name columns of a
  table the axes hold (the grammar of graphics' tidy data)?
- How does the agent write a layered plot in one call: nested `<place>`, or a
  `<plot>` shorthand that expands to axes and series?
