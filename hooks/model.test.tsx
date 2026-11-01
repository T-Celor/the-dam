import { expect, test } from 'claude-code/testing'

import { EMPTY, formatCount, gainNote, reportOf, statusOf } from './model'
import { candidates, compressRequest, graphNodes, missingNames, parseEconomizer, parseInstalled, setupSteps, squeezableField, squeezed, squeezeNote, teBootFrom } from './tools'

const ALL = { headroom: true, graphify: true, economizer: true, ponytail: true }

test('counts read short', () => {
  expect(formatCount(1_234_000)).toBe('1.2M')
  expect(formatCount(5_000)).toBe('5k')
})

test('a check announces only what is new', () => {
  const was = { saved: 1_000, pruned: 2, facts: 1, nodes: 50 }
  expect(gainNote(was, was)).toBe(null)
  expect(gainNote(was, { ...was, saved: 900 })).toBe(null)
  expect(gainNote(was, { ...was, saved: 2_500, pruned: 3 })).toBe('+1.5k tokens saved, +1 tools pruned')
})

test('the status line and the report state the real figures', () => {
  const context = { percent: 31, total: 310_000, max: 1_000_000 }
  const s = { ...EMPTY, tools: ALL, proxy: 'up' as const, context, saved: 5_000, pruned: 2, facts: 3, nodes: 400, hasGraph: true }
  expect(statusOf(s)).toBe('the dam: 5k saved, context 31%')
  expect(statusOf(EMPTY)).toBe('the dam: 0 saved')
  expect(reportOf(s)).toBe('Context: 310k of 1M tokens (31%)\nHeadroom: 5k tokens saved\nTokenEconomizer: 2 tools pruned, 3 facts learned\ngraphify: 400 graph nodes\nPonytail: active')
  expect(reportOf({ ...s, hasGraph: false })).toContain('no graph in this folder')
  expect(reportOf({ ...s, proxy: 'off' })).toContain('compression off')
  expect(reportOf(EMPTY).split('not installed').length - 1).toBe(4)
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
