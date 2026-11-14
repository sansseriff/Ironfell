# Themes: colour tokens with light and dark values

**Built 2026-11-08 (step 5 of the current order).** CSS's model for colour: named
tokens, each with a value per theme, used by name everywhere a colour goes, so a
drawing reads in light and dark mode and a document (or a lesson) restyles in one
place. The user's ask: "pairs of colors… just like is done in CSS for light mode and
dark mode", that the agent and a person can both check, revise and extend.

## 1. Decisions

| # | Decision |
|---|---|
| H1 | A token is a document node: a `token` (name, `token.light`, `token.dark`) in a `palette` root. People and the agent see, edit, rename and add tokens like any node. |
| H2 | A colour uses a token by **formula**: `@series-2` is a term in the formula language (`fill.color="{= @series-2}"`, or `fill.color="@series-2"` in an edit), so it composes (`mix(@surface, @accent, 0.2)`) and formulas stay the source of truth. |
| H3 | The theme is the host's (the app's light or dark appearance, as the canvas is cleared). The store publishes each token's value for it as `token.color` (`ProducerId::THEME`); `@name` reads that leaf, so a theme switch or an edited token flows through the ordinary reactive graph and redraws exactly what uses it. Nothing about the theme is saved. |
| H4 | **Built-in tokens**, the dataviz reference palette validated for contrast and colour-vision separation on each theme's surface: `surface ink ink-2 muted grid axis accent series-1 … series-8`. The first use of a built-in name adds the whole built-in theme to the document, in the same transaction (one undo), so every token in use is in the file. |
| H5 | SVG export writes the colours of the theme in use (the published values). |

## 2. For a person

The colour picker has a **Tokens** section above "On this page": each token a swatch
split into its light and dark values (the theme in use the larger half). Click one
to use it; the field then shows a chip, `@series-3`, and a Detach button keeps the
colour as a plain value. **+** makes a token from the colour shown (both themes the
same; edit either after); the pencil lists the document's tokens to rename them and
set each theme's value.

## 3. For the agent

A COLOURS section in the prompt: prefer tokens to literal colours; series in order
for categories; text in `ink`, `ink-2` or `muted`, never a series colour; edit or
extend the palette with `<set>` and `<token>`.

## 4. `std:plot`

Its frame, grid, axes, labels and title use `@surface @grid @axis @ink @muted`; its
series colours (`color`, `color2`, `color3`) default to `auto`, meaning
`@series-1..3`; `style2` and `style3` give the other series their own style (default:
the first's).

## 5. Not yet

- Several token sets (a lesson's own theme, a print theme) and choosing one per
  document or view: the palette is one root now; a context rule could pick.
- Per-view themes (a dark slide beside a light document).
- Tokens for more than colour (spacing, type sizes), and token aliases
  (`accent = @series-1`).
- The plot's legend (waits on series ownership: notes/plot-system-vision.md).

## 6. Done

**2026-11-08.** `palette.rs` (built-ins, `tokens`, `publish`, `ensure_ops`,
`add_ops`); `Expr::Token` with the graph resolving names to `token.color` leaves for
dependencies and evaluation; `Store::set_theme`, driven by the bridge from the
canvas's clear colour; the `tokens` command and the `add_token` author verb; the
picker's Tokens section and the field chip; edits accept `@name` for colours and
`<place>` arguments; `std:plot` on tokens with per-series styles. `tests/tokens.rs`,
`tests/plot.rs`; in the app the plot reads in both themes and a fill binds to a token
from the picker.
