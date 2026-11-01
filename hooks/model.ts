// What the plugin says, in words.
import type { Gains, Stats } from '../types'

export const EMPTY: Stats = {
  context: null,
  saved: 0,
  pruned: 0,
  facts: 0,
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
    ['nodes', 'graph nodes'],
  ]
  const parts = labels.filter(([kind]) => after[kind] > before[kind]).map(([kind, label]) => `+${formatCount(after[kind] - before[kind])} ${label}`)

  return parts.length > 0 ? parts.join(', ') : null
}

const MISSING = 'not installed, run /the-dam-setup'

function headroomLine(s: Stats): string {
  const saved = `${formatCount(s.saved)} tokens saved`
  if (!s.tools.headroom) return MISSING
  if (s.proxy === 'off') return `${saved}, compression off (/the-dam-compress)`

  return s.proxy === 'up' ? saved : `${saved}, proxy ${s.proxy === 'starting' ? 'starting' : 'stopped'}`
}

// The one line that stays on screen.
export function statusOf(s: Stats): string {
  const context = s.context ? `, context ${Math.round(s.context.percent)}%` : ''

  return `the dam: ${formatCount(s.saved)} saved${context}`
}

// What /the-dam answers.
export function reportOf(s: Stats): string {
  return [
    s.context ? `Context: ${formatCount(s.context.total)} of ${formatCount(s.context.max)} tokens (${Math.round(s.context.percent)}%)` : 'Context: not measured yet',
    `Headroom: ${headroomLine(s)}`,
    `TokenEconomizer: ${s.tools.economizer ? `${s.pruned} tools pruned, ${s.facts} facts learned` : MISSING}`,
    `graphify: ${!s.tools.graphify ? MISSING : s.hasGraph ? `${formatCount(s.nodes)} graph nodes` : 'no graph in this folder, run /graphify'}`,
    `Ponytail: ${s.tools.ponytail ? 'active' : MISSING}`,
  ].join('\n')
}
