# Guide sources

`plans/guide/*.html` is generated. Edit the pages in `src/` and the field-guide table in
`entries.py`, then rebuild from the repo root:

```sh
python3 plans/guide/_src/build.py            # every page
python3 plans/guide/_src/build.py 03-ops     # one page
```

Code excerpts (`<snip file=… lines=…>`) are copied from the working tree at build time, so a
rebuild picks up code changes. The build fails if a `<ref has=…>` or a field-guide entry no
longer matches the line it cites. That is the signal that the prose describing that code
needs another look. Line numbers written as plain text in the prose are not checked.

`guide.css`, `guide.js` and `img/` in the parent folder are hand-maintained, not generated.
