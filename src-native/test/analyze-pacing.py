#!/usr/bin/env python3
"""Summarize opt-in native CPU pacing traces; excludes the first five seconds.

CPU present return is not GPU completion or display presentation. Frame IDs
match render submissions only in an uninterrupted always-render diagnostic run.
"""
import json
import sys


def percentiles(values):
    values = sorted(values)
    if not values:
        return None
    return {label: round(values[min(len(values) - 1, int(len(values) * q))], 3)
            for label, q in [("p50", .5), ("p95", .95), ("max", 1)]}


for path in sys.argv[1:]:
    with open(path) as f:
        trace = json.load(f)
    rows = [r for r in trace["rows"] if r["start"] >= trace["epoch"] + 5]
    ticks = [r for r in rows if r["name"] == "tick"]
    submits = {r["frame"]: r for r in rows if r["name"] == "render_submit"}
    result = {"path": path, "steadyTicks": len(ticks)}
    result["callbackIntervalMs"] = percentiles([
        1000 * (b["start"] - a["start"]) for a, b in zip(ticks, ticks[1:])])
    for name in ["tick", "acquire", "render_submit"]:
        result[name + "Ms"] = percentiles([
            1000 * (r["end"] - r["start"]) for r in rows if r["name"] == name])
    result["callbackToCpuPresentReturnMs"] = percentiles([
        1000 * (submits[r["frame"]]["end"] - r["start"])
        for r in ticks if r["frame"] in submits])
    print(json.dumps(result))
