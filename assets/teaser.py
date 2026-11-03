# Draws the README's animation in a light and a dark theme: assets/teaser.gif and teaser-dark.gif, with their stills
# teaser.png and teaser-dark.png for reduced motion. Run: uv run --with pillow python assets/teaser.py
#
# Three acts, crossfaded:
#   Title    the logo, the name, what it does, its six parts and what it builds on.
#   Method   a sketch, not a measurement: what Claude reads in a session, as one bar of the work itself and five kinds
#            of waste. Each part of the dam is switched on in turn: its card lights up on the right, with a plain
#            example, and the next bar shrinks the waste it takes out, under a dashed line at the bar without the dam.
#   Results  the figures measured for this project, as the README states them.
# Layout is in points on a 1120 x 600 page, drawn at SS times OUT and resized down to OUT, which is the anti-aliasing.
# The logo is drawn from icon.svg's own shapes, so it stays sharp at any size.
import math
import os
from functools import lru_cache

from PIL import Image, ImageDraw, ImageFilter, ImageFont

HERE = os.path.dirname(os.path.abspath(__file__))
W, H, OUT, SS, FPS = 1120, 600, 1.5, 2, 12
K = OUT * SS
FONTS = 'C:/Windows/Fonts/'

THEMES = {
    'light': dict(BG='#F5F5F7', CARD='#FFFFFF', LINE='#E5E5EA', INK='#1D1D1F', MUTED='#6E6E73', FAINT='#C7C7CC', TRACK='#EDEDF0', WORK='#D2D2D7', SHADOW=34,
                  colors=['#007AFF', '#30B0C7', '#5856D6', '#FF9500', '#FF3B30', '#A2845E']),
    'dark': dict(BG='#000000', CARD='#1C1C1E', LINE='#2C2C2E', INK='#F5F5F7', MUTED='#98989D', FAINT='#48484A', TRACK='#2C2C2E', WORK='#48484A', SHADOW=0,
                 colors=['#0A84FF', '#40C8E0', '#5E5CE6', '#FF9F0A', '#FF453A', '#AC8E68']),
}
T = THEMES['light']

# The six parts, in the order they are switched on, each with the waste it takes out of the bar.
PART_TEXT = [
    dict(name='Compression', waste='repeated output', before=13, after=9,
         head='Folds repeated output, every line kept', chips=[('ERROR retry  ×4000', 'mono')], foot='checked against the source'),
    dict(name='Web pages', waste='page clutter', before=11, after=2,
         head='Turns fetched pages into Markdown', chips=[('261 KB HTML', 'mono'), ('→', 'arrow'), ('3.7 KB .md', 'solid')], foot='menus and banners left out'),
    dict(name='Map', waste='broad searches', before=9, after=4,
         head='Answers "where is it?" from a local map', chips=[('SmartCrusher', 'mono'), ('→', 'arrow'), ('crusher.rs:118', 'solid')], foot='code, docs and settings'),
    dict(name='Tool pruning', waste='unused tools', before=7, after=3,
         head='Leaves out tools never used here', chips=[('Read', 'mono'), ('Edit', 'mono'), ('Notebook', 'struck'), ('Cron', 'struck')], foot='one command brings any back'),
    dict(name='Loop guard', waste='stuck in loops', before=8, after=3,
         head='Spots loops and steps in', chips=[('same error ×3', 'mono'), ('→', 'arrow'), ('try another way', 'solid')], foot='a nudge, then a stop'),
    dict(name='Beaver', waste='the work itself', before=52, after=44,
         head='The smallest change that works', chips=[('50 lines', 'struck'), ('→', 'arrow'), ('1 line', 'solid')], foot='built on Ponytail'),
]
PARTS = []
STACK = [5, 0, 1, 2, 3, 4]      # bottom to top: the work itself, then the wastes


def use(theme):
    global T, PARTS
    T = THEMES[theme]
    PARTS = [dict(p, color=c, light=mix(T['CARD'], c, 0.14)) for p, c in zip(PART_TEXT, T['colors'])]


WEIGHTS = {'light': 300, 'regular': 400, 'semi': 600, 'bold': 700}


@lru_cache(maxsize=None)
def f(size, style='regular'):
    if style in ('mono', 'monob'):
        return ImageFont.truetype(FONTS + ('consola.ttf' if style == 'mono' else 'consolab.ttf'), int(size * K))
    if style == 'italic':
        return ImageFont.truetype(FONTS + 'segoeuii.ttf', int(size * K))
    font = ImageFont.truetype(FONTS + 'SegUIVar.ttf', int(size * K))
    # Segoe UI Variable's optical size: its Display cut for headlines, as Apple's SF Pro Display.
    font.set_variation_by_axes([WEIGHTS[style], max(5, min(36, size))])

    return font


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


class Pen:
    def __init__(self):
        self.im = Image.new('RGB', (int(W * K), int(H * K)), T['BG'])
        self.d = ImageDraw.Draw(self.im)

    def rect(self, x0, y0, x1, y1, fill=None, outline=None, width=1, r=0):
        self.d.rounded_rectangle([x0 * K, y0 * K, x1 * K, y1 * K], radius=r * K, fill=fill, outline=outline, width=max(1, int(width * K)) if outline else 0)

    # A card: a soft shadow under it in the light theme, a hairline around it in the dark one.
    def card(self, x0, y0, x1, y1, a, r=18, fill=None, edge=None):
        fill = fill or T['CARD']
        if a <= 0:
            return
        if T['SHADOW']:
            m = 26
            box = (int((x0 - m) * K), int((y0 - m + 6) * K), int((x1 + m) * K), int((y1 + m + 6) * K))
            layer = Image.new('L', (box[2] - box[0], box[3] - box[1]), 0)
            ImageDraw.Draw(layer).rounded_rectangle([m * K, m * K, (x1 - x0 + m) * K, (y1 - y0 + m) * K], radius=r * K, fill=int(T['SHADOW'] * a))
            layer = layer.filter(ImageFilter.GaussianBlur(11 * K))
            self.im.paste(Image.new('RGB', layer.size, (0, 0, 0)), box[:2], layer)
        self.rect(x0, y0, x1, y1, mix(T['BG'], fill, a), mix(T['BG'], edge or (T['LINE'] if not T['SHADOW'] else fill), a), 1, r)

    def circle(self, x, y, r, fill):
        self.d.ellipse([(x - r) * K, (y - r) * K, (x + r) * K, (y + r) * K], fill=fill)

    def line(self, x0, y0, x1, y1, fill, width=1, dash=0):
        if not dash:
            self.d.line([x0 * K, y0 * K, x1 * K, y1 * K], fill=fill, width=max(1, int(width * K)))
            return
        length = math.hypot(x1 - x0, y1 - y0)
        n = int(length // (dash * 2))
        for k in range(n + 1):
            a, b = k * dash * 2 / length, min(1, (k * dash * 2 + dash) / length)
            self.d.line([(x0 + (x1 - x0) * a) * K, (y0 + (y1 - y0) * a) * K, (x0 + (x1 - x0) * b) * K, (y0 + (y1 - y0) * b) * K], fill=fill, width=max(1, int(width * K)))

    def text(self, x, y, s, size, fill=None, style='regular', anchor='la'):
        self.d.text((x * K, y * K), s, font=f(size, style), fill=fill or T['INK'], anchor=anchor)

    # Text filled with a left-to-right gradient, faded in by `a` over the page.
    def gradient_text(self, x, y, s, size, style, c0, c1, a, anchor='la'):
        font = f(size, style)
        box = [int(v) for v in self.d.textbbox((x * K, y * K), s, font=font, anchor=anchor)]
        w, h = box[2] - box[0], box[3] - box[1]
        if w <= 0 or a <= 0:
            return
        mask = Image.new('L', (w, h), 0)
        ImageDraw.Draw(mask).text((x * K - box[0], y * K - box[1]), s, font=font, fill=int(255 * clamp(a)), anchor=anchor)
        ramp = Image.linear_gradient('L').rotate(90).resize((w, h))
        fill = Image.composite(Image.new('RGB', (w, h), rgb(c1)), Image.new('RGB', (w, h), rgb(c0)), ramp.transpose(Image.FLIP_LEFT_RIGHT))
        self.im.paste(fill, box[:2], mask)

    def tw(self, s, size, style='regular'):
        return self.d.textlength(s, font=f(size, style)) / K

    def check(self, x, y, r, color, ink):
        self.circle(x, y, r, color)
        self.d.line([(x - r * 0.42) * K, (y + r * 0.02) * K, (x - r * 0.1) * K, (y + r * 0.36) * K, (x + r * 0.46) * K, (y - r * 0.34) * K], fill=ink, width=max(1, int(1.7 * K)), joint='curve')

    def number(self, x, y, n, color, ink):
        self.circle(x, y, 8, color)
        self.text(x, y + 0.3, str(n), 9.5, ink, 'bold', 'mm')

    def logo(self, x, y, size, a):
        if a <= 0:
            return
        image = logo(int(size * K))
        alpha = image.getchannel('A').point(lambda v: int(v * clamp(a)))
        if T['SHADOW']:
            m = int(18 * K)
            shadow = Image.new('L', (image.width + 2 * m, image.height + 2 * m), 0)
            shadow.paste(alpha.point(lambda v: int(v * 0.28)), (m, m))
            shadow = shadow.filter(ImageFilter.GaussianBlur(9 * K))
            self.im.paste(Image.new('RGB', shadow.size, (0, 0, 0)), (int(x * K) - m, int((y + 7) * K) - m), shadow)
        self.im.paste(image.convert('RGB'), (int(x * K), int(y * K)), alpha)

    def done(self):
        return self.im.resize((int(W * OUT), int(H * OUT)), Image.LANCZOS)


@lru_cache(maxsize=None)
def logo(px):
    """icon.svg, shape by shape, at `px` pixels: the same drawing at any size."""
    n = 4                                # drawn larger again, then reduced, for smooth curves
    s = px * n / 512
    im = Image.new('RGBA', (px * n, px * n), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    e = lambda cx, cy, rx, ry, c: d.ellipse([(cx - rx) * s, (cy - ry) * s, (cx + rx) * s, (cy + ry) * s], fill=c)
    d.rounded_rectangle([0, 0, 512 * s, 512 * s], radius=112 * s, fill='#0E3A53')
    # The water: quadratic waves 24 deep every 128, over the bottom of the tile.
    water = [(x * s, (392 - 12 * math.sin(math.pi * x / 128)) * s) for x in range(0, 513, 4)] + [(512 * s, 512 * s), (0, 512 * s)]
    layer = Image.new('L', im.size, 0)
    ImageDraw.Draw(layer).polygon(water, fill=255)
    tile = Image.new('L', im.size, 0)
    ImageDraw.Draw(tile).rounded_rectangle([0, 0, 512 * s, 512 * s], radius=112 * s, fill=255)
    im.paste(Image.new('RGBA', im.size, '#1B6E8F'), (0, 0), Image.composite(layer, Image.new('L', im.size, 0), tile))
    for cx in (150, 362):
        e(cx, 138, 44, 44, '#5B3A21')
        e(cx, 138, 22, 22, '#E3C39B')
    e(256, 262, 158, 168, '#8A5A34')
    e(256, 300, 118, 112, '#A9743F')
    for cx in (194, 318):
        e(cx, 226, 32, 32, '#FFFFFF')
    for cx in (200, 312):
        e(cx, 230, 18, 18, '#21140C')
    for cx in (206, 318):
        e(cx, 223, 6, 6, '#FFFFFF')
    e(256, 322, 92, 66, '#E3C39B')
    e(256, 284, 34, 24, '#2B1A10')
    for x in (224, 258):
        d.rounded_rectangle([x * s, 346 * s, (x + 30) * s, 410 * s], radius=8 * s, fill='#FFFFFF')

    return im.resize((px, px), Image.LANCZOS)


# ---- act 1: the title -----------------------------------------------------------------------------

def title(pen, t):
    a = ease(t / 0.7)
    pen.logo(W / 2 - 52, 70 - 10 * (1 - a), 104, a)
    left, right = 'The ', 'dam'
    size = 80
    total = pen.tw(left, size, 'bold') + pen.tw(right, size, 'bold')
    x = W / 2 - total / 2
    pen.text(x, 278, left, size, mix(T['BG'], T['INK'], a), 'bold', 'ls')
    brown, orange = PARTS[5]['color'], PARTS[3]['color']
    pen.gradient_text(x + pen.tw(left, size, 'bold'), 278, right, size, 'bold', brown, orange, a, 'ls')
    b = ease((t - 0.4) / 0.7)
    pen.text(W / 2, 322, 'Spend fewer tokens in Claude Code. Without losing a line.', 23, mix(T['BG'], T['MUTED'], b), 'regular', 'mm')
    widths = [pen.tw(p['name'], 13, 'semi') + 44 for p in PARTS]
    gap = 12
    x = W / 2 - (sum(widths) + gap * (len(PARTS) - 1)) / 2
    for i, p in enumerate(PARTS):
        c = ease((t - 0.9 - i * 0.1) / 0.45)
        w, y = widths[i], 372 + 8 * (1 - c)
        pen.card(x, y, x + w, y + 34, c, 17)
        pen.circle(x + 18, y + 17, 5, mix(T['CARD'], p['color'], c))
        pen.text(x + 30, y + 17, p['name'], 13, mix(T['CARD'], T['INK'], c), 'semi', 'lm')
        x += w + gap
    d = ease((t - 1.8) / 0.6)
    pen.text(W / 2, 458, 'A Claude Code plugin  ·  Rust  ·  100% local', 14, mix(T['BG'], T['INK'], d), 'semi', 'mm')
    pen.text(W / 2, 484, 'Built on TokenEconomizer, Ponytail, Headroom, markdowner and html-to-markdown', 12.5, mix(T['BG'], T['MUTED'], d), 'regular', 'mm')


# ---- act 2: the method ----------------------------------------------------------------------------

CHART_X0, BASE_Y, SCALE = 214, 530, 3.9
BAR_W, BAR_GAP = 44, 24
LABELS = ['no dam', 'compress', 'web', 'map', 'prune', 'loops', 'beaver']
STEP0, STEP = 1.6, 1.9          # when bar 0 is drawn; how long each part takes


def bar_x(k):
    return CHART_X0 + k * (BAR_W + BAR_GAP)


def sizes(on):
    return [p['after'] if i < on else p['before'] for i, p in enumerate(PARTS)]


def draw_bar(pen, k, grow, highlight=-1, glow=0.0):
    x = bar_x(k)
    vals = sizes(k)
    y = BASE_Y
    total = sum(vals) * grow
    used = 0
    for i in STACK:
        h = min(vals[i], max(0, total - used)) * SCALE
        used += vals[i]
        if h <= 0:
            continue
        color = T['WORK'] if i == 5 else PARTS[i]['color']
        if i == highlight and glow:
            color = mix(color, '#FFFFFF', 0.4 * glow)
        # Segments a hair apart, rounded, as Apple's charts draw stacks.
        pen.rect(x, y - h + (1 if h > 2 else 0), x + BAR_W, y, color, None, 0, min(4, h / 2))
        y -= h


def method(pen, t):
    a = ease(t / 0.5)
    pen.text(40, 50, 'One kind of waste out, part by part.', 28, mix(T['BG'], T['INK'], a), 'bold', 'lm')
    pen.text(40, 80, 'What Claude reads in a session, as the dam switches its parts on.', 14, mix(T['BG'], T['MUTED'], a), 'regular', 'lm')
    for i, p in enumerate(PARTS):
        y0, y1 = 24 + i * 93, 24 + i * 93 + 82
        on = ease((t - (STEP0 + 0.6 + i * STEP)) / 0.45)
        fill = mix(T['CARD'], p['light'], on)
        pen.card(756, y0, 1092, y1, a, 16, fill, mix(T['LINE'], p['color'], on) if not T['SHADOW'] else None)
        if on <= 0:
            pen.text(774, y0 + 20, p['name'], 12, mix(T['BG'], T['FAINT'], a), 'semi', 'lm')
            continue
        c = lambda col: mix(fill, col, on)
        pen.number(782, y0 + 20, i + 1, c(p['color']), c('#FFFFFF'))
        pen.text(797, y0 + 20, p['name'], 12, c(p['color']), 'semi', 'lm')
        pen.check(1072, y0 + 20, 8, c(p['color']), c('#FFFFFF'))
        pen.text(1058, y0 + 20, p['foot'], 10.5, c(T['MUTED']), 'regular', 'rm')
        pen.text(774, y0 + 41, p['head'], 13.5, c(T['INK']), 'semi', 'lm')
        x = 774
        for s, kind in p['chips']:
            if kind == 'arrow':
                pen.text(x + 2, y0 + 63, s, 12, c(p['color']), 'semi', 'lm')
                x += 18
                continue
            w = pen.tw(s, 10.5, 'mono') + 14
            chip = c(p['color']) if kind == 'solid' else c(T['CARD'])
            pen.rect(x, y0 + 54, x + w, y0 + 72, chip, None if kind == 'solid' else c(T['LINE']), 1, 9)
            pen.text(x + 7, y0 + 63.5, s, 10.5, c('#FFFFFF') if kind == 'solid' else c(T['MUTED'] if kind == 'struck' else T['INK']), 'mono', 'lm')
            if kind == 'struck':
                pen.line(x + 5, y0 + 63.5, x + w - 5, y0 + 63.5, c(T['MUTED']), 1)
            x += w + 6
    # The chart.
    pen.line(CHART_X0 - 10, BASE_Y, bar_x(6) + BAR_W + 10, BASE_Y, mix(T['BG'], T['FAINT'], a), 1)
    for k in range(7):
        grow = ease((t - (STEP0 if k == 0 else STEP0 + 0.6 + (k - 1) * STEP + 0.35)) / 0.7)
        lit = grow > 0
        pen.text(bar_x(k) + BAR_W / 2, BASE_Y + 16, LABELS[k], 11, mix(T['BG'], T['INK'] if lit else T['FAINT'], a), 'regular', 'mm')
        if grow > 0:
            glow = 1 - clamp((t - (STEP0 + 0.6 + (k - 1) * STEP + 1.1)) / 0.6) if k else 0
            draw_bar(pen, k, grow, k - 1, glow)
    # The names of the waste, beside the first bar.
    names = ease((t - STEP0 - 0.5) / 0.5)
    y = BASE_Y
    for i in STACK:
        h = PARTS[i]['before'] * SCALE
        color = T['MUTED'] if i == 5 else PARTS[i]['color']
        pen.text(CHART_X0 - 22, y - h / 2, PARTS[i]['waste'], 12, mix(T['BG'], color, names), 'semi' if i != 5 else 'regular', 'rm')
        pen.line(CHART_X0 - 18, y - h / 2, CHART_X0 - 6, y - h / 2, mix(T['BG'], color, names), 1)
        y -= h
    # What a session costs without the dam, and what is saved at the end.
    top = BASE_Y - sum(sizes(0)) * SCALE
    dash = ease((t - STEP0 - 1.0) / 0.6)
    if dash:
        pen.line(bar_x(0), top, bar_x(0) + (bar_x(6) + BAR_W - bar_x(0)) * dash, top, mix(T['BG'], T['MUTED'], dash), 1, 4)
        pen.text(bar_x(6) + BAR_W, top - 10, 'without the dam', 11, mix(T['BG'], T['MUTED'], dash), 'regular', 'rm')
    saved = ease((t - (STEP0 + 0.6 + 6 * STEP)) / 0.6)
    if saved:
        low = BASE_Y - sum(sizes(6)) * SCALE
        x = bar_x(6) + BAR_W + 18
        ink = mix(T['BG'], T['INK'], saved)
        pen.line(x, top + 3, x, low - 3, ink, 1.2)
        pen.line(x - 4, top + 3, x + 4, top + 3, ink, 1.2)
        pen.line(x - 4, low - 3, x + 4, low - 3, ink, 1.2)
        pen.text(x + 7, (top + low) / 2, 'saved', 12, ink, 'semi', 'lm')
    pen.text(40, 578, 'An illustration, not a measurement. The measured figures follow.', 11, mix(T['BG'], T['MUTED'], a), 'regular', 'lm')


METHOD_LEN = STEP0 + 0.6 + 6 * STEP + 2.2


# ---- act 3: the results ---------------------------------------------------------------------------

TILES = [
    ('100', '%', 'of lines recoverable after compression', 'Headroom: 69.9%', 0),
    ('99', '%', 'smaller: a GitHub page as Markdown', '261 KB of HTML → 3.7 KB', 1),
    ('1.5', ' s', 'to map 2,112 source files', 'graphify: 816 s', 2),
    ('18.4–30.9', '%', 'lower cost in TokenEconomizer\'s paper', 'rewritten here in Rust', 3),
]
BARS = [('GitHub repository page', 1, 99), ('Wikipedia, "Beaver"', 1, 84), ('The Rust book, installation', 1, 81), ('Hacker News front page', 1, 73),
        ('Tool output, 43 real cases', 0, 24.1), ('Ponytail benchmark: output tokens', 5, 45)]


def results(pen, t):
    a = ease(t / 0.5)
    pen.text(40, 50, 'Measured, not promised.', 28, mix(T['BG'], T['INK'], a), 'bold', 'lm')
    pen.text(40, 80, 'On real outputs, pages and repositories.', 14, mix(T['BG'], T['MUTED'], a), 'regular', 'lm')
    tile_w = (W - 80 - 3 * 16) / 4
    for i, (big, unit, what, note, part) in enumerate(TILES):
        c = ease((t - 0.2 - i * 0.15) / 0.5)
        x = 40 + i * (tile_w + 16)
        pen.card(x, 104, x + tile_w, 214, c, 18)
        shown = big
        if big.replace('.', '').isdigit():
            value = float(big) * ease((t - 0.3 - i * 0.15) / 0.9)
            shown = f'{value:.1f}' if '.' in big else f'{value:.0f}'
        color = PARTS[part]['color']
        pen.gradient_text(x + 18, 156, shown, 36, 'bold', color, mix(color, PARTS[(part + 1) % 6]['color'], 0.6), c, 'ls')
        pen.text(x + 20 + pen.tw(shown, 36, 'bold'), 156, unit, 17, mix(T['CARD'], T['MUTED'], c), 'semi', 'ls')
        pen.text(x + 18, 180, what, 12, mix(T['CARD'], T['INK'], c), 'regular', 'lm')
        pen.text(x + 18, 198, note, 11, mix(T['CARD'], T['MUTED'], c), 'regular', 'lm')
    b = ease((t - 0.8) / 0.5)
    pen.card(40, 234, W - 40, 552, b, 20)
    pen.text(62, 262, 'Share of what Claude no longer reads', 15, mix(T['CARD'], T['INK'], b), 'semi', 'lm')
    pen.text(W - 62, 262, 'saved', 12, mix(T['CARD'], T['MUTED'], b), 'regular', 'rm')
    for i, (label, part, value) in enumerate(BARS):
        y = 302 + i * 42
        c = ease((t - 1.0 - i * 0.12) / 0.5)
        p = PARTS[part]
        pen.text(62, y - 7, label, 12.5, mix(T['CARD'], T['INK'], c), 'semi', 'lm')
        pen.text(62, y + 10, p['name'], 10.5, mix(T['CARD'], p['color'], c), 'regular', 'lm')
        x0, x1 = 340, W - 140
        pen.rect(x0, y - 6, x1, y + 6, mix(T['CARD'], T['TRACK'], c), None, 0, 6)
        g = ease((t - 1.2 - i * 0.12) / 0.9)
        if g:
            pen.rect(x0, y - 6, max(x0 + 12, x0 + (x1 - x0) * value / 100 * g), y + 6, p['color'], None, 0, 6)
            pen.text(W - 62, y, f'{value * g:.0f}%' if value == int(value) else f'{value * g:.1f}%', 15, mix(T['CARD'], T['INK'], g), 'semi', 'rm')
    pen.text(W / 2, 578, 'Built by beavers, measured by people  ·  github.com/T-Celor/the-dam', 11.5, mix(T['BG'], T['MUTED'], b), 'regular', 'mm')


ACTS = [(title, 3.8), (method, METHOD_LEN), (results, 6.0)]
FADE = 0.5


def frame(t):
    start = 0.0
    for n, (act, length) in enumerate(ACTS):
        if t < start + length:
            pen = Pen()
            act(pen, t - start)
            image = pen.done()
            into = t - start - (length - FADE)
            if into > 0:
                nxt, _ = ACTS[(n + 1) % len(ACTS)]
                other = Pen()
                nxt(other, 0 if n + 1 < len(ACTS) else 99)
                image = Image.blend(image, other.done(), into / FADE)
            return image
        start += length


def render(theme):
    use(theme)
    suffix = '' if theme == 'light' else '-dark'
    total = sum(length for _, length in ACTS)
    frames = [frame(k / FPS).quantize(colors=255, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE) for k in range(int(total * FPS))]
    durations = [1000 // FPS] * len(frames)
    # Hold the full method chart and the results.
    durations[int((ACTS[0][1] + METHOD_LEN - FADE - 0.05) * FPS)] = 2500
    durations[int((total - FADE - 0.05) * FPS)] = 3000
    gif = os.path.join(HERE, f'teaser{suffix}.gif')
    frames[0].save(gif, save_all=True, append_images=frames[1:], duration=durations, loop=0, optimize=True)
    still = Pen()
    method(still, METHOD_LEN - FADE - 0.05)
    still.done().save(os.path.join(HERE, f'teaser{suffix}.png'), optimize=True)
    print(theme, len(frames), 'frames', os.path.getsize(gif) // 1024, 'KB')


if __name__ == '__main__':
    import sys
    for theme in sys.argv[1:] or THEMES:
        render(theme)
