"""Blank canvas pixels: line/dot coverage and theme/custom backgrounds."""
from pathlib import Path
from collections import Counter
import sys
from PIL import Image
root = Path(sys.argv[1])
def counts(name):
    return Counter(Image.open(root / (name + '.png')).convert('RGB').getdata())
def dominant(name):
    return counts(name).most_common(1)[0][0]
def different(name):
    c = counts(name)
    bg = dominant('off')
    return sum(n for rgb, n in c.items() if max(abs(a-b) for a,b in zip(rgb, bg)) > 3)
assert dominant('off') == (255, 255, 255), dominant('off')
assert different('lines') > different('dots') > 20, (different('lines'), different('dots'))
for a, b in [('lines', 'lines-pan'), ('lines-pan', 'lines-zoom')]:
    assert Image.open(root / (a + '.png')).tobytes() != Image.open(root / (b + '.png')).tobytes(), (a, b)
    assert different(b) > 20, b
assert max(abs(a-b) for a,b in zip(dominant('custom'), (220,234,250))) < 3, dominant('custom')
assert dominant('custom') == dominant('custom-dark'), (dominant('custom'), dominant('custom-dark'))
assert max(dominant('theme-dark')) < 30, dominant('theme-dark')
assert min(dominant('theme-light')) > 250, dominant('theme-light')
print('pixels: lines, faint dots, custom colour, light/dark following pass')
