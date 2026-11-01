// A small vector engine. The panel draws SVG in a frame with no script or canvas,
// so everything is computed once here and the browser only plays it back.

export type Rng = () => number
export type Point = readonly [number, number]

// mulberry32: the same seed lands every shape in the same place from one draw to the next.
export function seeded(seed: number): Rng {
  let state = seed >>> 0

  return () => {
    state = (state + 0x6d2b79f5) >>> 0
    let t = state
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)

    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

export const num = (value: number): string => String(Math.round(value * 10) / 10)
export const between = (rng: Rng, low: number, high: number): number => low + rng() * (high - low)
export const clamp = (value: number, low: number, high: number): number => Math.min(high, Math.max(low, value))

export const rgb = (hex: string): number => parseInt(hex.slice(1), 16)

export function blend(from: number, to: number, t: number): number {
  const channel = (shift: number) => {
    const a = (from >> shift) & 255
    const b = (to >> shift) & 255

    return Math.round(a + (b - a) * t) << shift
  }

  return channel(16) | channel(8) | channel(0)
}

// Same inputs, same value: this is what makes facets and foam look varied without keeping state.
export function noise(x: number, y: number): number {
  let t = Math.imul(x, 73856093) ^ Math.imul(y, 19349663)
  t = Math.imul(t ^ (t >>> 15), t | 1)
  t ^= t + Math.imul(t ^ (t >>> 7), t | 61)

  return ((t ^ (t >>> 14)) >>> 0) / 4294967296
}

export function mix(from: string, to: string, t: number): string {
  const channel = (hex: string, at: number) => parseInt(hex.slice(at, at + 2), 16)
  const blended = [1, 3, 5].map(at => Math.round(channel(from, at) + (channel(to, at) - channel(from, at)) * t))

  return `#${blended.map(value => value.toString(16).padStart(2, '0')).join('')}`
}

// Catmull-Rom through the points, written as cubic Beziers.
export function curve(points: readonly Point[], isClosed = false): string {
  const at = (i: number): Point => {
    const count = points.length
    const index = isClosed ? (i + count) % count : clamp(i, 0, count - 1)

    return points[index] ?? [0, 0]
  }
  const start = at(0)
  let path = `M${num(start[0])} ${num(start[1])}`
  const segments = isClosed ? points.length : points.length - 1
  for (let i = 0; i < segments; i++) {
    const [p0, p1, p2, p3] = [at(i - 1), at(i), at(i + 1), at(i + 2)]
    const c1 = [p1[0] + (p2[0] - p0[0]) / 6, p1[1] + (p2[1] - p0[1]) / 6]
    const c2 = [p2[0] - (p3[0] - p1[0]) / 6, p2[1] - (p3[1] - p1[1]) / 6]
    path += `C${num(c1[0] ?? 0)} ${num(c1[1] ?? 0)} ${num(c2[0] ?? 0)} ${num(c2[1] ?? 0)} ${num(p2[0])} ${num(p2[1])}`
  }

  return isClosed ? `${path}Z` : path
}

// A jagged skyline closed down to `floor`, smoothed by two octaves of noise.
export function ridge(rng: Rng, from: number, to: number, base: number, amplitude: number, steps: number, floor: number): string {
  const phase = rng() * 9
  const points: Point[] = []
  for (let i = 0; i <= steps; i++) {
    const x = from + ((to - from) * i) / steps
    points.push([x, base - amplitude * (0.6 * Math.sin(i * 0.9 + phase) + 0.4 * (rng() * 2 - 1))])
  }

  return `${curve(points)}L${to} ${floor}L${from} ${floor}Z`
}

type Stop = readonly [number, string, number?]

const stops = (list: readonly Stop[]) =>
  list.map(([at, color, opacity]) => `<stop offset="${at}" stop-color="${color}"${opacity === undefined ? '' : ` stop-opacity="${opacity}"`}/>`).join('')

export const linear = (id: string, list: readonly Stop[], x2 = 0, y2 = 1): string =>
  `<linearGradient id="${id}" x1="0" y1="0" x2="${x2}" y2="${y2}">${stops(list)}</linearGradient>`

export const radial = (id: string, list: readonly Stop[]): string => `<radialGradient id="${id}">${stops(list)}</radialGradient>`

// One particle's clock and travel. A negative delay starts it mid-life, so a redraw never shows a restart.
export function motion(rng: Rng, low: number, high: number, travel: Record<string, number> = {}): string {
  const seconds = between(rng, low, high)
  const vars = Object.entries(travel).map(([name, value]) => `--${name}:${num(value)}px;`)

  return ` style="--t:${seconds.toFixed(2)}s;--d:-${(rng() * seconds).toFixed(2)}s;${vars.join('')}"`
}

export function repeat(count: number, draw: (i: number) => string): string {
  let drawn = ''
  for (let i = 0; i < count; i++) drawn += draw(i)

  return drawn
}
