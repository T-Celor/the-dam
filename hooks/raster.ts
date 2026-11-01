// The terminal's dam: a software renderer for the same world the panel draws. Each cell is one quadrant glyph,
// so it holds four smoothed pixels in two colours. The still layers are kept until the stats change; only the movers cost anything per frame.

import type { Heat, Stats } from '../types'
import { blend, clamp, noise } from './engine'
import type { Face, World } from './iso'
import { buildWorld, iconFaces, poseOf, strengthFor } from './iso'
import { pressureOf, stockOf } from './model'
import { counterEntries, formatCount } from './scene'

const WHITE = 0xffffff
const CLEAR = 0xffffffff

// `ratio` is a pixel's height over its width: a terminal's pixels are not square.
type Canvas = { width: number; height: number; pixels: Uint32Array; ratio?: number }

function put(c: Canvas, x: number, y: number, color: number) {
  const px = Math.round(x)
  const py = Math.round(y)
  if (px >= 0 && py >= 0 && px < c.width && py < c.height) c.pixels[py * c.width + px] = color
}

function fill(c: Canvas, x: number, y: number, w: number, tall: number, color: number) {
  for (let py = Math.round(y); py < Math.round(y + tall); py++) for (let px = Math.round(x); px < Math.round(x + w); px++) put(c, px, py, color)
}

// Scanline fill of one polygon, `scale` being from world units to pixels.
function fillFace(c: Canvas, face: Face, scale: number, dx = 0, dy = 0, angle = 0, alpha = 1) {
  const opacity = (face.alpha ?? 1) * alpha
  if (opacity <= 0) return
  const [cos, sin] = [Math.cos(angle), Math.sin(angle)]
  const count = face.pts.length / 2
  const xs = new Array<number>(count)
  const ys = new Array<number>(count)
  for (let i = 0; i < count; i++) {
    const [px, py] = [face.pts[i * 2] ?? 0, face.pts[i * 2 + 1] ?? 0]
    xs[i] = (px * cos - py * sin + dx) * scale
    ys[i] = ((px * sin + py * cos + dy) * scale) / (c.ratio ?? 1)
  }
  const paint = (at: number) => {
    const under = c.pixels[at] ?? 0
    c.pixels[at] = opacity >= 1 || under === CLEAR ? face.color : blend(under, face.color, opacity)
  }
  const low = Math.max(0, Math.floor(Math.min(...xs.length ? ys : [0])))
  const high = Math.min(c.height - 1, Math.ceil(Math.max(...ys)))
  let didPaint = false
  for (let y = low; y <= high; y++) {
    const line = y + 0.5
    const cuts: number[] = []
    for (let i = 0; i < count; i++) {
      const j = (i + 1) % count
      const [ya, yb] = [ys[i] ?? 0, ys[j] ?? 0]
      if (ya <= line !== yb <= line) cuts.push((xs[i] ?? 0) + ((line - ya) / (yb - ya)) * ((xs[j] ?? 0) - (xs[i] ?? 0)))
    }
    cuts.sort((a, b) => a - b)
    for (let n = 0; n + 1 < cuts.length; n += 2) {
      const from = Math.max(0, Math.ceil((cuts[n] ?? 0) - 0.5))
      const to = Math.min(c.width - 1, Math.ceil((cuts[n + 1] ?? 0) - 0.5) - 1)
      for (let x = from; x <= to; x++) {
        paint(y * c.width + x)
        didPaint = true
      }
    }
  }
  // A facet smaller than a pixel still has to show.
  if (!didPaint) {
    const [x, y] = [Math.floor(xs[0] ?? -1), Math.floor(ys[0] ?? -1)]
    if (x >= 0 && y >= 0 && x < c.width && y < c.height) paint(y * c.width + x)
  }
}

function drawGame(c: Canvas, s: Stats, t: number) {
  const size = Math.max(1, Math.floor(c.width / 90))
  for (let i = 0; i < Math.min(7, s.pennants); i++) {
    const x = c.width - 4 - i * 4 * size
    const flap = Math.floor(t * 3 + i) % 2
    const color = [0xe0553f, 0xf2bf2e, 0x2aa198][i % 3] ?? 0
    fill(c, x, 2, size * 3, size, color)
    fill(c, x + flap * size, 2 + size, size * 2, size, color)
    fill(c, x + size, 2 + size * 2, size, size, color)
  }
  if (s.isParty) {
    for (let i = 0; i < c.width; i++) {
      const life = (t * (0.5 + noise(i, 1) * 0.5) + noise(i, 2)) % 1
      put(c, noise(i, 3) * c.width, life * c.height, [0xf2bf2e, 0xe0553f, 0x2aa198, 0x6f7fd0, 0xff8fb4][i % 5] ?? 0)
    }
  }
}

type Still = { key: string; world: World; ground: Uint32Array; solids: Uint32Array; scale: number }
let still: Still | null = null

function stillFor(s: Stats, levels: Heat, width: number, height: number, ratio: number): Still {
  const key = JSON.stringify([s, levels, width, height, ratio])
  if (still?.key === key) return still
  // A small panel looks closer in, so a tree is still a tree at 60 pixels across.
  const tiles = 12 + 3 * clamp((width - 120) / 100, 0, 1)
  const world = buildWorld(s, levels, { width: 640, top: 0, height: (640 * height * ratio) / width, tiles })
  const scale = width / 640
  const layer = (faces: readonly Face[], base: number) => {
    const c: Canvas = { width, height, ratio, pixels: new Uint32Array(width * height).fill(base) }
    for (const face of faces) fillFace(c, face, scale)

    return c.pixels
  }
  still = { key, world, scale, ground: layer(world.ground, 0x7cb661), solids: layer(world.solids.flatMap(piece => piece.faces), CLEAR) }

  return still
}

export function renderPixels(s: Stats, levels: Heat, t: number, width: number, height: number, ratio = 1): Uint32Array {
  const { world, ground, solids, scale } = stillFor(s, levels, width, height, ratio)
  const c: Canvas = { width, height, ratio, pixels: ground.slice() }
  const percent = clamp(s.river ?? s.context?.percent ?? 0, 0, 100)
  const pressure = pressureOf(percent, strengthFor(s))
  const drawMovers = (movers: World['flow']) => {
    for (const m of movers) {
      const pose = poseOf(m, world.path, t)
      for (const face of m.faces) fillFace(c, face, scale, pose.dx, pose.dy, pose.angle, pose.alpha)
    }
  }
  drawMovers(world.flow)
  for (let i = 0; i < solids.length; i++) if (solids[i] !== CLEAR) c.pixels[i] = solids[i] ?? 0
  drawMovers(world.top)
  drawGame(c, s, t)
  if (pressure > 0.6 && Math.floor(t * 7) % 53 === 0) for (let i = 0; i < c.pixels.length; i++) c.pixels[i] = blend(c.pixels[i] ?? 0, WHITE, 0.4)

  return c.pixels
}

// Quadrant glyphs by which corners take the foreground: top left 1, top right 2, bottom left 4, bottom right 8.
const QUADRANTS = [0x20, 0x2598, 0x259d, 0x2580, 0x2596, 0x258c, 0x259e, 0x259b, 0x2597, 0x259a, 0x2590, 0x259c, 0x2584, 0x2599, 0x259f, 0x2588]
const gap = (a: number, b: number) => Math.abs((a >> 16) - (b >> 16)) + Math.abs(((a >> 8) & 255) - ((b >> 8) & 255)) + Math.abs((a & 255) - (b & 255))

// A cell shows two colours: it splits its four pixels around the two furthest apart, and shows the mean of each side.
export function quadrantOf(corners: readonly [number, number, number, number]): [number, number, number] {
  let [fg, bg, widest] = [corners[0], corners[0], -1]
  for (let a = 0; a < 4; a++) {
    for (let b = a + 1; b < 4; b++) {
      const distance = gap(corners[a] ?? 0, corners[b] ?? 0)
      if (distance > widest) [fg, bg, widest] = [corners[a] ?? 0, corners[b] ?? 0, distance]
    }
  }
  if (widest === 0) return [0x20, fg, fg]
  const mask = corners.reduce((bits, color, i) => (gap(color, fg) <= gap(color, bg) ? bits | (1 << i) : bits), 0)
  const near = corners.filter((_, i) => mask & (1 << i))
  const far = corners.filter((_, i) => !(mask & (1 << i)))

  return [QUADRANTS[mask] ?? 0x20, mean(near), mean(far)]
}

const INK = 0xffffff
const LOST = 0xffb27a
const PANEL = 0x10100e

// The bottom row is text, so the counters stay sharp whatever the panel's size.
export function counterLine(s: Stats, columns: number): Array<[string, number]> {
  const stock = stockOf(s.earned, s.wear)
  const river = s.context ? `${formatCount(s.context.total)}/${formatCount(s.context.max)}` : '?'
  const words = columns >= 44 ? ['river ', ' twigs ', ' logs ', ' trunks '] : ['~', ' /', ' =', ' #']
  const piece = (word: string, value: number, earned: number): Array<[string, number]> => [
    [`${word}${formatCount(value)}`, INK],
    [earned > value ? ` -${formatCount(earned - value)}` : '', LOST],
  ]

  return [
    [`${words[0]}${river}`, 0x8fd8f5],
    ...piece(words[1] ?? '', stock.twigs, s.earned.twigs),
    ...piece(words[2] ?? '', stock.logs, s.earned.logs),
    ...piece(words[3] ?? '', stock.trunks, s.earned.trunks),
  ]
}

const mean = (colors: readonly number[]) => {
  const total = [0, 0, 0]
  for (const color of colors) {
    total[0] = (total[0] ?? 0) + (color >> 16)
    total[1] = (total[1] ?? 0) + ((color >> 8) & 255)
    total[2] = (total[2] ?? 0) + (color & 255)
  }
  const channel = (i: number) => Math.round((total[i] ?? 0) / colors.length)

  return (channel(0) << 16) | (channel(1) << 8) | channel(2)
}

// Four rendered pixels across and down per cell of about 8 by 18: each is 2 wide and 4.5 tall.
const SUB_RATIO = 2.25
const WIDE_BLOCK = 80
const ICON_COLUMNS = 6
const DIM = 0xb9c4bb

// The engine has toBase64; this TypeScript lib does not know it yet.
const encode = (words: Uint32Array) => (new Uint8Array(words.buffer) as Uint8Array & { toBase64(): string }).toBase64()

// Each counter gets a little 3D icon, its figure and name, and a line saying what it is.
function paintBlock(words: Uint32Array, s: Stats, columns: number, firstRow: number, rows: number) {
  const across = columns >= WIDE_BLOCK ? 2 : 1
  const half = Math.floor(columns / across)
  const cell = (row: number, x: number, code: number, fg: number, bg = PANEL) => {
    const at = (row * columns + x) * 3
    words[at] = code
    words[at + 1] = fg
    words[at + 2] = bg
  }
  for (let row = firstRow; row < rows; row++) for (let x = 0; x < columns; x++) cell(row, x, 32, DIM)
  counterEntries(s).forEach((entry, i) => {
    const left = (i % across) * half
    const row = firstRow + Math.floor(i / across) * 2
    const icon: Canvas = { width: ICON_COLUMNS * 2, height: 4, pixels: new Uint32Array(ICON_COLUMNS * 8).fill(PANEL) }
    for (const face of iconFaces(entry.icon, 2.6)) fillFace(icon, face, 1, ICON_COLUMNS, 2)
    for (let down = 0; down < 2; down++) {
      for (let x = 0; x < ICON_COLUMNS; x++) {
        const at = down * 2 * icon.width + x * 2
        const [glyph, fg, bg] = quadrantOf([icon.pixels[at] ?? 0, icon.pixels[at + 1] ?? 0, icon.pixels[at + icon.width] ?? 0, icon.pixels[at + icon.width + 1] ?? 0])
        cell(row + down, left + x, glyph, fg, bg)
      }
    }
    const write = (y: number, text: string, color: number) => {
      const room = half - ICON_COLUMNS - 1
      for (let n = 0; n < Math.min(room, text.length); n++) cell(y, left + ICON_COLUMNS + n, text.charCodeAt(n), color)
    }
    write(row, `${entry.value} ${entry.word}${entry.lost > 0 ? ` -${formatCount(entry.lost)}` : ''}`, entry.lost > 0 ? LOST : INK)
    write(row + 1, entry.note, DIM)
  })
}

// Packs a frame the way Raster takes it: [codePoint, foreground, background] per cell.
export function frameCells(s: Stats, levels: Heat, t: number, columns: number, rows: number): string {
  // Two by two when wide, stacked when narrow, and one plain line when there is no room for either.
  const blockRows = columns >= WIDE_BLOCK ? 4 : 8
  const hasBlock = (columns >= WIDE_BLOCK && rows >= 12) || (columns >= 36 && rows >= 20)
  const pictureRows = Math.max(1, rows - (hasBlock ? blockRows : 1))
  const across = columns * 4
  const pixels = renderPixels(s, levels, t, across, pictureRows * 4, SUB_RATIO)
  const words = new Uint32Array(columns * rows * 3)
  for (let row = 0; row < pictureRows; row++) {
    for (let x = 0; x < columns; x++) {
      const at = (row * columns + x) * 3
      const smooth = (down: number, side: number) => {
        const from = (row * 4 + down * 2) * across + x * 4 + side * 2
        // Each glyph pixel averages four rendered ones, which is what softens the edges.
        return mean([pixels[from] ?? 0, pixels[from + 1] ?? 0, pixels[from + across] ?? 0, pixels[from + across + 1] ?? 0])
      }
      const [glyph, fg, bg] = quadrantOf([smooth(0, 0), smooth(0, 1), smooth(1, 0), smooth(1, 1)])
      words[at] = glyph
      words[at + 1] = fg
      words[at + 2] = bg
    }
  }
  if (hasBlock) {
    paintBlock(words, s, columns, pictureRows, rows)

    return encode(words)
  }
  let x = 0
  const put2 = (code: number, color: number) => {
    if (x >= columns) return
    const at = ((rows - 1) * columns + x) * 3
    words[at] = code
    words[at + 1] = color
    words[at + 2] = PANEL
    x++
  }
  put2(32, INK)
  for (const [text, color] of counterLine(s, columns)) for (const char of text) put2(char.charCodeAt(0), color)
  while (x < columns) put2(32, INK)

  return encode(words)
}
