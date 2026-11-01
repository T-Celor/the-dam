// What the plugin says, in words.
import type { Gains, Stats } from '../types'

export const EMPTY: Stats = {
  context: null,
  saved: 0,
  pruned: 0,
  facts: 0,
  loops: 0,
  pending: 0,
  overhead: 0,
  nodes: 0,
  tools: { headroom: false, graphify: false, economizer: false, ponytail: false },
  proxy: 'down',
  hasGraph: false,
}

export function formatCount(n: number): string {
  const short = (value: number) => value.toFixed(1).replace(/\.0$/, '')
  if (n >= 1_000_000) return `${short(n / 1_000_000)}M`
  if (n >= 1_000) return `${short(n / 1_000)}k`

  return String(Math.round(n))
}

// What a check found since the last one, or null when nothing is new.
export function gainNote(before: Gains, after: Gains): string | null {
  const labels: Array<[keyof Gains, string]> = [
    ['saved', 'tokens saved'],
    ['pruned', 'tools pruned'],
    ['facts', 'facts learned'],
    ['loops', 'loops stopped'],
    ['nodes', 'graph nodes'],
  ]
  const parts = labels.filter(([kind]) => after[kind] > before[kind]).map(([kind, label]) => `+${formatCount(after[kind] - before[kind])} ${label}`)

  return parts.length > 0 ? parts.join(', ') : null
}

const BEAVER = '🦫'
const OK = '✓'
const WAIT = '…'
const OFF = '✗'
const MISSING = 'not installed'

function headroomRow(s: Stats): [string, string] {
  const saved = `${formatCount(s.saved)} tokens saved`
  if (!s.tools.headroom) return [OFF, MISSING]
  if (s.proxy === 'off') return [OFF, `${saved}, compression off (/the-dam-compress)`]
  if (s.proxy === 'starting') return [WAIT, `${saved}, proxy starting`]

  return s.proxy === 'up' ? [OK, saved] : [OFF, `${saved}, proxy stopped`]
}

// Counts, as TokenEconomizer measures no token saving; its own cost it does measure.
function economizerLine(s: Stats): string {
  const pending = s.pending > 0 ? ` (${s.pending} pending)` : ''

  return `${s.pruned} tools pruned${pending}, ${s.facts} facts learned, ${s.loops} loops stopped, ${formatCount(s.overhead)} tokens spent`
}

// Prefixes a toast, so a gain reads as coming from the same place as the status line.
export const signed = (text: string) => `${BEAVER} ${text}`

// The one line that stays on screen.
export function statusOf(s: Stats): string {
  const context = s.context ? ` · context ${Math.round(s.context.percent)}%` : ''

  return signed(`${formatCount(s.saved)} saved${context}`)
}

// What /the-dam answers. The title stands alone because a host may prefix the first line.
export function reportOf(s: Stats): string {
  const context = s.context ? `${Math.round(s.context.percent)}% (${formatCount(s.context.total)} of ${formatCount(s.context.max)} tokens)` : 'not measured yet'
  const rows: Array<[string, [string, string]]> = [
    ['Headroom', headroomRow(s)],
    ['TokenEconomizer', s.tools.economizer ? [OK, economizerLine(s)] : [OFF, MISSING]],
    ['graphify', !s.tools.graphify ? [OFF, MISSING] : s.hasGraph ? [OK, `${formatCount(s.nodes)} graph nodes`] : [WAIT, 'no graph in this folder, run /graphify']],
    ['Ponytail', s.tools.ponytail ? [OK, 'active'] : [OFF, MISSING]],
  ]

  return [signed('The dam'), `  Context ${context}`, ...rows.map(([name, [mark, detail]]) => `  ${mark} ${name}: ${detail}`)].join('\n')
}
