# Draws assets/teaser.gif (and teaser.png, its still for reduced motion): the dam's tools, told by beavers.
# Run: uv run --with pillow python assets/teaser.py
import math
import os
from PIL import Image, ImageDraw, ImageFont

W, H, S = 800, 450, 2
HERE = os.path.dirname(os.path.abspath(__file__))
FONTS = 'C:/Windows/Fonts/' if os.name == 'nt' else '/usr/share/fonts/truetype/dejavu/'
SKY, SKY2 = (250, 243, 230), (236, 224, 204)
WATER, WATER2, FOAM = (58, 124, 165), (44, 100, 140), (170, 210, 230)
FUR, FUR_DARK, BELLY, TOOTH = (139, 90, 43), (92, 58, 26), (196, 150, 98), (255, 248, 231)
INK, PAPER, ACCENT, GREEN, RED, CLAUDE = (43, 43, 43), (255, 253, 247), (224, 122, 47), (46, 125, 50), (198, 40, 40), (217, 119, 87)


def font(size, bold=False, mono=False):
    name = 'consola.ttf' if mono else ('segoeuib.ttf' if bold else 'segoeui.ttf')
    try:
        return ImageFont.truetype(FONTS + name, size * S)
    except OSError:
        return ImageFont.truetype(FONTS + ('DejaVuSansMono.ttf' if mono else 'DejaVuSans-Bold.ttf' if bold else 'DejaVuSans.ttf'), size * S)


def ease(p):
    p = max(0.0, min(1.0, p))

    return p * p * (3 - 2 * p)


def seg(p, a, b):
    return ease((p - a) / (b - a)) if b > a else float(p >= b)


class Pen:
    """Drawing in 1x coordinates on a 2x canvas, downsampled for smooth edges."""

    def __init__(self):
        self.im = Image.new('RGB', (W * S, H * S), SKY)
        self.d = ImageDraw.Draw(self.im)

    def ell(self, cx, cy, rx, ry, fill, outline=None, width=0):
        self.d.ellipse([(cx - rx) * S, (cy - ry) * S, (cx + rx) * S, (cy + ry) * S], fill=fill, outline=outline, width=width * S)

    def rect(self, x0, y0, x1, y1, fill, radius=0, outline=None, width=0):
        self.d.rounded_rectangle([x0 * S, y0 * S, x1 * S, y1 * S], radius=radius * S, fill=fill, outline=outline, width=width * S)

    def line(self, points, fill, width=2):
        self.d.line([(x * S, y * S) for x, y in points], fill=fill, width=width * S, joint='curve')

    def poly(self, points, fill):
        self.d.polygon([(x * S, y * S) for x, y in points], fill=fill)

    def text(self, x, y, text, size=18, fill=INK, bold=False, mono=False, anchor='la'):
        self.d.text((x * S, y * S), text, font=font(size, bold, mono), fill=fill, anchor=anchor)

    def width_of(self, text, size, bold=False, mono=False):
        return self.d.textlength(text, font=font(size, bold, mono)) / S

    def done(self):
        return self.im.resize((W, H), Image.LANCZOS)


def scenery(pen, t, water_top=330):
    for y in range(0, water_top, 6):
        k = y / water_top
        pen.rect(0, y, W, y + 6, tuple(int(a + (b - a) * k) for a, b in zip(SKY, SKY2)))
    pen.rect(0, water_top, W, H, WATER)
    for row in range(4):
        y = water_top + 18 + row * 28
        pts = [(x, y + 4 * math.sin(x / 40 + t * 0.35 + row)) for x in range(0, W + 10, 10)]
        pen.line(pts, WATER2 if row % 2 else FOAM, 2)


def log(pen, x, y, length=70, r=11, label=None):
    pen.rect(x, y - r, x + length, y + r, FUR, radius=r)
    pen.ell(x + length - r, y, r - 1, r - 1, BELLY)
    pen.ell(x + length - r, y, r - 6, r - 6, FUR)
    if label:
        pen.text(x + length / 2 - 6, y, label, 11, PAPER, bold=True, anchor='mm')


def beaver(pen, x, y, s=1.0, t=0, flip=False, hat=False, chew=False, wave=False):
    def p(dx, dy):
        return x + (-dx if flip else dx) * s, y + dy * s

    def e(dx, dy, rx, ry, fill):
        cx, cy = p(dx, dy)
        pen.ell(cx, cy, rx * s, ry * s, fill)

    bob = math.sin(t * 0.5) * 2
    y += bob
    tail = [p(-30, 8), p(-78, -2 + math.sin(t * 0.6) * 4), p(-86, 14), p(-34, 24)]
    pen.poly(tail, FUR_DARK)
    for k in range(3):
        a, b = p(-44 - k * 12, 4), p(-50 - k * 12, 20)
        pen.line([a, b], FUR, 2)
    e(0, 0, 40, 32, FUR)
    e(8, 8, 22, 20, BELLY)
    e(-18, 30, 12, 6, FUR_DARK)
    e(20, 30, 12, 6, FUR_DARK)
    arm = -14 + (math.sin(t * 0.9) * 10 if wave else 0)
    e(30, 4 + arm * 0.3, 8, 12, FUR_DARK)
    e(30, -34, 26, 23, FUR)
    e(14, -54, 7, 7, FUR_DARK)
    e(42, -55, 7, 7, FUR_DARK)
    e(42, -30, 13, 10, BELLY)
    e(52, -36, 6, 4, INK)
    blink = (t % 37) in (0, 1)
    if blink:
        a, b = p(28, -40), p(38, -40)
        pen.line([a, b], INK, 2)
    else:
        e(33, -40, 4, 5, INK)
        e(34.5, -42, 1.5, 1.5, PAPER)
    shift = (math.sin(t * 1.6) * 2 + 2) if chew else 0
    tx, ty = p(45, -26 + shift * 0.3)
    pen.rect(tx - 4 * s, ty, tx + 4 * s, ty + (10 + shift) * s, TOOTH, radius=1)
    pen.line([(tx, ty), (tx, ty + (10 + shift) * s)], (200, 190, 170), 1)
    if hat:
        hx, hy = p(30, -56)
        pen.ell(hx, hy + 2 * s, 30 * s, 6 * s, (230, 180, 30))
        pen.d.pieslice([(hx - 22 * s) * S, (hy - 22 * s) * S, (hx + 22 * s) * S, (hy + 18 * s) * S], 180, 360, fill=(250, 200, 40))


def claude(pen, x, y, t, dizzy=0.0):
    pen.ell(x, y, 34, 34, CLAUDE)
    for k in range(8):
        a = k * math.pi / 4 + t * 0.05
        pen.line([(x + 38 * math.cos(a), y + 38 * math.sin(a)), (x + 46 * math.cos(a), y + 46 * math.sin(a))], CLAUDE, 4)
    for ex in (-11, 11):
        if dizzy > 0.3:
            pts = [(x + ex + (r / 2) * math.cos(r + t), y - 4 + (r / 2) * math.sin(r + t)) for r in [i * 0.4 for i in range(16)]]
            pen.line(pts, INK, 2)
        else:
            pen.ell(x + ex, y - 4, 4, 5, INK)
    if dizzy > 0.3:
        pen.ell(x, y + 14, 6, 4, INK)
    else:
        pen.d.arc([(x - 10) * S, (y + 2) * S, (x + 10) * S, (y + 16) * S], 20, 160, fill=INK, width=2 * S)


def card(pen, x, y, text, color=PAPER, ink=INK, size=11):
    w = pen.width_of(text, size, mono=True) + 12
    pen.rect(x, y, x + w, y + 20, color, radius=5, outline=(200, 190, 175), width=1)
    pen.text(x + 6, y + 10, text, size, ink, mono=True, anchor='lm')

    return w


def captions(pen, title, line, tag=None):
    pen.rect(16, 14, 16 + pen.width_of(title, 26, bold=True) + 28, 58, PAPER, radius=12, outline=(220, 205, 180), width=1)
    pen.text(30, 36, title, 26, INK, bold=True, anchor='lm')
    if tag:
        tw = pen.width_of(tag, 13, mono=True) + 16
        pen.rect(W - tw - 16, 22, W - 16, 48, INK, radius=8)
        pen.text(W - tw / 2 - 16, 35, tag, 13, PAPER, mono=True, anchor='mm')
    words, lines, cur = line.split(), [], ''
    for word in words:
        if pen.width_of(cur + ' ' + word, 17) > W - 80:
            lines.append(cur)
            cur = word
        else:
            cur = (cur + ' ' + word).strip()
    lines.append(cur)
    top = H - 22 - 26 * len(lines)
    pen.rect(24, top - 10, W - 24, H - 16, PAPER, radius=12, outline=(220, 205, 180), width=1)
    for n, text in enumerate(lines):
        pen.text(W / 2, top + 13 + 26 * n, text, 17, INK, anchor='mm')


# ---- scenes: each draws frame i of n -------------------------------------------------------------

def title(pen, i, n):
    p = i / n
    scenery(pen, i)
    for k in range(6):
        log(pen, 250 + k * 50, 330 - (k % 2) * 6, 64)
    beaver(pen, 400, 278 - 20 * abs(math.sin(i * 0.25)) * (1 - p), 1.15, i, wave=True)
    pen.text(W / 2, 70, 'The dam', 54, INK, bold=True, anchor='mm')
    pen.text(W / 2, 118, 'Spend fewer tokens in Claude Code, without losing a line.', 19, INK, anchor='mm')
    pen.text(W / 2, 148, 'A plugin, told by beavers.', 15, (120, 100, 80), anchor='mm')


def flood(pen, i, n):
    p = i / n
    scenery(pen, i)
    claude(pen, 690, 250, i, dizzy=p)
    for k in range(int(4 + 30 * p)):
        x = (k * 97 + i * 9) % 640 - 40
        card(pen, x, 300 - (k % 4) * 26, f'ERROR retry {k + 1}' if k % 3 else 'npm WARN deprecated')
    pen.text(690, 190, '...4,000 lines?' if p > 0.4 else '', 15, INK, bold=True, anchor='mm')
    captions(pen, 'The problem', 'Coding agents drink a river of tool output. Most of it is the same log line, again and again. Claude gets dizzy. Your bill does not.')


def compression(pen, i, n):
    p = i / n
    scenery(pen, i)
    for k in range(5):
        log(pen, 330, 336 - k * 22, 60)
    beaver(pen, 300, 222, 0.9, i, chew=True)
    claude(pen, 700, 250, i)
    for k in range(8):
        x = ((k * 45 + i * 12) % 300) - 20
        card(pen, x, 300 - (k % 3) * 24, 'ERROR retry')
    count = int(4000 * ease(p))
    log(pen, 440 + 150 * ((i * 0.08) % 1), 300, 120, 13, label=f'ERROR retry ×{count}')
    pen.text(560, 190, f'{100 * ease(p) * 0.241:.0f}% fewer tokens, 100% of the lines', 15, GREEN, bold=True, anchor='mm')
    captions(pen, 'Compression', 'Identical lines become one log and a count; numbers that vary become a table. Every result is decoded and checked. Not one line lost (we checked. Twice.)', 'dam-squeeze')


def web(pen, i, n):
    p = i / n
    scenery(pen, i)
    junk = [('COOKIES? ACCEPT ALL!!', RED), ('MENU · MENU · MENU', (90, 90, 90)), ('AD: BUY MORE LOGS', ACCENT), ('Share · Like · Subscribe', (60, 90, 160)), ('Footer © 1998–2026', (120, 120, 120))]
    pen.rect(90, 80, 330, 300, PAPER, radius=8, outline=(180, 170, 150), width=2)
    for k, (text, color) in enumerate(junk):
        go = seg(p, 0.1 + k * 0.12, 0.25 + k * 0.12)
        if go < 1:
            pen.rect(100 + go * 40, 92 + k * 36 + go * 220, 320 + go * 40, 120 + k * 36 + go * 220, color, radius=4)
            pen.text(210 + go * 40, 106 + k * 36 + go * 220, text, 12, PAPER, bold=True, anchor='mm')
    pen.rect(100, 272, 320, 292, (230, 240, 230), radius=4)
    pen.text(210, 282, 'the actual content', 11, INK, anchor='mm')
    beaver(pen, 395, 250, 0.9, i, chew=True)
    show = seg(p, 0.55, 0.85)
    if show:
        pen.rect(500, 120, 500 + 210 * show, 290, PAPER, radius=8, outline=(180, 170, 150), width=2)
        if show > 0.9:
            for k, line in enumerate(['# The actual content', '', 'Readable text, kept.', '| a table | kept |', '```code, kept```', '[links](made absolute)']):
                pen.text(514, 140 + k * 22, line, 12, INK, mono=True)
    size = 261 - (261 - 3.7) * seg(p, 0.1, 0.8)
    pen.text(210, 64, f'HTML {size:.1f} KB', 16, INK, bold=True, anchor='mm')
    captions(pen, 'Web pages', 'A page fetched with curl becomes clean Markdown: 261 KB of GitHub turns into 3.7 KB. The beaver eats the cookie banners, so Claude does not have to.', 'dam-squeeze --web')


def mapping(pen, i, n):
    p = i / n
    scenery(pen, i)
    open_w = 420 * seg(p, 0, 0.3)
    pen.rect(190, 90, 190 + max(open_w, 10), 300, (245, 230, 195), radius=6, outline=(170, 140, 90), width=2)
    nodes = [(220 + (k * 53) % 380, 120 + (k * 37) % 160) for k in range(22)]
    shown = int(len(nodes) * seg(p, 0.25, 0.55))
    for k in range(1, shown):
        a, b = nodes[k], nodes[(k * 7) % k]
        pen.line([a, b], (180, 150, 100), 1)
    for x, y in nodes[:shown]:
        pen.ell(x, y, 4, 4, FUR_DARK)
    beaver(pen, 120, 250, 0.85, i)
    if p > 0.55:
        pen.rect(40, 120, 175, 150, PAPER, radius=10, outline=(200, 190, 170), width=1)
        pen.text(108, 135, 'where is SmartCrusher?', 11, INK, anchor='mm')
    drop = seg(p, 0.65, 0.8)
    if drop:
        x, y = nodes[9]
        py = y - 60 * (1 - drop)
        pen.poly([(x, py), (x - 9, py - 16), (x + 9, py - 16)], RED)
        pen.ell(x, py - 20, 10, 10, RED)
        if drop >= 1:
            card(pen, x + 14, y - 30, 'crusher.rs:118 · called by 4', PAPER, INK)
    captions(pen, 'The map', 'Who defines, calls and imports what, plus your docs and settings. 2,551 files charted in 1.5 s, so Claude stops grepping the whole riverbed.', 'dam-graph')


def loop_guard(pen, i, n):
    p = i / n
    scenery(pen, i)
    cx, cy = 400, 360
    for r in range(5, 60, 9):
        pts = [(cx + (r + a * 3) * math.cos(a + i * 0.3) * 1.6, cy + (r + a * 3) * math.sin(a + i * 0.3) * 0.5) for a in [k * 0.3 for k in range(22)]]
        pen.line(pts, FOAM, 2)
    escape = seg(p, 0.6, 1)
    if escape:
        bx, by = cx + 260 * escape, cy - 20
    else:
        a = i * 0.6
        bx, by = cx + 50 * math.cos(a), cy - 10 + 14 * math.sin(a)
    claude(pen, bx, by - 30, i, dizzy=1 - escape)
    pen.text(bx, by - 90, 'oh. another way!' if escape else 'retry… retry… retry…', 14, INK, bold=True, anchor='mm')
    if p > 0.4:
        beaver(pen, 150, 270, 0.85, i)
        pen.line([(195, 250), (330 if escape == 0 else 300, 320)], FUR_DARK, 5)
    captions(pen, 'Loop guard', 'When a turn spins on the same error, a judge checks it is really stuck, then nudges Claude out of the whirlpool. Once. Politely. Beavers are polite.', 'TokenEconomizer')


def pruning(pen, i, n):
    p = i / n
    scenery(pen, i)
    pen.rect(250, 250, 560, 320, FUR, radius=6)
    for k in range(5):
        pen.line([(250 + k * 62, 250), (250 + k * 62, 320)], FUR_DARK, 2)
    kept = ['Read', 'Edit', 'Bash', 'Grep']
    tossed = ['NotebookEdit', 'CronCreate', 'LSP', 'RemoteTrigger', 'DesignSync']
    x = 262
    for name in kept:
        x += card(pen, x, 220, name, (220, 240, 220)) + 6
    for k, name in enumerate(tossed):
        go = seg(p, 0.1 + k * 0.14, 0.3 + k * 0.14)
        sx, ex, ey = 300 + k * 40, 20 + (k % 3) * 105, 286 + (k // 3) * 26
        tx = sx + go * (ex - sx)
        ty = 190 - math.sin(go * math.pi) * 90 + go * (ey - 190)
        card(pen, tx, ty, name, (245, 225, 215))
    beaver(pen, 650, 260, 0.85, i, flip=True, wave=True)
    gone = int(27 * seg(p, 0.1, 0.85))
    pen.text(140, 200, f'{gone} tools left on the shore', 15, INK, bold=True, anchor='mm')
    captions(pen, 'Tool pruning', 'Tools nobody used in five sessions stay on the shore, so every request carries a lighter raft. One command brings any of them back.', 'TokenEconomizer')


def facts(pen, i, n):
    p = i / n
    scenery(pen, i)
    pen.rect(340, 100, 640, 300, PAPER, radius=6, outline=(180, 170, 150), width=2)
    for k in range(7):
        pen.line([(352, 140 + k * 22), (628, 140 + k * 22)], (225, 215, 200), 1)
    pen.text(490, 118, 'what this repo taught me', 14, INK, bold=True, anchor='mm')
    notes = ['tests: python -m pytest', 'not pytest (not installed)', 'run npm run gen first', 'API_URL must be set']
    for k, note in enumerate(notes):
        shown = int(len(note) * seg(p, 0.1 + k * 0.15, 0.25 + k * 0.15))
        pen.text(360, 132 + k * 22 + 22, note[:shown], 13, INK, mono=True)
    if p > 0.8:
        pen.rect(470, 248, 630, 288, (230, 245, 230), radius=8, outline=GREEN, width=2)
        pen.text(550, 268, 'confirmed in 3 sessions', 12, GREEN, bold=True, anchor='mm')
    beaver(pen, 230, 250, 0.9, i, chew=True)
    captions(pen, 'Learned facts', 'The beaver remembers how your repository works, and tells Claude at the start of each session, instead of letting it rediscover pytest every morning.', 'TokenEconomizer')


def lazy(pen, i, n):
    p = i / n
    scenery(pen, i)
    pen.rect(380, 90, 640, 300, (40, 44, 52), radius=8)
    lines = int(50 - 49 * seg(p, 0.25, 0.75))
    for k in range(min(lines, 16)):
        pen.rect(396, 104 + k * 12, 396 + 60 + (k * 37) % 160, 110 + k * 12, (120, 160, 200) if k % 3 else (200, 160, 120), radius=2)
    pen.text(510, 286, f'{lines} line{"s" if lines > 1 else ""}', 14, PAPER, bold=True, anchor='mm')
    if p > 0.75:
        pen.text(510, 70, '−49 lines, same feature', 16, GREEN, bold=True, anchor='mm')
    beaver(pen, 230, 250, 0.95, i, hat=True)
    captions(pen, 'Beaver mode', 'Built on Ponytail: the smallest change that fully solves the task. The best code is the code never written. He is very lazy. Professionally.', '/beaver full')


def ending(pen, i, n):
    p = i / n
    scenery(pen, i)
    for k in range(8):
        log(pen, 170 + k * 58, 330 - (k % 2) * 6, 64)
    beaver(pen, 150, 270, 0.8, i, wave=True)
    beaver(pen, 650, 270, 0.8, i + 9, flip=True, wave=True)
    status = 'full · 3.1k 25% · 27 · 1 · 5 · 2.2k · 31%'
    shown = int(len(status) * seg(p, 0.05, 0.5))
    pen.rect(150, 110, 650, 160, (30, 30, 30), radius=10)
    pen.text(170, 135, status[:shown] + ('▌' if i % 6 < 3 else ''), 18, (230, 230, 230), mono=True, anchor='lm')
    pen.text(W / 2, 90, 'your status line, one figure per beaver job', 13, (110, 90, 70), anchor='mm')
    if p > 0.55:
        pen.rect(190, 190, 610, 236, PAPER, radius=10, outline=(200, 190, 170), width=1)
        pen.text(W / 2, 213, '/plugin install the-dam@the-dam', 18, INK, mono=True, anchor='mm')
    captions(pen, 'The dam', 'All local, all Rust, no line lost. Now go build something. The beavers have it from here.')


SCENES = [(title, 26), (flood, 34), (compression, 40), (web, 44), (mapping, 42), (loop_guard, 40), (pruning, 40), (facts, 40), (lazy, 38), (ending, 50)]


def main():
    frames, durations = [], []
    for scene, n in SCENES:
        for i in range(n):
            pen = Pen()
            scene(pen, i, n - 1)
            frames.append(pen.done().quantize(colors=96, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE))
            durations.append(100)
        durations[-1] = 1800
    durations[-1] = 4000
    frames[0].save(os.path.join(HERE, 'teaser.gif'), save_all=True, append_images=frames[1:], duration=durations, loop=0, optimize=True, disposal=1)
    still = Pen()
    ending(still, 49, 49)
    still.done().save(os.path.join(HERE, 'teaser.png'), optimize=True)
    print(len(frames), 'frames', os.path.getsize(os.path.join(HERE, 'teaser.gif')) // 1024, 'KB')


if __name__ == '__main__':
    main()
