# Guide 2 sources

The series is four Foundations pages (`f1-…` to `f4-…`, numbered `F1`–`F4`), which
replace the older `plans/guide/` with only what is still true of the code, then Parts
1–13 and the field guide. Parts 10–13 (October 17) cover the engine boundary; Part 14 (October 27) opens with a one-page map of the app and covers contexts (behaviour as data); the earlier pages were corrected at the same time, so they describe the code as of the last rebuild rather than as of the commit they were first written at.

`plans/guide2/*.html` is generated. Edit the pages in `src/` and the field-guide table in
`entries.py`, then rebuild from the repo root:

```sh
python3 plans/guide2/_src/build.py              # every page
python3 plans/guide2/_src/build.py 10-boundary  # one page
```

Code excerpts (`<snip file=… lines=…>`) are copied from the working tree at build time, so a
rebuild picks up code changes; the build prints each excerpt's first and last line so a
shifted range is easy to spot. The build fails if a `<ref has=…>` or a field-guide entry no
longer matches the line it cites. That is the signal that the prose describing that code
needs another look. Line numbers written as plain text in the prose are not checked.

`guide.css`, `guide.js` and `highlight.min.js` (highlight.js 11.9.0, vendored so the pages
read offline) in the parent folder are hand-maintained, not generated. The tooling is
adapted from `plans/guide/_src/`; the systems and their colours differ.

**Known drift (2026-10-27):** a full rebuild now fails: older pages cite files and lines
that have since moved (`iron_document/src/component.rs` is gone; refs in Parts 10–13
such as `geom.rs:365` no longer match). Part 14 and the index were built alone; the
other pages' sidebars and "of 14" counters were patched in their built HTML. Those
pages need a pass against today's code before the next full rebuild.
