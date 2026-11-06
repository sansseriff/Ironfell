#!/usr/bin/env python3
"""Smoothness of a tracker that moves by itself (latency-probe --watch CSV).

On a smooth 60 Hz animation every displayed frame moves the shape by the same
step. A repeated frame shows as a step near 0 (the screen showed the same
position twice), a skipped one as a step near 2x. Steps far larger (a loop
jumping back) and frames where the shape is still are left out.

    judder.py [--key square] run.csv ...
"""
import csv, sys, statistics as st

def analyse(path, key):
    rows = [r for r in csv.DictReader(open(path)) if r.get(key)]
    t = [float(r['t_ms']) for r in rows]
    x = [float(r[key]) for r in rows]
    steps = []
    for i in range(1, len(x)):
        dt = t[i] - t[i - 1]
        steps.append((dt, x[i] - x[i - 1]))
    moving = [abs(dx) for dt, dx in steps if abs(dx) > 0.05]
    if len(moving) < 10:
        return None
    typical = st.median(moving)
    # Per displayed screen frame (the capture may skip a screen frame; scale by dt).
    norm = []
    for dt, dx in steps:
        frames = max(1, round(dt / 16.667))
        if abs(dx) > 5 * typical * frames:   # a loop wrap
            continue
        norm.append((frames, abs(dx) / frames / typical))
    n = len(norm)
    repeat = sum(1 for f, r in norm if r < 0.35)
    double = sum(1 for f, r in norm if r > 1.6)
    uneven = sum(1 for f, r in norm if 0.35 <= r < 0.75 or 1.3 < r <= 1.6)
    gaps = sum(1 for f, r in norm if f > 1)
    return dict(frames=n, step_pt=round(typical, 2), repeated=repeat, doubled=double, uneven=uneven, capture_gaps=gaps,
                bad_pct=round(100 * (repeat + double) / max(1, n), 1))

if __name__ == '__main__':
    args = sys.argv[1:]
    key = 'square'
    if args[:1] == ['--key']:
        key, args = args[1], args[2:]
    print(f"{'run':44} frames  step   repeated  doubled  uneven  bad%")
    for p in args:
        r = analyse(p, key)
        name = p.split('/')[-1][:44]
        if not r: print(f"{name:44} (not moving)"); continue
        print(f"{name:44} {r['frames']:6} {r['step_pt']:5}  {r['repeated']:8} {r['doubled']:8} {r['uneven']:7}  {r['bad_pct']:5}")
