// What the panel says, separate from how it is drawn.
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

export function contextBar(percent: number, width = 20): string {
  const filled = Math.round((Math.min(100, Math.max(0, percent)) / 100) * width)

  return '█'.repeat(filled) + '░'.repeat(width - filled)
}

export type Health = 'ok' | 'wait' | 'off'
export type Row = { name: string; health: Health; detail: string }

const MISSING = 'not installed: /the-dam-setup'

export function rowsOf(s: Stats): Row[] {
  const proxy: Record<Stats['proxy'], Row> = {
    up: { name: 'Headroom', health: 'ok', detail: `${formatCount(s.saved)} tokens saved` },
    starting: { name: 'Headroom', health: 'wait', detail: `starting, ${formatCount(s.saved)} tokens saved` },
    down: { name: 'Headroom', health: 'off', detail: `proxy stopped, ${formatCount(s.saved)} tokens saved` },
    off: { name: 'Headroom', health: 'off', detail: 'turned off: /the-dam-compress' },
  }

  return [
    s.tools.headroom ? proxy[s.proxy] : { name: 'Headroom', health: 'off', detail: MISSING },
    s.tools.economizer
      ? { name: 'TokenEconomizer', health: 'ok', detail: `${s.pruned} tools pruned, ${s.facts} facts learned` }
      : { name: 'TokenEconomizer', health: 'off', detail: MISSING },
    !s.tools.graphify
      ? { name: 'graphify', health: 'off', detail: MISSING }
      : s.hasGraph
        ? { name: 'graphify', health: 'ok', detail: `${formatCount(s.nodes)} graph nodes` }
        : { name: 'graphify', health: 'wait', detail: 'no graph here: /graphify' },
    s.tools.ponytail ? { name: 'Ponytail', health: 'ok', detail: 'active' } : { name: 'Ponytail', health: 'off', detail: MISSING },
  ]
}
