import { atom, read, update } from 'claude-code'
import type { Hook, Register } from 'claude-code'

import type { ProxyState, Stats } from '../types'
import type { Materials } from './model'
import { earnedFrom, gainNote, nextRiver, nextWear, NO_MATERIALS, pressureOf } from './model'
import { frameCells } from './raster'
import { COLD, cool, damScene, EMPTY, MOST_ASPECT, nextPennants, sceneAlt, strengthFor, usedBy, warm } from './scene'
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
const DOCK_COLUMNS = 90
const PANE_ROWS = 24
// shortcut: a code-font cell is about 8 by 18 pixels, read the font's real metric if the fit is off.
const CELL_WIDTH = 8
const CELL_HEIGHT = 18

// Change the tag when Stats changes: it declines what an earlier version stored.
const stats = atom({ plugin: 'the-dam', key: 'stats' } as const, EMPTY, { shape: 'stats-12' })
const heat = atom({ plugin: 'the-dam', key: 'heat' } as const, COLD, { shape: 'heat-2' })
const COOL_EVERY_MS = 45_000
const WATER_EVERY_MS = 15_000
const FRAME_MS = 100
const PAUSED = 'paused'
const RASTER = 'dam'

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

const CHECK_AFTER_MS = 15_000
let isCheckPending = false
let lastEarned: Materials | null = null

// Reads every tool's figures again and announces what is new, at most once per CHECK_AFTER_MS while tools run.
async function checkGains($: Dollar) {
  await refresh($)
  const { earned } = await read($, stats)
  const note = lastEarned ? gainNote(lastEarned, earned) : null
  lastEarned = earned
  if (note) $.ui.toast(`The dam grew: ${note}`)
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

// The terminal has no SVG: the same dam is computed pixel by pixel and repainted in place.
let mounted: { columns: number; rows: number } | null = null
let sceneTime = 0
let isPaused = false

async function paintFrame($: Dollar) {
  if (!mounted || isPaused) return
  sceneTime += FRAME_MS / 1000
  const { columns, rows } = mounted
  const cells = frameCells(await read($, stats), await read($, heat), sceneTime, columns, rows)
  const painted = await $.ui.blit({ requestId: PANE, key: RASTER, columns, rows, cells })
  // Closed or resized: the next draw says where it is now.
  if (painted.deny) mounted = null
}

async function togglePause($: Dollar): Promise<string> {
  isPaused = !isPaused
  await $.store.set(PAUSED, isPaused)

  return isPaused ? 'The dam is paused.' : 'The dam is moving again.'
}

let keptOutputs = 0
let sessionModel: string | undefined
let projectCwd: string | undefined

const savedKey = (cwd: string) => `saved:${cwd}`
const pennantsKey = (cwd: string) => `pennants:${cwd}`
const wearKey = (cwd: string) => `wear:${cwd}`
const PARTY_MS = 5_000

async function celebrate($: Dollar) {
  await update($, stats, current => ({ ...current, isParty: true }))
  $.clock.after(PARTY_MS, () => void update($, stats, current => ({ ...current, isParty: false })))
}

// Scores the water's move, and throws a party when a pennant is won.
async function scoreWater($: Dollar, before: number | null, after: number): Promise<number> {
  if (!projectCwd) return 0
  const had = Number((await $.store.get(pennantsKey(projectCwd))) ?? 0)
  const now = nextPennants(had, before, after)
  if (now !== had) await $.store.set(pennantsKey(projectCwd), now)
  if (now > had) void celebrate($)

  return now
}

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
  await update($, heat, h => warm(h, ['hazel']))
  const gain = Math.max(0, result.before - result.after)
  if (projectCwd) {
    const saved = Number((await $.store.get(savedKey(projectCwd))) ?? 0) + gain
    await $.store.set(savedKey(projectCwd), saved)
    await update($, stats, current => ({ ...current, saved, earned: { ...current.earned, twigs: Math.floor(saved / 100) } }))
  }

  return { ...record, [field]: result.text + squeezeNote(result.before, result.after, path) }
}

let hasEditedCode = false

// Keeps the graph on the code: AST only, no LLM, and only where someone already built one.
async function reweave($: Dollar) {
  if (!hasEditedCode) return
  const { graphify } = await locate($)
  if (!graphify || !(await attempt(() => $.fs.stat(GRAPH)))) return
  hasEditedCode = false
  await update($, heat, h => warm(h, ['mimi'], 3))
  await attempt(() => $.process.run([graphify, 'update', '.'], { timeoutMs: 120_000 }))
}

async function readContext($: Dollar): Promise<Stats['context']> {
  const usage = await attempt(() => $.session.usage({ breakdown: 'summary' }))
  const breakdown = usage?.context.breakdown
  const percent = usage?.context.percent ?? breakdown?.percentage
  if (!usage || percent === undefined) return null

  return {
    percent,
    total: breakdown?.totalTokens ?? usage.context.tokens ?? 0,
    max: usage.context.window,
    layers: (breakdown?.categories ?? [])
      .filter(row => row.kind === 'used' && row.tokens > 0)
      .sort((a, b) => b.tokens - a.tokens)
      .map(row => ({ name: row.name, tokens: row.tokens })),
  }
}

const WATER_EVERY_S = WATER_EVERY_MS / 1000

// shortcut: wear only moves while a session runs, so nothing is repaired between sessions; store a timestamp if that matters.
async function refreshWater($: Dollar) {
  const context = await readContext($)
  if (!context) return
  const current = await read($, stats)
  const pennants = await scoreWater($, current.context?.percent ?? null, context.percent)
  const strength = strengthFor(current)
  const wear = nextWear(current.wear, pressureOf(context.percent, strength), strength, WATER_EVERY_S)
  if (projectCwd) await $.store.set(wearKey(projectCwd), wear)
  await update($, stats, now => ({ ...now, context, pennants, wear, river: nextRiver(now.river, context.percent) }))
}

// Each tool's own figures. A tool that cannot answer simply adds nothing.
async function readSources($: Dollar, tools: Located, saved: number) {
  const [economizer, graph] = await Promise.all([
    attempt(async () =>
      tools.python && tools.teBoot ? parseEconomizer((await $.process.run([tools.python, tools.teBoot, 'cli', 'status', '--json'])).stdout) : null,
    ),
    attempt(async () => graphNodes(await $.fs.read(GRAPH))),
  ])

  return earnedFrom({ saved, pruned: economizer?.pruned ?? 0, facts: economizer?.facts ?? 0, nodes: graph ?? 0 })
}

async function refresh($: Dollar) {
  const tools = await locate($)
  const [context, proxy, graph, storedSaved, storedWear] = await Promise.all([
    readContext($),
    proxyState($),
    attempt(() => $.fs.stat(GRAPH)),
    projectCwd ? $.store.get(savedKey(projectCwd)) : 0,
    projectCwd ? $.store.get(wearKey(projectCwd)) : null,
  ])
  const saved = Number(storedSaved ?? 0)
  const earned = await readSources($, tools, saved)
  const before = (await read($, stats)).context?.percent ?? null
  const pennants = context ? await scoreWater($, before, context.percent) : (await read($, stats)).pennants
  await update($, stats, current => ({
    ...current,
    pennants,
    context,
    saved,
    earned,
    wear: (storedWear as typeof NO_MATERIALS | undefined) ?? current.wear,
    river: context ? nextRiver(current.river, context.percent) : current.river,
    tools: { hazel: !!tools.headroom, mimi: !!tools.graphify, pompom: tools.hasPonytail, truffle: tools.hasEconomizer },
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
    await $.command.register({ name: 'the-dam-pause', description: 'Pause or resume the animation in the terminal' })
    await $.command.register({ name: 'the-dam-compress', description: 'Turn compression of large tool outputs on or off' })
    void $.ui.open({ id: PANE, title: TITLE, columns: DOCK_COLUMNS, rows: PANE_ROWS })
    void installMissing($)
      .then(() => ensureProxy($))
      .then(() => checkGains($))
    $.clock.every(COOL_EVERY_MS, () => void update($, heat, cool))
    $.clock.every(WATER_EVERY_MS, () => void refreshWater($))
    isPaused = Boolean(await $.store.get(PAUSED))
    $.clock.every(FRAME_MS, () => void paintFrame($))

    return next(e)
  })

  on('command.run', { command: 'the-dam' }, async $ => {
    void refresh($)
    await $.ui.open({ id: PANE, title: TITLE, columns: DOCK_COLUMNS, rows: PANE_ROWS })

    return { text: 'The dam is open.' }
  })

  on('command.run', { command: 'the-dam-pause' }, async $ => ({ text: await togglePause($) }))

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
    const subject =
      e.tool === 'Bash'
        ? e.command
        : e.tool === 'Skill'
          ? e.skill
          : e.tool === 'Read'
            ? e.file_path
            : e.tool === 'Grep'
              ? (e.path ?? '')
              : ''
    const used = usedBy(e.tool, subject)
    if (used.length > 0) await update($, heat, h => warm(h, used))
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

  // The scene alone. Its alternative carries every figure, for screen readers and the terminal.
  on('ui.render', { component: 'Pane', requestId: PANE }, async ($, e) => {
    const s = await read($, stats)
    const levels = await read($, heat)
    if (e.surface === 'terminal') {
      const { Raster } = $.ui.resolve(e)
      const columns = Math.min(512, e.props.bodyColumns)
      const rows = Math.min(256, Math.max(2, e.props.scroll.bodyRows))
      mounted = { columns, rows }

      return <Raster key={RASTER} columns={columns} rows={rows} cells={frameCells(s, levels, sceneTime, columns, rows)} />
    }
    const { Svg } = $.ui.resolve(e)

    // An explicit size: without one the frame keeps its default 150 pixels of height.
    const width = Math.max(320, e.props.bodyColumns * CELL_WIDTH)
    // Slightly under the room, so the panel never grows a scrollbar.
    const height = Math.max(240, Math.round((e.props.scroll.bodyRows - 1) * CELL_HEIGHT * 0.96))
    const source = damScene(s, levels, height / width)

    return <Svg source={source} alt={sceneAlt(s)} width={width} height={Math.min(height, Math.round(width * MOST_ASPECT))} isInteractive />
  })
}
