"""Overlays UmlCraft's SVG export of a diagram on Astah's, to see where they differ.

Usage: python -I svg_compare.py astah.svg umlcraft.svg out-prefix

Writes out-prefix-overlay.png (Astah in red, UmlCraft in blue, both in black) and
out-prefix-side.png (Astah left, UmlCraft right). Both exports leave a margin of 10 around the
drawing at 96 dpi, so their top left corners line up when both draw the same. Needs Inkscape and
Pillow.
"""
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image, ImageChops

INKSCAPE = r'C:\Program Files\Inkscape\bin\inkscape.com'


def render(svg, png):
    subprocess.run([INKSCAPE, str(svg), '--export-type=png', '--export-dpi=96',
                    '--export-background=white', '--export-background-opacity=1',
                    f'--export-filename={png}'], check=True, capture_output=True)
    return Image.open(png).convert('L')


def pad(image, size):
    canvas = Image.new('L', size, 255)
    canvas.paste(image, (0, 0))
    return canvas


def main(astah, ours, prefix):
    with tempfile.TemporaryDirectory() as tmp:
        a = render(astah, Path(tmp) / 'astah.png')
        b = render(ours, Path(tmp) / 'ours.png')
    size = (max(a.width, b.width), max(a.height, b.height))
    a, b = pad(a, size), pad(b, size)
    # Ink is dark: where only Astah draws, red stays; where only UmlCraft draws, blue stays.
    Image.merge('RGB', (b, ImageChops.multiply(a, b), a)).save(f'{prefix}-overlay.png')
    side = Image.new('L', (size[0] * 2 + 10, size[1]), 255)
    side.paste(a, (0, 0))
    side.paste(b, (size[0] + 10, 0))
    side.save(f'{prefix}-side.png')


if __name__ == '__main__':
    main(*sys.argv[1:4])
