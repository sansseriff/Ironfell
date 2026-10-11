#!/usr/bin/env python3
"""Build the guide: expand <snip>, <ref>, <chip>, <out>, <raw> in _src/src/*.html into plans/guide/*.html.

Run from anywhere:  python3 plans/guide/_src/build.py   (or name pages: ... build.py 03-ops)

Every snippet is read from the repository at build time, so code and line numbers
cannot drift from the source. Every <ref has="..."> and field-guide entry is checked
against the line it cites; a mismatch fails the build.
"""
import html
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
SRC = os.path.join(HERE, "src")
OUT = os.path.abspath(os.path.join(HERE, ".."))

SYSTEMS = {
    "store": "Document model",
    "ops": "Ops & history",
    "io": "Files & views",
    "react": "Reactive graph",
    "time": "Scripted time",
    "bridge": "Bevy bridge",
    "shell": "FFI & shell",
    "model": "Model surface",
}

PAGES = [
    # slug, number, title, blurb
    ("index", "", "Iron's Document Spine", ""),
    ("01-orientation", "1", "The shape of the change", "What moved where, the day's timeline, and a Rust phrasebook"),
    ("02-document", "2", "What the document is made of", "Ids, nodes, typed components, slots, the registry, order keys"),
    ("03-ops", "3", "Changing it: ops, transactions, undo", "The only way anything changes, and how it runs backwards"),
    ("04-bridge", "4", "Into Bevy: the reconciler and writeback", "How the document becomes entities, and how a drag becomes an op"),
    ("05-reactive", "5", "Bindings and the reactive graph", "Expressions, dependency heights, gestures, and the slider"),
    ("06-time", "6", "Scripted time: clips on a timeline", "Clips as nodes, targets as relations, sampling as a layer"),
    ("07-model", "7", "The model surface", "Views in, <edit> markup out, one transaction on the same undo stack"),
    ("08-field-guide", "", "Field guide", "Every function and type, searchable, with its system and file:line"),
]

errors = []
file_cache = {}


def lines_of(path, rev=None):
    key = (path, rev)
    if key not in file_cache:
        if rev:
            text = subprocess.run(["git", "-C", REPO, "show", f"{rev}:{path}"], capture_output=True, text=True, check=True).stdout
        else:
            with open(os.path.join(REPO, path), encoding="utf-8") as f:
                text = f.read()
        file_cache[key] = text.split("\n")
    return file_cache[key]


def system_of(path):
    p = path
    if p.startswith("src/iron_document/"):
        name = os.path.basename(p)
        if "/tests/" in p:
            return {"spine.rs": "ops", "reactive.rs": "react", "time.rs": "time"}.get(name, "ops")
        return {
            "id.rs": "store", "value.rs": "store", "component.rs": "store", "registry.rs": "store",
            "document.rs": "store", "order.rs": "store", "op.rs": "ops", "lib.rs": "ops",
            "serial.rs": "io", "view.rs": "io", "expr.rs": "react", "graph.rs": "react", "time.rs": "time",
            "dump.rs": "io",
        }.get(name, "store")
    if p.startswith("src/document_bridge/") or p.startswith("src/bevy_app/") or p in ("src/panels.rs", "src/lib.rs"):
        return "bridge"
    if p.startswith("src-ui/model/"):
        return "model"
    return "shell"


def lang_of(path):
    if path.endswith(".rs"):
        return "rust"
    if path.endswith(".ts"):
        return "typescript"
    if path.endswith(".svelte") or path.endswith(".html"):
        return "xml"
    if path.endswith(".json"):
        return "json"
    return "plaintext"


def attrs(s):
    return dict(re.findall(r'([a-z_-]+)="([^"]*)"', s))


def vscode(path, line):
    return f"vscode://file{REPO}/{path}:{line}"


def chip(sys_key):
    return f'<span class="chip sys-{sys_key}">{SYSTEMS[sys_key]}</span>'


def parse_ranges(spec):
    out = []
    for part in filter(None, spec.split(",")):
        if "-" in part:
            a, b = part.split("-")
            out.append((int(a), int(b)))
        else:
            out.append((int(part), int(part)))
    return out


def expand_snip(m):
    a = attrs(m.group(1))
    path, rev = a["file"], a.get("rev")
    (start, end), = parse_ranges(a["lines"])
    src = lines_of(path, rev)
    if end > len(src):
        errors.append(f"{path}: range {start}-{end} past end ({len(src)})")
    body = src[start - 1:end]
    body = [l.replace("\t", "    ") for l in body]
    indents = [len(l) - len(l.lstrip(" ")) for l in body if l.strip()]
    cut = min(indents) if indents else 0
    body = [l[cut:] for l in body]
    print(f"  snip {path}:{start}-{end}{' @' + rev if rev else ''}\n      first: {body[0].strip()[:90]}\n      last:  {body[-1].strip()[:90]}")
    sys_key = a.get("sys") or system_of(path)
    gutter = "\n".join(str(n) for n in range(start, end + 1))
    bands = ""
    for (x, y) in parse_ranges(a.get("mark", "")):
        if x < start or y > end:
            errors.append(f"{path}: mark {x}-{y} outside {start}-{end}")
        bands += f'<span class="band" style="--from:{x - start};--len:{y - x + 1}"></span>'
    note = f'<span class="note">{a["note"]}</span>' if a.get("note") else ""
    where = f'<a class="path" href="{vscode(path, start)}">{path}</a>' if not rev else f'<span class="path">{path}</span>'
    rng = f"lines {start}–{end}" if start != end else f"line {start}"
    if rev:
        rng += f" · as of <code>{rev}</code>, before the change"
    lang = a.get("lang") or lang_of(path)
    code = html.escape("\n".join(body))
    return (
        f'<figure class="code"><figcaption>{chip(sys_key)}{where}<span class="range">{rng}</span>{note}</figcaption>'
        f'<div class="code-scroll"><div class="code-grid">{bands}<pre class="gutter" aria-hidden="true">{gutter}</pre>'
        f'<pre class="src"><code class="language-{lang}">{code}</code></pre></div></div></figure>'
    )


def expand_ref(m):
    a = attrs(m.group(1))
    label = m.group(2)
    path, line = a["file"], int(a["line"])
    src = lines_of(path)
    if line > len(src):
        errors.append(f"ref {path}:{line} past end")
    elif a.get("has") and html.unescape(a["has"]) not in src[line - 1]:
        errors.append(f"ref {path}:{line} lacks {a['has']!r}: {src[line - 1].strip()!r}")
    text = label or f"{os.path.basename(path)}:{line}"
    return f'<a class="ref" href="{vscode(path, line)}" title="{path}:{line}"><code>{text}</code></a>'


def expand_out(m):
    a = attrs(m.group(1))
    with open(os.path.join(SRC, "outputs", a["src"]), encoding="utf-8") as f:
        text = f.read().rstrip("\n")
    title = a.get("title", "")
    return f'<figure class="output"><figcaption>{title}</figcaption><pre class="plain"><code>{html.escape(text)}</code></pre></figure>'


def expand_raw(m):
    a = attrs(m.group(1))
    text = m.group(2).strip("\n")
    lang = a.get("lang", "plaintext")
    cap = f'<figcaption>{a["title"]}</figcaption>' if a.get("title") else ""
    return f'<figure class="plainfig">{cap}<pre class="plain"><code class="language-{lang}">{html.escape(text)}</code></pre></figure>'


def transform(text):
    text = re.sub(r"<raw([^>]*)>(.*?)</raw>", expand_raw, text, flags=re.S)
    text = re.sub(r"<snip([^>]*)/>", expand_snip, text)
    text = re.sub(r"<ref([^>]*)>(.*?)</ref>", expand_ref, text, flags=re.S)
    text = re.sub(r"<out([^>]*)/>", expand_out, text)
    text = re.sub(r'<chip sys="([a-z]+)"/>', lambda m: chip(m.group(1)), text)
    return text


FONTS = ('<link rel="preconnect" href="https://fonts.googleapis.com"><link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>'
         '<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=IBM+Plex+Sans+Condensed:wght@300;400;600&amp;'
         'family=JetBrains+Mono:wght@400;600&amp;family=Source+Serif+4:ital,opsz,wght@0,8..60,400;0,8..60,600;1,8..60,400&amp;display=swap">')
HLJS = '<script src="https://cdnjs.cloudflare.com/ajax/libs/highlight.js/11.9.0/highlight.min.js"></script>'


def page(i, body):
    slug, num, title, _ = PAGES[i]
    href = lambda s: "index.html" if s == "index" else f"{s}.html"
    nav = []
    for s, n, t, _ in PAGES:
        cur = ' aria-current="page"' if s == slug else ""
        nav.append(f'<li><a href="{href(s)}"{cur}><span class="num">{n}</span>{t if s != "index" else "Start here"}</a></li>')
    prev_ = PAGES[i - 1] if i > 0 else None
    next_ = PAGES[i + 1] if i + 1 < len(PAGES) else None
    pager = ""
    if prev_:
        pager += f'<a href="{href(prev_[0])}" aria-label="Previous part">&larr;</a>'
    if num:
        pager += f'<span class="here">Part {num} of 7</span>'
    if next_:
        pager += f'<a href="{href(next_[0])}" aria-label="Next part">&rarr;</a>'
    np = '<nav class="nextprev">'
    if prev_:
        np += f'<a href="{href(prev_[0])}"><small>Previous</small><span>{("Part " + prev_[1] + ": ") if prev_[1] else ""}{prev_[2]}</span></a>'
    if next_:
        np += f'<a class="next" href="{href(next_[0])}"><small>Next</small><span>{("Part " + next_[1] + ": ") if next_[1] else ""}{next_[2]}</span></a>'
    np += "</nav>"
    doc_title = title if slug == "index" else f"{('Part ' + num + ' · ') if num else ''}{title} — Iron's Document Spine"
    inner = f"""<title>{html.escape(doc_title)}</title>
{FONTS}
<link rel="stylesheet" href="guide.css">
<header class="topbar"><div class="topbar-inner"><a class="series" href="index.html">Iron's Document Spine <span>· a guide to the Sept 18 work</span></a><div class="pager">{pager}</div></div></header>
<div class="frame">
<nav class="sidebar" aria-label="Series"><h4>The series</h4><ol>{''.join(nav)}</ol><h4>On this page</h4><ul class="toc"></ul></nav>
<main>
{body}
{np}
</main>
</div>
{HLJS}
<script src="guide.js"></script>
"""
    return f"""<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover">
</head>
<body>
{inner}</body>
</html>
"""


def build_guide_rows():
    sys.path.insert(0, HERE)
    import entries  # noqa
    rows = []
    for e in entries.ENTRIES:
        name, kind, path, line, sysk, part, what = e
        src = lines_of(path)
        needle = name.split("::")[-1].split(".")[-1].replace("()", "")
        if line > len(src) or needle not in src[line - 1]:
            errors.append(f"guide: {name} not on {path}:{line}: {src[line - 1].strip() if line <= len(src) else 'EOF'!r}")
        chap = PAGES[int(part)][0] + ".html" if part else ""
        chap_link = f'<a href="{chap}">Part {part}</a>' if part else ""
        rows.append(
            f'<tr data-sys="{sysk}"><td><code>{html.escape(name)}</code><span class="kind">{kind}</span></td>'
            f'<td>{chip(sysk)}</td><td class="where"><a class="ref" href="{vscode(path, line)}"><code>{html.escape(path)}:{line}</code></a></td>'
            f'<td class="what">{what}</td><td>{chap_link}</td></tr>'
        )
    print(f"  field guide: {len(rows)} entries")
    return "\n".join(rows)


def main():
    only = set(sys.argv[1:])
    for i, (slug, *_rest) in enumerate(PAGES):
        path = os.path.join(SRC, f"{slug}.html")
        if not os.path.exists(path):
            print(f"skip {slug} (no source)")
            continue
        if only and slug not in only:
            continue
        print(f"build {slug}")
        with open(path, encoding="utf-8") as f:
            body = f.read()
        if "<!--GUIDE-ROWS-->" in body:
            body = body.replace("<!--GUIDE-ROWS-->", build_guide_rows())
        body = transform(body)
        with open(os.path.join(OUT, f"{slug}.html"), "w", encoding="utf-8") as f:
            f.write(page(i, body))
    if errors:
        print("\nERRORS:")
        for e in errors:
            print("  " + e)
        sys.exit(1)
    print("ok")


if __name__ == "__main__":
    main()
