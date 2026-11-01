import type { CritterId, Heat, Stats } from '../types'
import { clamp, num, radial, repeat, seeded } from './engine'
import type { Face, Icon, Mover, World } from './iso'
import { buildWorld, iconFaces, isFlooded, strengthFor } from './iso'
import { NO_MATERIALS, pressureOf, stockOf } from './model'

export { fallWidth, isFlooded, levelOf, strengthFor, twigCount } from './iso'
export type { Level } from './iso'

export const EMPTY: Stats = {
  context: null,
  saved: 0,
  tools: { hazel: false, mimi: false, pompom: false, truffle: false },
  proxy: 'down',
  hasGraph: false,
  pennants: 0,
  isParty: false,
  earned: NO_MATERIALS,
  wear: NO_MATERIALS,
  river: null,
}

export const COLD: Heat = { hazel: 0, mimi: 0, pompom: 0, truffle: 0 }
const MAX_HEAT = 9

export function warm(heat: Heat, used: readonly CritterId[], by = 1): Heat {
  const next = { ...heat }
  for (const id of used) next[id] = Math.min(MAX_HEAT, next[id] + by)

  return next
}

export function cool(heat: Heat): Heat {
  return {
    hazel: Math.max(0, heat.hazel - 1),
    mimi: Math.max(0, heat.mimi - 1),
    pompom: Math.max(0, heat.pompom - 1),
    truffle: Math.max(0, heat.truffle - 1),
  }
}

// Who a tool call wakes up. `subject` is the command, skill or path the call names.
export function usedBy(tool: string, subject: string): CritterId[] {
  const used: CritterId[] = []
  if (tool === 'Edit' || tool === 'Write') used.push('pompom')
  if (/\bheadroom\b/i.test(subject)) used.push('hazel')
  if (/\bgraphify/i.test(subject)) used.push('mimi')
  if (/\bponytail/i.test(subject) && !used.includes('pompom')) used.push('pompom')
  if (/tokeneconomizer|(^|[\s/])te\s+(status|toolprune|microskills|config|repos)\b/i.test(subject)) used.push('truffle')

  return used
}

export function formatCount(n: number): string {
  const short = (value: number) => value.toFixed(1).replace(/\.0$/, '')
  if (n >= 1_000_000) return `${short(n / 1_000_000)}M`
  if (n >= 1_000) return `${short(n / 1_000)}k`

  return String(Math.round(n))
}

const MAX_PENNANTS = 7
const AVERT_FROM = 70
const AVERT_DROP = 20
const CALM_BELOW = 30

// The game's one rule: bring high water down and earn a pennant; let it reach the top and lose half.
export function nextPennants(pennants: number, before: number | null, after: number): number {
  if (after >= 100) return Math.floor(pennants / 2)
  if (before !== null && before >= AVERT_FROM && before < 100 && after <= before - AVERT_DROP) return pennants + 1

  return pennants
}

const WIDTH = 640
const HEIGHT = 480
export const MOST_ASPECT = 2.4
// How many ground tiles fit across the panel.
const TILES = 15

const hex = (color: number) => `#${color.toString(16).padStart(6, '0')}`
const whole = (value: number) => String(Math.round(value))
const outline = (pts: number[], digits: (value: number) => string) => {
  let d = ''
  for (let i = 0; i < pts.length; i += 2) d += `${i === 0 ? 'M' : 'L'}${digits(pts[i] ?? 0)} ${digits(pts[i + 1] ?? 0)}`

  return `${d}Z`
}

// Neighbours of one colour share a path: far fewer characters. On the ground, a thin same-colour stroke hides the seams.
function drawFaces(faces: readonly Face[], digits = whole, isSealed = false): string {
  let out = ''
  let run = ''
  let color = -1
  const flush = () => {
    if (run) out += `<path d="${run}" fill="${hex(color)}"${isSealed ? ` stroke="${hex(color)}" stroke-width=".5"` : ''}/>`
    run = ''
  }
  for (const face of faces) {
    if (face.mark || face.alpha !== undefined) {
      flush()
      const opacity = face.alpha === undefined ? '' : ` opacity="${face.alpha}"`
      out += `<path${face.mark ? ` class="${face.mark}"` : ''} d="${outline(face.pts, digits)}" fill="${hex(face.color)}"${opacity}/>`
      continue
    }
    if (face.color !== color) flush()
    color = face.color
    run += outline(face.pts, digits)
  }
  flush()

  return out
}

function drawMover(m: Mover): string {
  if (m.mode === 'still') return `<g class="${m.cls}">${drawFaces(m.faces, num)}</g>`
  const clock = `--t:${m.seconds.toFixed(2)}s;--d:-${(m.phase * m.seconds).toFixed(2)}s;`
  const travel = m.mode === 'flow' ? `--a:${Math.round(m.from * 100)}%;--b:${Math.round(m.to * 100)}%` : `--x:${num(m.x)}px;--y:${num(m.y)}px`

  return `<g class="${m.cls}" style="${clock}${travel}">${drawFaces(m.faces, num)}</g>`
}

function drawWorld(world: World): string {
  const solids = world.solids.map(piece => (piece.sway ? `<g class="sway" style="animation-delay:-${(((piece.depth * 7) % 5) + 5).toFixed(1)}s">${drawFaces(piece.faces)}</g>` : drawFaces(piece.faces, num))).join('')
  const flow = world.flow.map(drawMover).join('')

  return `${drawFaces(world.ground, whole, true)}${world.isFlooded ? `<g class="rush">${flow}</g>` : flow}${solids}${world.top.map(drawMover).join('')}`
}

// Flags on a rope slung across the top corner.
function drawPennants(count: number): string {
  const shown = Math.min(MAX_PENNANTS, count)
  if (shown === 0) return ''
  const sag = (x: number) => 14 + Math.sin(((628 - x) / 190) * Math.PI) * 12
  const flags = repeat(shown, i => {
    const x = 612 - i * 26

    return `<path class="flap" d="M${x} ${num(sag(x))}h18l-9 20Z" fill="${['#e0553f', '#f2bf2e', '#2aa198'][i % 3]}" stroke="#00000030" style="animation-delay:${i * -0.3}s"/>`
  })

  return `<path d="M628 14Q534 40 438 14" fill="none" stroke="#3b2415" stroke-width="1.400"/>${flags}`
}

function drawParty(): string {
  const rng = seeded(97)
  const colors = ['#f2bf2e', '#e0553f', '#2aa198', '#6f7fd0', '#ff8fb4']

  return repeat(44, i => `<rect class="confetti" x="${num(rng() * WIDTH)}" y="-12" width="6" height="10" rx="1.500" fill="${colors[i % colors.length]}" style="animation-delay:${(rng() * 1.2).toFixed(2)}s;animation-duration:${(1.8 + rng()).toFixed(2)}s"/>`)
}

function drawDefs(extra: number): string {
  return `<defs>${[
    radial('edge', [[0.72, '#000', 0], [1, '#04121c', 0.2]]),
  ].join('')}<clipPath id="frame"><rect y="${-extra}" width="${WIDTH}" height="${HEIGHT + extra}" rx="14"/></clipPath></defs>`
}

// Every moving thing below is one transform or opacity, so the browser can play it cheaply.
const styleFor = (path: number[]) => {
  const course = path.reduce((d, value, i) => `${d}${i % 2 ? ' ' : i === 0 ? 'M' : 'L'}${Math.round(value)}`, '')

  return `<style>
html,body{margin:0}
svg{display:block;width:100%;height:100%}
.p{animation:var(--t) var(--d) linear infinite}
.flow,.debris{offset-path:path('${course}');offset-rotate:auto;transform-box:view-box;transform-origin:0 0;animation-name:ride}
.streak:not(.flow),.jet,.sweat,.rain{animation-name:drop;animation-timing-function:ease-in}
.foam{animation-name:foam;animation-timing-function:ease-in-out}
.jump{animation-name:jump;animation-timing-function:ease-in-out}
.sway{transform-box:fill-box;transform-origin:bottom center;animation:sway 4.6s ease-in-out infinite alternate}
.busy{animation:busy .6s ease-in-out infinite}
.bob{animation:bob 1.3s ease-in-out infinite alternate}
.shake{animation:shake .3s ease-in-out infinite}
.flap{transform-box:fill-box;transform-origin:top center;animation:flap 1s ease-in-out infinite alternate}
.flash{opacity:0;animation:flash 7s linear infinite}
.confetti{opacity:0;animation:confetti 2s ease-in 2 forwards}
@keyframes ride{0%{offset-distance:var(--a);opacity:0}8%{opacity:1}92%{opacity:1}100%{offset-distance:var(--b);opacity:0}}
@keyframes drop{0%{transform:none;opacity:1}75%{opacity:1}100%{transform:translate(var(--x),var(--y));opacity:0}}
@keyframes foam{50%{transform:translateY(-2px)}}
@keyframes jump{0%,62%,96%,100%{opacity:0;transform:none}63%{opacity:1;transform:translate(-11px,0)}79%{opacity:1;transform:translate(0,-26px)}95%{opacity:1;transform:translate(11px,0)}}
@keyframes sway{from{transform:rotate(-1.2deg)}to{transform:rotate(1.2deg)}}
@keyframes busy{50%{transform:translateY(-3px)}}
@keyframes bob{from{transform:translateY(-3px)}to{transform:translateY(3px)}}
@keyframes shake{25%{transform:translateX(-1px)}75%{transform:translateX(1px)}}
@keyframes flap{from{transform:skewX(-9deg)}to{transform:skewX(11deg)}}
@keyframes flash{0%,91%,93%,100%{opacity:0}92%,94%{opacity:.4}}
@keyframes confetti{0%{opacity:1;transform:none}100%{opacity:1;transform:translateY(500px) rotate(520deg)}}
svg:hover *{animation-play-state:paused}
@media (prefers-reduced-motion:reduce){*{animation:none!important}}
</style>`
}

export type CounterEntry = { icon: Icon; value: string; word: string; lost: number; note: string }

// Each counter says what it is, where it comes from, and why it may still be at zero.
export function counterEntries(s: Stats): CounterEntry[] {
  const stock = stockOf(s.earned, s.wear)
  const headroom = s.proxy === 'up' ? `Headroom: ${formatCount(s.saved)} tokens cut (1 per 100)` : `Headroom proxy ${s.proxy}: nothing cut now`

  return [
    { icon: 'river', value: s.context ? `${formatCount(s.context.total)}/${formatCount(s.context.max)}` : '?', word: 'river', lost: 0, note: `In your context. Base: ${s.tools.pompom ? 'Ponytail' : 'no Ponytail'}` },
    { icon: 'twigs', value: formatCount(stock.twigs), word: 'twigs', lost: s.earned.twigs - stock.twigs, note: headroom },
    { icon: 'logs', value: formatCount(stock.logs), word: 'logs', lost: s.earned.logs - stock.logs, note: 'Economizer here: tools cut + facts' },
    { icon: 'trunks', value: formatCount(stock.trunks), word: 'trunks', lost: s.earned.trunks - stock.trunks, note: s.hasGraph ? 'graphify: 1 per 50 graph nodes' : 'graphify: no graph in this folder' },
  ]
}

const CELL_X = [8, 324]
const CELL_Y = [392, 435]

// One figure for each thing on screen: the river, then what the dam is made of.
function drawCounters(s: Stats): string {
  const cells = counterEntries(s).map((entry, i) => {
    const x = CELL_X[i % 2] ?? 0
    const y = CELL_Y[Math.floor(i / 2)] ?? 0
    const lost = entry.lost > 0 ? `<tspan dx="7" font-size="13" fill="#ffb27a">-${formatCount(entry.lost)}</tspan>` : ''

    return `<rect x="${x}" y="${y}" width="308" height="41" rx="7" fill="#10100e" opacity=".78"/><g transform="translate(${x + 23} ${y + 21})">${drawFaces(iconFaces(entry.icon, 15), num)}</g><text x="${x + 46}" y="${y + 18}" font-size="16" font-weight="700" fill="#fff"><tspan>${entry.value}</tspan><tspan dx="6" font-size="13" font-weight="500" fill="#cfd8d0">${entry.word}</tspan>${lost}</text><text x="${x + 46}" y="${y + 33}" font-size="12" fill="#b9c4bb">${entry.note}</text>`
  })

  return `<g font-family="system-ui,sans-serif" shape-rendering="auto">${cells.join('')}</g>`
}

// A river seen from above in low-poly isometric 3D: the river is the context, and the dam is made of what the tools saved.
// `aspect` is the panel's height over its width: a taller panel shows more valley, so the scene always fills it.
export function damScene(s: Stats, levels: Heat, aspect = HEIGHT / WIDTH): string {
  const extra = Math.round(clamp(WIDTH * aspect - HEIGHT, 0, WIDTH * MOST_ASPECT - HEIGHT))
  const tall = HEIGHT + extra
  const percent = clamp(s.river ?? s.context?.percent ?? 0, 0, 100)
  const pressure = pressureOf(percent, strengthFor(s))
  const world = buildWorld(s, levels, { width: WIDTH, top: -extra, height: tall, tiles: TILES })
  const flash = pressure > 0.6 ? `<rect class="flash" y="${-extra}" width="${WIDTH}" height="${tall}" fill="#fff"/>` : ''

  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 ${-extra} ${WIDTH} ${tall}" shape-rendering="crispEdges" role="img"><title>${sceneAlt(s).replace(/&/g, '&amp;').replace(/</g, '&lt;')}</title>${styleFor(world.path)}${drawDefs(extra)}<g clip-path="url(#frame)"><rect y="${-extra}" width="${WIDTH}" height="${tall}" fill="#7cb661"/><g class="${pressure > 0.4 ? 'shake' : 'world'}">${drawWorld(world)}</g>${flash}<rect y="${-extra}" width="${WIDTH}" height="${tall}" fill="url(#edge)"/><g transform="translate(0 ${-extra})">${drawPennants(s.pennants)}</g>${drawCounters(s)}${s.isParty ? drawParty() : ''}</g></svg>`
}

export function sceneAlt(s: Stats): string {
  const stock = stockOf(s.earned, s.wear)
  const water = s.context
    ? `Context ${Math.round(s.context.percent)}% full, ${formatCount(s.context.total)} of ${formatCount(s.context.max)} tokens${
        s.context.layers.length > 0 ? `: ${s.context.layers.map(layer => `${layer.name} ${formatCount(layer.tokens)}`).join(', ')}` : ''
      }`
    : 'Context not measured yet'
  const percent = s.river ?? s.context?.percent ?? 0
  const pressure = pressureOf(percent, strengthFor(s))
  const dam = isFlooded(s) ? 'A torrent is sweeping over the dam' : pressure > 0 ? 'The dam is leaking' : 'The dam holds'
  const lost = s.earned.twigs - stock.twigs + (s.earned.logs - stock.logs) + (s.earned.trunks - stock.trunks)

  return `${water}. ${dam}: ${formatCount(stock.twigs)} twigs, ${formatCount(stock.logs)} logs, ${formatCount(stock.trunks)} trunks${lost > 0 ? `, ${formatCount(lost)} washed away` : ''}. ${formatCount(s.saved)} tokens saved in this project, ${s.pennants} floods averted.`
}
