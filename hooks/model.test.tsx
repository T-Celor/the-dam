import { expect, test } from 'claude-code/testing'

import { EMPTY, economizerEntries, formatCount, gainNote, journalLine, reportOf, statusOf, totalLines } from './model'
import { candidates, codeNames, exactEntries, graphNodes, graphQuestion, grepName, missingNames, parseEconomizer, setupSteps, squeezableField, squeezed, squeezeNote, savedPreview, standoutLines, worthGraphing } from './tools'

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
  expect(statusOf({ ...s, level: 'full' })).toBe('🦫 full · 💧 5k · 🪵 2+4 · 🌀 1 · 🧠 3 · 🗺️ 400 · 🌊 31%')
  expect(statusOf(EMPTY)).toBe('🦫')
  expect(statusOf({ ...EMPTY, saved: 1_500, pruned: 2 })).toBe('🦫 · 💧 1.5k · 🪵 2')
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

const NOTHING = { squeezer: null, graph: null, ponytail: null, economizer: null }
const EVERYTHING = { squeezer: 'h', graph: 'g', ponytail: 'y', economizer: 'e' }

test('setup installs only what is missing, and nothing twice', () => {
  expect(setupSteps(EVERYTHING, '/plugin')).toEqual([])
  expect(missingNames(EVERYTHING)).toEqual([])
  expect(missingNames(NOTHING)).toEqual(['dam'])
  const labels = setupSteps(NOTHING, '/plugin').map(step => step.label)
  expect(labels.some(label => label.includes('TokenEconomizer'))).toBe(false)
  expect(labels.includes('dam')).toBe(true)
})

test('tools are found from the files Claude keeps', () => {
  expect(candidates('x', '/p', undefined)).toEqual(['/p/bin/windows-x64/x.exe', '/p/bin/linux-x64/x', '/p/bin/linux-arm64/x', '/p/bin/macos-arm64/x', '/p/bin/macos-x64/x', 'x'])
})

test('only big Bash text gets squeezed, never file bytes or search hits', () => {
  const big = 'x'.repeat(7000)
  expect(squeezableField('Bash', { stdout: big })).toBe('stdout')
  expect(squeezableField('Bash', { stdout: 'short' })).toBe(null)
  expect(squeezableField('Grep', { content: big, mode: 'content' })).toBe('content')
  expect(squeezableField('Grep', { content: big, mode: 'count' })).toBe(null)
  expect(squeezableField('Read', { content: big })).toBe(null)
  expect(squeezableField('Bash', { stdout: 'preview', persistedOutputPath: '/p' })).toBe('stdout')
  expect(squeezableField('Bash', { stdout: big, backgroundTaskId: 't' })).toBe(null)
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

test('the lines that stand out are kept once, at most ten', () => {
  const text = ['ok 1', 'ERROR linker failed', 'ok 2', 'ERROR linker failed', 'warning: unused', 'errorless'].join('\n')
  expect(standoutLines(text)).toEqual(['ERROR linker failed', 'warning: unused'])
  expect(standoutLines(Array.from({ length: 30 }, (_, n) => `error ${n}`).join('\n')).length).toBe(10)
})

test('the preview of a saved output names the files, what stands out, and starts on a whole line', () => {
  const condensed = Array.from({ length: 400 }, (_, n) => `line ${n}`).join('\n')
  const preview = savedPreview(condensed, 4000, 900, '/saved.txt', '/original.txt', ['ERROR boom'])
  expect(preview).toContain('/saved.txt')
  expect(preview).toContain('/original.txt')
  expect(preview).toContain('ERROR boom')
  expect(preview.endsWith('\n')).toBe(true)
  expect(preview.length).toBeLessThan(2600)
})

test('the journal reads the events TokenEconomizer wrote for this session only', () => {
  const events = [
    '{"kind":"inject","facts":3,"at":1700000000,"session_id":"s1"}',
    '{"kind":"nudge","reason":"error_outputs: 5 failures","tool_calls":12,"at":1700000060,"session_id":"s1"}',
    '{"kind":"nudge","reason":"x","at":1700000061,"session_id":"other"}',
    '{"kind":"turn","at":1700000070,"session_id":"s1"}',
    'not json',
  ].join('\n')
  const entries = economizerEntries(events, 's1')
  expect(entries.map(entry => entry.text)).toEqual(['3 learned facts added to the context', 'loop redirected (error_outputs), after 12 tool calls'])
  expect(journalLine({ at: new Date(2026, 0, 1, 9, 5).getTime(), icon: '💧', text: 'x' })).toBe('09:05  💧 x')
})

test('the totals give this session share and every tool figure', () => {
  const lines = totalLines({ ...EMPTY, saved: 45_200, pending: 2, level: 'ultra' }, 3_100)
  expect(lines[0]).toBe('💧 45.2k tokens saved by compression, 3.1k in this session')
  expect(lines).toContain('🪵 0 tools pruned, 2 waiting')
  expect(lines).toContain('🦫 Ponytail ultra')
  expect(lines).toContain('🗺️ no code graph here')
})

test('a graph question is read from the command that asks it', () => {
  expect(graphQuestion('"C:/p/bin/windows-x64/dam-graph.exe" query "how is the log condensed"')).toBe('"how is the log condensed"')
  expect(graphQuestion('dam-graph update')).toBe(null)
  expect(graphQuestion('ls')).toBe(null)
})

test('only names that look like code are sent to the graph', () => {
  expect(codeNames('why does SmartCrusherPlanner drop rows in crush_rows() and parseConfig?')).toEqual(['SmartCrusherPlanner', 'crush_rows', 'parseConfig'])
  expect(codeNames('fix the typo in README and merci beaucoup')).toEqual([])
  expect(codeNames('call run() then Foo')).toEqual([])
})

test('a Grep for one name is recognised, a broader pattern is not', () => {
  expect(grepName('\\bSmartCrusher\\b')).toBe('SmartCrusher')
  expect(grepName('def compress_tool_results')).toBe('compress_tool_results')
  expect(grepName('parse\\(')).toBe('parse')
  expect(grepName('TODO|FIXME')).toBe(null)
  expect(grepName('foo.*bar')).toBe(null)
})

test('only the entries about exactly that name are kept', () => {
  const answer = '3 nodes. Best matches of 3:\n\nSmartCrusherPlanner a.rs:49\n\nSmartCrusher b.py:269\n  called by: main()\n\ncrusher() c.py:1'
  expect(exactEntries(answer, 'SmartCrusher')).toEqual(['SmartCrusher b.py:269\n  called by: main()'])
  expect(exactEntries(answer, 'crusher')).toEqual(['crusher() c.py:1'])
})
