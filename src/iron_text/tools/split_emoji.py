"""Split a COLR emoji font into chunks loaded on demand (plans/specs/text.md §5.1).

    uv run --with fonttools python -I src/iron_text/tools/split_emoji.py \
        path/to/Noto-COLRv1.ttf public/fonts/emoji

Each codepoint gets one home chunk, cut from codepoint order into chunks of
about equal weight, so neighbours (the faces, the animals) load together.
Base chunks hold single emoji only. Every sequence of more than one
emoji codepoint (a ZWJ family or gendered person, a skin tone, a flag, a
tag flag) goes to a *sequence* chunk keyed by its first codepoint, with the
components it needs, fetched only when a text actually joins something to
that emoji. Most of the font's weight is in sequences (each a full drawing),
so a plain smiley costs a few hundred KB. Base chunks leave out ZWJ, the
tone modifiers, tags and regional indicators, so the subsetter cannot pull a
sequence's ligature into them.

Each chunk is its own family ("Noto Color Emoji 01", ...): parley picks one
font per family, so chunks sharing a name would hide each other's glyphs.
`chunks.json` beside them lists each chunk's family, file and home ranges;
`iron_text` reads it to ask for the chunk an emoji lives in.
"""

import json
import os
import sys

from fontTools import subset
from fontTools.ttLib import TTFont

CHUNKS = 10
SEQ_CHUNKS = 8
# In every chunk: presentation selector and keycap.
ALWAYS = {0xFE0F, 0x20E3}
# Only in sequence chunks: what joins emoji into sequences.
JOINERS = {0x200D} | set(range(0x1F3FB, 0x1F400)) | set(range(0xE0020, 0xE0080))
REGIONAL = set(range(0x1F1E6, 0x1F200))


def sequences(font):
    """Every ligature, as the codepoints of its components (glyphs mapped
    back through the cmap; components with no codepoint are skipped)."""
    rev = {}
    for cp, g in font.getBestCmap().items():
        rev.setdefault(g, cp)
    out = []
    for lookup in font["GSUB"].table.LookupList.Lookup:
        for st in lookup.SubTable:
            st = getattr(st, "ExtSubTable", st)
            for first, ligs in getattr(st, "ligatures", {}).items():
                for lig in ligs:
                    cps = [rev.get(g) for g in [first] + list(lig.Component)]
                    cps = [c for c in cps if c is not None]
                    if cps:
                        out.append(cps)
    return out


def ranges(cps):
    out = []
    for c in sorted(cps):
        if out and out[-1][1] == c - 1:
            out[-1][1] = c
        else:
            out.append([c, c])
    return out


def cut(order, weight, n):
    """`order` cut into at most `n` runs of about equal total weight."""
    total = sum(weight[c] for c in order) or 1
    out, acc, cur = [], 0, []
    for c in order:
        cur.append(c)
        acc += weight[c]
        if acc >= total * (len(out) + 1) / n and len(out) < n - 1:
            out.append(cur)
            cur = []
    if cur:
        out.append(cur)
    return out


def write(src, dst, name, family, unicodes):
    opts = subset.Options()
    opts.layout_features = ["*"]
    opts.name_IDs = ["*"]
    opts.notdef_outline = True
    f = TTFont(src)
    sub = subset.Subsetter(opts)
    sub.populate(unicodes=sorted(unicodes))
    sub.subset(f)
    for rec in f["name"].names:
        if rec.nameID in (1, 4, 16):
            rec.string = family
        elif rec.nameID == 6:
            rec.string = family.replace(" ", "")
    path = os.path.join(dst, name)
    f.save(path)
    return os.path.getsize(path)


def main(src, dst):
    os.makedirs(dst, exist_ok=True)
    for old in os.listdir(dst):
        if old.startswith("noto-emoji-"):
            os.remove(os.path.join(dst, old))
    font = TTFont(src)
    cmap = font.getBestCmap()
    singles = sorted(c for c in cmap if c not in ALWAYS and c not in JOINERS and c not in REGIONAL and c > 0x7F)
    heads = set(singles) | REGIONAL
    seq_needs, seq_weight = {}, {}
    for seq in sequences(font):
        parts = [c for c in seq if c not in ALWAYS]
        if len(parts) < 2:
            continue
        head = next((c for c in parts if c in heads), None)
        if head is None:
            continue
        seq_weight[head] = seq_weight.get(head, 0) + 1
        seq_needs.setdefault(head, set()).update(c for c in parts if c != head)
    family0 = font["name"].getDebugName(1)
    manifest = {"source": os.path.basename(src), "chunks": [], "sequences": []}
    weight = {c: 1 for c in singles}
    for i, home in enumerate(cut(singles, weight, CHUNKS), 1):
        family, file = f"{family0} {i:02d}", f"noto-emoji-{i:02d}.ttf"
        unicodes = (set(home) | ALWAYS) & set(cmap)
        size = write(src, dst, file, family, unicodes)
        manifest["chunks"].append({"family": family, "file": f"emoji/{file}", "ranges": ranges(home), "bytes": size})
        print(f"{file}: {len(home)} emoji, {size // 1024} KB")
    # Sequence chunks by weight. Person sequences share body and hand
    # outlines across emoji, so splitting them finer duplicates those
    # drawings into every piece (18 MB in all, tried); a few larger chunks
    # keep the sharing, and they load only when such a sequence is typed.
    for i, home in enumerate(cut(sorted(seq_weight), seq_weight, SEQ_CHUNKS), 1):
        family, file = f"{family0} Seq {i:02d}", f"noto-emoji-seq-{i:02d}.ttf"
        unicodes = (set(home) | ALWAYS | JOINERS | set().union(*(seq_needs[c] for c in home))) & set(cmap)
        size = write(src, dst, file, family, unicodes)
        manifest["sequences"].append({"family": family, "file": f"emoji/{file}", "ranges": ranges(home), "bytes": size})
        print(f"{file}: sequences of {len(home)} emoji ({sum(seq_weight[c] for c in home)}), {size // 1024} KB")
    with open(os.path.join(dst, "chunks.json"), "w") as out:
        json.dump(manifest, out, separators=(",", ":"))


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2])
