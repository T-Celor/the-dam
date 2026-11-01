// The dam's world: real 3D solids, flat shaded and projected isometrically into flat polygons.
// Both renderers draw this one description, so the app and the terminal always show the same place.

import type { CritterId, Heat, Stats } from '../types'
import { blend, clamp, noise, rgb, seeded } from './engine'
import { isOverwhelmed, pressureOf, shapeOf, stockOf, strengthOf } from './model'

export type Face = { pts: number[]; color: number; alpha?: number; mark?: string }
export type Piece = { depth: number; faces: Face[]; sway?: boolean }
// flow follows the river; drop goes in a line; hop bounces; jump is a fish's arc; still holds.
export type Mode = 'flow' | 'drop' | 'hop' | 'jump' | 'still'
export type Mover = { cls: string; mode: Mode; faces: Face[]; seconds: number; phase: number; x: number; y: number; from: number; to: number }
export type World = { ground: Face[]; flow: Mover[]; solids: Piece[]; top: Mover[]; path: number[]; isFlooded: boolean }
export type View = { width: number; top: number; height: number; tiles: number }

type V3 = readonly [number, number, number]

const MAX_TWIGS = 170
// What fits on the dam and stays readable; the counter goes on beyond it.
const MOST_TWIGS = 90
const MOST_LOGS = 40
const MOST_TRUNKS = 12
const FALL_WIDTH = 300
const LIGHT: V3 = [-0.33, 0.52, 0.79]
const WHITE = 0xffffff

// log scale: 1k tokens is about 74 twigs, 1M about 147, so the pile keeps growing without filling up.
export function twigCount(saved: number): number {
  return saved <= 0 ? 0 : Math.min(MAX_TWIGS, Math.round(Math.log10(saved + 1) * 24.5))
}

// The river's width follows the context, up to what the dam's crest can carry.
export function fallWidth(percent: number, topHalf = 172): number {
  return percent <= 0 ? 0 : Math.min(topHalf * 1.72, Math.max(8, (percent / 100) * FALL_WIDTH))
}

export type Level = 0 | 1 | 2 | 3

export function levelOf(heat: number): Level {
  if (heat <= 0) return 0
  if (heat <= 2) return 1

  return heat <= 5 ? 2 : 3
}

export const strengthFor = (s: Stats): number => strengthOf(s.tools.pompom, s.earned, s.wear)

export function isFlooded(s: Stats): boolean {
  return isOverwhelmed(s.river ?? s.context?.percent ?? 0, strengthFor(s))
}

const smooth = (t: number) => {
  const x = clamp(t, 0, 1)

  return x * x * (3 - 2 * x)
}

// Rounded colours merge into fewer paths, and the coarse steps suit the low-poly look.
const facet = (color: number) => (color & 0xf8f8f8) | 0x040404

const WATER = ['#2a8fc4', '#38a6d6', '#4cb8dd', '#3a9db8', '#5f8fd8', '#7fc8e4', '#3f86ad'].map(rgb)
const BARK = rgb('#7d5230')
const SAWN = rgb('#e2c08e')
const WOOD = ['#c9a06a', '#a97b45', '#8a5e34', '#dcb988', '#75502c'].map(rgb)
const HAT: Record<CritterId, number> = { hazel: rgb('#f2bf2e'), mimi: rgb('#2f7ea3'), pompom: rgb('#8d6fd0'), truffle: rgb('#d9533c') }
const CREW: readonly CritterId[] = ['hazel', 'mimi', 'pompom', 'truffle']

function solidsFor(project: (p: V3) => [number, number], tint: (color: number) => number) {
  const lit = (color: number, a: V3, b: V3, c: V3) => {
    const u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]] as const
    const w = [c[0] - a[0], c[1] - a[1], c[2] - a[2]] as const
    const n = [u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0]] as const
    const size = Math.hypot(n[0], n[1], n[2]) || 1
    const facing = Math.abs(n[0] * LIGHT[0] + n[1] * LIGHT[1] + n[2] * LIGHT[2]) / size

    return tint(blend(blend(color, 0, 0.36), blend(color, WHITE, 0.14), facing))
  }
  const flat = (points: readonly V3[], color: number, mark?: string): Face => ({
    pts: points.flatMap(p => project(p)),
    color: lit(color, points[0] as V3, points[1] as V3, points[2] as V3),
    ...(mark ? { mark } : {}),
  })
  const faces2 = (points: readonly V3[]) => points.flatMap(p => project(p))
  const isFront = (pts: number[]) => {
    let area = 0
    for (let i = 0; i < pts.length; i += 2) {
      const j = (i + 2) % pts.length
      area += (pts[i] ?? 0) * (pts[j + 1] ?? 0) - (pts[j] ?? 0) * (pts[i + 1] ?? 0)
    }

    return area > 0
  }
  // Any cylinder, cone or box: rings around an axis, with its hidden sides dropped.
  const frustum = (a: V3, b: V3, r1: number, r2: number, sides: number, color: number, cap = color, mark?: string): Face[] => {
    const axis = [b[0] - a[0], b[1] - a[1], b[2] - a[2]] as const
    const isUpright = Math.abs(axis[2]) > Math.abs(axis[1])
    const e1: V3 = isUpright ? [1, 0, 0] : [1, 0, 0]
    const e2: V3 = isUpright ? [0, 1, 0] : [0, 0, 1]
    const ring = (c: V3, r: number) =>
      Array.from({ length: sides }, (_, i): V3 => {
        const at = (i / sides) * Math.PI * 2 + 0.4
        const [p, q] = [Math.cos(at) * r, Math.sin(at) * r]

        return [c[0] + e1[0] * p + e2[0] * q, c[1] + e1[1] * p + e2[1] * q, c[2] + e1[2] * p + e2[2] * q]
      })
    const low = ring(a, r1)
    const high = ring(b, r2)
    const out: Face[] = []
    const add = (points: V3[], paint: number) => {
      const pts = faces2(points)
      if (isFront(pts) === isUpright) out.push({ pts, color: lit(paint, points[0] as V3, points[1] as V3, points[2] as V3) })
    }
    for (let i = 0; i < sides; i++) {
      const j = (i + 1) % sides
      add([low[i] as V3, low[j] as V3, high[j] as V3, high[i] as V3], blend(color, 0, noise(i, sides) * 0.12))
    }
    if (r2 > 0) add([...high], cap)
    if (r1 > 0) add([...low].reverse(), cap)
    const first = out[0]
    if (first && mark) first.mark = mark

    return out
  }

  return { lit, flat, faces2, frustum }
}

export type Icon = 'river' | 'twigs' | 'logs' | 'trunks'

// The counters' icons: small solids of the very things they count, `k` pixels to the tile, centred on 0 0.
export function iconFaces(icon: Icon, k: number): Face[] {
  const { frustum } = solidsFor(([x, y, z]) => [(x - y) * 0.866 * k, (x + y) * 0.5 * k - z * 0.82 * k], facet)
  if (icon === 'river') {
    const blue = rgb('#4cb8dd')

    return [...frustum([0, 0, -0.55], [0, 0, -0.15], 0.25, 0.46, 6, blue), ...frustum([0, 0, -0.15], [0, 0, 0.7], 0.46, 0, 6, blue)]
  }
  if (icon === 'twigs') {
    const stick = (x: number, z: number, skew: number, color: number) => frustum([x - skew, -0.72, z], [x + skew, 0.72, z], 0.07, 0.05, 4, color, SAWN)

    return [...stick(-0.25, -0.2, 0.3, WOOD[1] ?? 0), ...stick(0.2, -0.05, -0.28, WOOD[0] ?? 0), ...stick(0, 0.15, 0.12, WOOD[3] ?? 0)]
  }
  if (icon === 'logs') return frustum([0, -0.6, 0], [0, 0.6, 0], 0.3, 0.3, 6, rgb('#9c6d3e'), SAWN)

  return frustum([0, -0.85, 0], [0, 0.85, 0], 0.5, 0.5, 7, BARK, SAWN)
}

export function buildWorld(s: Stats, levels: Heat, view: View): World {
  const percent = clamp(s.river ?? s.context?.percent ?? 0, 0, 100)
  const storm = clamp((percent - 45) / 50, 0, 1)
  const strength = strengthFor(s)
  const pressure = pressureOf(percent, strength)
  const flooded = isFlooded(s)
  const shape = shapeOf(strength)
  const stock = stockOf(s.earned, s.wear)
  const k = view.width / view.tiles
  const cx = view.width / 2
  // A wide, low view needs the dam lower down, or its crest leaves the frame.
  const cy = view.top + view.height * (view.height / view.width < 0.6 ? 0.56 : 0.42)
  // A low-resolution view needs its characters larger than life to stay readable.
  const big = 1 + ((15 - view.tiles) / 3) * 0.3

  const crestHalf = shape.topHalf / 50
  const damTall = shape.height / 130
  const thick = 0.7 + 0.8 * shape.size
  const slope = damTall * 0.75
  const bow = 0.45 + 0.55 * shape.size
  // A stream, not a great river: even a full context stays narrower than the dam.
  const river = flooded ? 2.2 : s.context ? 0.08 + fallWidth(percent, shape.topHalf) / 480 : 0.08
  const lake = damTall * (0.4 + 0.5 * (percent / 100))
  const step = damTall * 0.96

  const project = ([x, y, z]: V3): [number, number] => [cx + (x - y) * 0.866 * k, cy + (x + y) * 0.5 * k - z * 0.82 * k]
  const toWorld = (sx: number, sy: number): [number, number] => {
    const a = (sx - cx) / (0.866 * k)
    const b = (sy - cy) / (0.5 * k)

    return [(a + b) / 2, (b - a) / 2]
  }
  const dusk = rgb('#39465a')
  const tint = (color: number) => facet(blend(color, dusk, storm * 0.38))
  const { lit, flat, faces2, frustum } = solidsFor(project, tint)

  // The course: straight through the dam, winding between the panel's sides above and below it, however tall it is.
  const bend = (from: number, direction: number, stop: (y: number) => boolean) => {
    const points: Array<[number, number]> = []
    let y = from
    let side = direction
    while (!stop(y)) {
      y += direction > 0 ? 230 : -230
      side = -side
      points.push(toWorld(view.width * (side > 0 ? 0.62 : 0.38), y))
    }

    return points
  }
  const below = bend(project([4.6, -0.4, 0])[1], 1, y => y > view.top + view.height + 60)
  const above = bend(project([-4.6, 0.4, 0])[1], -1, y => y < view.top - 60).reverse()
  const course: Array<[number, number]> = [...above, [-4.6, 0.4], [-2.4, 0], [0, 0], [2.4, 0], [4.6, -0.4], ...below]
  const trail: Array<[number, number]> = []
  for (let i = 0; i < course.length - 1; i++) {
    const at = (n: number) => course[clamp(n, 0, course.length - 1)] as [number, number]
    const [p0, p1, p2, p3] = [at(i - 1), at(i), at(i + 1), at(i + 2)]
    for (let t = 0; t < 1; t += 1 / 7) {
      const spline = (a: number, b: number, c: number, d: number) => 0.5 * (2 * b + (c - a) * t + (2 * a - 5 * b + 4 * c - d) * t * t + (3 * b - 3 * c + d - a) * t * t * t)
      trail.push([spline(p0[0], p1[0], p2[0], p3[0]), spline(p0[1], p1[1], p2[1], p3[1])])
    }
  }
  trail.push(course[course.length - 1] as [number, number])
  const widthAt = (x: number) => (x < 0 && !flooded ? Math.min(crestHalf * 0.92, river * (1 + 2.6 * smooth((x + 5) / 3.5) * smooth(-x / 0.6))) : river)
  const distance = (x: number, y: number) => {
    let best = Infinity
    for (const [px, py] of trail) best = Math.min(best, Math.hypot(x - px, y - py))

    return best
  }
  const height = (x: number, y: number, far: number) => {
    const hills = 0.45 * (Math.sin(x * 0.37 + 1.3) * Math.cos(y * 0.31) + 0.5 * Math.sin(x * 0.83 + y * 0.57) + 0.6)

    return step * smooth((0.5 - bow - x) / 2.2) + Math.max(0, hills) * smooth((far - 0.8) / 3.5)
  }

  // Ground: a jittered triangle mesh, shaded from its real slopes.
  const ground: Face[] = []
  const corners = [toWorld(0, view.top), toWorld(view.width, view.top), toWorld(0, view.top + view.height), toWorld(view.width, view.top + view.height)]
  // Coarser in a tall view, so the mesh stays within what the panel accepts.
  const CELL = 2.8 * Math.max(1, Math.sqrt(view.height / view.width / 0.75))
  const [minX, maxX] = [Math.min(...corners.map(c => c[0])) - 3, Math.max(...corners.map(c => c[0])) + 3]
  const [minY, maxY] = [Math.min(...corners.map(c => c[1])) - 3, Math.max(...corners.map(c => c[1])) + 3]
  const vertex = (i: number, j: number): V3 => {
    const x = i * CELL + (noise(i, j) - 0.5) * 0.8
    const y = j * CELL + (noise(j, i + 9) - 0.5) * 0.8

    return [x, y, height(x, y, distance(x, y))]
  }
  const grass = ['#8fc46f', '#8ac069'].map(rgb)
  const SAND = rgb('#dcc98f')
  const tri = (a: V3, b: V3, c: V3, seed: number) => {
    const pts = faces2([a, b, c])
    const xs = [pts[0] ?? 0, pts[2] ?? 0, pts[4] ?? 0]
    const ys = [pts[1] ?? 0, pts[3] ?? 0, pts[5] ?? 0]
    const margin = k * 2
    if (Math.max(...xs) < -margin || Math.min(...xs) > view.width + margin || Math.max(...ys) < view.top - margin || Math.min(...ys) > view.top + view.height + margin) return
    const mx = (a[0] + b[0] + c[0]) / 3
    const my = (a[1] + b[1] + c[1]) / 3
    const isShore = distance(mx, my) < widthAt(mx) + 0.5
    const base = isShore ? SAND : (grass[Math.floor(noise(seed, 3) * 2)] ?? 0)
    ground.push({ pts, color: facet(blend(tint(base), lit(base, a, b, c), 0.3)) })
  }
  const firstI = Math.floor(minX / CELL)
  const firstJ = Math.floor(minY / CELL)
  for (let i = firstI; i * CELL < maxX; i++) {
    for (let j = firstJ; j * CELL < maxY; j++) {
      const [a, b, c, d] = [vertex(i, j), vertex(i + 1, j), vertex(i + 1, j + 1), vertex(i, j + 1)]
      tri(a, b, c, i * 31 + j)
      tri(a, c, d, i * 37 + j * 5)
    }
  }

  // One colour after another: the hills are too gentle to hide each other, and this shrinks the panel's source a lot.
  ground.sort((a, b) => a.color - b.color)

  // The river as ribbons of facets over the mesh: one stripe per context layer below the dam.
  const fallsAt = thick / 2 + slope - bow - 0.1
  const waterZ = (x: number) => (x < fallsAt && !flooded ? lake : 0)
  const crossAt = (i: number) => {
    const here = trail[i] as [number, number]
    const prev = trail[Math.max(0, i - 1)] as [number, number]
    const after = trail[Math.min(trail.length - 1, i + 1)] as [number, number]
    const [dx, dy] = [after[0] - prev[0], after[1] - prev[1]]
    const size = Math.hypot(dx, dy) || 1

    return { x: here[0], y: here[1], nx: -dy / size, ny: dx / size, half: widthAt(here[0]) }
  }
  const total = s.context?.layers.reduce((sum, layer) => sum + layer.tokens, 0) ?? 0
  const layers = total > 0 ? (s.context?.layers ?? []).map(layer => layer.tokens / total) : [1]
  const hasWater = flooded || (s.context !== null && percent > 0)
  if (hasWater) {
    for (let i = 0; i < trail.length - 1; i++) {
      const [a, b] = [crossAt(i), crossAt(i + 1)]
      const edge = (c: typeof a, at: number): V3 => [c.x + c.nx * c.half * at, c.y + c.ny * c.half * at, waterZ(c.x)]
      if (a.x < fallsAt && !flooded) {
        ground.push({ pts: faces2([edge(a, -1), edge(b, -1), edge(b, 1), edge(a, 1)]), color: tint(blend(rgb('#2f93c8'), rgb('#4cb8dd'), noise(i, 11) * 0.5)) })
        continue
      }
      let across = -1
      layers.forEach((share, n) => {
        const next = across + share * 2
        const color = blend(WATER[n % WATER.length] ?? 0, WHITE, noise(i, n) * 0.1)
        ground.push({ pts: faces2([edge(a, across), edge(b, across), edge(b, next), edge(a, next)]), color: tint(color) })
        across = next
      })
    }
  }

  const path = trail.flatMap(([x, y]) => project([x, y, waterZ(x)]))
  const damAt = trail.findIndex(([x]) => x >= fallsAt) / Math.max(1, trail.length - 1)
  const rng = seeded(21)
  const make = (cls: string, mode: Mode, faces: Face[], low: number, high: number, extra: Partial<Mover> = {}): Mover => ({
    cls,
    mode,
    faces,
    seconds: low + rng() * (high - low),
    phase: rng(),
    x: 0,
    y: 0,
    from: 0,
    to: 1,
    ...extra,
  })
  const box = (x: number, y: number, w: number, h: number, color: number, alpha?: number): Face => ({
    pts: [x - w, y - h, x + w, y - h, x + w, y + h, x - w, y + h],
    color,
    ...(alpha === undefined ? {} : { alpha }),
  })

  // Streaks ride the river; there are more of them, and they go faster, as the context fills.
  const flow: Mover[] = []
  if (hasWater) {
    const lap = Math.max(5, 26 - percent * 0.19) * (flooded ? 0.4 : 1)
    const count = flooded ? 70 : Math.round(5 + percent * 0.08)
    for (let i = 0; i < count; i++) {
      const lateral = (rng() * 2 - 1) * river * k * 0.72
      const long = (3 + rng() * 6) * (1 + percent / 200)
      flow.push(make('p flow streak', 'flow', [box(0, lateral, long, 0.9, WHITE, 0.75)], lap * 0.85, lap * 1.2))
    }
  }

  const solids: Piece[] = []
  const top: Mover[] = []

  // Forest: low-poly pines and round trees, standing wherever the river and the dam leave room.
  const treeAt = (x: number, y: number, seed: number): Piece => {
    const size = 0.85 + noise(seed, 1) * 0.6
    const z = height(x, y, distance(x, y))
    const trunk = frustum([x, y, z], [x, y, z + 0.5 * size], 0.12 * size, 0.09 * size, 5, rgb('#6d4a2c'))
    const green = [rgb('#2f7d55'), rgb('#3c8f5e'), rgb('#256b4a')][seed % 3] ?? 0
    const crown =
      seed % 4 === 0
        ? [...frustum([x, y, z + 0.4 * size], [x, y, z + 1 * size], 0.42 * size, 0.56 * size, 6, rgb('#5fa84f')), ...frustum([x, y, z + 1 * size], [x, y, z + 1.5 * size], 0.56 * size, 0.1 * size, 6, rgb('#6fb95c'))]
        : [0, 1, 2].flatMap(n => frustum([x, y, z + (0.4 + n * 0.5) * size], [x, y, z + (1.2 + n * 0.5) * size], (0.6 - n * 0.14) * size, 0, 6, green))

    return { depth: x + y, faces: [...trunk, ...crown], sway: true }
  }
  let seed = 0
  // Sparser in a tall view, so the forest stays within what the panel accepts.
  const grove = 2.9 * Math.max(1, Math.sqrt(view.height / view.width / 0.75))
  for (let i = Math.floor(minX / grove); i * grove < maxX; i++) {
    for (let j = Math.floor(minY / grove); j * grove < maxY; j++) {
      seed++
      if (noise(i, j + 50) < 0.55) continue
      const x = (i + noise(i + 7, j)) * grove
      const y = (j + noise(i, j + 7)) * grove
      const [sx, sy] = project([x, y, 0])
      if (sx < -k || sx > view.width + k || sy < view.top - k || sy > view.top + view.height + 2 * k) continue
      if (distance(x, y) < widthAt(x) + 1.7 || (Math.abs(y) < crestHalf + 1.8 && Math.abs(x + bow / 2) < thick + slope + bow + 1.4)) continue
      solids.push(treeAt(x, y, seed))
    }
  }

  if (!flooded) {
    const dam: Face[] = []
    const front = thick / 2
    const foot = front + slope
    const MUD = rgb('#6f4d30')
    // A real beaver dam: an arc bowed against the current, a lumpy mound of mud that thins into the banks.
    const crown = (y: number) => 0.62 + 0.38 * Math.sqrt(clamp(1 - (y / (crestHalf + 0.6)) ** 2, 0, 1))
    const archX = (y: number) => bow * ((y / (crestHalf + 0.6)) ** 2 - 1)
    const onSlope = (at: number, y: number): V3 => [archX(y) + foot - slope * at, y, damTall * crown(y) * at]
    const SEGMENTS = 12
    const STATIONS = [-front, front, front + slope * 0.5, foot]
    const LIFTS = [0.72, 1, 0.52, 0]
    const mound = (n: number, u: number): V3 => {
      const y = ((n / SEGMENTS) * 2 - 1) * (crestHalf + 0.6)
      const lump = u === 3 ? 0 : (noise(n, u + 40) - 0.5) * 0.2

      return [archX(y) + (STATIONS[u] ?? 0) + (noise(u + 3, n) - 0.5) * 0.22, y, damTall * crown(y) * ((LIFTS[u] ?? 0) + lump)]
    }
    let isMarked = false
    for (let u = 0; u < 3; u++) {
      for (let n = 0; n < SEGMENTS; n++) {
        const [a, b, c, d] = [mound(n, u), mound(n, u + 1), mound(n + 1, u + 1), mound(n + 1, u)]
        const paint = blend(MUD, rgb('#7d5a38'), noise(n, u) * 0.5)
        dam.push(flat([a, b, c], paint, isMarked ? undefined : 'damBody'), flat([a, c, d], blend(paint, 0, 0.08)))
        isMarked = true
      }
    }
    // Twigs are the Headroom share: strewn over the mound, and bristling along its crest as they pile up.
    const twigRng = seeded(7)
    for (let i = 0; i < Math.min(stock.twigs, MOST_TWIGS); i++) {
      const y = (twigRng() * 2 - 1) * (crestHalf + 0.3)
      const [px, py] = project(onSlope(i % 3 === 0 ? 1 : twigRng(), y))
      const angle = i % 3 === 0 ? -1.2 + twigRng() * 2.4 : twigRng() * Math.PI
      const reach = (0.16 + twigRng() * 0.2) * k
      const [ax, ay] = [Math.cos(angle) * reach, Math.sin(angle) * reach * 0.7]
      dam.push({ pts: [px - ax, py - ay - 0.8, px + ax, py + ay - 0.8, px + ax, py + ay + 0.8, px - ax, py - ay + 0.8], color: tint(WOOD[i % WOOD.length] ?? 0), mark: 'twig' })
    }
    // One log on the counter is one log here. Each piece keeps its own place, so a gain adds to the dam and moves nothing.
    type Beam = { at: number; y: number; half: number; radius: number; mark: string; color: number; seed: number }
    const beams: Beam[] = []
    const reachY = crestHalf + 0.2
    const place = (i: number) => (i * 0.618) % 1
    const spread = (i: number) => (noise(i, 77) * 2 - 1) * reachY * 0.9
    for (let i = 0; i < Math.min(stock.trunks, MOST_TRUNKS); i++) beams.push({ at: 0.28 + place(i + 3) * 0.3, y: spread(i + 100) * 0.3, half: reachY * (0.6 + noise(i, 78) * 0.2), radius: 0.21, mark: 'trunk', color: BARK, seed: i + 10 })
    for (let i = 0; i < Math.min(stock.logs, MOST_LOGS); i++) beams.push({ at: 0.3 + place(i) * 0.62, y: spread(i), half: 0.5 + noise(i, 79) * 0.3, radius: 0.13, mark: 'rondin', color: rgb('#9c6d3e'), seed: i + 20 })
    beams.sort((a, b) => b.at - a.at)
    for (const beam of beams) {
      const [x, y, z] = onSlope(beam.at, beam.y)
      const skew = (noise(beam.seed, 5) - 0.5) * 0.5 * beam.half
      dam.push(...frustum([x - skew, y - beam.half, z + beam.radius], [x + skew, y + beam.half, z + beam.radius + (noise(beam.seed, 6) - 0.5) * 0.25], beam.radius, beam.radius * 0.85, 6, beam.color, SAWN, beam.mark))
    }
    // The Ponytail base: three pale poles along the foot, each tied with its ribbon.
    if (s.tools.pompom) {
      const span = (crestHalf + 0.4) * 0.64
      for (let i = 2; i >= 0; i--) {
        const y = (i - 1) * span
        const [x, , z] = onSlope(0.1, y)
        const [from, to] = [y - span * 0.47, y + span * 0.47]
        dam.push(...frustum([x, from, z + 0.2], [x, to, z + 0.2], 0.2, 0.2, 7, rgb('#e0c08c'), SAWN, 'pole'))
        dam.push(...frustum([x, y - 0.1, z + 0.2], [x, y + 0.1, z + 0.2], 0.23, 0.23, 7, HAT.pompom))
      }
    }
    // Each hole in the dam spits water and carries away what the stock can spare: twigs first, trunks last.
    if (pressure > 0) {
      for (let hole = 0; hole < Math.ceil(pressure * 4); hole++) {
        const side = hole % 2 ? 1 : -1
        const y = side * crestHalf * (0.72 - hole * 0.07)
        const [x, , z] = onSlope(0.62 - hole * 0.12, y)
        const size = 0.1 + 0.13 * pressure
        dam.push({ pts: faces2([[x, y - size, z + size], [x + size * 0.4, y - size, z - size], [x + size * 0.4, y + size, z - size], [x, y + size, z + size]]), color: 0x160c06 })
        const [hx, hy] = project([x, y, z])
        for (let i = 0; i < 3 + pressure * 6; i++) {
          top.push(make('p jet', 'drop', [box(hx, hy, 1.2, 1.2, WHITE)], 0.5, 0.9, { x: (0.25 + rng() * 0.5) * k, y: (z * 0.82 + 0.3) * k }))
        }
        const carried = (mark: string, count: number, low: number, high: number, faces: () => Face[]) => {
          for (let i = 0; i < count; i++) top.push(make(`p debris ${mark}`, 'flow', faces(), low, high, { from: damAt, to: 1 }))
        }
        if (stock.twigs > 0) carried('gone-twig', Math.round(1 + pressure * 3), 7, 11, () => [box(0, (rng() - 0.5) * river * k, 5, 1.1, WOOD[Math.floor(rng() * 5)] ?? 0)])
        if (stock.logs > 0 && pressure > 0.25) carried('gone-log', Math.round(pressure * 2), 9, 13, () => [box(0, (rng() - 0.5) * river * k, 9, 3, BARK), box(8, 0, 1.6, 3, SAWN)])
        if (stock.trunks > 0 && pressure > 0.6 && hole < 2) carried('gone-trunk', 1, 11, 16, () => [box(0, (rng() - 0.5) * river * k, 18, 5, BARK), box(17, 0, 2.5, 5, SAWN)])
      }
    }
    // The spillway: the context itself, trickling through the crest in one sheet per layer.
    if (s.context && percent > 0) {
      const half = Math.min(river, crestHalf * 0.92)
      const crestX = archX(0) + front
      const footX = archX(0) + foot + 0.2
      const crestZ = damTall + 0.02
      let across = -half
      layers.forEach((share, n) => {
        const next = across + share * half * 2
        dam.push({ pts: faces2([[crestX, across, crestZ], [footX, across, 0], [footX, next, 0], [crestX, next, crestZ]]), color: tint(WATER[n % WATER.length] ?? 0), alpha: 0.9, mark: 'band' })
        across = next
      })
      const [lx, ly] = project([crestX, -half, crestZ])
      const [rx, ry] = project([crestX, half, crestZ])
      const [dropX, dropY] = [(footX - crestX) * 0.866 * k, (footX - crestX) * 0.5 * k + crestZ * 0.82 * k]
      for (let i = 0; i < Math.round(3 + percent * 0.12); i++) {
        const at = rng()
        const [x, y] = [lx + (rx - lx) * at, ly + (ry - ly) * at]
        top.push(make('p streak', 'drop', [{ pts: [x - 0.7, y, x + 0.7, y, x + 0.7, y + 6, x - 0.7, y + 6], color: WHITE, alpha: 0.85 }], 0.6, 1.1, { x: dropX, y: dropY }))
      }
      const [fx, fy] = [project([footX, -half, 0]), project([footX, half, 0])]
      for (let i = 0; i < Math.round(4 + half * 8 + percent * 0.06); i++) {
        const at = rng()
        const size = 1.4 + rng() * 2
        top.push(make('p foam', 'hop', [box(fx[0] + (fy[0] - fx[0]) * at + (rng() - 0.5) * 8, fx[1] + (fy[1] - fx[1]) * at + (rng() - 0.5) * 5, size, size * 0.7, 0xf2fbff)], 0.7, 1.4, { y: 2 }))
      }
    }
    solids.push({ depth: 0.01, faces: dam })
  }

  // The crew: one beaver per tool, in its colours, awake when that tool has just been used.
  const beaver = (id: CritterId, x: number, y: number, z: number, mood: number, isScared: boolean): Face[] => {
    const r = 0.3 * big
    const FUR = rgb('#96683c')
    const parts = [
      ...frustum([x - r * 0.9, y - r * 1.1, z + 0.02], [x - r * 0.9, y - r * 2.4, z + 0.02], r * 0.5, r * 0.34, 4, rgb('#4a2f1b')),
      ...frustum([x, y, z], [x, y, z + r * 1.6], r, r * 0.74, 6, FUR, rgb('#b98a58')),
      ...frustum([x, y, z + r * 1.6], [x, y, z + r * 2.6], r * 0.8, r * 0.56, 6, rgb('#a8784a'), rgb('#b98a58')),
      ...frustum([x, y, z + r * 2.6], [x, y, z + r * 3.5], r * 0.68, 0, 6, HAT[id]),
    ]
    const [hx, hy] = project([x + r * 0.4, y + r * 0.4, z + r * 2.05])
    const size = Math.max(0.9, r * k * 0.13)
    const eye = mood > 0 ? 0x111111 : rgb('#5d3d24')
    parts.push(box(hx - size * 2.2, hy, size, mood > 0 ? size : size * 0.4, eye), box(hx + size * 2.2, hy, size, mood > 0 ? size : size * 0.4, eye), box(hx, hy + size * 2.4, size * 1.1, size * 1.2, WHITE))
    if (isScared) top.push(make('p sweat', 'drop', [box(hx + size * 6, hy - size * 4, size, size, rgb('#9fdcf5'))], 0.8, 1.1, { x: 5, y: 16 }))

    return parts
  }
  const crew = CREW.filter(id => s.tools[id])
  const isScared = pressure > 0 || flooded
  crew.forEach((id, i) => {
    const mood = isScared ? 3 : levelOf(levels[id])
    if (flooded) {
      const stop = trail[Math.round((0.32 + i * 0.13) * (trail.length - 1))] ?? [0, 0]
      const raft = frustum([stop[0], stop[1] - 0.95, 0.12], [stop[0], stop[1] + 0.95, 0.12], 0.17, 0.17, 6, BARK, SAWN)
      top.push(make('bob', 'hop', [...raft, ...beaver(id, stop[0], stop[1], 0.26, mood, true)], 1.1, 1.4, { y: 4 }))

      return
    }
    const y = (i % 2 ? 1 : -1) * crestHalf * (i < 2 ? 0.8 : 0.45)
    const edge = (y / (crestHalf + 0.6)) ** 2
    const faces = beaver(id, bow * (edge - 1), y, damTall * (0.62 + 0.38 * Math.sqrt(1 - edge)), mood, isScared)
    top.push(make(mood > 0 ? 'busy' : 'calm', mood > 0 ? 'hop' : 'still', faces, 0.5, 0.7, { y: 3 }))
  })
  if (flooded) {
    for (let i = 0; i < Math.min(26, twigCount(stock.twigs * 100)); i++) {
      top.push(make('p debris', 'flow', [box(0, (rng() - 0.5) * river * k, 6, 1.2, WOOD[i % WOOD.length] ?? 0)], 5, 9))
    }
  }
  if (s.context && percent < 30) {
    for (const at of [0.18, 0.74]) {
      const [x, y] = [path[Math.round(at * (path.length / 2 - 1)) * 2] ?? 0, path[Math.round(at * (path.length / 2 - 1)) * 2 + 1] ?? 0]
      top.push(make('p jump', 'jump', [box(x, y, 5, 2.4, rgb('#ff9d5c')), box(x + 6, y, 2, 2.6, rgb('#d96f32'))], 4, 6))
    }
  }
  if (storm > 0.3) {
    for (let i = 0; i < Math.round(storm * 34); i++) {
      const x = rng() * (view.width + 60)
      const y = view.top + rng() * view.height * 0.5
      top.push(make('p rain', 'drop', [{ pts: [x, y, x + 1, y, x - 3, y + 11, x - 4, y + 11], color: rgb('#cfe4ee'), alpha: 0.6 }], 0.5, 0.9, { x: -40, y: view.height * 0.55 }))
    }
  }

  solids.sort((a, b) => a.depth - b.depth)

  return { ground, flow, solids, top, path, isFlooded: flooded }
}

// Where a mover stands at one moment. The terminal asks each frame; the panel's CSS plays the same curves.
export function poseOf(m: Mover, path: number[], t: number): { dx: number; dy: number; angle: number; alpha: number } {
  const life = m.seconds > 0 ? (t / m.seconds + m.phase) % 1 : 0
  if (m.mode === 'drop') return { dx: m.x * life, dy: m.y * life * life, angle: 0, alpha: Math.min(1, (1 - life) * 4) }
  if (m.mode === 'hop') return { dx: 0, dy: -Math.abs(Math.sin(life * Math.PI)) * m.y, angle: 0, alpha: 1 }
  if (m.mode === 'jump') {
    const leap = (life - 0.62) / 0.34

    return leap < 0 || leap > 1 ? { dx: 0, dy: 0, angle: 0, alpha: 0 } : { dx: (leap - 0.5) * 22, dy: -Math.sin(leap * Math.PI) * 26, angle: 0, alpha: 1 }
  }
  if (m.mode === 'still') return { dx: 0, dy: 0, angle: 0, alpha: 1 }
  const points = path.length / 2 - 1
  const at = (m.from + (m.to - m.from) * life) * points
  const i = clamp(Math.floor(at), 0, points - 1)
  const [x0, y0, x1, y1] = [path[i * 2] ?? 0, path[i * 2 + 1] ?? 0, path[i * 2 + 2] ?? 0, path[i * 2 + 3] ?? 0]
  const part = at - i

  return { dx: x0 + (x1 - x0) * part, dy: y0 + (y1 - y0) * part, angle: Math.atan2(y1 - y0, x1 - x0), alpha: Math.min(1, life * 12, (1 - life) * 12) }
}
