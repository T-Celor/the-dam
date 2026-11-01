import { atom, read, update } from 'claude-code'
import type { Hook, Register } from 'claude-code'

import type { Gains, ProxyState, Stats } from '../types'
import { contextBar, EMPTY, formatCount, gainNote, rowsOf } from './model'
import type { Located } from './tools'
import {
  candidates,
  compressRequest,
  GRAPH_NUDGE,
  graphNodes,
  missingNames,
  parseEconomizer,
  parseInstalled,
  PROXY,
  setupSteps,
  squeezableField,
  squeezed,
  squeezeNote,
  teBootFrom,
} from './tools'

const PANE = 'the-dam'
const TITLE = 'The dam'
const GRAPH = 'graphify-out/graph.json'
const DOCK_COLUMNS = 48
const PANE_ROWS = 13

// Change the tag when Stats changes: it declines what an earlier version stored.
const stats = atom({ plugin: 'the-dam', key: 'stats' } as const, EMPTY, { shape: 'stats-13' })
const CONTEXT_EVERY_MS = 15_000

async function attempt<T>(reading: () => Promise<T>): Promise<T | null> {
  try {
    return await reading()
  } catch {
    return null
  }
}

type Dollar = Parameters<Hook<'session.start'>>[0]

async function homeOf($: Dollar) {
  return ((await $.env.get('USERPROFILE')) ?? (await $.env.get('HOME')))?.replace(/\\/g, '/')
}

// A file on disk answers at once; starting Headroom takes over ten seconds and can time out under load.
async function firstExisting($: Dollar, names: readonly string[]) {
  for (const name of names) {
    if (name.includes('/') && (await attempt(() => $.fs.stat(name)))) return name
  }
  for (const name of names) {
    if (name.includes('/')) continue
    const run = await attempt(() => $.process.run([name, '--version'], { timeoutMs: 60_000 }))
    if (run?.exitCode === 0) return name
  }

  return null
}

async function firstPython($: Dollar, names: readonly string[]) {
  for (const name of names) {
    const run = await attempt(() => $.process.run([name, '--version']))
    if (run?.exitCode === 0) return name
  }

  return null
}

let located: Located | null = null

async function locate($: Dollar): Promise<Located> {
  if (located) return located
  const home = await homeOf($)
  const installed = await attempt(async () => parseInstalled(await $.fs.read(`${home}/.claude/plugins/installed_plugins.json`)))
  const uvPython = await attempt(async () => {
    const run = await $.process.run(['uv', 'python', 'find', '3.12'])

    return run.exitCode === 0 ? run.stdout.trim() : null
  })
  const envPython = await $.env.get('TE_PYTHON')
  located = {
    headroom: await firstExisting($, candidates('headroom', home)),
    graphify: await firstExisting($, candidates('graphify', home)),
    python: await firstPython($, [envPython, uvPython, 'python3', 'py'].filter(name => !!name) as string[]),
    teBoot: await attempt(async () => teBootFrom(await $.fs.read(`${home}/.claude/plugins/known_marketplaces.json`))),
    hasEconomizer: installed?.[0] ?? false,
    hasPonytail: installed?.[1] ?? false,
  }
  const answer = located
  // Only a complete answer is kept, so one slow start never reads as "not installed" all session.
  if (missingNames(answer).length > 0 || !answer.python) located = null

  return answer
}

async function doctor($: Dollar): Promise<string> {
  located = null
  const tools = await locate($)
  await refresh($)

  return JSON.stringify({ home: await homeOf($), cwd: projectCwd, tools, missing: missingNames(tools), proxy: await proxyState($), stats: await read($, stats) }, null, 1)
}

async function runSetup($: Dollar): Promise<string> {
  located = null
  const found = await locate($)
  const home = await homeOf($)
  if (!home) return 'Could not find your home folder, so nothing was installed.'
  const isWindows = (await $.env.get('USERPROFILE')) !== undefined
  const hasClone = (await attempt(() => $.fs.stat(`${home}/TokenEconomizer`))) !== null
  const steps = setupSteps(found, home, isWindows, hasClone)
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
  const after = await locate($)
  // TokenEconomizer's own hooks need this where no python3 is on PATH, as a Store alias is not.
  if (isWindows && after.python && !(await $.env.get('TE_PYTHON')) && after.python !== 'python3') {
    const run = await attempt(() => $.process.run(['setx', 'TE_PYTHON', after.python as string]))
    report.push(`${run?.exitCode === 0 ? 'Done' : 'Failed'}: TE_PYTHON`)
  }
  void refresh($)
  if (report.length === 0) return 'All four tools are already installed.'

  return `${report.join('\n')}\nRestart Claude Code to load any new plugins.`
}

async function runRoute($: Dollar): Promise<string> {
  // Headroom's own doctor: Desktop overwrites ANTHROPIC_BASE_URL, so routing cannot reach it.
  const surfaces = await $.session.surfaces()
  if (!surfaces.includes('terminal'))
    return 'Routing does not work in Claude Desktop, which overwrites ANTHROPIC_BASE_URL. Run /the-dam-route from `claude` in a terminal.'
  const { headroom } = await locate($)
  if (!headroom) return 'Headroom is not installed. Run /the-dam-setup first.'
  const run = await attempt(() => $.process.run([headroom, 'init', '-g', 'claude'], { timeoutMs: 120_000 }))
  if (run?.exitCode !== 0) return `Routing failed.\n${(run?.stderr ?? '').slice(-400)}`
  void refresh($)

  return `Headroom routing starts with your next session.\n${run.stdout.slice(-400)}`
}

// Nothing to run by hand: whatever is missing gets installed, once per session, in the background.
async function installMissing($: Dollar) {
  if (missingNames(await locate($)).length === 0) return
  $.ui.toast((await runSetup($)).split('\n').slice(-1)[0] ?? 'Setup finished.')
}

const OPEN_TRIES = 20

// At session start the terminal may not be drawing yet, so keep asking until a surface seats the panel.
async function openWhenPlaced($: Dollar, tries = OPEN_TRIES) {
  const opened = await attempt(() => $.ui.open({ id: PANE, title: TITLE, columns: DOCK_COLUMNS, rows: PANE_ROWS }))
  if (!opened?.isPlaced && tries > 1) $.clock.after(1_000, () => void openWhenPlaced($, tries - 1))
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
  if (note) $.ui.toast(`The dam: ${note}`)
}

function checkSoon($: Dollar) {
  if (isCheckPending) return
  isCheckPending = true
  $.clock.after(CHECK_AFTER_MS, () => {
    isCheckPending = false
    void checkGains($)
  })
}

const COMPRESS_OFF = 'compress-off'
const SQUEEZE_TIMEOUT_MS = 3_000
const KEPT_OUTPUTS = 20

async function isProxyUp($: Dollar) {
  return (await attempt(() => $.http.fetch(`${PROXY}/readyz`)))?.ok === true
}

let isProxyStarting = false

// Several sessions share one proxy; it lives as long as the session that started it.
async function ensureProxy($: Dollar) {
  if (isProxyStarting || (await $.store.get(COMPRESS_OFF)) || (await isProxyUp($))) return
  const { headroom } = await locate($)
  if (!headroom) return
  isProxyStarting = true
  const poll = $.clock.every(5_000, () => {
    void (async () => {
      if (!(await isProxyUp($))) return
      isProxyStarting = false
      poll.cancel()
      void refresh($)
    })()
  })
  void (async () => {
    try {
      for await (const _ of $.process.spawn({ argv: [headroom, 'proxy', '--port', '8787'] })) {
        // The output only needs draining.
      }
    } catch {
      // A proxy that cannot start leaves every tool output as it was.
    } finally {
      isProxyStarting = false
      poll.cancel()
    }
  })()
}

async function proxyState($: Dollar): Promise<ProxyState> {
  if (await $.store.get(COMPRESS_OFF)) return 'off'
  if (await isProxyUp($)) return 'up'

  return isProxyStarting ? 'starting' : 'down'
}

async function toggleCompression($: Dollar): Promise<string> {
  const wasOff = Boolean(await $.store.get(COMPRESS_OFF))
  await $.store.set(COMPRESS_OFF, !wasOff)
  if (wasOff) await ensureProxy($)
  void refresh($)

  return wasOff
    ? 'Compression is on. Large tool outputs will be condensed.'
    : 'Compression is off. A running proxy stays up until its session ends.'
}

let keptOutputs = 0
let sessionModel: string | undefined
let projectCwd: string | undefined

// All projects together: a folder of its own for each session must not start the pile again from nothing.
const SAVED = 'saved'

async function squeeze($: Dollar, tool: string, record: Record<string, unknown>) {
  const field = squeezableField(tool, record)
  if (!field || (await read($, stats)).proxy !== 'up') return null
  const original = record[field] as string
  const response = await Promise.race([
    $.http.fetch(`${PROXY}/v1/compress`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: compressRequest(original, sessionModel),
    }),
    $.clock.sleep(SQUEEZE_TIMEOUT_MS).then(() => null),
  ])
  if (!response?.ok) return null
  const result = squeezed(response.text)
  if (!result) return null
  // A ring of files: the full outputs stay readable without piling up on disk.
  keptOutputs = (keptOutputs + 1) % KEPT_OUTPUTS
  const path = `${await homeOf($)}/.claude/the-dam-outputs/${keptOutputs}.txt`
  await $.fs.write(path, original)
  const gain = Math.max(0, result.before - result.after)
  // A count that fails must not throw away the compression itself.
  await attempt(async () => {
    const saved = Number((await $.store.get(SAVED)) ?? 0) + gain
    await $.store.set(SAVED, saved)
    await update($, stats, current => ({ ...current, saved }))
  })

  return { ...record, [field]: result.text + squeezeNote(result.before, result.after, path) }
}

let hasEditedCode = false

// Keeps the graph on the code: AST only, no LLM, and only where someone already built one.
async function reweave($: Dollar) {
  if (!hasEditedCode) return
  const { graphify } = await locate($)
  if (!graphify || !(await attempt(() => $.fs.stat(GRAPH)))) return
  hasEditedCode = false
  await attempt(() => $.process.run([graphify, 'update', '.'], { timeoutMs: 120_000 }))
}

async function readContext($: Dollar): Promise<Stats['context']> {
  const usage = await attempt(() => $.session.usage({ breakdown: 'summary' }))
  const breakdown = usage?.context.breakdown
  const percent = usage?.context.percent ?? breakdown?.percentage
  if (!usage || percent === undefined) return null

  return { percent, total: breakdown?.totalTokens ?? usage.context.tokens ?? 0, max: usage.context.window }
}

async function refreshContext($: Dollar) {
  const context = await readContext($)
  if (context) await update($, stats, now => ({ ...now, context }))
}

// Each tool's own figures. A tool that cannot answer simply adds nothing.
async function readSources($: Dollar, tools: Located, saved: number) {
  const [economizer, graph] = await Promise.all([
    attempt(async () =>
      tools.python && tools.teBoot ? parseEconomizer((await $.process.run([tools.python, tools.teBoot, 'cli', 'status', '--json'])).stdout) : null,
    ),
    attempt(async () => graphNodes(await $.fs.read(GRAPH))),
  ])

  return { saved, pruned: economizer?.pruned ?? 0, facts: economizer?.facts ?? 0, nodes: graph ?? 0 }
}

async function refresh($: Dollar) {
  const tools = await locate($)
  const [context, proxy, graph, storedSaved] = await Promise.all([readContext($), proxyState($), attempt(() => $.fs.stat(GRAPH)), $.store.get(SAVED)])
  const gains = await readSources($, tools, Number(storedSaved ?? 0))
  await update($, stats, current => ({
    ...current,
    ...gains,
    context: context ?? current.context,
    tools: { headroom: !!tools.headroom, graphify: !!tools.graphify, ponytail: tools.hasPonytail, economizer: tools.hasEconomizer },
    proxy,
    hasGraph: graph !== null,
  }))
}

export const register: Register = on => {
  on('session.start', async ($, e, next) => {
    projectCwd = e.cwd
    await $.command.register({ name: 'the-dam', description: 'Open the dam' })
    await $.command.register({
      name: 'the-dam-setup',
      description: 'Install whichever of Headroom, graphify, TokenEconomizer and Ponytail is missing',
    })
    await $.command.register({ name: 'the-dam-route', description: 'Route Claude Code through Headroom (terminal only)' })
    await $.command.register({ name: 'the-dam-doctor', description: 'Check that the plugin can find each tool' })
    await $.command.register({ name: 'the-dam-compress', description: 'Turn compression of large tool outputs on or off' })
    void openWhenPlaced($)
    void installMissing($)
      .then(() => ensureProxy($))
      .then(() => checkGains($))
    $.clock.every(CONTEXT_EVERY_MS, () => void refreshContext($))

    return next(e)
  })

  on('command.run', { command: 'the-dam' }, async $ => {
    void refresh($)
    await $.ui.open({ id: PANE, title: TITLE, columns: DOCK_COLUMNS, rows: PANE_ROWS })

    return { text: 'The dam is open.' }
  })

  on('command.run', { command: 'the-dam-doctor' }, async $ => ({ text: await doctor($) }))

  on('command.run', { command: 'the-dam-setup' }, async $ => ({ text: await runSetup($) }))

  on('command.run', { command: 'the-dam-compress' }, async $ => ({ text: await toggleCompression($) }))

  on('command.run', { command: 'the-dam-route' }, async $ => ({ text: await runRoute($) }))

  // Tells the model about the graph only where one exists, so other projects pay nothing.
  on('prompt.compose', async ($, e, next) => {
    sessionModel = e.model
    const composed = await next(e)
    if (!(await read($, stats)).hasGraph) return composed

    return {
      sections: [...composed.sections, { id: 'the-dam:graphify', text: GRAPH_NUDGE, scope: 'session' as const }],
    }
  })

  on('tool.call', async ($, e, next) => {
    if (e.tool === 'Edit' || e.tool === 'Write') hasEditedCode = true

    const ran = await next(e)
    if (ran.deny !== undefined || ran.isError) return ran
    // Runs where ANTHROPIC_BASE_URL cannot: a big output is condensed before the model reads it.
    const record = await attempt(() => squeeze($, e.tool, ran.result as Record<string, unknown>))
    checkSoon($)

    return record ? ({ ...ran, result: record } as typeof ran) : ran
  }).catch(($, e, next) => next(e)) // None of this may ever block a tool.

  on('turn.complete', async ($, e, next) => {
    void reweave($)
      .then(() => ensureProxy($))
      .then(() => checkGains($))

    return next(e)
  })

  on('ui.render', { component: 'Pane', requestId: PANE }, async ($, e) => {
    const { Box, Text } = $.ui.resolve(e)
    const s = await read($, stats)
    const percent = s.context?.percent ?? 0
    const level = percent >= 90 ? 'error' : percent >= 70 ? 'warning' : 'success'
    const color = { ok: 'success', wait: 'warning', off: 'error' } as const

    return (
      <Box flexDirection="column">
        {s.context ? (
          <Box flexDirection="column" marginBottom={1}>
            <Text>
              <Text color={level}>{contextBar(percent)}</Text> <Text bold>{Math.round(percent)}%</Text>
            </Text>
            <Text dimColor>
              {formatCount(s.context.total)} of {formatCount(s.context.max)} tokens in context
            </Text>
          </Box>
        ) : (
          <Box marginBottom={1}>
            <Text dimColor>Context not measured yet</Text>
          </Box>
        )}
        {rowsOf(s).map(row => (
          <Box key={row.name} flexDirection="column">
            <Text>
              <Text color={color[row.health]}>●</Text> <Text bold>{row.name}</Text>
            </Text>
            <Text dimColor>  {row.detail}</Text>
          </Box>
        ))}
      </Box>
    )
  })
}
