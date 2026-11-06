# Same frame or a frame behind, per displayed frame of a probe sweep
# (latency-probe --csv; plans/rendering/worker-frame-hold.md §6g).
#
# Each frame's Vello tracker (the document `square`, drawn by the worker) is
# compared with a DOM tracker (the `svg` ring, moved in the event). While the
# pointer moves at `--speed` pt/s, one frame (1/60 s) is `speed/60` pt. A gap
# under half of that is the same frame (S), under 1.5 frames one frame behind
# (L), more is further behind (F), and Vello ahead of the DOM is A. Flips
# between S and L are the judder the frame modes remove; a steady S (early) or
# a steady L (late) is smooth.
#
#   python3 scripts/latency/flips.py [--speed 600] [--vello square] [--dom svg] run.csv [...]
import csv, sys

args = sys.argv[1:]
opts = {'speed': '600', 'vello': 'square', 'dom': 'svg'}
while args and args[0].startswith('--'):
    opts[args[0][2:]] = args[1]; args = args[2:]
step = float(opts['speed']) / 60.0
for path in args:
    rows = list(csv.DictReader(open(path)))
    seq = []
    prev = None
    for r in rows:
        try:
            dom = float(r[opts['dom']]); vello = float(r[opts['vello']])
        except (ValueError, KeyError):
            prev = None; continue
        if prev is not None and abs(dom - prev) > 0.5:
            d = 1 if dom > prev else -1
            gap = (dom - vello) * d  # how far Vello trails the DOM
            seq.append('A' if gap < -0.5 * step else 'S' if gap < 0.5 * step else 'L' if gap < 1.5 * step else 'F')
        prev = dom
    text = ''.join(seq)
    flips = sum(1 for a, b in zip(text, text[1:]) if a != b)
    n = max(1, len(text))
    pct = lambda c: 100 * text.count(c) / n
    name = path.split('/')[-1].replace('.csv', '')
    print(f"{name:46} frames {len(text):4}  same {pct('S'):5.1f}%  behind {pct('L'):5.1f}%  further {pct('F'):4.1f}%  ahead {pct('A'):4.1f}%  flips {flips:3} ({100*flips/n:4.1f}%)")
