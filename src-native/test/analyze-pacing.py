#!/usr/bin/env python3
"""Summarize native CPU pacing traces, excluding the first five seconds.

These are CPU submission times, not GPU completion or display presentation.
Schema 2 carries host frame IDs through extraction, including skipped renders.
Legacy traces can only be joined during uninterrupted always-render runs.
"""
import json
import sys


def percentiles(values):
    values = sorted(values)
    if not values:
        return None
    return {label: round(values[min(len(values) - 1, int(len(values) * q))], 3)
            for label, q in [("p05", .05), ("p50", .5), ("p95", .95), ("max", 1)]}


def analyze(trace, warmup=5):
    rows = trace["rows"]
    ticks = sorted((r for r in rows if r["name"] == "tick"
                    and r["start"] >= trace["epoch"] + warmup), key=lambda r: r["start"])
    submits = {r["frame"]: r for r in rows if r["name"] == "render_submit"}
    joined = [(r, submits[r["frame"]]) for r in ticks
              if r["rendered"] and r["frame"] in submits]
    result = {"schema": trace.get("schema", 1), "steadyTicks": len(ticks),
              "renderedTicks": sum(r["rendered"] for r in ticks), "matchedSubmissions": len(joined),
              "settings": {k: trace.get(k) for k in
                           ["presentMode", "frameLatency", "renderMode", "pacing", "marginMs"]}}
    result["callbackIntervalMs"] = percentiles([
        1000 * (b.get("callback", b["start"]) - a.get("callback", a["start"]))
        for a, b in zip(ticks, ticks[1:])])
    for name in ["tick", "acquire", "render_submit"]:
        result[name + "Ms"] = percentiles([
            1000 * (r["end"] - r["start"]) for r in rows
            if r["name"] == name and r["start"] >= trace["epoch"] + warmup])
    result["scheduledWaitMs"] = percentiles([
        1000 * (r["start"] - r.get("callback", r["start"])) for r in ticks])
    result["timerOvershootMs"] = percentiles([
        1000 * (r["start"] - r["planned_start"]) for r in ticks
        if r.get("planned_start", 0) > r.get("callback", r["start"])])
    result["steadyCallbackSpanSeconds"] = round(
        ticks[-1]["start"] - ticks[0]["start"], 3) if len(ticks) > 1 else 0
    result["comparisonWarnings"] = []
    result["missingSubmissions"] = result["renderedTicks"] - len(joined)
    if result["missingSubmissions"]:
        result["comparisonWarnings"].append("Some rendered host frames have no matching CPU submission.")
    if len(ticks) < 100:
        result["comparisonWarnings"].append("Too few callbacks for a steady-state comparison; check window visibility.")
    result["hostToCpuPresentReturnMs"] = percentiles([
        1000 * (s["end"] - t["start"]) for t, s in joined])
    result["callbackToCpuPresentReturnMs"] = percentiles([
        1000 * (s["end"] - t.get("callback", t["start"])) for t, s in joined])
    result["inputReceiptToCpuPresentReturnMs"] = percentiles([
        1000 * (s["end"] - t["input_received"]) for t, s in joined
        if t.get("input_received") is not None])
    deadlines = [(t, s) for t, s in joined if t["target"] > 0]
    result["cpuDeadlineMisses"] = sum(s["end"] > t["target"] for t, s in deadlines)
    result["cpuDeadlineSamples"] = len(deadlines)
    result["cpuHeadroomMs"] = percentiles([
        1000 * (t["target"] - s["end"]) for t, s in deadlines])
    if trace.get("schema", 1) < 2:
        result["frameMatching"] = "legacy: only valid for uninterrupted always-render runs"
    return result


if __name__ == "__main__":
    for path in sys.argv[1:]:
        with open(path) as f:
            result = analyze(json.load(f))
        print(json.dumps({"path": path, **result}))
