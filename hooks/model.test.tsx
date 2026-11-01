import { expect, test } from 'claude-code/testing'

import { contextBar, EMPTY, formatCount, gainNote, rowsOf } from './model'
import { candidates, compressRequest, graphNodes, missingNames, parseEconomizer, parseInstalled, setupSteps, squeezableField, squeezed, squeezeNote, teBootFrom } from './tools'

const PANE = {
  plugin: 'the-dam',
  component: 'Pane',
  requestId: 'the-dam',
  props: {
    title: 'The dam',
    isFocused: false,
    bodyColumns: 48,
    placement: 'dock',
    scroll: { offset: 0, bodyRows: 13 },
    view: {},
  },
} as const

const ALL = { headroom: true, graphify: true, economizer: true, ponytail: true }
const detail = (s: typeof EMPTY, name: string) => rowsOf(s).find(row => row.name === name)

test('counts read short, and the bar stays in its width', () => {
  expect(formatCount(1_234_000)).toBe('1.2M')
  expect(formatCount(5_000)).toBe('5k')
  expect(contextBar(50, 10)).toBe('█████░░░░░')
  expect(contextBar(140, 10)).toBe('██████████')
  expect(contextBar(-5, 10)).toBe('░░░░░░░░░░')
})

test('a check announces only what is new', () => {
  const was = { saved: 1_000, pruned: 2, facts: 1, nodes: 50 }
  expect(gainNote(was, was)).toBe(null)
  expect(gainNote(was, { ...was, saved: 900 })).toBe(null)
  expect(gainNote(was, { ...was, saved: 2_500, pruned: 3 })).toBe('+1.5k tokens saved, +1 tools pruned')
})

test('every row states its real figure and what to do when it is off', () => {
  expect(rowsOf(EMPTY).every(row => row.health === 'off' && row.detail.includes('/the-dam-setup'))).toBe(true)
  const s = { ...EMPTY, tools: ALL, proxy: 'up' as const, saved: 5_000, pruned: 2, facts: 3, nodes: 400, hasGraph: true }
  expect(detail(s, 'Headroom')).toEqual({ name: 'Headroom', health: 'ok', detail: '5k tokens saved' })
  expect(detail(s, 'TokenEconomizer')?.detail).toBe('2 tools pruned, 3 facts learned')
  expect(detail(s, 'graphify')?.detail).toBe('400 graph nodes')
  expect(detail({ ...s, hasGraph: false }, 'graphify')?.detail).toBe('no graph here: /graphify')
  expect(detail({ ...s, proxy: 'off' }, 'Headroom')?.detail).toBe('turned off: /the-dam-compress')
})

test('tool figures are read from their own output', () => {
  expect(parseEconomizer('{"toolprune":{"applied":[1,2]},"microskills":{"injected_lines":5}}')).toEqual({ pruned: 2, facts: 5 })
  expect(graphNodes('{"nodes":[{},{}]}')).toBe(2)
})

const NOTHING = { headroom: null, graphify: null, python: null, teBoot: null, hasEconomizer: false, hasPonytail: false }
const EVERYTHING = { headroom: 'h', graphify: 'g', python: 'p', teBoot: 't', hasEconomizer: true, hasPonytail: true }

test('setup installs only what is missing, and nothing twice', () => {
  expect(setupSteps(EVERYTHING, '/home', false, false)).toEqual([])
  expect(missingNames(EVERYTHING)).toEqual([])
  expect(missingNames(NOTHING)).toEqual(['Headroom', 'graphify', 'TokenEconomizer', 'Ponytail'])
  const labels = setupSteps(NOTHING, '/home', false, true).map(step => step.label)
  expect(labels.includes('clone TokenEconomizer')).toBe(false)
  expect(labels.includes('plugin Ponytail')).toBe(true)
})

test('tools are found from the files Claude keeps', () => {
  expect(parseInstalled('{"plugins":{"ponytail@ponytail":[]}}')).toEqual([false, true])
  expect(teBootFrom('{"tokeneconomizer":{"installLocation":"C:\\\\u\\\\TE"}}')).toBe('C:/u/TE/plugin/scripts/te_bootstrap.py')
  expect(teBootFrom('{}')).toBe(null)
  expect(candidates('x', undefined)).toEqual(['x'])
})

test('only big Bash and Grep text gets squeezed, never exact file bytes', () => {
  const big = 'x'.repeat(7000)
  expect(squeezableField('Bash', { stdout: big })).toBe('stdout')
  expect(squeezableField('Bash', { stdout: 'short' })).toBe(null)
  expect(squeezableField('Grep', { content: big, mode: 'content' })).toBe('content')
  expect(squeezableField('Read', { content: big })).toBe(null)
})

test('a squeeze keeps its answer only when it saves enough, and names the full output', () => {
  const answer = (saved: number) =>
    JSON.stringify({ tokens_saved: saved, tokens_before: 900, tokens_after: 100, messages: [{}, {}, { content: [{ content: 'short' }] }] })
  expect(squeezed(answer(800))?.text).toBe('short')
  expect(squeezed(answer(10))).toBe(null)
  expect(squeezeNote(900, 100, '/k/1.txt')).toContain('/k/1.txt')
  expect(JSON.parse(compressRequest('hi', 'claude-x')).model).toBe('claude-x')
})

const UP = { value: { value: { shape: 'stats-13', value: { ...EMPTY, proxy: 'up' } }, version: 1 } } as never
const BIG = 'noisy line\n'.repeat(800)

test('a big output reaches the model condensed, with the path of the whole', async ($, on) => {
  const written: string[] = []
  on('tool.call', () => ({ result: { stdout: BIG, stderr: '', interrupted: false } }))
  on('env.get', () => ({ value: 'C:/u' }))
  on('clock.sleep', () => new Promise(() => undefined))
  on('fs.write', (_, e) => {
    written.push(e.text)

    return { value: undefined }
  })
  on('state.get', (_, e, next) => (e.key === 'stats' ? UP : next(e)))
  on('http.fetch', () => ({
    value: {
      ok: true,
      status: 200,
      headers: {},
      text: JSON.stringify({ tokens_saved: 900, tokens_before: 1000, tokens_after: 100, messages: [{}, {}, { content: [{ content: 'short' }] }] }),
    },
  }))

  const ran = await $.tool.call({ tool: 'Bash', command: 'ls' })
  const stdout = (ran.result as { stdout: string }).stdout
  expect(stdout.startsWith('short')).toBe(true)
  expect(written).toEqual([BIG])
})

test('a proxy that fails leaves the output untouched', async ($, on) => {
  on('tool.call', () => ({ result: { stdout: BIG, stderr: '', interrupted: false } }))
  on('clock.sleep', () => new Promise(() => undefined))
  on('http.fetch', () => {
    throw new Error('refused')
  })
  on('state.get', (_, e, next) => (e.key === 'stats' ? UP : next(e)))

  const ran = await $.tool.call({ tool: 'Bash', command: 'ls' })
  expect((ran.result as { stdout: string }).stdout).toBe(BIG)
})

test('the pane draws plain text on every surface', async $ => {
  for (const surface of ['desktop', 'terminal'] as const) {
    const pane = await $.ui.mount({ ...PANE, surface })
    expect(await pane.drawn()).toMatchObject({ type: 'Box' })
    await pane.unmount()
  }
})
