#!/usr/bin/env python3
"""Compare spatial.txt's canvas/editor screenshots at SIZE=1600x1000@1 or @2.
Requires Pillow. The upper 900 CSS pixels contain the complete canvas/editor;
the excluded bottom 100 contain the timeline and changing FPS HUD.
"""
from pathlib import Path
from PIL import Image, ImageChops

shots = Path(__file__).parent / "screenshots"
for case in ("main", "embeds", "transformed", "zoom"):
    with Image.open(shots / f"spatial-{case}-reference.png") as a, Image.open(shots / f"spatial-{case}-indexed.png") as b:
        assert a.size == b.size and a.width in (1600, 3200), f"unexpected viewport: {a.size}, {b.size}"
        box = (0, 0, a.width, 900 * (a.width // 1600))
        diff = ImageChops.difference(a.convert("RGB").crop(box), b.convert("RGB").crop(box))
        assert diff.getbbox() is None, f"{case}: different pixels in {diff.getbbox()}"
        print(f"{case}: complete canvas/editor pixels identical")
