"""The beaver icon, as SVG and as a 1024 px PNG, from one list of shapes. Standard library only."""
import os, struct, sys, zlib

OUT = sys.argv[1]
os.makedirs(OUT, exist_ok=True)
SIZE = 512
# (kind, fill, parameters) back to front, on a 512 grid.
BG, WATER, DARK, FUR, LIGHT, MUZZLE, NOSE, WHITE, INK = '#0E3A53', '#1B6E8F', '#5B3A21', '#8A5A34', '#A9743F', '#E3C39B', '#2B1A10', '#FFFFFF', '#21140C'
SHAPES = [
    ('rrect', BG, (0, 0, 512, 512, 112)),
    ('wave', WATER, (0, 392, 512, 120)),
    ('circle', DARK, (150, 138, 44)),      # ears
    ('circle', DARK, (362, 138, 44)),
    ('circle', MUZZLE, (150, 138, 22)),
    ('circle', MUZZLE, (362, 138, 22)),
    ('ellipse', FUR, (256, 262, 158, 168)),   # head
    ('ellipse', LIGHT, (256, 300, 118, 112)),  # face
    ('circle', WHITE, (194, 226, 32)),     # eyes
    ('circle', WHITE, (318, 226, 32)),
    ('circle', INK, (200, 230, 18)),
    ('circle', INK, (312, 230, 18)),
    ('circle', WHITE, (206, 223, 6)),
    ('circle', WHITE, (318, 223, 6)),
    ('ellipse', MUZZLE, (256, 322, 92, 66)),   # muzzle
    ('ellipse', NOSE, (256, 284, 34, 24)),     # nose
    ('rrect', WHITE, (224, 346, 30, 64, 8)),   # teeth
    ('rrect', WHITE, (258, 346, 30, 64, 8)),
]


def wave_y(x, y):
    import math
    return y + 12 * math.sin(x / 512 * 2 * math.pi * 2)


def inside(kind, p, x, y):
    if kind == 'circle':
        return (x - p[0]) ** 2 + (y - p[1]) ** 2 <= p[2] ** 2
    if kind == 'ellipse':
        return ((x - p[0]) / p[2]) ** 2 + ((y - p[1]) / p[3]) ** 2 <= 1
    if kind == 'rrect':
        x0, y0, w, h, r = p
        dx = max(x0 + r - x, 0, x - (x0 + w - r))
        dy = max(y0 + r - y, 0, y - (y0 + h - r))
        return dx * dx + dy * dy <= r * r and x0 <= x <= x0 + w and y0 <= y <= y0 + h
    if kind == 'wave':
        return y >= wave_y(x, p[1]) and inside('rrect', (0, 0, 512, 512, 112), x, y)
    return False


def rgb(h):
    return tuple(int(h[i:i + 2], 16) for i in (1, 3, 5))


PIXELS, SAMPLES = 1024, 3
raw = bytearray()
for py in range(PIXELS):
    raw.append(0)
    for px in range(PIXELS):
        total = [0, 0, 0, 0]
        for sy in range(SAMPLES):
            for sx in range(SAMPLES):
                x = (px + (sx + .5) / SAMPLES) * SIZE / PIXELS
                y = (py + (sy + .5) / SAMPLES) * SIZE / PIXELS
                color = None
                for kind, fill, p in SHAPES:
                    if inside(kind, p, x, y):
                        color = fill
                if color:
                    r, g, b = rgb(color)
                    total[0] += r; total[1] += g; total[2] += b; total[3] += 1
        n = total[3]
        if n:
            raw += bytes((total[0] // n, total[1] // n, total[2] // n, 255 * n // SAMPLES ** 2))
        else:
            raw += bytes((0, 0, 0, 0))


def chunk(tag, data):
    return struct.pack('>I', len(data)) + tag + data + struct.pack('>I', zlib.crc32(tag + data))


png = b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', PIXELS, PIXELS, 8, 6, 0, 0, 0)) + chunk(b'IDAT', zlib.compress(bytes(raw), 9)) + chunk(b'IEND', b'')
open(OUT + '/icon.png', 'wb').write(png)

parts = []
for kind, fill, p in SHAPES:
    if kind == 'circle':
        parts.append(f'<circle cx="{p[0]}" cy="{p[1]}" r="{p[2]}" fill="{fill}"/>')
    elif kind == 'ellipse':
        parts.append(f'<ellipse cx="{p[0]}" cy="{p[1]}" rx="{p[2]}" ry="{p[3]}" fill="{fill}"/>')
    elif kind == 'rrect':
        parts.append(f'<rect x="{p[0]}" y="{p[1]}" width="{p[2]}" height="{p[3]}" rx="{p[4]}" fill="{fill}"/>')
    elif kind == 'wave':
        parts.append(f'<path d="M0 392 q64 -24 128 0 t128 0 t128 0 t128 0 V400 A112 112 0 0 1 400 512 H112 A112 112 0 0 1 0 400 Z" fill="{fill}"/>')
open(OUT + '/icon.svg', 'w', encoding='utf-8', newline='\n').write(f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512" role="img" aria-label="The dam: a beaver">{"".join(parts)}</svg>\n')
