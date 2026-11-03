# Draws assets/teaser.gif and teaser.png (its still, for reduced motion). Run: uv run --with pillow python assets/teaser.py
#
# Three acts, crossfaded, in the manner of TokenEconomizer's teaser:
#   Title    the name, what it does, its six parts and what it builds on.
#   Method   a sketch, not a measurement: what Claude reads in a session, as one bar of the work itself and five kinds
#            of waste. Each part of the dam is switched on in turn: its card lights up on the right, with a plain
#            example, and the next bar shrinks the waste it takes out, under a dashed line at the bar without the dam.
#   Results  the figures measured for this project, as the README states them.
# Every frame is drawn at SS times its size and resized down, which is the anti-aliasing.
import math
import os

from PIL import Image, ImageDraw, ImageFont

HERE = os.path.dirname(os.path.abspath(__file__))
W, H, SS, FPS = 1120, 600, 2, 12
FONTS = 'C:/Windows/Fonts/'

BG, CARD, LINE, INK, MUTED = '#F2F4F8', '#FFFFFF', '#D5DBE3', '#1F2933', '#6B7785'
BROWN, BROWN_LIGHT, WORK = '#8A5A34', '#F3E8DC', '#C9D1DA'
# The six parts, in the order they are switched on, each with the waste it takes out of the bar.
PARTS = [
    dict(name='Compression', color='#2F6FB0', light='#E3EDF7', waste='repeated output', before=13, after=9,
         head='Folds repeated output, every line kept', chips=[('ERROR retry  ×4000', 'mono')], foot='checked against the source before use'),
    dict(name='Web pages', color='#2A8C80', light='#E0F2EF', waste='page clutter', before=11, after=2,
         head='Turns fetched pages into Markdown', chips=[('261 KB HTML', 'mono'), ('→', 'arrow'), ('3.7 KB .md', 'solid')], foot='menus, ads and cookie banners left out'),
    dict(name='Map', color='#6F55A3', light='#ECE7F5', waste='broad searches', before=9, after=4,
         head='Answers "where is it?" from a local map', chips=[('SmartCrusher', 'mono'), ('→', 'arrow'), ('crusher.rs:118', 'solid')], foot='code, docs and settings, rebuilt each session'),
    dict(name='Tool pruning', color='#A8741A', light='#F6EEDD', waste='unused tools', before=7, after=3,
         head='Leaves out tools never used here', chips=[('Read', 'mono'), ('Edit', 'mono'), ('Notebook', 'struck'), ('Cron', 'struck')], foot='one command brings any back'),
    dict(name='Loop guard', color='#B84843', light='#F8E5E3', waste='stuck in loops', before=8, after=3,
         head='Spots loops and steps in', chips=[('same error ×3', 'mono'), ('→', 'arrow'), ('try another way', 'solid')], foot='a nudge when the same error repeats'),
    dict(name='Beaver', color=BROWN, light=BROWN_LIGHT, waste='the work itself', before=52, after=44,
         head='The smallest change that works', chips=[('50 lines', 'struck'), ('→', 'arrow'), ('1 line', 'solid')], foot='lazy-senior-dev rules, built on Ponytail'),
]
STACK = [5, 0, 1, 2, 3, 4]      # bottom to top: the work itself, then the wastes


def f(size, style='regular'):
    name = {'regular': 'segoeui.ttf', 'bold': 'segoeuib.ttf', 'semi': 'seguisb.ttf', 'light': 'segoeuisl.ttf', 'italic': 'segoeuii.ttf', 'mono': 'consola.ttf', 'monob': 'consolab.ttf'}[style]
    return ImageFont.truetype(FONTS + name, int(size * SS))


def clamp(x):
    return max(0.0, min(1.0, x))


def ease(x):
    x = clamp(x)

    return 1 - (1 - x) ** 3


def mix(a, b, k):
    a, b = [tuple(int(c[i:i + 2], 16) for i in (1, 3, 5)) if isinstance(c, str) else c for c in (a, b)]

    return tuple(int(x + (y - x) * clamp(k)) for x, y in zip(a, b))


class Pen:
    def __init__(self):
        self.im = Image.new('RGB', (W * SS, H * SS), BG)
        self.d = ImageDraw.Draw(self.im)

    def rect(self, x0, y0, x1, y1, fill=None, outline=None, width=1, r=0):
        self.d.rounded_rectangle([x0 * SS, y0 * SS, x1 * SS, y1 * SS], radius=r * SS, fill=fill, outline=outline, width=int(width * SS))

    def circle(self, x, y, r, fill):
        self.d.ellipse([(x - r) * SS, (y - r) * SS, (x + r) * SS, (y + r) * SS], fill=fill)

    def line(self, x0, y0, x1, y1, fill, width=1, dash=0):
        if not dash:
            self.d.line([x0 * SS, y0 * SS, x1 * SS, y1 * SS], fill=fill, width=int(width * SS))
            return
        length = math.hypot(x1 - x0, y1 - y0)
        n = int(length // (dash * 2))
        for k in range(n + 1):
            a, b = k * dash * 2 / length, min(1, (k * dash * 2 + dash) / length)
            self.d.line([(x0 + (x1 - x0) * a) * SS, (y0 + (y1 - y0) * a) * SS, (x0 + (x1 - x0) * b) * SS, (y0 + (y1 - y0) * b) * SS], fill=fill, width=int(width * SS))

    def text(self, x, y, s, size, fill=INK, style='regular', anchor='la'):
        self.d.text((x * SS, y * SS), s, font=f(size, style), fill=fill, anchor=anchor)

    def tw(self, s, size, style='regular'):
        return self.d.textlength(s, font=f(size, style)) / SS

    def check(self, x, y, r, color):
        self.circle(x, y, r, color)
        self.d.line([(x - r * 0.45) * SS, y * SS, (x - r * 0.1) * SS, (y + r * 0.4) * SS, (x + r * 0.5) * SS, (y - r * 0.4) * SS], fill='white', width=int(1.6 * SS), joint='curve')

    def number(self, x, y, n, color):
        self.circle(x, y, 7.5, color)
        self.text(x, y + 0.5, str(n), 9.5, 'white', 'bold', 'mm')

    def paste(self, image, x, y, size):
        self.im.paste(image.resize((int(size * SS), int(size * SS)), Image.LANCZOS), (int(x * SS), int(y * SS)), image.resize((int(size * SS), int(size * SS)), Image.LANCZOS))

    def done(self):
        return self.im.resize((W, H), Image.LANCZOS)


ICON = Image.open(os.path.join(HERE, 'icon.png')).convert('RGBA')


# ---- act 1: the title -----------------------------------------------------------------------------

def title(pen, t):
    a = ease(t / 0.6)
    pen.paste(ICON, W / 2 - 34, 62 - 8 * (1 - a), 68)
    left, right = 'The ', 'dam'
    size = 58
    total = pen.tw(left, size, 'semi') + pen.tw(right, size, 'bold')
    x = W / 2 - total / 2
    pen.text(x, 178, left, size, mix(BG, INK, a), 'semi', 'ls')
    pen.text(x + pen.tw(left, size, 'semi'), 178, right, size, mix(BG, BROWN, a), 'bold', 'ls')
    pen.line(W / 2 - 22, 196, W / 2 + 22, 196, mix(BG, BROWN, a), 2.5)
    b = ease((t - 0.4) / 0.6)
    pen.text(W / 2, 228, 'Spend fewer tokens in Claude Code, without losing a line', 19, mix(BG, MUTED, b), 'light', 'mm')
    widths = [pen.tw(p['name'], 12.5, 'semi') + 40 for p in PARTS]
    for row in range(2):
        items = list(range(row * 3, row * 3 + 3))
        x = W / 2 - (sum(widths[i] for i in items) + 12 * 2) / 2
        for i in items:
            c = ease((t - 0.8 - i * 0.12) / 0.4)
            p, w, y = PARTS[i], widths[i], 268 + row * 38
            pen.rect(x, y + 6 * (1 - c), x + w, y + 28 + 6 * (1 - c), mix(BG, p['light'], c), mix(BG, p['color'], c), 1.3, 14)
            pen.circle(x + 15, y + 14 + 6 * (1 - c), 7.5, mix(BG, p['color'], c))
            pen.text(x + 15, y + 14.5 + 6 * (1 - c), str(i + 1), 9.5, mix(BG, '#FFFFFF', c), 'bold', 'mm')
            pen.text(x + 28, y + 14 + 6 * (1 - c), p['name'], 12.5, mix(BG, p['color'], c), 'semi', 'lm')
            x += w + 12
    d = ease((t - 1.8) / 0.6)
    line = [('A Claude Code plugin', 'semi'), ('  ·  Rust  ·  ', 'regular'), ('100% local', 'semi')]
    x = W / 2 - sum(pen.tw(s, 13, st) for s, st in line) / 2
    for s, st in line:
        pen.text(x, 380, s, 13, mix(BG, INK if st == 'semi' else MUTED, d), st, 'lm')
        x += pen.tw(s, 13, st)
    built = 'Built on TokenEconomizer · Ponytail · Headroom · markdowner · html-to-markdown'
    pen.text(W / 2, 412, built, 12, mix(BG, MUTED, d), 'regular', 'mm')


# ---- act 2: the method ----------------------------------------------------------------------------

CHART_X0, BASE_Y, SCALE = 214, 528, 4.1
BAR_W, BAR_GAP = 46, 22
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
        color = WORK if i == 5 else PARTS[i]['color']
        if i == highlight and glow:
            color = mix(color, '#FFFFFF', 0.35 * glow)
        pen.rect(x, y - h, x + BAR_W, y, color)
        y -= h


def method(pen, t):
    a = ease(t / 0.5)
    pen.text(40, 44, 'Each part of the dam takes one kind of waste out of what Claude reads', 18, mix(BG, INK, a), 'semi', 'lm')
    # The panel of parts.
    pen.rect(748, 26, 1092, 576, mix(BG, CARD, a), mix(BG, LINE, a), 1.2, 10)
    pen.text(766, 50, 'The dam', 15, mix(BG, INK, a), 'semi', 'lm')
    pen.text(766, 69, 'switches its parts on, one by one', 11, mix(BG, MUTED, a), 'regular', 'lm')
    pen.line(766, 82, 1074, 82, mix(BG, LINE, a), 1)
    for i, p in enumerate(PARTS):
        y0, y1 = 92 + i * 80, 92 + i * 80 + 72
        on = ease((t - (STEP0 + 0.6 + i * STEP)) / 0.45)
        pen.rect(766, y0, 1074, y1, mix(CARD, p['light'], on * 0.55), mix(mix(CARD, LINE, a), p['color'], on), 1 + on * 0.6, 7)
        if on <= 0:
            pen.line(772, y0 + 6, 1068, y0 + 6, CARD, 1)
            continue
        c = lambda col: mix(CARD, col, on)
        pen.number(780, y0 + 15, i + 1, c(p['color']))
        pen.text(793, y0 + 15, p['name'], 11, c(p['color']), 'semi', 'lm')
        pen.check(1058, y0 + 15, 7.5, c(p['color']))
        pen.text(776, y0 + 33, p['head'], 12.5, c(INK), 'semi', 'lm')
        x = 776
        for s, kind in p['chips']:
            if kind == 'arrow':
                pen.text(x + 2, y0 + 52, s, 12, c(p['color']), 'bold', 'lm')
                x += 18
                continue
            w = pen.tw(s, 10.5, 'mono') + 12
            fill = c(p['color']) if kind == 'solid' else c('#FFFFFF')
            pen.rect(x, y0 + 44, x + w, y0 + 60, fill, c(p['color'] if kind != 'struck' else LINE), 1, 3)
            pen.text(x + 6, y0 + 52.5, s, 10.5, 'white' if kind == 'solid' else c(MUTED if kind == 'struck' else INK), 'mono', 'lm')
            if kind == 'struck':
                pen.line(x + 4, y0 + 52.5, x + w - 4, y0 + 52.5, c(MUTED), 1)
            x += w + 5
        pen.text(1044, y0 + 15, p['foot'], 9.5, c(MUTED), 'italic', 'rm')
    # The chart.
    pen.line(CHART_X0 - 10, BASE_Y, bar_x(6) + BAR_W + 10, BASE_Y, mix(BG, '#AAB4C0', a), 1)
    for k in range(7):
        grow = ease((t - (STEP0 if k == 0 else STEP0 + 0.6 + (k - 1) * STEP + 0.35)) / 0.7)
        lit = k > 0 and grow > 0
        pen.text(bar_x(k) + BAR_W / 2, BASE_Y + 14, LABELS[k], 10.5, mix(BG, INK if lit or (k == 0 and grow) else '#AAB4C0', a), 'regular', 'mm')
        if grow > 0:
            glow = 1 - clamp((t - (STEP0 + 0.6 + (k - 1) * STEP + 1.1)) / 0.6) if k else 0
            draw_bar(pen, k, grow, k - 1, glow)
    # The names of the waste, beside the first bar.
    names = ease((t - STEP0 - 0.5) / 0.5)
    y = BASE_Y
    for i in STACK:
        h = PARTS[i]['before'] * SCALE
        color = MUTED if i == 5 else PARTS[i]['color']
        pen.text(CHART_X0 - 22, y - h / 2, PARTS[i]['waste'], 11.5, mix(BG, color, names), 'semi' if i != 5 else 'regular', 'rm')
        pen.line(CHART_X0 - 18, y - h / 2, CHART_X0 - 6, y - h / 2, mix(BG, color, names), 1)
        y -= h
    # What a session costs without the dam, and what is saved at the end.
    top = BASE_Y - sum(sizes(0)) * SCALE
    dash = ease((t - STEP0 - 1.0) / 0.6)
    if dash:
        pen.line(bar_x(0), top, bar_x(0) + (bar_x(6) + BAR_W - bar_x(0)) * dash, top, '#97A3B1', 1, 4)
        pen.text(bar_x(6) + BAR_W, top - 9, 'without the dam', 10, mix(BG, MUTED, dash), 'italic', 'rm')
    saved = ease((t - (STEP0 + 0.6 + 6 * STEP)) / 0.6)
    if saved:
        low = BASE_Y - sum(sizes(6)) * SCALE
        x = bar_x(6) + BAR_W + 18
        pen.line(x, top + 3, x, low - 3, mix(BG, INK, saved), 1.2)
        pen.line(x - 4, top + 3, x + 4, top + 3, mix(BG, INK, saved), 1.2)
        pen.line(x - 4, low - 3, x + 4, low - 3, mix(BG, INK, saved), 1.2)
        pen.text(x + 6, (top + low) / 2, 'saved', 11, mix(BG, INK, saved), 'semi', 'lm')
    pen.text(40, 572, 'An illustration, not a measurement: the measured figures follow.', 10.5, mix(BG, MUTED, a), 'italic', 'lm')


METHOD_LEN = STEP0 + 0.6 + 6 * STEP + 2.2


# ---- act 3: the results ---------------------------------------------------------------------------

TILES = [
    ('100', '%', 'of lines recoverable after compression', 'Headroom: 69.9%'),
    ('99', '%', 'smaller: a GitHub page read as Markdown', '261 KB of HTML → 3.7 KB'),
    ('1.5', ' s', 'to map 2,112 source files', 'graphify: 816 s'),
    ('18.4–30.9', '%', 'lower cost in TokenEconomizer\'s paper', 'rewritten here in Rust'),
]
BARS = [('GitHub repository page', 'web', 99), ('Wikipedia, "Beaver"', 'web', 84), ('The Rust book, installation', 'web', 81), ('Hacker News front page', 'web', 73),
        ('Tool output, 43 real cases', 'compression', 24.1), ('Ponytail benchmark: output tokens', 'beaver', 45)]
KIND = {'web': PARTS[1], 'compression': PARTS[0], 'beaver': PARTS[5]}


def results(pen, t):
    a = ease(t / 0.5)
    pen.text(40, 44, 'Measured on real outputs, pages and repositories', 18, mix(BG, INK, a), 'semi', 'lm')
    tile_w = (W - 80 - 3 * 14) / 4
    for i, (big, unit, what, note) in enumerate(TILES):
        c = ease((t - 0.2 - i * 0.15) / 0.5)
        x = 40 + i * (tile_w + 14)
        pen.rect(x, 70, x + tile_w, 168, mix(BG, CARD, c), mix(BG, LINE, c), 1.2, 8)
        shown = big
        if big.replace('.', '').isdigit():
            value = float(big) * ease((t - 0.3 - i * 0.15) / 0.9)
            shown = f'{value:.1f}' if '.' in big else f'{value:.0f}'
        pen.text(x + 16, 112, shown, 30, mix(BG, '#2A5EA0', c), 'semi', 'ls')
        pen.text(x + 18 + pen.tw(shown, 30, 'semi'), 112, unit, 16, mix(BG, MUTED, c), 'regular', 'ls')
        pen.text(x + 16, 134, what, 11.5, mix(BG, INK, c), 'regular', 'lm')
        pen.text(x + 16, 152, note, 10.5, mix(BG, MUTED, c), 'italic', 'lm')
    b = ease((t - 0.8) / 0.5)
    pen.rect(40, 186, W - 40, 548, mix(BG, CARD, b), mix(BG, LINE, b), 1.2, 8)
    pen.text(58, 210, 'Share of what Claude no longer reads', 13, mix(BG, INK, b), 'semi', 'lm')
    pen.text(W - 58, 210, 'saved', 11, mix(BG, MUTED, b), 'regular', 'rm')
    for i, (label, kind, value) in enumerate(BARS):
        y = 250 + i * 48
        c = ease((t - 1.0 - i * 0.12) / 0.5)
        p = KIND[kind]
        pen.text(58, y, label, 12, mix(BG, INK, c), 'semi', 'lm')
        pen.text(58, y + 17, p['name'], 10, mix(BG, p['color'], c), 'regular', 'lm')
        x0, x1 = 330, W - 140
        pen.rect(x0, y - 7, x1, y + 7, mix(BG, '#EEF1F5', c), None, 0, 3)
        g = ease((t - 1.2 - i * 0.12) / 0.9)
        if g:
            pen.rect(x0, y - 7, x0 + (x1 - x0) * value / 100 * g, y + 7, p['color'], None, 0, 3)
            pen.text(x1 + 70, y, f'{value * g:.0f}%' if value == int(value) else f'{value * g:.1f}%', 13, mix(BG, INK, g), 'semi', 'rm')
    pen.text(W / 2, 572, 'Built by beavers, measured by people  ·  github.com/T-Celor/the-dam', 11, mix(BG, MUTED, b), 'regular', 'mm')


ACTS = [(title, 3.6), (method, METHOD_LEN), (results, 6.0)]
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


def main():
    total = sum(length for _, length in ACTS)
    frames = []
    for k in range(int(total * FPS)):
        frames.append(frame(k / FPS).quantize(colors=64, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE))
    durations = [1000 // FPS] * len(frames)
    # Hold the full method chart and the results.
    hold_method = int((ACTS[0][1] + METHOD_LEN - FADE - 0.05) * FPS)
    durations[hold_method] = 2500
    durations[int((total - FADE - 0.05) * FPS)] = 3000
    frames[0].save(os.path.join(HERE, 'teaser.gif'), save_all=True, append_images=frames[1:], duration=durations, loop=0, optimize=True)
    still = Pen()
    method(still, METHOD_LEN - FADE - 0.05)
    still.done().save(os.path.join(HERE, 'teaser.png'), optimize=True)
    print(len(frames), 'frames', os.path.getsize(os.path.join(HERE, 'teaser.gif')) // 1024, 'KB')


if __name__ == '__main__':
    main()
