import { expect, test } from 'claude-code/testing'

import { EMPTY, formatCount, gainNote, reportOf, statusOf } from './model'
import { candidates, graphNodes, missingNames, parseEconomizer, setupSteps, squeezableField, squeezed, squeezeNote, worthGraphing } from './tools'

const ALL = { squeezer: true, graphify: true, economizer: true, ponytail: true }

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
  const s = { ...EMPTY, tools: ALL, context, saved: 5_000, pruned: 2, pending: 4, facts: 3, loops: 1, overhead: 10_500, nodes: 400, hasGraph: true }
  expect(statusOf(s)).toBe('🦫 5k saved · context 31%')
  expect(statusOf(EMPTY)).toBe('🦫 0 saved')
  expect(reportOf(s)).toBe('🦫 The dam\n  Context 31% (310k of 1M tokens)\n  ✓ Compression: 5k tokens saved\n  ✓ TokenEconomizer: 2 tools pruned (4 pending), 3 facts learned, 1 loops stopped, 10.5k tokens spent\n  ✓ Code graph: 400 nodes\n  ✓ Ponytail: active')
  expect(reportOf({ ...s, hasGraph: false })).toContain('… Code graph: not for this folder')
  expect(reportOf({ ...s, isCompressing: false })).toContain('✗ Compression: 5k tokens saved, turned off')
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
  expect(graphNodes('P\ta.py\nN\tF\ta.py\t0\t1\nN\tf\tgo\t0\t2\nE\t0\t1\td\n')).toBe(2)
})

const NOTHING = { squeezer: null, graph: null, ponytail: null, python: null, teBoot: null }
const EVERYTHING = { squeezer: 'h', graph: 'g', ponytail: 'y', python: 'p', teBoot: 't' }

test('setup installs only what is missing, and nothing twice', () => {
  expect(setupSteps(EVERYTHING, '/plugin')).toEqual([])
  expect(missingNames(EVERYTHING)).toEqual([])
  expect(missingNames(NOTHING)).toEqual(['dam', 'Python'])
  const labels = setupSteps(NOTHING, '/plugin').map(step => step.label)
  expect(labels.some(label => label.includes('TokenEconomizer'))).toBe(false)
  expect(labels.includes('dam')).toBe(true)
})

test('tools are found from the files Claude keeps', () => {
  expect(candidates('x', '/p', undefined)).toEqual(['/p/bin/x.exe', 'x'])
})

test('only big Bash text gets squeezed, never file bytes or search hits', () => {
  const big = 'x'.repeat(7000)
  expect(squeezableField('Bash', { stdout: big })).toBe('stdout')
  expect(squeezableField('Bash', { stdout: 'short' })).toBe(null)
  expect(squeezableField('Grep', { content: big, mode: 'content' })).toBe(null)
  expect(squeezableField('Read', { content: big })).toBe(null)
  expect(squeezableField('Bash', { stdout: big, persistedOutputPath: '/p' })).toBe(null)
  expect(squeezableField('Bash', { stdout: 'x'.repeat(30_000) })).toBe(null)
})

test('a squeeze keeps its answer only when it saves enough, and names the full output', () => {
  const big = 'x'.repeat(4_000)
  expect(squeezed(big, 'short')).toEqual({ text: 'short', before: 1000, after: 2 })
  expect(squeezed(big, big.slice(100))).toBe(null)
  expect(squeezeNote(900, 100, '/k/1.txt')).toContain('/k/1.txt')
})

const BIG = 'noisy line\n'.repeat(800)

test('a big output reaches the model condensed, with the path of the whole', async ($, on) => {
  const written: string[] = []
  on('tool.call', () => ({ result: { stdout: BIG, stderr: '', interrupted: false } }))
  on('env.get', () => ({ value: 'C:/u' }))
  on('fs.write', (_, e) => {
    written.push(e.text)

    return { value: undefined }
  })
  on('process.run', () => ({ value: { exitCode: 0, stdout: 'short', stderr: '' } }) as never)
  on('clock.after', () => ({ value: {} }) as never)
  on('store.get', () => ({ value: undefined }) as never)
  on('store.set', () => ({ value: undefined }) as never)

  const ran = await $.tool.call({ tool: 'Bash', command: 'ls' })
  const stdout = (ran.result as { stdout: string }).stdout
  expect(stdout.startsWith('short')).toBe(true)
  expect(written).toEqual([BIG])
})

test('a compressor that fails leaves the output untouched', async ($, on) => {
  on('tool.call', () => ({ result: { stdout: BIG, stderr: '', interrupted: false } }))
  on('env.get', () => ({ value: 'C:/u' }))
  on('clock.after', () => ({ value: {} }) as never)
  on('process.run', () => {
    throw new Error('refused')
  })

  const ran = await $.tool.call({ tool: 'Bash', command: 'ls' })
  expect((ran.result as { stdout: string }).stdout).toBe(BIG)
})
