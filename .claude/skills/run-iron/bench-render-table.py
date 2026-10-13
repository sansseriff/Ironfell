#!/usr/bin/env python3
"""Format bench-render.sh output as a Markdown table.

    .claude/skills/run-iron/bench-render.sh | tee bench.txt | python3 .claude/skills/run-iron/bench-render-table.py

Columns: frames per second (throughput, GPU unthrottled; idle frames
included, so it rewards a gesture that changes little), then the p50 CPU
time of each phase on the frames that ran it (src/perf.rs): `changed_frame`
is a whole worker frame in which some view re-encoded, the cost of one
change; `frame` is every frame. Times in ms. `commands` is the largest
encode's command count.
"""
import json
import sys

PHASES = ["changed_frame", "display_list", "extract", "encode", "render", "gesture_reconcile", "pick", "frame"]
print("| shapes | size | scenario | fps | " + " | ".join(PHASES) + " | commands |")
print("|" + "---|" * (5 + len(PHASES)))
for line in sys.stdin:
    parts = line.strip().split(" ", 3)
    if len(parts) < 4 or not parts[3].startswith("{"):
        if line.strip():
            print(f"<!-- {line.strip()} -->")
        continue
    n, size, scenario, raw = parts
    m = json.loads(raw)
    cell = lambda p: "–" if not m.get(p) or m[p]["n"] == 0 else f'{m[p]["p50"]:.2f}'
    fps = m.get("fps")
    fps_text = f"{fps:.0f}" if fps is not None else "–"
    print(f"| {n} | {size} | {scenario} | {fps_text} | " + " | ".join(cell(p) for p in PHASES) + f" | {m.get('commands', 0)} |")
