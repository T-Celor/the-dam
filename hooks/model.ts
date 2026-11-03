// What the plugin says, in words.
import type { Flow, Gains, JournalEntry, Stats } from '../types'

export const EMPTY: Stats = {
  context: null,
  saved: 0,
  pruned: 0,
  facts: 0,
  loops: 0,
  pending: 0,
  overhead: 0,
  nodes: 0,
  tools: { squeezer: false, graphify: false, economizer: false, beaver: false },
  isCompressing: true,
  hasGraph: false,
  level: null,
  session: { saved: 0, seen: 0, spent: 0 },
  project: { saved: 0, seen: 0, spent: 0 },
}

// One element of the dam per tool, shown with its figure.
export const ICON = { beaver: '🦫', water: '💧', logs: '🪵', whirl: '🌀', memory: '🧠', map: '🗺️', level: '🌊' } as const

export function formatCount(n: number): string {
  const short = (value: number) => value.toFixed(1).replace(/\.0$/, '')
  if (n >= 1e12) return `${short(n / 1e12)}T`
  if (n >= 1e9) return `${short(n / 1e9)}G`
  if (n >= 1_000_000) return `${short(n / 1_000_000)}M`
  if (n >= 1_000) return `${short(n / 1_000)}k`

  return String(Math.round(n))
}

// The share of what the tools printed that compression took out, or null before anything was printed.
export function percentOf(flow: Flow): string | null {
  return flow.seen > 0 ? `${Math.round((Math.min(flow.saved, flow.seen) / flow.seen) * 100)}%` : null
}

// What compression saved against every token the session's requests counted (input, cache and output), as `saved/total (%)`.
export function savedOfSpent(flow: Flow): string {
  if (flow.spent <= 0) return formatCount(flow.saved)
  const share = (flow.saved / (flow.saved + flow.spent)) * 100

  return `${formatCount(flow.saved)}/${formatCount(flow.spent)} (${share < 1 ? share.toFixed(2) : share < 10 ? share.toFixed(1) : Math.round(share)}%)`
}

type FlowRecord = { project?: unknown; saved?: unknown; seen?: unknown; spent?: unknown } | null | undefined

// One project's compression, from every session's record.
export function projectFlow(records: FlowRecord[], project: string): Flow {
  return records.reduce<Flow>((sum, record) => (record?.project === project ? { saved: sum.saved + Number(record.saved ?? 0), seen: sum.seen + Number(record.seen ?? 0), spent: sum.spent + Number(record.spent ?? 0) } : sum), { saved: 0, seen: 0, spent: 0 })
}

const withShare = savedOfSpent

// What a check found since the last one, or null when nothing is new.
export function gainNote(before: Gains, after: Gains): string | null {
  const labels: Array<[keyof Gains, string]> = [
    ['saved', 'tokens saved'],
    ['pruned', 'tools pruned'],
    ['facts', 'facts learned'],
    ['loops', 'loops stopped'],
    ['nodes', 'places on the map'],
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
  const saved = `${withShare(s.session)} tokens saved in this session, ${withShare(s.project)} in this project, ${formatCount(s.saved)} in all`
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
  // Every tool keeps its place, so the line reads the same in every session; `–` where there is nothing to measure.
  const parts: Array<[string, string]> = [
    [ICON.beaver, s.level ?? 'off'],
    [ICON.water, savedOfSpent(s.session)],
    [ICON.logs, `${s.pruned}${s.pending > 0 ? `+${s.pending}` : ''}`],
    [ICON.whirl, String(s.loops)],
    [ICON.memory, String(s.facts)],
    [ICON.map, s.hasGraph && s.nodes > 0 ? formatCount(s.nodes) : '–'],
    [ICON.level, s.context ? `${Math.round(s.context.percent)}%` : '–'],
  ]

  return parts.map(([icon, figure]) => `${icon} ${figure}`).join(' · ')
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
    if (event.kind === 'inject') entries.push({ at, icon: ICON.memory, text: `${Number(event.facts ?? 0)} learned facts added to the context`, count: Number(event.facts ?? 0) })
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

// Rows the pane spends on its own lines: the journal's title, the gap and the totals' title.
const PANE_TITLE_ROWS = 3

// The journal as the pane shows it: newest first, as many as fit above the totals, then how many older ones are left out.
// `bodyRows` absent (a pane laid inline fits its tree) keeps every entry.
export function journalView(entries: JournalEntry[], totals: number, bodyRows?: number): { shown: JournalEntry[]; older: number } {
  const newest = [...entries].sort((a, b) => b.at - a.at)
  const room = bodyRows === undefined ? newest.length : Math.max(1, bodyRows - totals - PANE_TITLE_ROWS)
  if (newest.length <= room) return { shown: newest, older: 0 }
  const shown = newest.slice(0, Math.max(0, room - 1))

  return { shown, older: newest.length - shown.length }
}

// What this session did, counted from its journal; compression is counted in tokens, in the stats.
export type SessionFigures = { loops: number; facts: number; mapAnswers: number }

export function sessionFigures(entries: JournalEntry[]): SessionFigures {
  const of = (icon: string, starts: string) => entries.filter(entry => entry.icon === icon && entry.text.startsWith(starts))

  return {
    loops: of(ICON.whirl, 'loop redirected').length,
    facts: of(ICON.memory, '').reduce((sum, entry) => sum + (entry.count ?? 0), 0),
    mapAnswers: of(ICON.map, 'map answered').length + of(ICON.map, 'map asked').length,
  }
}

// The totals under the journal, this session's; what only exists for the whole repository says so.
export function totalLines(s: Stats, here: SessionFigures): string[] {
  const context = s.context ? `${Math.round(s.context.percent)}% of the context used (${formatCount(s.context.total)} of ${formatCount(s.context.max)} tokens)` : 'context not measured yet'

  return [
    `${ICON.water} ${savedOfSpent(s.session)} tokens saved of this session's total${percentOf(s.session) ? `, ${percentOf(s.session)} of what the tools printed` : ''}`,
    `   this project ${withShare(s.project)}, every project ${formatCount(s.saved)}`,
    `${ICON.logs} ${s.pruned} tools left out of this session${s.pending > 0 ? `, ${s.pending} more waiting` : ''}`,
    `${ICON.whirl} ${here.loops} loops stopped`,
    `${ICON.memory} ${here.facts} learned facts added`,
    `${ICON.map} ${s.hasGraph ? `${here.mapAnswers} answers from the map (${formatCount(s.nodes)} places)` : 'no map here'}`,
    `${ICON.beaver} Beaver ${s.level ?? 'off'}`,
    `${ICON.level} ${context}`,
    `TokenEconomizer's own model calls in this repository: ${formatCount(s.overhead)} tokens`,
  ]
}

// What /the-dam answers. The title stands alone because a host may prefix the first line.
export function reportOf(s: Stats): string {
  const context = s.context ? `${Math.round(s.context.percent)}% (${formatCount(s.context.total)} of ${formatCount(s.context.max)} tokens)` : 'not measured yet'
  const rows: Array<[string, [string, string]]> = [
    ['Compression', compressionRow(s)],
    ['TokenEconomizer', s.tools.economizer ? [OK, economizerLine(s)] : [OFF, MISSING]],
    ['Map', !s.tools.graphify ? [OFF, MISSING] : s.hasGraph ? [OK, `${formatCount(s.nodes)} places`] : [WAIT, 'not for this folder: under 20 tracked code or document files']],
    ['Beaver', s.tools.beaver ? [OK, 'active'] : [OFF, MISSING]],
  ]

  return [signed('The dam'), `  Context ${context}`, ...rows.map(([name, [mark, detail]]) => `  ${mark} ${name}: ${detail}`)].join('\n')
}
