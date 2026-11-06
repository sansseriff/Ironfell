#!/usr/bin/env python3
"""Tabulate latency-probe runs (plans/rendering/latency-view.md §4).

    scripts/latency/summarize.py .agents/latency/measure/*.json

One row per run: p50 (p90) lag in ms per tracker, the Vello-minus-DOM gap
(engine ring minus the Canvas2D ring drawn in the event), frame interval and
whether every tracker came to rest on the pointer. `--markdown` prints a
Markdown table.
"""
import json
import sys

KEYS = ["canvas", "raf", "svg", "konva", "webgl", "webgpu", "engine", "square"]
args = [a for a in sys.argv[1:] if not a.startswith("--")]
markdown = "--markdown" in sys.argv
rows = []
for path in args:
    if path.endswith("summary.json") or "/summary-" in path:
        continue
    with open(path) as f:
        j = json.load(f)
    lag = j["lag_ms"]
    if not all(lag.get(k, {}).get("n") for k in ["canvas", "engine", "square"]):
        continue
    rest = j.get("rest_offset_pt", {})
    rows.append({
        "label": j["label"],
        **{k: (lag[k]["p50"], lag[k]["p90"]) if lag.get(k, {}).get("n") else None for k in KEYS},
        "gap": lag["engine"]["p50"] - lag["canvas"]["p50"],
        "frame": j["frame_interval_ms"].get("p50", 0),
        "rest": "yes" if rest and all(abs(v) <= 1 for v in rest.values() if v != -999) else "no",
    })

head = ["run"] + KEYS + ["vello−dom", "frame", "concentric at rest"]
fmt = lambda r: [r["label"]] + [f"{r[k][0]:.1f} ({r[k][1]:.1f})" if r[k] else "–" for k in KEYS] + [f"{r['gap']:+.1f}", f"{r['frame']:.1f}", r["rest"]]
if markdown:
    print("| " + " | ".join(head) + " |")
    print("|" + "---|" * len(head))
    for r in rows:
        print("| " + " | ".join(fmt(r)) + " |")
else:
    w = [max(len(h), *(len(fmt(r)[i]) for r in rows)) if rows else len(h) for i, h in enumerate(head)]
    print("  ".join(h.ljust(w[i]) for i, h in enumerate(head)))
    for r in rows:
        print("  ".join(c.ljust(w[i]) for i, c in enumerate(fmt(r))))
