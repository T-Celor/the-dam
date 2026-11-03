# Draws the README's pictures, each in a light and a dark theme (`name.png` and `name-dark.png`, `.gif` likewise):
#   gains        the measured figures, one still infographic, at the top of the README
#   part-*       one short loop per part of the dam, for How it works
# Run: uv run --with pillow python assets/draw.py [name ...]
# Flat infographic style: a faintly striped ground with water, a log dam and its beaver, section tabs, rings, pins and leader lines.
# Everything is drawn at SS times its size and reduced, which is the anti-aliasing.
import math
import os
import sys
from functools import lru_cache

from PIL import Image, ImageDraw, ImageFont

HERE = os.path.dirname(os.path.abspath(__file__))
SS, OUT, FPS = 2, 1, 15
# Every picture comes out 3840 pixels wide (4K), drawn at 1.5 to 2 times that and reduced.
STILL_WIDTH = 3840


def set_scale(out):
    global SS, OUT
    OUT = out
    SS = out * (2 if out < 3 else 1.5)
    font.cache_clear()
    ground.cache_clear()
FONTS = 'C:/Windows/Fonts/'

THEMES = {
    'light': dict(BG='#F3F1EC', STRIPE='#ECE9E2', BAND='#E4E0D7', INK='#2E2E30', SOFT='#55555A', MUTED='#8A877F', TRACK='#DCD7CC', CARD='#FBFAF7',
                  WATER='#C6DFE6', DEEP='#A3CCDA', RIPPLE='#E6F2F5', LEAF='#A9C48A', REED='#9DB884'),
    'dark': dict(BG='#2A2A2D', STRIPE='#2F2F32', BAND='#38383C', INK='#EDEDED', SOFT='#C9C9C9', MUTED='#8F8F94', TRACK='#424247', CARD='#323236',
                 WATER='#25404A', DEEP='#2C5262', RIPPLE='#3D6B7B', LEAF='#5E7A48', REED='#56703F'),
}
T = THEMES['dark']
TEAL, ORANGE, GREEN, RED, BEIGE, INDIGO, YELLOW = '#3FA7C8', '#F0A93B', '#8DB356', '#E2564B', '#B98B5E', '#7F8BD0', '#D8BE4E'


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


WATER_H = 34


def beaver_on_logs(d, x, y, s):
    """A small beaver sitting on the end of a log dam, its feet at (x, y), `s` times its 60-point height; drawn as icon.svg's beaver."""
    e = lambda cx, cy, rx, ry, c: d.ellipse([(x + cx * s) * SS - rx * s * SS, (y + cy * s) * SS - ry * s * SS, (x + cx * s) * SS + rx * s * SS, (y + cy * s) * SS + ry * s * SS], fill=c)
    # The dam: log ends in a pyramid, then the beaver on top.
    for row, count in enumerate((4, 3, 2)):
        for k in range(count):
            cx, cy = (k - (count - 1) / 2) * 15, 34 - row * 13
            e(cx, cy, 8, 8, '#7A4E2C')
            e(cx, cy, 5.2, 5.2, '#C9A27E')
            e(cx, cy, 2, 2, '#8A5A34')
    # The tail, flat and crossed, behind.
    d.rounded_rectangle([(x + 6 * s) * SS, (y - 14 * s) * SS, (x + 30 * s) * SS, (y - 4 * s) * SS], radius=5 * s * SS, fill='#4A2F1B')
    for k in range(4):
        d.line([(x + (10 + k * 5) * s) * SS, (y - 13 * s) * SS, (x + (12 + k * 5) * s) * SS, (y - 5 * s) * SS], fill='#6B4428', width=max(1, int(s * SS)))
    e(0, -16, 15, 15, '#8A5A34')                       # body
    e(0, -12, 10, 10, '#A9743F')
    for cx in (-9, 9):
        e(cx, -40, 4.5, 4.5, '#5B3A21')                # ears
    e(0, -33, 13, 12, '#8A5A34')                       # head
    for cx in (-5, 5):
        e(cx, -36, 3.4, 3.4, '#FFFFFF')
        e(cx + 0.4, -35.6, 1.9, 1.9, '#21140C')
    e(0, -28, 7, 5, '#E3C39B')                         # muzzle
    e(0, -30.5, 2.6, 1.9, '#2B1A10')
    d.rectangle([(x - 2.2 * s) * SS, (y - 27 * s) * SS, (x + 2.2 * s) * SS, (y - 22.5 * s) * SS], fill='#FFFFFF')
    d.line([x * SS, (y - 27 * s) * SS, x * SS, (y - 22.5 * s) * SS], fill='#E3C39B', width=max(1, int(0.6 * s * SS)))
    for cx in (-7, 7):
        e(cx, -2, 4.5, 2.6, '#5B3A21')                  # feet


def leaf(d, x, y, length, angle, color, vein):
    a = math.radians(angle)
    ux, uy, nx, ny = math.cos(a), math.sin(a), -math.sin(a), math.cos(a)
    pts = []
    for k in range(0, 21):
        f = k / 20
        wdt = math.sin(math.pi * f) * length * 0.22
        pts.append((x + ux * length * f + nx * wdt, y + uy * length * f + ny * wdt))
    for k in range(20, -1, -1):
        f = k / 20
        wdt = math.sin(math.pi * f) * length * 0.22
        pts.append((x + ux * length * f - nx * wdt, y + uy * length * f - ny * wdt))
    d.polygon([(px * SS, py * SS) for px, py in pts], fill=color)
    d.line([x * SS, y * SS, (x + ux * length * 0.9) * SS, (y + uy * length * 0.9) * SS], fill=vein, width=round(SS))


def nature(d, w, h, t, beaver_scale, branch):
    """Water along the bottom, reeds on the left, a log dam with its beaver on the right, and a leafy branch if asked."""
    base = h - WATER_H
    for color, top, amp, period, phase in ((t['WATER'], base, 3.5, 26, 0), (t['DEEP'], base + 12, 3, 19, 1.3)):
        pts = [(x, top + amp * math.sin(x / period + phase)) for x in range(0, w + 4, 4)] + [(w, h), (0, h)]
        d.polygon([(px * SS, py * SS) for px, py in pts], fill=color)
    for x0 in range(30, w - 160, 90):
        d.line([x0 * SS, (base + 20) * SS, (x0 + 18) * SS, (base + 20) * SS], fill=t['RIPPLE'], width=round(SS * 2))
    reeds = ((10, 46, -4), (18, 60, 2), (26, 40, 6), (34, 52, 9)) if h > 400 else ()   # a short loop's caption starts where they would stand
    for k, (dx, hgt, lean) in enumerate(reeds):
        d.line([dx * SS, (base + 8) * SS, (dx + lean) * SS, (base + 8 - hgt) * SS], fill=t['REED'], width=round(SS * 2))
        if k % 2:
            d.ellipse([(dx + lean - 3) * SS, (base + 8 - hgt) * SS, (dx + lean + 3) * SS, (base + 22 - hgt) * SS], fill='#8A5A34')
    if beaver_scale:
        beaver_on_logs(d, w - 40 * beaver_scale - 16, base + 6 - 34 * beaver_scale, beaver_scale)
    if branch:
        d.line([w * SS, 12 * SS, (w - 70) * SS, 30 * SS, (w - 150) * SS, 26 * SS], fill='#7A5A3A', width=round(SS * 3), joint='curve')
        for k, (lx, ly, ang) in enumerate(((w - 30, 20, 120), (w - 60, 28, 70), (w - 90, 30, 115), (w - 120, 27, 60), (w - 148, 26, 160))):
            leaf(d, lx, ly, 26 if k % 2 else 30, ang, t['LEAF'], mix(t['LEAF'], t['BG'], 0.4))


@lru_cache(maxsize=None)
def ground(w, h, theme, beaver_scale=1.0, branch=False):
    """The striped ground and the nature around, drawn once per size."""
    t = THEMES[theme]
    im = Image.new('RGB', (round(w * SS), round(h * SS)), t['BG'])
    d = ImageDraw.Draw(im)
    for x in range(-round(h * SS), round(w * SS), round(7 * SS)):
        d.line([x, h * SS, x + h * SS, 0], fill=t['STRIPE'], width=round(SS * 2))
    nature(d, w, h, t, beaver_scale, branch)

    return im


class Pen:
    def __init__(self, w, h, theme, beaver_scale=1.0, branch=False):
        self.w, self.h = w, h
        self.im = ground(w, h, theme, beaver_scale, branch).copy()
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
        return self.im.resize((round(self.w * OUT), round(self.h * OUT)), Image.LANCZOS)


def heading(pen, x, y, bold, light, size=40):
    pen.text(x, y, bold, size, T['SOFT'], 'bold', 'ls')
    pen.text(x + pen.tw(bold + ' ', size, 'bold'), y, light, size * 0.62, T['MUTED'], 'reg', 'ls')


# ---- the measured figures ------------------------------------------------------------------------

PAGES = [('GitHub', '261 KB → 3.7 KB', 99), ('Wikipedia', '778 KB → 128 KB', 84), ('Rust book', '31 KB → 6.1 KB', 81), ('Hacker News', '34 KB → 9.4 KB', 73), ('Rust blog', '99 KB → 42 KB', 58)]


# `g`, from 0 to 1, is how far a section has drawn itself in, for the animated version; each item follows a little later.
def stagger(g, i, n):
    return ease(g * (1 + 0.15 * (n - 1)) - 0.15 * i)


def web_rings(pen, x, y, gap=118, g=1.0):
    for i, (name, sizes, share) in enumerate(PAGES):
        cx = x + 50 + i * gap
        gi = stagger(g, i, len(PAGES))
        pen.ring(cx, y + 50, 46, 11, share / 100 * gi, ORANGE)
        pen.text(cx, y + 51, f'{share * gi:.0f}%', 17, T['INK'], 'bold', 'mm')
        pen.text(cx, y + 118, name, 13, T['INK'], 'bold', 'mm')
        pen.text(cx, y + 136, sizes, 10.5, T['MUTED'], 'reg', 'mm')


PONYTAIL = [('Lines of code', 47), ('Output tokens', 55), ('Cost', 74), ('Time', 59)]


# Bars of the share of the no-rules baseline that is left, an arrow on top, the baseline as a track behind.
# Animated, each bar starts at the baseline and is cut down to what is left.
def beaver_bars(pen, x, y, h=190, gap=86, g=1.0):
    for i, (name, kept) in enumerate(PONYTAIL):
        bx = x + i * gap
        left = 100 - (100 - kept) * stagger(g, i, len(PONYTAIL))
        pen.rect(bx, y, bx + 30, y + h, T['TRACK'], 15)
        top = y + h - h * left / 100
        pen.rect(bx + 4, top + 12, bx + 26, y + h, BEIGE, 11)
        pen.poly([(bx - 2, top + 16), (bx + 15, top - 4), (bx + 32, top + 16)], BEIGE)
        if left < 99.5:
            pen.text(bx + 15, top - 18, f'−{100 - left:.0f}%', 14, T['INK'], 'bold', 'mm')
        pen.text(bx + 15, y + h + 18, name, 11.5, T['SOFT'], 'reg', 'mm')
    pen.line([(x - 14, y), (x + 3 * gap + 44, y)], T['MUTED'], 1)
    pen.text(x + 3 * gap + 48, y, 'baseline', 10.5, T['MUTED'], 'reg', 'lm')


def compression_rings(pen, cx, cy, g=1.0):
    pen.ring(cx, cy, 92, 15, ease(g * 1.2), TEAL)
    pen.ring(cx, cy, 64, 15, 0.699 * ease(g * 1.2 - 0.2), mix(T['TRACK'], T['MUTED'], 0.6))
    pen.text(cx, cy - 6, f'{100 * ease(g * 1.2):.0f}%', 20, T['INK'], 'bold', 'mm')
    pen.text(cx, cy + 15, 'lines kept', 10.5, T['MUTED'], 'reg', 'mm')
    pen.leader(cx + 70, cy - 60, cx + 150, cy - 92, TEAL, 'the dam: 100% of lines', '24.1% fewer tokens · 15 ms · 11 MB', ease(g * 2 - 0.8))
    pen.leader(cx + 50, cy + 28, cx + 150, cy + 60, mix(T['TRACK'], T['MUTED'], 0.8), 'Headroom: 69.9% of lines', '27.3% fewer tokens · 45 ms · about 1 GB', ease(g * 2 - 1))


# Animated, the two builds run as a race: the dam is done at once, graphify keeps going.
def map_rows(pen, x, y, w=330, g=1.0):
    rows = [('graphify', 816, T['MUTED']), ('the dam', 1.5, GREEN)]
    for i, (name, secs, color) in enumerate(rows):
        yy = y + i * 40
        run = ease(g * 1.4) if i == 0 else ease(g * 6)
        pen.text(x, yy, name, 12.5, T['INK'], 'bold', 'lm')
        pen.rect(x + 90, yy - 6, x + 90 + w, yy + 6, T['TRACK'], 6)
        pen.rect(x + 90, yy - 6, x + 90 + max(12, w * secs / 816 * run), yy + 6, color, 6)
        pen.text(x + 100 + w, yy, f'{secs * run:.0f} s' if secs > 10 else f'{secs * run:.1f} s', 12.5, T['INK'], 'bold', 'lm')
    pen.text(x, y + 76, 'Right place named, out of 15 questions', 11.5, T['SOFT'], 'reg', 'lm')
    for i in range(round(15 * ease(g * 1.3 - 0.2))):
        px = x + 12 + i * 23
        pen.pin(px, y + 116, 6.5, GREEN)
        if i >= 12:
            pen.circle(px, y + 136, 2.5, T['MUTED'])
    pen.text(x + 360, y + 107, '15 / 15', 14, T['INK'], 'bold', 'lm')
    pen.text(x + 360, y + 125, 'graphify: 12', 10.5, T['MUTED'], 'reg', 'lm')


# Each section's start, in seconds, in the animated version.
STARTS = dict(web=0.3, compression=0.9, beaver=1.6, map=2.2, economizer=3.0)
SECTION_S, GAINS_S, GAINS_HOLD_MS = 1.6, 5.0, 5000


def gains(theme, t=None):
    g = {k: 1.0 if t is None else clamp((t - v) / SECTION_S) for k, v in STARTS.items()}
    W, H = 1400, 780
    pen = Pen(W, H, theme, 1.7, True)
    heading(pen, 56, 72, 'The dam', 'Measured gains, every line kept')
    pen.tab(0, 104, 'Web pages', ORANGE, 190)
    pen.text(210, 119, 'Share of a fetched page the model no longer reads', 12.5, T['MUTED'], 'reg', 'lm')
    web_rings(pen, 56, 158, g=g['web'])
    pen.tab(700, 104, 'Compression', TEAL, 190)
    pen.text(910, 119, '43 real tool outputs, against Headroom', 12.5, T['MUTED'], 'reg', 'lm')
    compression_rings(pen, 830, 252, g['compression'])
    pen.tab(0, 360, "Beaver's rules", BEIGE, 190)
    pen.text(210, 375, "Ponytail's benchmark, 39 tasks × 5 runs", 12.5, T['MUTED'], 'reg', 'lm')
    beaver_bars(pen, 92, 444, 180, g=g['beaver'])
    pen.tab(700, 360, 'Map', GREEN, 190)
    pen.text(910, 375, '2,112 source files', 12.5, T['MUTED'], 'reg', 'lm')
    map_rows(pen, 756, 430, g=g['map'])
    # TokenEconomizer's published range, as a band of a ring.
    cx, cy = 1120, 640
    pen.ring(cx, cy, 44, 10, 0.309 * ease(g['economizer']), mix(RED, T['BG'], 0.55))
    pen.ring(cx, cy, 44, 10, 0.184 * ease(g['economizer']), RED, track=False)
    pen.text(cx, cy - 6, '18–31%', 12.5, T['INK'], 'bold', 'mm')
    pen.text(cx, cy + 10, 'lower cost', 9, T['MUTED'], 'reg', 'mm')
    pen.text(cx - 64, cy - 12, 'TokenEconomizer', 12.5, RED, 'bold', 'rm')
    pen.text(cx - 64, cy + 6, '18.4–30.9% lower cost,', 10.5, T['MUTED'], 'reg', 'rm')
    pen.text(cx - 64, cy + 21, 'SWE-Bench, five agent–model pairs', 10.5, T['MUTED'], 'reg', 'rm')
    pen.text(56, H - 64, 'All local, in Rust  ·  github.com/T-Celor/the-dam', 11.5, T['MUTED'], 'reg', 'lm')

    return pen.done()



STILLS = {'gains': gains}


# ---- the opening: the plugin in a few seconds ----------------------------------------------------

INTRO_W, INTRO_H, INTRO_S, INTRO_HOLD_MS = 960, 540, 7.4, 3500
CARDS = [('Compression', TEAL, '−24% tokens', 'every line kept'), ('Web pages', ORANGE, '−99% a page', 'HTML to Markdown'),
         ('Map', GREEN, '1.5 s to map', '2,112 files'), ('Tool pruning', INDIGO, 'fewer tools', 'on every request'),
         ('Loop guard', RED, 'one nudge', 'out of a loop'), ('Learned facts', YELLOW, 'remembered', 'across sessions'),
         ('Beaver', BEIGE, '−45% output', 'smallest change')]
COMMAND = '/plugin install the-dam@the-dam'


def intro(theme, t):
    pen = Pen(INTRO_W, INTRO_H, theme, 0)
    a = ease(t / 0.8)
    if a:
        beaver_on_logs(pen.d, 150, 196 + 14 * (1 - a), 2.5 * (0.85 + 0.15 * a))
    b = ease((t - 0.4) / 0.7)
    pen.text(262, 150, 'The dam', 62, mix(T['BG'], T['INK'], b), 'bold', 'ls')
    pen.text(264, 192, 'Fewer tokens in Claude Code. Every line kept.', 20, mix(T['BG'], T['SOFT'], b), 'reg', 'ls')
    pen.text(264, 222, 'A Claude Code plugin  ·  Rust  ·  100% local', 13, mix(T['BG'], T['MUTED'], b), 'reg', 'ls')
    w, gap = 122, 9
    x0 = (INTRO_W - (7 * w + 6 * gap)) / 2
    for i, (name, color, big, small) in enumerate(CARDS):
        c = ease((t - 1.6 - i * 0.32) / 0.4)
        if c <= 0:
            continue
        x, y = x0 + i * (w + gap), 334 + 10 * (1 - c)
        pen.rect(x, y, x + w, y + 96, mix(T['BG'], T['CARD'], c), 12)
        pen.rect(x, y, x + w, y + 4, mix(T['BG'], color, c), 2)
        pen.text(x + 12, y + 24, name, 11.5, mix(T['CARD'], color, c), 'bold', 'lm')
        pen.text(x + 12, y + 52, big, 15, mix(T['CARD'], T['INK'], c), 'bold', 'lm')
        pen.text(x + 12, y + 74, small, 10, mix(T['CARD'], T['MUTED'], c), 'reg', 'lm')
    typed = COMMAND[:round(len(COMMAND) * clamp((t - 4.6) / 1.4))]
    if typed:
        width = pen.tw(COMMAND, 14, 'mono') + 36
        x = (INTRO_W - width) / 2
        pen.rect(x, 452, x + width, 482, T['CARD'], 15)
        pen.text(x + 18, 467.5, typed + ('_' if len(typed) < len(COMMAND) else ''), 14, T['INK'], 'mono', 'lm')

    return pen.done()


# ---- one loop per part --------------------------------------------------------------------------

GW, GH = 600, 330


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
    pen.text(40, GH - 58, 'Repeats folded into a template and a table. Every line can be read back.', 11.5, T['SOFT'], 'reg', 'lm')


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
    pen.ring(450, 150, 58, 13, 0.99 * g, ORANGE)
    pen.text(450, 144, f'{99 * g:.0f}%', 22, T['INK'], 'bold', 'mm')
    pen.text(450, 168, 'smaller', 10.5, T['MUTED'], 'reg', 'mm')
    pen.text(450, 232, '261 KB of HTML → 3.7 KB', 11.5, mix(T['BG'], T['SOFT'], g), 'reg', 'mm')
    pen.text(36, GH - 58, 'A fetched page becomes Markdown: its main content, without menus, ads or scripts.', 11.5, T['SOFT'], 'reg', 'lm')


NODES = [(80, 120), (170, 90), (150, 190), (260, 150), (340, 90), (360, 210), (450, 140), (490, 200), (540, 95)]
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
    pen.text(36, GH - 58, 'A local map answers "where is it?" in a few hundred tokens.', 11.5, T['SOFT'], 'reg', 'lm')


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
    pen.text(40, GH - 58, 'Tools never used in this repository are left out. One command brings any back.', 11.5, T['SOFT'], 'reg', 'lm')


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
        x1, y1 = x0 + (370 - x0) * out, y0 + (226 - y0) * out
        pen.line([(x0, y0), (x1, y1)], RED, 3)
        pen.circle(x1, y1, 9, RED)
        pen.pill(386, 226, 'try another way', 12, mix(T['BG'], RED, out), mix(T['BG'], '#FFFFFF', out))
    pen.text(36, GH - 58, 'The same failure three times: a judge checks, then one nudge steers away.', 11.5, T['SOFT'], 'reg', 'lm')


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
    pen.text(36, GH - 58, 'What three sessions confirm is told to the next one at its start.', 11.5, T['SOFT'], 'reg', 'lm')


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
    pen.text(36, GH - 58, 'The smallest change that fully solves the task, and nothing more.', 11.5, T['SOFT'], 'reg', 'lm')


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
        frames.append(pen.done())
    durations = [1000 // FPS] * len(frames)
    durations[-1] = HOLD_MS

    return frames, durations


def save_loop(path, frames, hold_ms, colors=128):
    # One palette, from the last frame, which holds every colour: small accents keep their colour in every frame.
    ref = frames[-1].quantize(colors=colors, method=Image.Quantize.FASTOCTREE, dither=Image.Dither.NONE)
    frames = [f.quantize(palette=ref, dither=Image.Dither.NONE) for f in frames]
    durations = [1000 // FPS] * len(frames)
    durations[-1] = hold_ms
    frames[0].save(path, save_all=True, append_images=frames[1:], duration=durations, loop=0, optimize=True)
    print(path, os.path.getsize(path) // 1024, 'KB')


def save_still(path, image):
    image.save(path, optimize=True)
    print(path, image.size, os.path.getsize(path) // 1024, 'KB')


def main(names):
    for theme in THEMES:
        use(theme)
        suffix = '' if theme == 'light' else '-dark'
        out = lambda name, ext: os.path.join(HERE, f'{name}{suffix}.{ext}')
        for name in names:
            if name == 'intro':
                set_scale(STILL_WIDTH / (INTRO_W if name == 'intro' else GW))
                save_loop(out('intro', 'gif'), [intro(theme, k / FPS) for k in range(int(INTRO_S * FPS))], INTRO_HOLD_MS, 255)
                set_scale(STILL_WIDTH / INTRO_W)
                save_still(out('intro', 'png'), intro(theme, 99))
            elif name in STILLS:
                set_scale(STILL_WIDTH / 1400)
                save_still(out(name, 'png'), STILLS[name](theme))
                if name == 'gains':
                    # Also as a loop that draws itself in, section by section, then holds the full picture.
                    set_scale(STILL_WIDTH / 1400)
                    save_loop(out('gains', 'gif'), [gains(theme, k / FPS) for k in range(int(GAINS_S * FPS))], GAINS_HOLD_MS, 255)
            else:
                set_scale(STILL_WIDTH / (INTRO_W if name == 'intro' else GW))
                frames, durations = gif(theme, name.removeprefix('part-'))
                save_loop(out(name, 'gif'), frames, durations[-1])


if __name__ == '__main__':
    main(sys.argv[1:] or ['intro', *STILLS, *(f'part-{key}' for key in PARTS)])
