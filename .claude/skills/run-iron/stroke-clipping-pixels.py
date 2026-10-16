"""Check stroke outsets past an embed source's geometry, away from AA edges."""
import json
import sys
from pathlib import Path
from PIL import Image

root = Path(sys.argv[1])
edited = len(sys.argv) > 2
p = json.loads((root / 'stroke-panel.json').read_text())
im = Image.open(root / ('stroke-clipping-edited.png' if edited else 'stroke-clipping.png')).convert('RGB')

def pixel(x, y):
    return im.getpixel((round(p['x'] * p['dpr'] + x), round(p['y'] * p['dpr'] + y)))

w, h, half = (260, 180, 16) if edited else (240, 160, 8)
x, y = 120, 100  # automatic embed's placement + source rect's local offset
for point in [(x-half/2, y+h/2), (x+w+half/2, y+h/2),
              (x+w/2, y-half/2), (x+w/2, y+h+half/2)]:
    assert max(pixel(*point)) < 10, ('automatic frame cut stroke', point, pixel(*point))

# The second embed has an authored frame ending at source (280, 200).
# It intentionally trims the same source's stroke (and its later size change).
for point in [(714, 180), (590, 264)]:
    assert min(pixel(*point)) > 245, ('explicit frame failed to clip', point, pixel(*point))
print('pixels: automatic stroke bounds update after edits; explicit frame clips pass' if edited
      else 'pixels: automatic embed preserves all four stroke outsets pass')
