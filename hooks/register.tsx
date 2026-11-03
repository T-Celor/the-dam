import { atom, read, update } from 'claude-code'
import type { Hook, Register } from 'claude-code'

import type { Gains, JournalEntry, Stats } from '../types'
import { EMPTY, ICON, economizerEntries, formatCount, gainNote, journalLine, reportOf, signed, statusOf, totalLines } from './model'
import type { Located } from './tools'
import {
  candidates,
  graphNudge,
  graphNodes,
  codeNames,
  exactEntries,
  graphFacts,
  graphQuestion,
  grepName,
  missingNames,
  parseEconomizer,
  setupSteps,
  fitsInline,
  savedPreview,
  standoutLines,
  squeezableField,
  squeezed,
  squeezeNote,
  worthGraphing,
} from './tools'

const GRAPH = '.dam-graph/graph.tsv'
// Change the tag when Stats changes: it declines what an earlier version stored.
const stats = atom({ plugin: 'the-dam', key: 'stats' } as const, EMPTY, { shape: 'stats-16' })
// What each tool did in this session, newest last.
const journal = atom({ plugin: 'the-dam', key: 'journal' } as const, [] as JournalEntry[], { shape: 'journal-1' })
const MOST_JOURNAL = 300
const PANE = 'the-dam'
const CONTEXT_EVERY_MS = 15_000

async function change($: Parameters<Hook<'session.start'>>[0], edit: (current: Stats) => Stats) {
  await update($, stats, edit)
  $.ui.status(statusOf(await read($, stats)))
}

async function attempt<T>(reading: () => Promise<T>): Promise<T | null> {
  try {
    return await reading()
  } catch {
    return null
  }
}

type Dollar = Parameters<Hook<'session.start'>>[0]

async function note($: Dollar, icon: string, text: string) {
  await update($, journal, list => [...list, { at: Date.now(), icon, text }].slice(-MOST_JOURNAL))
}

async function homeOf($: Dollar) {
  return ((await $.env.get('USERPROFILE')) ?? (await $.env.get('HOME')))?.replace(/\\/g, '/')
}

// Run, not just looked at: a build for another system exists here and cannot start.
async function firstExisting($: Dollar, names: readonly string[]) {
  for (const name of names) {
    if (name.includes('/') && !(await attempt(() => $.fs.stat(name)))) continue
    const run = await attempt(() => $.process.run([name, '--version'], { timeoutMs: 10_000 }))
    if (run?.exitCode === 0) return name
  }

  return null
}

let located: Located | null = null

async function locate($: Dollar): Promise<Located> {
  if (located) return located
  const home = await homeOf($)
  located = {
    squeezer: await firstExisting($, candidates('dam-squeeze', $.plugin.root, home)),
    graph: await firstExisting($, candidates('dam-graph', $.plugin.root, home)),
    ponytail: await firstExisting($, candidates('dam-ponytail', $.plugin.root, home)),
    economizer: await firstExisting($, candidates('dam-economizer', $.plugin.root, home)),
  }
  const answer = located
  // Only a complete answer is kept, so one slow start never reads as "not installed" all session.
  if (missingNames(answer).length > 0) located = null

  return answer
}

async function runSetup($: Dollar): Promise<string> {
  located = null
  const found = await locate($)
  const home = await homeOf($)
  if (!home) return 'Could not find your home folder, so nothing was installed.'
  const isWindows = (await $.env.get('USERPROFILE')) !== undefined
  const steps = setupSteps(found, $.plugin.root)
  const report: string[] = []
  for (const step of steps) {
    $.ui.toast(`Installing ${step.label}...`)
    const run = await attempt(() => $.process.run(step.argv, { timeoutMs: 600_000 }))
    const isDone = run?.exitCode === 0
    report.push(`${isDone ? 'Done' : 'Failed'}: ${step.label}`)
    // Later steps build on this one, so a failure stops here rather than half-installing.
    if (!isDone) {
      report.push((run?.stderr || 'Command not found').slice(-400))
      break
    }
  }
  located = null
  await locate($)
  void attempt(() => refresh($))
  if (report.length === 0) return 'All four tools are already installed.'

  return `${report.join('\n')}\nRestart Claude Code to load any new plugins.`
}

// Nothing to run by hand: the session's first step installs what is missing, and passes when nothing is.
async function installMissing($: Dollar) {
  if (missingNames(await locate($)).length === 0) return
  const failed = (await runSetup($)).split('\n').find(line => line.startsWith('Failed'))
  $.ui.toast(signed(failed ?? 'Tools installed. Restart Claude Code to load new plugins.'))
}

const CHECK_AFTER_MS = 15_000
let isCheckPending = false
let lastGains: Gains | null = null

// Reads every tool's figures again and announces what is new, at most once per CHECK_AFTER_MS while tools run.
async function checkGains($: Dollar) {
  await refresh($)
  const now = await read($, stats)
  const note = lastGains ? gainNote(lastGains, now) : null
  lastGains = now
  if (note) $.ui.toast(signed(note))
}

function checkSoon($: Dollar) {
  if (isCheckPending) return
  isCheckPending = true
  $.clock.after(CHECK_AFTER_MS, () => {
    isCheckPending = false
    void attempt(() => checkGains($))
  })
}

const COMPRESS_OFF = 'compress-off'
const SQUEEZE_TIMEOUT_MS = 3_000
const KEPT_OUTPUTS = 20
// Every session's originals together; older ones go at each session start.
const KEPT_ALL_OUTPUTS = 200
// This process's own name for its files and count, so that sessions running side by side never write over each other.
const SESSION_TAG = `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`

async function toggleCompression($: Dollar): Promise<string> {
  const wasOff = Boolean(await $.store.get(COMPRESS_OFF))
  await $.store.set(COMPRESS_OFF, !wasOff)
  void attempt(() => refresh($))

  return wasOff
    ? 'Compression is on. Large tool outputs will be condensed.'
    : 'Compression is off.'
}

let keptOutputs = 0

// The tokens saved, as one key per session: each session writes only its own, and the total is their sum.
const SAVED = 'saved'
const SAVED_HERE = `saved:${SESSION_TAG}`

async function savedTotal($: Dollar) {
  const keys = (await $.store.keys()).filter(key => key === SAVED || key.startsWith('saved:'))
  const values = await Promise.all(keys.map(key => $.store.get(key)))

  return values.reduce<number>((sum, value) => sum + Number(value ?? 0), 0)
}

async function squeeze($: Dollar, tool: string, record: Record<string, unknown>) {
  const field = squeezableField(tool, record)
  const { squeezer } = await locate($)
  if (!field || !squeezer || (await $.store.get(COMPRESS_OFF))) return null
  // A large output Claude Code already saved: condensed from the file, and shown inline if it then fits.
  const persisted = field === 'stdout' && typeof record.persistedOutputPath === 'string' ? record.persistedOutputPath : null
  const original = persisted ? await $.fs.read(persisted) : (record[field] as string)
  const run = await attempt(() => $.process.run([squeezer], { stdin: original, timeoutMs: SQUEEZE_TIMEOUT_MS }))
  if (run?.exitCode !== 0) return null
  const result = squeezed(original, run.stdout)
  if (!result) return null
  const isInline = !persisted || fitsInline(result.text)
  let path = isInline ? persisted : null
  if (!path) {
    // A ring of files: the full outputs stay readable without piling up on disk.
    keptOutputs = (keptOutputs + 1) % KEPT_OUTPUTS
    path = `${await homeOf($)}/.claude/the-dam-outputs/${SESSION_TAG}-${keptOutputs}.txt`
    await $.fs.write(path, original)
  }
  const gain = Math.max(0, result.before - result.after)
  const kept = persisted && !isInline ? ', too long to show: its saved file was condensed' : ''
  void attempt(() => note($, ICON.water, `${tool} output condensed from ${formatCount(result.before)} to ${formatCount(result.after)} tokens${kept}`))
  // A count that fails must not throw away the compression itself.
  await attempt(async () => {
    await $.store.set(SAVED_HERE, Number((await $.store.get(SAVED_HERE)) ?? 0) + gain)
    const saved = await savedTotal($)
    await change($, current => ({ ...current, saved }))
  })

  // Still too long to show: the saved file holds the condensed form, and the model gets our preview of it.
  if (persisted && !isInline) {
    await $.fs.write(persisted, result.text + squeezeNote(result.before, result.after, path))
    const preview = savedPreview(result.text, result.before, result.after, persisted, path, standoutLines(original))

    return { ...record, stdout: preview, persistedOutputPath: undefined, persistedOutputSize: undefined }
  }
  const condensed = { ...record, [field]: result.text + squeezeNote(result.before, result.after, path) }

  return persisted ? { ...condensed, persistedOutputPath: undefined, persistedOutputSize: undefined } : condensed
}

let hasEditedCode = false
const GRAPH_TIMEOUT_MS = 120_000

// Keeps the graph on the code after an edit, only where there is one.
async function reweave($: Dollar) {
  if (!hasEditedCode) return
  const { graph } = await locate($)
  if (!graph || !(await attempt(() => $.fs.stat(GRAPH)))) return
  hasEditedCode = false
  await attempt(() => $.process.run([graph, 'update'], { timeoutMs: 120_000 }))
}

async function graphable($: Dollar) {
  const { graph } = await locate($)
  if (!graph) return false
  const listing = await attempt(() => $.process.run(['git', 'ls-files']))

  return listing?.exitCode === 0 && worthGraphing(listing.stdout)
}

// Rebuilt at every opening of a real project, as it takes seconds at most: a `git pull` cannot leave it behind.
async function buildGraph($: Dollar) {
  const { graph } = await locate($)
  const excludePath = await attempt(() => $.process.run(['git', 'rev-parse', '--git-path', 'info/exclude']))
  if (!graph || excludePath?.exitCode !== 0) return
  // Out of git's sight without touching the project's own .gitignore.
  const path = excludePath.stdout.trim()
  const excluded = (await attempt(() => $.fs.read(path))) ?? ''
  if (!excluded.includes('.dam-graph')) await $.fs.write(path, `${excluded.trimEnd()}\n.dam-graph/\n`.trimStart())
  const started = Date.now()
  const built = await attempt(() => $.process.run([graph, 'update'], { timeoutMs: GRAPH_TIMEOUT_MS }))
  if (built?.exitCode === 0) await note($, ICON.map, `code graph built: ${built.stdout.trim()} in ${((Date.now() - started) / 1000).toFixed(1)} s`)
}

const GRAPH_QUERY_TIMEOUT_MS = 5_000

// What the graph says of each of `names`, exact matches only; null when there is no graph or it knows none of them.
async function askGraph($: Dollar, names: string[]) {
  const { graph } = await locate($)
  if (!graph || names.length === 0 || !(await attempt(() => $.fs.stat(GRAPH)))) return null
  const answers = await Promise.all(names.map(name => attempt(() => $.process.run([graph, 'query', name], { timeoutMs: GRAPH_QUERY_TIMEOUT_MS }))))
  const entries = answers.flatMap((run, n) => (run?.exitCode === 0 ? exactEntries(run.stdout, names[n] as string) : []))

  return entries.length > 0 ? graphFacts(entries, graph) : null
}

// Whether this project gets a graph, known a moment after the session starts: the first prompt waits for that, not for the graph.
let isGraphable: Promise<boolean> = Promise.resolve(false)
const GRAPH_WAIT_MS = 3_000

async function readContext($: Dollar): Promise<Stats['context']> {
  const usage = await attempt(() => $.session.usage({ breakdown: 'summary' }))
  const breakdown = usage?.context.breakdown
  const percent = usage?.context.percent ?? breakdown?.percentage
  if (!usage || percent === undefined) return null

  return { percent, total: breakdown?.totalTokens ?? usage.context.tokens ?? 0, max: usage.context.window }
}

async function refreshContext($: Dollar) {
  const context = await readContext($)
  if (context) await change($, now => ({ ...now, context }))
}

// What changed since the last reading, in the journal: the first reading says where the session starts from.
let seen: { tools: string; level: string | null } | null = null

async function noteChanges($: Dollar, gains: { pruned: number; pending: number }, level: string | null) {
  const tools = `${gains.pruned}+${gains.pending}`
  if (seen?.tools !== tools && (seen || gains.pruned + gains.pending > 0)) {
    await note($, ICON.logs, `${gains.pruned} tools pruned in this repository${gains.pending > 0 ? `, ${gains.pending} waiting for it to be quiet` : ''}`)
  }
  if (seen?.level !== level && (seen || level)) await note($, ICON.ponytail, `Ponytail ${level ?? 'off'}`)
  seen = { tools, level }
}

// Each tool's own figures. A tool that cannot answer simply adds nothing.
// Where TokenEconomizer keeps this repository's state, from its status: its events are the journal's other half.
let economizerState: string | null = null

async function readEconomizer($: Dollar, program: string) {
  const out = (await $.process.run([program, 'status', '--json'])).stdout
  economizerState = (JSON.parse(out) as { state?: string }).state ?? null

  return parseEconomizer(out)
}

// Ponytail writes this session's level to a file named after it.
async function readLevel($: Dollar) {
  const dir = (await $.env.get('CLAUDE_CONFIG_DIR'))?.replace(/\\/g, '/') ?? `${await homeOf($)}/.claude`
  const session = (await $.session.id()).replace(/[^A-Za-z0-9._-]/g, '_').slice(0, 200)
  const level = (await attempt(() => $.fs.read(`${dir}/ponytail-modes/session-${session}`)))?.trim()

  return level && level !== 'off' ? level : null
}

async function readSources($: Dollar, tools: Located, saved: number) {
  const [economizer, graph] = await Promise.all([
    attempt(async () => (tools.economizer ? readEconomizer($, tools.economizer) : null)),
    attempt(async () => graphNodes(await $.fs.read(GRAPH))),
  ])

  return { saved, pruned: 0, pending: 0, facts: 0, loops: 0, overhead: 0, ...economizer, nodes: graph ?? 0 }
}

async function refresh($: Dollar) {
  const tools = await locate($)
  const [context, isOff, graph, storedSaved] = await Promise.all([readContext($), $.store.get(COMPRESS_OFF), attempt(() => $.fs.stat(GRAPH)), savedTotal($)])
  const gains = await readSources($, tools, Number(storedSaved ?? 0))
  const level = tools.ponytail ? await readLevel($) : null
  await noteChanges($, gains, level)
  await change($, current => ({
    ...current,
    ...gains,
    context: context ?? current.context,
    tools: { squeezer: !!tools.squeezer, graphify: !!tools.graph, ponytail: !!tools.ponytail, economizer: !!tools.economizer },
    isCompressing: !isOff,
    hasGraph: graph !== null,
    level,
  }))
}

export const register: Register = on => {
  on('session.start', async ($, e, next) => {
    await $.command.register({ name: 'the-dam', description: 'Show what each tool saved' })
    await $.command.register({ name: 'the-dam-compress', description: 'Turn compression of large tool outputs on or off' })
    // In the background, and a step that throws must not take the session with it.
    void attempt(async () => {
      await installMissing($)
      await checkGains($)
      const { squeezer } = await locate($)
      const home = await homeOf($)
      if (squeezer && home) await $.process.run([squeezer, '--prune', `${home}/.claude/the-dam-outputs`, String(KEPT_ALL_OUTPUTS)])
    })
    isGraphable = attempt(() => graphable($)).then(Boolean)
    void isGraphable.then(isWorth => {
      if (isWorth) void attempt(() => buildGraph($).then(() => checkGains($)))
    })
    $.clock.every(CONTEXT_EVERY_MS, () => void refreshContext($))

    return next(e)
  })

  on('command.run', { command: 'the-dam' }, async $ => {
    await refresh($)
    const opened = await attempt(() => $.ui.open({ id: PANE, title: 'The dam' }))
    const now = await read($, stats)

    // Where no pane can be drawn (`claude -p`, a surface without panes), the report is the answer.
    return { text: opened?.isPlaced ? `${statusOf(now)}\nThe session's journal and the totals are open beside the chat.` : reportOf(now) }
  })

  on('ui.render', { component: 'Pane', requestId: PANE }, async ($, e) => {
    const { Box, Text } = $.ui.resolve(e)
    const [now, own, session, savedHere] = await Promise.all([read($, stats), read($, journal), $.session.id(), $.store.get(SAVED_HERE)])
    const events = economizerState ? ((await attempt(() => $.fs.read(`${economizerState}/events.jsonl`))) ?? '') : ''
    const entries = [...own, ...economizerEntries(events, session)].sort((a, b) => a.at - b.at)

    return (
      <Box flexDirection="column">
        <Text>This session</Text>
        {entries.length === 0 && <Text dimColor>Nothing yet: each tool writes here when it acts.</Text>}
        {entries.map(entry => (
          <Text>{journalLine(entry)}</Text>
        ))}
        <Text> </Text>
        <Text>Totals</Text>
        {totalLines(now, Number(savedHere ?? 0)).map(line => (
          <Text dimColor={line.startsWith('TokenEconomizer')}>{line}</Text>
        ))}
      </Box>
    )
  })

  on('command.run', { command: 'the-dam-compress' }, async $ => ({ text: await toggleCompression($) }))

  // Tells the model about the graph only where one exists, so other projects pay nothing.
  on('prompt.compose', async ($, e, next) => {
    const composed = await next(e)
    // A query builds the graph when it is not there yet, so the model can be told before the first build ends.
    const isWorth = await attempt(() => Promise.race([isGraphable, $.clock.sleep(GRAPH_WAIT_MS).then(() => false)]))
    if (!isWorth && !(await attempt(() => $.fs.stat(GRAPH)))) return composed

    return {
      sections: [...composed.sections, { id: 'the-dam:graph', text: graphNudge((await locate($)).graph ?? 'dam-graph'), scope: 'session' as const }],
    }
  })

  // A prompt that names code gets what the graph knows of those names, without anyone asking for it.
  on('prompt.submit', async ($, e, next) => {
    if (e.command || e.isMeta) return next(e)
    const names = codeNames(e.text)
    const facts = await attempt(() => askGraph($, names))
    if (!facts) return next(e)
    void attempt(() => note($, ICON.map, `code graph answered for ${names.join(', ')} in your prompt`))

    return next({ ...e, context: [...(e.context ?? []), facts] })
  }).catch(($, e, next) => next(e))

  on('tool.call', async ($, e, next) => {
    if (e.tool === 'Edit' || e.tool === 'Write') hasEditedCode = true
    const asked = e.tool === 'Bash' ? graphQuestion(e.command) : null
    if (asked) void attempt(() => note($, ICON.map, `code graph asked: ${asked}`))

    const ran = await next(e)
    if (ran.deny !== undefined || ran.isError) return ran
    // A big output is condensed before the model reads it.
    const record = (await attempt(() => squeeze($, e.tool, ran.result as Record<string, unknown>))) ?? (ran.result as Record<string, unknown>)
    checkSoon($)
    // A search for one name also gets where the graph says it is defined and who calls it.
    const name = e.tool === 'Grep' ? grepName(e.pattern) : null
    const facts = name && record?.mode === 'content' && typeof record.content === 'string' ? await attempt(() => askGraph($, [name])) : null
    if (facts) void attempt(() => note($, ICON.map, `code graph answered for ${name} beside a Grep`))
    const result = facts ? { ...record, content: `${record.content}\n\n${facts}` } : record

    return result === ran.result ? ran : ({ ...ran, result } as typeof ran)
  }).catch(($, e, next) => next(e)) // None of this may ever block a tool.

  on('turn.complete', async ($, e, next) => {
    void attempt(() => reweave($).then(() => checkGains($)))

    return next(e)
  })
}
