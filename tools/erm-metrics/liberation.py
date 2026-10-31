#!/usr/bin/env python3
"""Writes the text metrics of ER diagrams: Liberation Sans Regular and Bold advances and
kerning pairs, in font units, to crates/umlcraft-erm/src/metrics/liberation-sans.txt.

    python3 tools/erm-metrics/liberation.py [FONT_DIR]

Needs fontTools (pip install fonttools). FONT_DIR defaults to Fedora's
/usr/share/fonts/liberation-sans-fonts. Liberation Sans is metric-compatible with Arial and
Helvetica, which draw.io names; the browser draw.io runs in applies the pairs of the `kern`
table (checked against draw.io exports: "AVAVAVAVAV" is 72 px wide at 12 px, 80 without).
Liberation Sans is licensed under the SIL Open Font License 1.1; only its widths are used.
"""

import os
import sys

from fontTools.ttLib import TTFont

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(ROOT, "crates", "umlcraft-erm", "src", "metrics", "liberation-sans.txt")
# Latin-1 plus what names and labels commonly hold.
EXTRA = "–—‘’‚“”„…€•→←↔≤≥≠×÷−"


def face(path):
    font = TTFont(path)
    cmap = font.getBestCmap()
    hmtx = font["hmtx"]
    chars = [chr(c) for c in range(0x20, 0x100)] + list(EXTRA)
    advances = {}
    for ch in chars:
        glyph = cmap.get(ord(ch))
        if glyph is not None:
            advances[ch] = hmtx[glyph][0]
    glyph_char = {cmap[ord(ch)]: ch for ch in advances}
    pairs = {}
    for table in font["kern"].kernTables if "kern" in font else []:
        for (left, right), value in table.kernTable.items():
            if value and left in glyph_char and right in glyph_char:
                pairs[(glyph_char[left], glyph_char[right])] = value
    return font, advances, pairs


def main():
    folder = sys.argv[1] if len(sys.argv) > 1 else "/usr/share/fonts/liberation-sans-fonts"
    lines = [
        "# Liberation Sans advances and kerning pairs in font units, written by",
        "# tools/erm-metrics/liberation.py. Lines: `units`, `ascent`, `descent`, then per face",
        "# `<face> <hex code point> <advance>` and `<face> kern <hex> <hex> <value>`.",
    ]
    first = True
    for name, file in (("regular", "LiberationSans-Regular.ttf"),
                       ("bold", "LiberationSans-Bold.ttf")):
        font, advances, pairs = face(os.path.join(folder, file))
        if first:
            lines.insert(3, f"# {font['name'].getDebugName(4)}, {font['name'].getDebugName(5)}")
            lines.append(f"units {font['head'].unitsPerEm}")
            lines.append(f"ascent {font['hhea'].ascent}")
            lines.append(f"descent {-font['hhea'].descent}")
            first = False
        for ch, adv in sorted(advances.items(), key=lambda kv: ord(kv[0])):
            lines.append(f"{name} {ord(ch):04x} {adv}")
        for (a, b), v in sorted(pairs.items(), key=lambda kv: (ord(kv[0][0]), ord(kv[0][1]))):
            lines.append(f"{name} kern {ord(a):04x} {ord(b):04x} {v}")
    with open(OUT, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(lines) + "\n")
    print(f"{OUT}: {len(lines)} lines")


if __name__ == "__main__":
    main()
