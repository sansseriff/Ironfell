"""Raster tiles against live output (plans/specs/render-cache.md §5).

raster-pixels.py <screenshots dir> <x> <y> <w> <h>: for each pair
<name>-raster.png / <name>-live.png that raster-pixels.txt took, compare the
panel rectangle and check the declared tolerance: at most 0.2% of pixels
differ by more than 2 levels, and at most 0.01% by more than 8. Tiles are
baked at the view's scale on whole-pixel origins, so they match live output
except for antialiasing computed at another strip phase and 8-bit
premultiplied rounding. Each chunk frame (a group's position,
instance-cache.md §2) is its own raster group, composited separately, so
where antialiased edges of several overlap the rounding adds up: 1 or 2
levels on a few percent of pixels (2.2% at the Prussia file's fit, 47
groups). A shifted or missing piece differs by far more.
"""
import os
import sys
from pathlib import Path
from PIL import Image, ImageChops

root = Path(sys.argv[1])
x, y, w, h = map(int, sys.argv[2:6])
# Which screenshots pair up: PREFIX names them, PAIR the two kinds compared
# (instance-pixels.txt sets "shared flat").
prefix = os.environ.get("PREFIX", "rp-")
one, other = os.environ.get("PAIR", "raster live").split()
box = (x, y, x + w, y + h)
failed = False
for raster in sorted(root.glob(f'{prefix}*-{one}.png')):
    name = raster.name[: -len(f'-{one}.png')]
    live = root / f'{name}-{other}.png'
    a = Image.open(raster).convert('RGB').crop(box)
    b = Image.open(live).convert('RGB').crop(box)
    d = ImageChops.difference(a, b).getdata()
    differ = over2 = big = worst = 0
    for px in d:
        m = max(px)
        if m:
            differ += 1
            over2 += m > 2
            big += m > 8
            worst = max(worst, m)
    n = w * h
    ok = over2 <= n * 0.002 and big <= n * 0.0001
    failed |= not ok
    print(f"{name}: {differ} of {n} differ, {over2} by more than 2, {big} by more than 8, worst {worst}: {'pass' if ok else 'FAIL'}")
sys.exit(1 if failed else 0)
