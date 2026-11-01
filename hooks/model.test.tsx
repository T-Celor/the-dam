import { expect, test } from 'claude-code/testing'

import { EMPTY, formatCount, gainNote, reportOf, statusOf } from './model'
import { candidates, compressRequest, graphNodes, missingNames, parseEconomizer, parseInstalled, setupSteps, squeezableField, squeezed, squeezeNote, teBootFrom, worthGraphing } from './tools'

const ALL = { headroom: true, graphify: true, economizer: true, ponytail: true }

test('counts read short', () => {
  expect(formatCount(1_234_000)).toBe('1.2M')
  expect(formatCount(5_000)).toBe('5k')
})

test('a check announces only what is new', () => {
  const was = { saved: 1_000, pruned: 2, facts: 1, loops: 0, nodes: 50 }
  expect(gainNote(was, was)).toBe(null)
  expect(gainNote(was, { ...was, saved: 900 })).toBe(null)
  expect(gainNote(was, { ...was, saved: 2_500, pruned: 3 })).toBe('+1.5k tokens saved, +1 tools pruned')
})

test('the status line and the report state the real figures', () => {
  const context = { percent: 31, total: 310_000, max: 1_000_000 }
  const s = { ...EMPTY, tools: ALL, proxy: 'up' as const, context, saved: 5_000, pruned: 2, pending: 4, facts: 3, loops: 1, overhead: 10_500, nodes: 400, hasGraph: true }
  expect(statusOf(s)).toBe('🦫 5k saved · context 31%')
  expect(statusOf(EMPTY)).toBe('🦫 0 saved')
  expect(reportOf(s)).toBe('🦫 The dam\n  Context 31% (310k of 1M tokens)\n  ✓ Headroom: 5k tokens saved\n  ✓ TokenEconomizer: 2 tools pruned (4 pending), 3 facts learned, 1 loops stopped, 10.5k tokens spent\n  ✓ graphify: 400 graph nodes\n  ✓ Ponytail: active')
  expect(reportOf({ ...s, hasGraph: false })).toContain('… graphify: no graph in this folder')
  expect(reportOf({ ...s, proxy: 'off' })).toContain('✗ Headroom: 5k tokens saved, compression off')
  expect(reportOf(EMPTY).split('not installed').length - 1).toBe(4)
})

test('only a real project is worth a graph', () => {
  const files = (n: number, name: string) => Array.from({ length: n }, (_, i) => `src/${i}${name}`).join('\n')
  expect(worthGraphing(files(20, '.ts'))).toBe(true)
  expect(worthGraphing(files(19, '.py'))).toBe(false)
  expect(worthGraphing(files(100, '.md'))).toBe(false)
  expect(worthGraphing('')).toBe(false)
})

test('tool figures are read from their own output', () => {
  expect(parseEconomizer('{"toolprune":{"applied":[1,2],"proposal":[3]},"microskills":{"injected_lines":5},"ledger":{"nudges":2,"aborts":1,"overhead_tokens":10548.15}}')).toEqual({ pruned: 2, pending: 1, facts: 5, loops: 3, overhead: 10548 })
  expect(parseEconomizer('{}')).toEqual({ pruned: 0, pending: 0, facts: 0, loops: 0, overhead: 0 })
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

const UP = { value: { value: { shape: 'stats-14', value: { ...EMPTY, proxy: 'up' } }, version: 1 } } as never
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
