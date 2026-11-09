import { expect, test } from 'claude-code/testing'

import { EMPTY, WORKER_MODEL, stepCost, usdOf, carryStep, pinnedModel, economizerEntries, projectFlow, savedOfSpent, formatCount, gainNote, journalLine, journalView, reportOf, sessionFigures, statusOf, totalLines } from './model'
import { candidates, mcpText, withMcpText, graphNodes, graphQuestion, missingNames, parseEconomizer, setupSteps, squeezableField, savedPreview, standoutLines, worthGraphing } from './tools'

const ALL = { squeezer: true, graphify: true, economizer: true, beaver: true }

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
  const session = { saved: 5_000, seen: 20_000, spent: 95_000, cost: 120_000 }
  const s = { ...EMPTY, tools: ALL, context, saved: 90_000, session, project: { saved: 12_000, seen: 30_000, spent: 228_000 }, pruned: 2, pending: 4, facts: 3, loops: 1, overhead: 10_500, nodes: 400, hasGraph: true }
  expect(statusOf({ ...s, level: 'full' })).toBe('🦫 full · 💧 5k/95k (5.0%) · 💸 120k · 🪵 2+4 · 🌀 1 · 🧠 3 · 🗺️ 400 · 🌊 31%')
  expect(statusOf(EMPTY)).toBe('🦫 off · 💧 0 · 💸 – · 🪵 0 · 🌀 0 · 🧠 0 · 🗺️ – · 🌊 –')
  expect([usdOf(1.5), usdOf({ totalUsd: 2 }), usdOf({ x: 1 }), usdOf('3'), usdOf(null)]).toEqual([1.5, 2, null, null, null])
  expect(statusOf({ ...EMPTY, session: { saved: 0, seen: 0, spent: 0, cost: 900, costUsd: 0.42 } })).toContain('💸 $0.42')
  expect(stepCost({ input_tokens: 10, cache_read_input_tokens: 100, cache_creation_input_tokens: 4, output_tokens: 2 })).toBe(35)
  expect(statusOf({ ...EMPTY, session: { saved: 1_500, seen: 0, spent: 0 }, pruned: 2 })).toBe('🦫 off · 💧 1.5k · 💸 – · 🪵 2 · 🌀 0 · 🧠 0 · 🗺️ – · 🌊 –')
  expect(reportOf(s)).toBe('🦫 The dam\n  Context 31% (310k of 1M tokens)\n  ✓ Compression: 5k/95k (5.0%) tokens saved in this session, 12k/228k (5.0%) in this project, 90k in all\n  ✓ TokenEconomizer: 2 tools pruned (4 pending), 3 facts learned, 1 loops stopped, 10.5k tokens spent\n  ✓ Map: 400 places\n  ✓ Beaver: active')
  expect(reportOf({ ...s, hasGraph: false })).toContain('… Map: not for this folder')
  expect(reportOf({ ...s, isCompressing: false })).toContain('✗ Compression: 5k/95k (5.0%) tokens saved in this session')
  expect(reportOf(EMPTY).split('not installed').length - 1).toBe(4)
})

test('only a real project is worth a graph', () => {
  const files = (n: number, name: string) => Array.from({ length: n }, (_, i) => `src/${i}${name}`).join('\n')
  expect(worthGraphing(files(20, '.ts'))).toBe(true)
  expect(worthGraphing(files(19, '.py'))).toBe(false)
  expect(worthGraphing(files(100, '.md'))).toBe(true)
  expect(worthGraphing(files(100, '.txt'))).toBe(false)
  expect(worthGraphing('')).toBe(false)
})

test('tool figures are read from their own output', () => {
  expect(parseEconomizer('{"toolprune":{"applied":[1,2],"proposal":[3]},"microskills":{"injected_lines":5},"ledger":{"nudges":2,"aborts":1,"overhead_tokens":10548.15}}')).toEqual({ pruned: 2, pending: 1, facts: 5, loops: 3, overhead: 10548 })
  expect(parseEconomizer('{}')).toEqual({ pruned: 0, pending: 0, facts: 0, loops: 0, overhead: 0 })
  expect(graphNodes('P\ta.py\nN\tF\ta.py\t0\t1\nN\tf\tgo\t0\t2\nE\t0\t1\td\n')).toBe(2)
})

const NOTHING = { squeezer: null, graph: null, beaver: null, economizer: null }
const EVERYTHING = { squeezer: 'h', graph: 'g', beaver: 'y', economizer: 'e' }

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
  expect(squeezableField('mcp__s__t', { text: big })).toBe('text')
  expect(squeezableField('Bash', { stdout: 'x'.repeat(2_500) })).toBe('stdout')
  expect(mcpText([{ type: 'text', text: 'a' }, { type: 'text', text: 'b' }])).toBe('a\nb')
  expect(mcpText([{ type: 'image', data: '' }])).toBe(null)
  expect(mcpText({ rows: [] })).toBe(null)
  expect(withMcpText('x', 'y')).toBe('y')
  expect(withMcpText([{ type: 'text', text: 'x' }], 'y')).toEqual([{ type: 'text', text: 'y' }])
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
  // `dam squeeze condense` decides and answers JSON; any other call (`--version`) just succeeds.
  const condensed = JSON.stringify({ text: 'short', note: '\n\n[condensed; original: {path}.]', before: 900, after: 2, url: null, web: false })
  on('process.run', (_, e) => ({ value: { exitCode: 0, stdout: e.argv.includes('condense') ? condensed : 'dam 0.32.0', stderr: '' } }) as never)
  on('clock.after', () => ({ value: {} }) as never)
  on('store.get', () => ({ value: undefined }) as never)
  on('store.set', () => ({ value: undefined }) as never)

  const ran = await $.tool.call({ tool: 'Bash', command: 'ls' })
  const stdout = (ran.result as { stdout: string }).stdout
  expect(stdout.startsWith('short')).toBe(true)
  expect(stdout).toMatch(/original: C:\/u\/\.claude\/the-dam-outputs\/.+\.txt\.\]$/)
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

test('the totals are this session\'s, counted from its journal', () => {
  const entries = [
    { at: 1, icon: '🌀', text: 'loop redirected (x), after 9 tool calls' },
    { at: 2, icon: '🌀', text: 'turn still stuck after a redirect' },
    { at: 3, icon: '🧠', text: '3 learned facts added to the context', count: 3 },
    { at: 4, icon: '🗺️', text: 'map built: 10 nodes in 1.0 s' },
    { at: 5, icon: '🗺️', text: 'map answered for crush_rows in your prompt' },
  ]
  const here = sessionFigures(entries)
  expect(here).toEqual({ loops: 1, facts: 3, mapAnswers: 1 })
  const s = { ...EMPTY, saved: 133_000, session: { saved: 3_100, seen: 12_400, spent: 3_096_900 }, project: { saved: 45_200, seen: 90_400, spent: 0 }, pending: 2, level: 'ultra', hasGraph: true, nodes: 400 }
  const lines = totalLines(s, here)
  expect(lines[0]).toBe("💧 3.1k/3.1M (0.10%) tokens kept out of this session's requests: 3.1k taken out of tool outputs (25% of what they printed), then not re-read")
  expect(lines[1]).toBe('   this project 45.2k, every project 133k')
  expect(projectFlow([{ project: 'C:/a', saved: 10, seen: 40 }, { project: 'C:/b', saved: 5, seen: 5 }, null, { project: 'C:/a', saved: 2, seen: 20 }], 'C:/a')).toEqual({ saved: 12, seen: 60, spent: 0, avoided: 0 })
  expect(lines).toContain('🪵 0 tools left out of this session, 2 more waiting')
  expect(lines).toContain('🌀 1 loops stopped')
  expect(lines).toContain('🗺️ 1 answers from the map (400 places)')
  expect(lines).toContain('🦫 Beaver ultra')
  expect(totalLines(EMPTY, sessionFigures([]))).toContain('🗺️ no map here')
})

test('a graph question is read from the command that asks it', () => {
  expect(graphQuestion('"C:/p/bin/windows-x64/dam-graph.exe" query "how is the log condensed"')).toBe('"how is the log condensed"')
  expect(graphQuestion('dam-graph update')).toBe(null)
  expect(graphQuestion('"C:/p/bin/windows-x64/dam.exe" graph query "where is fold"')).toBe('"where is fold"')
  expect(graphQuestion('dam graph query fold_lines')).toBe('fold_lines')
  expect(graphQuestion('dam graph update')).toBe(null)
  expect(graphQuestion('dam squeeze')).toBe(null)
  expect(graphQuestion('ls')).toBe(null)
})

test('the journal shows the newest first and leaves room for the totals', () => {
  const entries = [1, 2, 3, 4, 5, 6].map(at => ({ at, icon: '💧', text: String(at) }))
  expect(journalView(entries, 8).shown.map(e => e.at)).toEqual([6, 5, 4, 3, 2, 1])
  const tight = journalView(entries, 8, 15)
  expect(tight.shown.map(e => e.at)).toEqual([6, 5, 4])
  expect(tight.older).toBe(3)
  expect(journalView(entries, 8, 17)).toEqual({ shown: [...entries].reverse(), older: 0 })
  expect(journalView(entries, 8, 5).shown).toEqual([])
})

test('token counts reach G and T, and savings read against the session total', () => {
  expect(formatCount(2_500_000_000)).toBe('2.5G')
  expect(formatCount(3_000_000_000_000)).toBe('3T')
  expect(savedOfSpent({ saved: 40_000, seen: 0, spent: 360_000 })).toBe('40k/360k (10%)')
  expect(savedOfSpent({ saved: 1_000, seen: 0, spent: 300_000, avoided: 100_000 })).toBe('100k/300k (25%)')
})

test('a saved token counts once per request that does not carry it, until a compaction', () => {
  let c = { carry: 2_000, avoided: 0, messages: 0 }
  c = carryStep(c, 10, 500)
  c = carryStep(c, 12, 500)
  expect(c).toEqual({ carry: 2_000, avoided: 5_000, messages: 12 })
  expect(carryStep(c, 3, 500)).toEqual({ carry: 0, avoided: 5_500, messages: 3 })
})

test('built-in subagents are workers on Haiku; a named model, a fork and other plugins agents keep theirs', () => {
  const engine = { plugin: 'engine' }
  expect(pinnedModel({ provider: engine, subagentType: 'general-purpose' })).toBe(WORKER_MODEL)
  expect(pinnedModel({ provider: engine, subagentType: 'Explore', model: 'inherit' })).toBe(WORKER_MODEL)
  expect(pinnedModel({ provider: engine, subagentType: 'Explore', model: 'opus' })).toBe(null)
  expect(pinnedModel({ provider: engine, subagentType: 'fork' })).toBe(null)
  expect(pinnedModel({ provider: { plugin: 'the-dam' }, subagentType: 'advisor' })).toBe(null)
})
