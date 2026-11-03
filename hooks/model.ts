// What the plugin says, in words.
import type { Gains, JournalEntry, Stats } from '../types'

export const EMPTY: Stats = {
  context: null,
  saved: 0,
  pruned: 0,
  facts: 0,
  loops: 0,
  pending: 0,
  overhead: 0,
  nodes: 0,
  tools: { squeezer: false, graphify: false, economizer: false, ponytail: false },
  isCompressing: true,
  hasGraph: false,
  level: null,
}

// One element of the dam per tool, shown with its figure.
export const ICON = { ponytail: '🦫', water: '💧', logs: '🪵', whirl: '🌀', memory: '🧠', map: '🗺️', level: '🌊' } as const

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

function compressionRow(s: Stats): [string, string] {
  const saved = `${formatCount(s.saved)} tokens saved`
  if (!s.tools.squeezer) return [OFF, MISSING]

  return s.isCompressing ? [OK, saved] : [OFF, `${saved}, turned off (/the-dam-compress)`]
}

// Counts, as TokenEconomizer measures no token saving; its own cost it does measure.
function economizerLine(s: Stats): string {
  const pending = s.pending > 0 ? ` (${s.pending} pending)` : ''

  return `${s.pruned} tools pruned${pending}, ${s.facts} facts learned, ${s.loops} loops stopped, ${formatCount(s.overhead)} tokens spent`
}

// Prefixes a toast, so a gain reads as coming from the same place as the status line.
export const signed = (text: string) => `${BEAVER} ${text}`

// The one line that stays on screen: each tool's icon and figure, and nothing for a tool with nothing to show.
export function statusOf(s: Stats): string {
  const tools = s.pruned + s.pending > 0 ? `${s.pruned}${s.pending > 0 ? `+${s.pending}` : ''}` : null
  const parts: Array<[string, string | null]> = [
    [ICON.ponytail, s.level ?? ''],
    [ICON.water, s.saved > 0 ? formatCount(s.saved) : null],
    [ICON.logs, tools],
    [ICON.whirl, s.loops > 0 ? String(s.loops) : null],
    [ICON.memory, s.facts > 0 ? String(s.facts) : null],
    [ICON.map, s.hasGraph && s.nodes > 0 ? formatCount(s.nodes) : null],
    [ICON.level, s.context ? `${Math.round(s.context.percent)}%` : null],
  ]

  return parts.filter(([, figure]) => figure !== null).map(([icon, figure]) => (figure ? `${icon} ${figure}` : icon)).join(' · ')
}

const LOOP_REASON = /^[^:(]+/

// TokenEconomizer's own record of this session (its `events.jsonl`), as journal lines.
export function economizerEntries(text: string, sessionId: string): JournalEntry[] {
  const entries: JournalEntry[] = []
  for (const line of text.split('\n')) {
    let event: Record<string, unknown>
    try {
      event = JSON.parse(line)
    } catch {
      continue
    }
    if (event?.session_id !== sessionId || typeof event.at !== 'number') continue
    const at = event.at * 1000
    const reason = String(event.reason ?? '').match(LOOP_REASON)?.[0].trim() ?? ''
    if (event.kind === 'inject') entries.push({ at, icon: ICON.memory, text: `${Number(event.facts ?? 0)} learned facts added to the context` })
    if (event.kind === 'nudge') entries.push({ at, icon: ICON.whirl, text: `loop redirected${reason ? ` (${reason})` : ''}, after ${Number(event.tool_calls ?? 0)} tool calls` })
    if (event.kind === 'abort') entries.push({ at, icon: ICON.whirl, text: `turn still stuck after a redirect${reason ? ` (${reason})` : ''}: you were told how to stop it` })
  }

  return entries
}

const two = (n: number) => String(n).padStart(2, '0')

export function journalLine(entry: JournalEntry): string {
  const at = new Date(entry.at)

  return `${two(at.getHours())}:${two(at.getMinutes())}  ${entry.icon} ${entry.text}`
}

// The totals under the journal: every session's for what is counted across sessions, this one's beside it.
export function totalLines(s: Stats, savedHere: number): string[] {
  const context = s.context ? `${Math.round(s.context.percent)}% of the context used (${formatCount(s.context.total)} of ${formatCount(s.context.max)} tokens)` : 'context not measured yet'

  return [
    `${ICON.water} ${formatCount(s.saved)} tokens saved by compression, ${formatCount(savedHere)} in this session`,
    `${ICON.logs} ${s.pruned} tools pruned${s.pending > 0 ? `, ${s.pending} waiting` : ''}`,
    `${ICON.whirl} ${s.loops} loops stopped`,
    `${ICON.memory} ${s.facts} learned facts in use`,
    `${ICON.map} ${s.hasGraph ? `${formatCount(s.nodes)} nodes in the code graph` : 'no code graph here'}`,
    `${ICON.ponytail} Ponytail ${s.level ?? 'off'}`,
    `${ICON.level} ${context}`,
    `TokenEconomizer's own model calls: ${formatCount(s.overhead)} tokens spent`,
  ]
}

// What /the-dam answers. The title stands alone because a host may prefix the first line.
export function reportOf(s: Stats): string {
  const context = s.context ? `${Math.round(s.context.percent)}% (${formatCount(s.context.total)} of ${formatCount(s.context.max)} tokens)` : 'not measured yet'
  const rows: Array<[string, [string, string]]> = [
    ['Compression', compressionRow(s)],
    ['TokenEconomizer', s.tools.economizer ? [OK, economizerLine(s)] : [OFF, MISSING]],
    ['Code graph', !s.tools.graphify ? [OFF, MISSING] : s.hasGraph ? [OK, `${formatCount(s.nodes)} nodes`] : [WAIT, 'not for this folder: under 20 tracked code files']],
    ['Ponytail', s.tools.ponytail ? [OK, 'active'] : [OFF, MISSING]],
  ]

  return [signed('The dam'), `  Context ${context}`, ...rows.map(([name, [mark, detail]]) => `  ${mark} ${name}: ${detail}`)].join('\n')
}
