#!/usr/bin/env python3
"""Check displayed marker geometry; screenshots do not measure photon latency."""
import json
from pathlib import Path
from PIL import Image

root = Path(__file__).resolve().parents[3]
images = [case for path in sorted((root / '.agents/latency').glob('film-images-*.json')) for case in json.loads(path.read_text())]
for case in images:
    im = Image.open(root / case['file']).convert('RGB')
    k = case['dpr']; stage = case['stage']; p = case['pointer']
    region = im.crop((0, int((stage['y'] + 2) * k), im.width, im.height))
    y0 = int((stage['y'] + 2) * k)
    predicates = {
        'DOM': lambda r,g,b: r < 60 and g > 160 and b > 180,
        'engine': lambda r,g,b: r > 210 and g < 70 and 100 < b < 220,
        'shape': lambda r,g,b: r > 220 and 150 < g < 240 and b < 60,
    }
    for name, predicate in predicates.items():
        points = [(i % region.width, i // region.width + y0)
                  for i, color in enumerate(region.getdata()) if predicate(*color)]
        assert points, (case['file'], name, 'not visible')
        x = (min(q[0] for q in points) + max(q[0] for q in points)) / 2
        y = (min(q[1] for q in points) + max(q[1] for q in points)) / 2
        offset = {'DOM':-96, 'engine':96, 'shape':0}[name]
        assert abs(x - (p['x'] + offset) * k) <= 1.5*k, (case['file'],name,'x',x,p)
        assert abs(y - p['y'] * k) <= 1.5*k, (case['file'],name,'y',y,p)
    print(case['file'], 'all three target centres/offsets pass')
