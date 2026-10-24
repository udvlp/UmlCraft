"""Dumps the drawing operations of an Astah (Batik) SVG in Astah's logical coordinates.

Usage: python -I svg_measure.py file.svg
"""
import re
import sys
import xml.etree.ElementTree as ET

NS = '{http://www.w3.org/2000/svg}'
NUM = re.compile(r'-?\d+(?:\.\d+)?(?:[eE]-?\d+)?')


def parse_transform(t):
    m = [1, 0, 0, 1, 0, 0]
    for name, args in re.findall(r'(\w+)\(([^)]*)\)', t or ''):
        a = [float(x) for x in NUM.findall(args)]
        if name == 'matrix':
            n = a
        elif name == 'translate':
            n = [1, 0, 0, 1, a[0], a[1] if len(a) > 1 else 0]
        elif name == 'scale':
            n = [a[0], 0, 0, a[1] if len(a) > 1 else a[0], 0, 0]
        else:
            raise ValueError(name)
        m = mul(m, n)
    return m


def mul(a, b):
    return [
        a[0] * b[0] + a[2] * b[1],
        a[1] * b[0] + a[3] * b[1],
        a[0] * b[2] + a[2] * b[3],
        a[1] * b[2] + a[3] * b[3],
        a[0] * b[4] + a[2] * b[5] + a[4],
        a[1] * b[4] + a[3] * b[5] + a[5],
    ]


def style_of(el):
    s = {}
    for part in (el.get('style') or '').split(';'):
        if ':' in part:
            k, v = part.split(':', 1)
            s[k.strip()] = v.strip()
    return s


# Page transform of the diagram content: logical = (page - offset) / scale.
SCALE = 1.3333


def logical(m, x, y):
    px = m[0] * x + m[2] * y + m[4]
    py = m[1] * x + m[3] * y + m[5]
    return px, py


def walk(el, m, style, out):
    for child in el:
        tag = child.tag.replace(NS, '')
        if tag in ('defs', 'clipPath'):
            continue
        cm = mul(m, parse_transform(child.get('transform')))
        st = dict(style)
        st.update(style_of(child))
        if tag == 'g':
            walk(child, cm, st, out)
        else:
            out.append((tag, child, cm, st))


def fmt(v):
    return f'{v:.2f}'


def main(path):
    root = ET.parse(path).getroot()
    out = []
    walk(root, [1, 0, 0, 1, 0, 0], {}, out)
    # Find the diagram page transform from the first content group.
    base = None
    for tag, el, m, st in out:
        if abs(m[0] - SCALE) < 1e-3:
            base = (m[4] - m[0] * 0, m[5])
            break
    for tag, el, m, st in out:
        if abs(m[0] - SCALE) > 1e-3:
            continue
        # Express in logical coordinates: undo only the uniform page scale and offset.
        def lp(x, y):
            px, py = logical(m, x, y)
            return (px - OFF[0]) / SCALE, (py - OFF[1]) / SCALE
        sw = float(st.get('stroke-width', '1'))
        attrs = []
        for k in ('fill', 'stroke', 'stroke-dasharray', 'font-weight', 'font-style', 'stroke-linecap'):
            if k in st and st[k] not in ('none',):
                attrs.append(f'{k}={st[k]}')
        if tag == 'rect':
            x, y = float(el.get('x')), float(el.get('y'))
            w, h = float(el.get('width')), float(el.get('height'))
            a, b = lp(x, y), lp(x + w, y + h)
            kind = 'fill' if 'fill:none' not in (el.get('style') or '') else 'stroke'
            print(f'rect   {fmt(a[0])} {fmt(a[1])} {fmt(b[0]-a[0])}x{fmt(b[1]-a[1])} sw={sw} {kind} {" ".join(attrs)}')
        elif tag == 'line':
            a = lp(float(el.get('x1')), float(el.get('y1')))
            b = lp(float(el.get('x2')), float(el.get('y2')))
            print(f'line   {fmt(a[0])},{fmt(a[1])} -> {fmt(b[0])},{fmt(b[1])} sw={sw} {" ".join(attrs)}')
        elif tag == 'path':
            nums = [float(x) for x in NUM.findall(el.get('d'))]
            pts = [lp(nums[i], nums[i + 1]) for i in range(0, len(nums) - 1, 2)]
            xs, ys = [p[0] for p in pts], [p[1] for p in pts]
            d = el.get('d')
            glyphs = 'Q' in d or len(pts) > 12
            origin = lp(0, 0)
            kind = 'text ' if glyphs else 'path '
            print(f'{kind}  origin {fmt(origin[0])},{fmt(origin[1])} bbox {fmt(min(xs))},{fmt(min(ys))} .. {fmt(max(xs))},{fmt(max(ys))} sw={sw} {" ".join(attrs)}'
                  + ('' if glyphs else f' d={d[:120]}'))
        elif tag == 'polygon' or tag == 'polyline':
            print(f'{tag} {el.get("points")} sw={sw} {" ".join(attrs)}')
        elif tag in ('circle', 'ellipse'):
            cx, cy = float(el.get('cx')), float(el.get('cy'))
            rx = float(el.get('rx') or el.get('r'))
            ry = float(el.get('ry') or el.get('r'))
            c = lp(cx, cy)
            print(f'{tag} center {fmt(c[0])},{fmt(c[1])} radius {fmt(rx * m[0] / SCALE)},{fmt(ry * m[3] / SCALE)} sw={sw} {" ".join(attrs)}')
        else:
            print('other', tag)


if __name__ == '__main__':
    # Offset of logical (0, 0) on the page, from "matrix(1.3333,0,0,1.3333,dx,dy)".
    OFF = (float(sys.argv[2]), float(sys.argv[3])) if len(sys.argv) > 3 else (0.0, 0.0)
    main(sys.argv[1])
