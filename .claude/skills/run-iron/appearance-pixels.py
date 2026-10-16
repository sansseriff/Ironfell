"""Pixel assertions for appearance.ts's authored fixture, away from AA edges."""
import json
import sys
from pathlib import Path
from PIL import Image
root = Path(sys.argv[1])
p = json.loads((root / 'panel.json').read_text())
im = Image.open(root / 'appearance.png').convert('RGB')
def pixel(x, y):
    return im.getpixel((round(p['x']*p['dpr']+x), round(p['y']*p['dpr']+y)))
if len(sys.argv) > 2:
    im = Image.open(root / 'radial.png').convert('RGB')
    centre, edge = pixel(700, 110), pixel(798, 110)
    assert centre[0] > centre[2] + 150 and edge[2] > edge[0] + 150, (centre, edge)
    print('pixels: radial gradient pass')
    sys.exit(0)
a, b, c = pixel(425, 110), pixel(475, 110), pixel(525, 110)
assert max(abs(x-y) for x, y in zip(b, c)) <= 2, (a, b, c)
assert a[0] > 240 and a[1] > 100 and a[2] > 100, (a, b, c)
assert b[2] > 240 and b[0] > 100 and b[1] > 100, (a, b, c)
l, r = pixel(610, 110), pixel(790, 110)
assert l[0] > l[2]+100 and r[2] > r[0]+100, (l, r)
assert pixel(450, 310)[1] > 240, pixel(450, 310)
assert pixel(405, 265)[0] > 100, pixel(405, 265)
print('pixels: isolated overlap, linear gradient and ellipse clip pass')
