# Draws the README's pictures, each in a light and a dark theme (`name.png` and `name-dark.png`, `.gif` likewise):
#   gains        the measured figures, one still infographic, at the top of the README
#   web, beaver  the two charts of the Gains section, as stills
#   part-*       one short loop per part of the dam, for How it works
# Run: uv run --with pillow python assets/draw.py [name ...]
# Flat infographic style: a faintly striped ground, a colour band on top, section tabs, rings, pins and leader lines.
# Everything is drawn at SS times its size and reduced, which is the anti-aliasing.
import math
import os
import sys
from functools import lru_cache

from PIL import Image, ImageDraw, ImageFont

HERE = os.path.dirname(os.path.abspath(__file__))
SS, FPS = 2, 15
FONTS = 'C:/Windows/Fonts/'

THEMES = {
    'light': dict(BG='#F3F1EC', STRIPE='#ECE9E2', BAND='#E4E0D7', INK='#2E2E30', SOFT='#55555A', MUTED='#8A877F', TRACK='#DCD7CC', CARD='#FBFAF7'),
    'dark': dict(BG='#2A2A2D', STRIPE='#2F2F32', BAND='#38383C', INK='#EDEDED', SOFT='#C9C9C9', MUTED='#8F8F94', TRACK='#424247', CARD='#323236'),
}
T = THEMES['dark']
TEAL, ORANGE, GREEN, RED, BEIGE, INDIGO, YELLOW = '#3FA7C8', '#F0A93B', '#8DB356', '#E2564B', '#B98B5E', '#7F8BD0', '#D8BE4E'
BAND_COLORS = [TEAL, RED, ORANGE, GREEN, BEIGE, INDIGO]


def use(theme):
    global T
    T = THEMES[theme]


@lru_cache(maxsize=None)
def font(size, style):
    name = {'reg': 'GOTHIC.TTF', 'bold': 'GOTHICB.TTF', 'mono': 'consola.ttf', 'monob': 'consolab.ttf'}[style]

    return ImageFont.truetype(FONTS + name, int(size * SS))


def clamp(x):
    return max(0.0, min(1.0, x))


def ease(x):
    x = clamp(x)

    return 1 - (1 - x) ** 3


def rgb(c):
    return tuple(int(c[i:i + 2], 16) for i in (1, 3, 5)) if isinstance(c, str) else c


def mix(a, b, k):
    a, b = rgb(a), rgb(b)

    return tuple(int(x + (y - x) * clamp(k)) for x, y in zip(a, b))


@lru_cache(maxsize=None)
def ground(w, h, theme):
    """The striped ground and the colour band, drawn once per size."""
    t = THEMES[theme]
    im = Image.new('RGB', (w * SS, h * SS), t['BG'])
    d = ImageDraw.Draw(im)
    for x in range(-h * SS, w * SS, 7 * SS):
        d.line([x, h * SS, x + h * SS, 0], fill=t['STRIPE'], width=SS * 2)
    seg = w / len(BAND_COLORS)
    for i, c in enumerate(BAND_COLORS):
        d.rectangle([i * seg * SS, 0, (i + 1) * seg * SS, 5 * SS], fill=c)

    return im


class Pen:
    def __init__(self, w, h, theme):
        self.w, self.h = w, h
        self.im = ground(w, h, theme).copy()
        self.d = ImageDraw.Draw(self.im)

    def k(self, *v):
        return [x * SS for x in v]

    def rect(self, x0, y0, x1, y1, fill=None, r=0, outline=None, width=1):
        if x1 - x0 <= 0 or y1 - y0 <= 0:
            return
        self.d.rounded_rectangle(self.k(x0, y0, x1, y1), radius=min(r, (x1 - x0) / 2, (y1 - y0) / 2) * SS, fill=fill, outline=outline, width=int(width * SS) if outline else 0)

    def circle(self, x, y, r, fill=None, outline=None, width=1):
        self.d.ellipse(self.k(x - r, y - r, x + r, y + r), fill=fill, outline=outline, width=int(width * SS) if outline else 0)

    def line(self, pts, fill, width=1):
        self.d.line([v * SS for p in pts for v in p], fill=fill, width=max(1, int(width * SS)), joint='curve')

    def poly(self, pts, fill):
        self.d.polygon([(x * SS, y * SS) for x, y in pts], fill=fill)

    def text(self, x, y, s, size, fill=None, style='reg', anchor='la'):
        self.d.text((x * SS, y * SS), s, font=font(size, style), fill=fill or T['INK'], anchor=anchor)

    def tw(self, s, size, style='reg'):
        return self.d.textlength(s, font=font(size, style)) / SS

    # A ring from the top, clockwise: `share` of it in `color` over the track.
    def ring(self, x, y, r, width, share, color, track=None):
        box = self.k(x - r, y - r, x + r, y + r)
        if track is not False:
            self.d.arc(box, 0, 360, fill=track or T['TRACK'], width=int(width * SS))
        if share > 0:
            self.d.arc(box, -90, -90 + 360 * min(share, 0.9999), fill=color, width=int(width * SS))
            for a in (-90, -90 + 360 * min(share, 0.9999)):        # round ends
                rad = math.radians(a)
                self.circle(x + (r - width / 2) * math.cos(rad), y + (r - width / 2) * math.sin(rad), width / 2, color)

    # A section's tab: a band from the left edge with its name in its colour, as the reference infographic labels sections.
    def tab(self, x, y, name, color, w=None):
        w = w or self.tw(name, 15, 'reg') + 44
        self.rect(x, y, x + w, y + 30, T['BAND'])
        self.text(x + w - 18, y + 15, name, 15, color, 'reg', 'rm')

    def pin(self, x, y, r, color, a=1.0):
        c = mix(T['BG'], color, a)
        self.poly([(x - r * 0.72, y - r * 0.9), (x + r * 0.72, y - r * 0.9), (x, y + r * 0.55)], c)
        self.circle(x, y - r * 1.25, r, c)
        self.circle(x, y - r * 1.25, r * 0.42, mix(T['BG'], T['CARD'], a))

    # A label at the end of a leader line: a dot, an elbow, then the text.
    def leader(self, x0, y0, x1, y1, color, title, note, a=1.0, right=True):
        c = mix(T['BG'], color, a)
        self.circle(x0, y0, 4, c)
        self.line([(x0, y0), (x1 - (12 if right else -12), y1), (x1, y1)], c, 1.2)
        self.circle(x1, y1, 2.5, c)
        anchor = 'lm' if right else 'rm'
        dx = 8 if right else -8
        self.text(x1 + dx, y1 - 8, title, 13, mix(T['BG'], T['INK'], a), 'bold', anchor)
        if note:
            self.text(x1 + dx, y1 + 9, note, 11, mix(T['BG'], T['MUTED'], a), 'reg', anchor)

    def pill(self, x, y, s, size, fill, ink, style='mono', pad=9):
        w = self.tw(s, size, style) + pad * 2
        self.rect(x, y - size * 0.95, x + w, y + size * 0.95, fill, size)
        self.text(x + pad, y + 0.5, s, size, ink, style, 'lm')

        return w

    def done(self):
        return self.im.resize((self.w, self.h), Image.LANCZOS)


def heading(pen, x, y, bold, light, size=40):
    pen.text(x, y, bold, size, T['SOFT'], 'bold', 'ls')
    pen.text(x + pen.tw(bold + ' ', size, 'bold'), y, light, size * 0.62, T['MUTED'], 'reg', 'ls')


# ---- the measured figures ------------------------------------------------------------------------

PAGES = [('GitHub', '261 KB → 3.7 KB', 99), ('Wikipedia', '778 KB → 128 KB', 84), ('Rust book', '31 KB → 6.1 KB', 81), ('Hacker News', '34 KB → 9.4 KB', 73), ('Rust blog', '99 KB → 42 KB', 58)]


def web_rings(pen, x, y, gap=118):
    for i, (name, sizes, share) in enumerate(PAGES):
        cx = x + 50 + i * gap
        pen.ring(cx, y + 50, 46, 11, share / 100, ORANGE)
        pen.text(cx, y + 51, f'{share}%', 17, T['INK'], 'bold', 'mm')
        pen.text(cx, y + 118, name, 13, T['INK'], 'bold', 'mm')
        pen.text(cx, y + 136, sizes, 10.5, T['MUTED'], 'reg', 'mm')


PONYTAIL = [('Lines of code', 47), ('Output tokens', 55), ('Cost', 74), ('Time', 59)]


# Bars of the share of the no-rules baseline that is left, an arrow on top, the baseline as a track behind.
def beaver_bars(pen, x, y, h=190, gap=86):
    for i, (name, left) in enumerate(PONYTAIL):
        bx = x + i * gap
        pen.rect(bx, y, bx + 30, y + h, T['TRACK'], 15)
        top = y + h - h * left / 100
        pen.rect(bx + 4, top + 12, bx + 26, y + h, BEIGE, 11)
        pen.poly([(bx - 2, top + 16), (bx + 15, top - 4), (bx + 32, top + 16)], BEIGE)
        pen.text(bx + 15, top - 18, f'−{100 - left}%', 14, T['INK'], 'bold', 'mm')
        pen.text(bx + 15, y + h + 18, name, 11.5, T['SOFT'], 'reg', 'mm')
    pen.line([(x - 14, y), (x + 3 * gap + 44, y)], T['MUTED'], 1)
    pen.text(x + 3 * gap + 48, y, 'baseline', 10.5, T['MUTED'], 'reg', 'lm')


def compression_rings(pen, cx, cy):
    pen.ring(cx, cy, 92, 15, 1.0, TEAL)
    pen.ring(cx, cy, 64, 15, 0.699, mix(T['TRACK'], T['MUTED'], 0.6))
    pen.text(cx, cy - 6, '100%', 20, T['INK'], 'bold', 'mm')
    pen.text(cx, cy + 15, 'lines kept', 10.5, T['MUTED'], 'reg', 'mm')
    pen.leader(cx + 70, cy - 60, cx + 150, cy - 92, TEAL, 'the dam: 100% of lines', '24.1% fewer tokens · 15 ms · 11 MB')
    pen.leader(cx + 50, cy + 28, cx + 150, cy + 60, mix(T['TRACK'], T['MUTED'], 0.8), 'Headroom: 69.9% of lines', '27.3% fewer tokens · 45 ms · about 1 GB')


def map_rows(pen, x, y, w=330):
    rows = [('graphify', 816, T['MUTED']), ('the dam', 1.5, GREEN)]
    for i, (name, secs, color) in enumerate(rows):
        yy = y + i * 40
        pen.text(x, yy, name, 12.5, T['INK'], 'bold', 'lm')
        pen.rect(x + 90, yy - 6, x + 90 + w, yy + 6, T['TRACK'], 6)
        pen.rect(x + 90, yy - 6, x + 90 + max(12, w * secs / 816), yy + 6, color, 6)
        pen.text(x + 100 + w, yy, f'{secs:g} s', 12.5, T['INK'], 'bold', 'lm')
    pen.text(x, y + 76, 'Right place named, out of 15 questions', 11.5, T['SOFT'], 'reg', 'lm')
    for i in range(15):
        px = x + 12 + i * 23
        pen.pin(px, y + 116, 6.5, GREEN)
        if i >= 12:
            pen.circle(px, y + 136, 2.5, T['MUTED'])
    pen.text(x + 360, y + 107, '15 / 15', 14, T['INK'], 'bold', 'lm')
    pen.text(x + 360, y + 125, 'graphify: 12', 10.5, T['MUTED'], 'reg', 'lm')


def gains(theme):
    W, H = 1400, 720
    pen = Pen(W, H, theme)
    heading(pen, 56, 72, 'The dam', 'Measured gains, every line kept')
    pen.tab(0, 104, 'Web pages', ORANGE, 190)
    pen.text(210, 119, 'Share of a fetched page the model no longer reads', 12.5, T['MUTED'], 'reg', 'lm')
    web_rings(pen, 56, 158)
    pen.tab(700, 104, 'Compression', TEAL, 190)
    pen.text(910, 119, '43 real tool outputs, against Headroom', 12.5, T['MUTED'], 'reg', 'lm')
    compression_rings(pen, 830, 252)
    pen.tab(0, 360, "Beaver's rules", BEIGE, 190)
    pen.text(210, 375, "Ponytail's benchmark, 39 tasks × 5 runs", 12.5, T['MUTED'], 'reg', 'lm')
    beaver_bars(pen, 92, 444, 180)
    pen.tab(700, 360, 'Map', GREEN, 190)
    pen.text(910, 375, '2,112 source files', 12.5, T['MUTED'], 'reg', 'lm')
    map_rows(pen, 756, 430)
    # TokenEconomizer's published range, as a band of a ring.
    cx, cy = 1290, 640
    pen.ring(cx, cy, 44, 10, 0.309, mix(RED, T['BG'], 0.55))
    pen.ring(cx, cy, 44, 10, 0.184, RED, track=False)
    pen.text(cx, cy - 6, '18–31%', 12.5, T['INK'], 'bold', 'mm')
    pen.text(cx, cy + 10, 'lower cost', 9, T['MUTED'], 'reg', 'mm')
    pen.text(cx - 64, cy - 12, 'TokenEconomizer', 12.5, RED, 'bold', 'rm')
    pen.text(cx - 64, cy + 6, '18.4–30.9% lower cost,', 10.5, T['MUTED'], 'reg', 'rm')
    pen.text(cx - 64, cy + 21, 'SWE-Bench, five agent–model pairs', 10.5, T['MUTED'], 'reg', 'rm')
    pen.text(56, H - 24, 'All local, in Rust  ·  github.com/T-Celor/the-dam', 11.5, T['MUTED'], 'reg', 'lm')

    return pen.done()


def web(theme):
    pen = Pen(680, 250, theme)
    pen.tab(0, 22, 'Web pages', ORANGE, 170)
    pen.text(186, 37, 'Share of a fetched page the model no longer reads', 12, T['MUTED'], 'reg', 'lm')
    web_rings(pen, 36, 78, 124)

    return pen.done()


def beaver(theme):
    pen = Pen(520, 330, theme)
    pen.tab(0, 22, "Beaver's rules", BEIGE, 170)
    pen.text(186, 37, 'What is left of the no-rules baseline', 12, T['MUTED'], 'reg', 'lm')
    beaver_bars(pen, 70, 92, 180, 92)

    return pen.done()


STILLS = {'gains': gains, 'web': web, 'beaver': beaver}


# ---- one loop per part --------------------------------------------------------------------------

GW, GH = 600, 300


def frame_of(theme, name, sub, color):
    pen = Pen(GW, GH, theme)
    pen.tab(0, 22, name, color, pen.tw(name, 15) + 60)
    pen.text(GW - 24, 37, sub, 11.5, T['MUTED'], 'reg', 'rm')

    return pen


def part_compression(pen, t):
    lines = [f'ERROR retry {n} of 4000: connection reset' for n in (1, 2, 3, 4, 5)]
    fold = ease((t - 1.4) / 0.6)
    for i, s in enumerate(lines):
        a = ease((t - i * 0.18) / 0.3) * (1 - fold)
        y = 92 + i * 26 * (1 - fold) + 52 * fold
        if a > 0:
            pen.text(40, y, s, 12.5, mix(T['BG'], T['SOFT'], a), 'mono', 'lm')
    show = ease((t - 1.8) / 0.4)
    if show:
        pen.pill(40, 130, 'ERROR retry {n} of 4000 …  ×4000', 12, mix(T['BG'], TEAL, show), mix(T['BG'], '#FFFFFF', show))
        pen.text(40, 166, 'n = 1, 2, 3 … 4000, in a table under it', 11.5, mix(T['BG'], T['MUTED'], show), 'reg', 'lm')
    pen.ring(500, 150, 52, 12, 0.241 * ease((t - 2.0) / 0.8), TEAL)
    pen.text(500, 144, f'{24.1 * ease((t - 2.0) / 0.8):.0f}%', 20, T['INK'], 'bold', 'mm')
    pen.text(500, 166, 'fewer tokens', 10, T['MUTED'], 'reg', 'mm')
    pen.text(40, GH - 30, 'Repeats folded into a template and a table. Every line can be read back.', 11.5, T['SOFT'], 'reg', 'lm')


def part_web(pen, t):
    soup = ['<nav class="menu">…</nav>', '<div class="ad">…</div>', '<h1>Beaver</h1>', '<script>track()</script>', '<p>Beavers build dams…</p>', '<footer>cookies…</footer>']
    keep = {2: '# Beaver', 4: 'Beavers build dams…'}
    sweep = clamp((t - 0.8) / 1.2)
    pen.rect(36, 70, 330, 250, T['CARD'], 10)
    for i, s in enumerate(soup):
        y = 92 + i * 27
        passed = sweep * 6 > i
        if i in keep:
            pen.text(52, y, keep[i] if passed else s, 12, ORANGE if passed else T['SOFT'], 'mono', 'lm')
        else:
            pen.text(52, y, s, 12, mix(T['SOFT'], T['CARD'], 0.85 if passed else 0), 'mono', 'lm')
    if 0 < sweep < 1:
        pen.rect(40, 80 + sweep * 162, 326, 82 + sweep * 162, ORANGE, 1)
    g = ease((t - 2.1) / 0.8)
    pen.ring(470, 150, 58, 13, 0.99 * g, ORANGE)
    pen.text(470, 144, f'{99 * g:.0f}%', 22, T['INK'], 'bold', 'mm')
    pen.text(470, 168, 'smaller', 10.5, T['MUTED'], 'reg', 'mm')
    pen.text(470, 232, '261 KB of HTML → 3.7 KB', 11.5, mix(T['BG'], T['SOFT'], g), 'reg', 'mm')
    pen.text(36, GH - 30, 'A fetched page becomes Markdown: its main content, without menus, ads or scripts.', 11.5, T['SOFT'], 'reg', 'lm')


NODES = [(80, 120), (170, 90), (150, 190), (260, 150), (340, 90), (360, 210), (450, 140), (520, 205), (540, 95)]
EDGES = [(0, 1), (0, 2), (1, 3), (2, 3), (3, 4), (3, 5), (4, 6), (5, 6), (6, 7), (6, 8)]


def part_map(pen, t):
    a = ease(t / 0.4)
    for i, j in EDGES:
        pen.line([NODES[i], NODES[j]], mix(T['BG'], T['TRACK'], a), 2)
    path = [0, 1, 3, 4, 6, 8]
    walk = clamp((t - 0.9) / 1.4) * (len(path) - 1)
    for k in range(int(walk)):
        pen.line([NODES[path[k]], NODES[path[k + 1]]], GREEN, 3)
    if walk < len(path) - 1:
        k = int(walk)
        (x0, y0), (x1, y1) = NODES[path[k]], NODES[path[k + 1]]
        f = walk - k
        pen.line([(x0, y0), (x0 + (x1 - x0) * f, y0 + (y1 - y0) * f)], GREEN, 3)
    for n, (x, y) in enumerate(NODES):
        lit = n in path[:int(walk) + 1]
        pen.circle(x, y, 7, mix(T['BG'], GREEN if lit else T['MUTED'], a))
    pen.pill(36, 70, 'where is SmartCrusher?', 11.5, T['CARD'], T['INK'])
    found = ease((t - 2.3) / 0.4)
    if found:
        x, y = NODES[8]
        pen.circle(x, y, 9, T['BG'])
        pen.pin(x, y + 6, 11 * found, GREEN)
        pen.leader(x - 14, y - 16, 420, 58, GREEN, 'crusher.rs:118', 'defined here, 3 callers', found, right=False)
    pen.text(36, GH - 30, 'A local map answers "where is it?" in a few hundred tokens.', 11.5, T['SOFT'], 'reg', 'lm')


TOOLS = [('Read', 1), ('Edit', 1), ('Bash', 1), ('Grep', 1), ('Glob', 1), ('Write', 1), ('Notebook', 0), ('Cron', 0), ('Worktree', 0), ('Teams', 0), ('Slides', 0), ('Monitor', 0)]


def part_pruning(pen, t):
    out = ease((t - 1.0) / 0.7)
    kept = 0
    for i, (name, used) in enumerate(TOOLS):
        col, row = i % 4, i // 4
        x, y = 40 + col * 118, 82 + row * 44
        if used:
            x2, y2 = 40 + (kept % 3) * 118, 82 + (kept // 3) * 44
            kept += 1
            x, y = x + (x2 - x) * ease((t - 1.6) / 0.6), y + (y2 - y) * ease((t - 1.6) / 0.6)
            pen.pill(x, y + 14, name, 12, INDIGO, '#FFFFFF', 'reg', 12)
        elif out < 1:
            c = mix(T['CARD'], T['BG'], out)
            w = pen.pill(x, y + 14, name, 12, c, mix(T['SOFT'], T['BG'], out), 'reg', 12)
            if out:
                pen.line([(x + 8, y + 14), (x + 8 + (w - 16) * out, y + 14)], mix(T['MUTED'], T['BG'], max(0, out - 0.6) * 2.5), 1.2)
    n = 12 - round(6 * ease((t - 1.0) / 1.0))
    pen.text(540, 140, str(n), 40, T['INK'], 'bold', 'mm')
    pen.text(540, 172, 'tools on every request', 10, T['MUTED'], 'reg', 'mm')
    pen.text(40, GH - 30, 'Tools never used in this repository are left out. One command brings any back.', 11.5, T['SOFT'], 'reg', 'lm')


def part_loops(pen, t):
    cx, cy, r = 170, 155, 58
    laps = clamp(t / 2.1) * 3
    pen.ring(cx, cy, r, 4, 0, RED)
    a = math.radians(-90 + 360 * (laps % 1))
    out = ease((t - 2.2) / 0.6)
    if out == 0:
        pen.circle(cx + r * math.cos(a), cy + r * math.sin(a), 9, RED)
    for k in range(min(3, int(laps) + 1)):
        pen.pill(270, 92 + k * 32, f'same error  ×{k + 1}', 12, mix(T['CARD'], RED, 0.18 + 0.3 * k), T['INK'])
    if out:
        x0, y0 = cx + r * 0.71, cy + r * 0.71
        x1, y1 = x0 + (400 - x0) * out, y0 + (226 - y0) * out
        pen.line([(x0, y0), (x1, y1)], RED, 3)
        pen.circle(x1, y1, 9, RED)
        pen.pill(416, 226, 'try another way', 12, mix(T['BG'], RED, out), mix(T['BG'], '#FFFFFF', out))
    pen.text(36, GH - 30, 'The same failure three times: a judge checks, then one nudge steers away.', 11.5, T['SOFT'], 'reg', 'lm')


def part_facts(pen, t):
    pen.line([(60, 160), (420, 160)], T['TRACK'], 3)
    for i in range(3):
        x = 90 + i * 140
        a = ease((t - i * 0.6) / 0.35)
        pen.circle(x, 160, 8, mix(T['BG'], YELLOW, a))
        pen.text(x, 186, f'session {i + 1}', 11.5, mix(T['BG'], T['SOFT'], a), 'reg', 'mm')
        pen.pin(x, 132, 8 * a, YELLOW, a)
        pen.text(x, 94, '`cargo build` fails' if i < 2 else 'confirmed', 10.5, mix(T['BG'], T['MUTED'], a), 'mono' if i < 2 else 'reg', 'mm')
    c = ease((t - 2.0) / 0.5)
    if c:
        pen.rect(452, 98, 570, 220, mix(T['BG'], T['CARD'], c), 12)
        pen.text(468, 120, 'learned fact', 11, mix(T['BG'], YELLOW, c), 'bold', 'lm')
        for k, s in enumerate(['build with', 'cargo build', '--manifest-path', 'dam/Cargo.toml']):
            pen.text(468, 146 + k * 18, s, 11, mix(T['BG'], T['INK'], c), 'mono' if k else 'reg', 'lm')
    pen.text(36, GH - 30, 'What three sessions confirm is told to the next one at its start.', 11.5, T['SOFT'], 'reg', 'lm')


def part_beaver(pen, t):
    widths = [210, 260, 180, 290, 240, 150, 270, 200, 230, 170]
    shrink = ease((t - 1.0) / 0.9)
    for i, w in enumerate(widths[1:], 1):
        a = ease((t - i * 0.06) / 0.3) * (1 - shrink)
        y = 80 + i * 16 * (1 - shrink)
        if a > 0:
            pen.rect(40, y, 40 + w, y + 10, mix(T['BG'], T['TRACK'], a), 5)
    pen.rect(40, 80, 40 + widths[0] * (1 - shrink) + 120 * shrink, 90, mix(T['BG'], BEIGE, ease(t / 0.3)), 5)
    s = ease((t - 1.9) / 0.4)
    if s:
        pen.text(40, 130, 'return items.sort()', 13, mix(T['BG'], T['INK'], s), 'mono', 'lm')
    pen.text(470, 120, '50', 34, mix(T['BG'], T['MUTED'], 1 - shrink * 0.6), 'bold', 'mm')
    pen.text(470, 150, 'lines', 11, T['MUTED'], 'reg', 'mm')
    pen.text(470, 185, '→ 1', 26, mix(T['BG'], BEIGE, s), 'bold', 'mm')
    pen.text(36, GH - 30, 'The smallest change that fully solves the task, and nothing more.', 11.5, T['SOFT'], 'reg', 'lm')


PARTS = {
    'compression': ('Compression', 'Bash · PowerShell · Grep · MCP', TEAL, part_compression),
    'web': ('Web pages', 'curl · wget · Invoke-WebRequest', ORANGE, part_web),
    'map': ('Map', 'code · documents · settings', GREEN, part_map),
    'pruning': ('Tool pruning', 'after the repository is idle', INDIGO, part_pruning),
    'loops': ('Loop guard', 'within a turn', RED, part_loops),
    'facts': ('Learned facts', 'at session start', YELLOW, part_facts),
    'beaver': ('Beaver', 'lazy senior dev rules', BEIGE, part_beaver),
}
LOOP_S, HOLD_MS = 3.2, 2200


def gif(theme, key):
    name, sub, color, draw = PARTS[key]
    frames = []
    for k in range(int(LOOP_S * FPS)):
        pen = frame_of(theme, name, sub, color)
        draw(pen, k / FPS)
        frames.append(pen.done().quantize(colors=128, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE))
    durations = [1000 // FPS] * len(frames)
    durations[-1] = HOLD_MS

    return frames, durations


def main(names):
    for theme in THEMES:
        use(theme)
        suffix = '' if theme == 'light' else '-dark'
        for name in names:
            if name in STILLS:
                path = os.path.join(HERE, f'{name}{suffix}.png')
                STILLS[name](theme).save(path, optimize=True)
            else:
                frames, durations = gif(theme, name.removeprefix('part-'))
                path = os.path.join(HERE, f'{name}{suffix}.gif')
                frames[0].save(path, save_all=True, append_images=frames[1:], duration=durations, loop=0, optimize=True)
            print(path, os.path.getsize(path) // 1024, 'KB')


if __name__ == '__main__':
    main(sys.argv[1:] or [*STILLS, *(f'part-{key}' for key in PARTS)])
