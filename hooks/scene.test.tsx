import { expect, test } from 'claude-code/testing'

import { earnedFrom, gainNote, isOverwhelmed, NO_MATERIALS, nextRiver, nextWear, pressureOf, stockOf, strengthOf, visibleCount } from './model'
import type { Stats } from '../types'
import { counterLine, frameCells, quadrantOf, renderPixels } from './raster'
import { COLD, cool, damScene, EMPTY, fallWidth, formatCount, isFlooded, levelOf, nextPennants, sceneAlt, twigCount, usedBy, warm } from './scene'
import { candidates, compressRequest, graphNodes, missingNames, parseEconomizer, parseInstalled, setupSteps, squeezableField, squeezed, squeezeNote, teBootFrom } from './tools'

const PANE = {
  plugin: 'the-dam',
  component: 'Pane',
  requestId: 'the-dam',
  props: {
    title: 'The dam',
    isFocused: false,
    bodyColumns: 60,
    placement: 'dock',
    scroll: { offset: 0, bodyRows: 40 },
    view: {},
  },
} as const

const ALL = { hazel: true, mimi: true, pompom: true, truffle: true }
const at = (percent: number, saved = 5_000, tools = ALL) => ({
  ...EMPTY,
  saved,
  tools,
  earned: earnedFrom({ saved, pruned: 0, facts: 0, nodes: 0 }),
  context: { percent, total: percent * 10_000, max: 1_000_000, layers: [{ name: 'Messages', tokens: 300 }, { name: 'Tools', tokens: 100 }] },
})

const countOf = (svg: string, mark: string) => svg.split(mark).length - 1
const RICH = { ...at(100, 1_000_000), earned: earnedFrom({ saved: 1_000_000, pruned: 12, facts: 18, nodes: 400 }) }

test('twigs grow with tokens saved, without ever overflowing', () => {
  expect(twigCount(0)).toBe(0)
  expect(twigCount(1_000_000)).toBeGreaterThan(twigCount(1_000))
  expect(twigCount(1e15)).toBe(170)
  const twigs = (saved: number) => countOf(damScene(at(20, saved), COLD), 'class="twig"')
  expect(twigs(0)).toBe(0)
  expect(twigs(1_000_000)).toBeGreaterThan(twigs(1_000))
})

test('Ponytail alone is the small dam, and each tool adds its own material', () => {
  const none = { hazel: false, mimi: false, pompom: false, truffle: false }
  const small = { ...at(20, 0, { ...none, pompom: true }) }
  expect(countOf(damScene(small, COLD), 'class="pole"')).toBe(3)
  expect(countOf(damScene(small, COLD), 'class="trunk"')).toBe(0)
  expect(countOf(damScene({ ...at(20, 0, none) }, COLD), 'class="pole"')).toBe(0)
  const grown = { ...small, earned: earnedFrom({ saved: 0, pruned: 4, facts: 0, nodes: 120 }) }
  expect(countOf(damScene(grown, COLD), 'class="rondin"')).toBeGreaterThan(0)
  expect(countOf(damScene(grown, COLD), 'class="trunk"')).toBe(visibleCount(3, 4, 5))
  expect(strengthOf(true, grown.earned, NO_MATERIALS)).toBeGreaterThan(strengthOf(true, NO_MATERIALS, NO_MATERIALS))
  expect(strengthOf(true, NO_MATERIALS, NO_MATERIALS)).toBe(0.15)
})

test('strength and visible pieces saturate, whatever the figures', () => {
  const huge = earnedFrom({ saved: 1e15, pruned: 1e6, facts: 1e6, nodes: 1e9 })
  expect(strengthOf(true, huge, NO_MATERIALS)).toBeLessThan(1.01)
  expect(visibleCount(1e9, 12, 10)).toBe(12)
  expect(visibleCount(1, 12, 10)).toBe(1)
  expect(visibleCount(0, 12, 10)).toBe(0)
  expect(damScene({ ...RICH, earned: huge }, COLD).length).toBeLessThan(131_072)
})

test('the river follows the context and eases down when it is cleaned', () => {
  expect(countOf(damScene(at(50), COLD), 'class="band"')).toBe(2)
  expect(fallWidth(80)).toBeGreaterThan(fallWidth(20))
  expect(fallWidth(100, 84)).toBeLessThan(fallWidth(100, 172))
  expect(nextRiver(null, 40)).toBe(40)
  expect(nextRiver(100, 10)).toBe(55)
  expect(nextRiver(10.4, 10)).toBe(10)
})

test('a stronger dam leaks later, and a leak carries off twigs before trunks', () => {
  expect(pressureOf(70, 0.15)).toBeGreaterThan(0)
  expect(pressureOf(70, 0.8)).toBe(0)
  const leaking = damScene(RICH, COLD)
  expect(leaking.includes('gone-twig')).toBe(true)
  expect(leaking.includes('gone-trunk')).toBe(true)
  const mild = { ...RICH, river: 85.5 }
  expect(pressureOf(85.5, strengthOf(false, mild.earned, NO_MATERIALS))).toBeLessThan(0.6)
  expect(damScene(mild, COLD).includes('class="p debris gone-trunk"')).toBe(false)
  expect(damScene(at(40), COLD).includes('class="p debris')).toBe(false)
})

test('1M saved at full context then cleaned: bounded loss, full repair, no wipeout', () => {
  const strength = strengthOf(true, RICH.earned, NO_MATERIALS)
  expect(strength).toBeGreaterThan(0.4)
  expect(isOverwhelmed(100, strength)).toBe(false)
  let wear = NO_MATERIALS
  for (let i = 0; i < 2000; i++) wear = nextWear(wear, 1, strength, 15)
  expect(wear.twigs).toBeLessThan(0.3)
  expect(wear.trunks).toBeLessThan(wear.twigs)
  const stock = stockOf(RICH.earned, wear)
  expect(stock.twigs).toBeGreaterThan(7_000)
  for (let i = 0; i < 400; i++) wear = nextWear(wear, 0, strength, 15)
  expect(wear.twigs).toBeLessThan(0.001)
  expect(damScene(at(20, 50_000), COLD)).toContain('next star: 100k tokens saved')
  expect(counterLine(at(20, 50_000), 90).map(([text]) => text).join('')).toContain('stars 2/5 (next at 100k saved)')
})

test('only a weak dam is swept away by a full context', () => {
  expect(isFlooded(at(99))).toBe(false)
  expect(isFlooded(at(100, 0))).toBe(true)
  expect(damScene(at(100, 0), COLD).includes('class="rush"')).toBe(true)
  expect(damScene(at(100, 0), COLD).includes('class="damBody"')).toBe(false)
  expect(damScene(RICH, COLD).includes('class="damBody"')).toBe(true)
})

test('every counter is on the picture, with what was washed away', () => {
  const svg = damScene({ ...RICH, wear: { twigs: 0.1, logs: 0, trunks: 0 } }, COLD)
  expect(svg).toContain('>9k<')
  expect(svg).toContain('>30<')
  expect(svg).toContain('>8<')
  expect(svg).toContain('>-1k<')
  expect(svg).toContain('>1M/1M<')
})

test('a redraw lands everything in the same place, within what the panel accepts', () => {
  expect(damScene(at(60), COLD)).toBe(damScene(at(60), COLD))
  for (const percent of [10, 99, 100]) {
    const party = { ...RICH, ...{ context: at(percent).context }, pennants: 50, isParty: true }
    expect(damScene(party, { hazel: 9, mimi: 9, pompom: 9, truffle: 9 }).length).toBeLessThan(131_072)
  }
})

test('the scene takes the shape of the panel, keeping the counters on screen', () => {
  expect(damScene(at(50), COLD)).toContain('viewBox="0 0 640 480"')
  expect(damScene(at(50), COLD, 1.5)).toContain('viewBox="0 -480 640 960"')
  expect(damScene(at(50), COLD, 9)).toContain('viewBox="0 -1056 640 1536"')
  expect(damScene(at(50), COLD, 0.2)).toContain('viewBox="0 0 640 480"')
})

test('the terminal frame fills its box, moves with time, and shows the data', () => {
  const pixels = (s: Stats, t = 1) => renderPixels(s, COLD, t, 60, 48)
  const differs = (a: Uint32Array, b: Uint32Array) => a.filter((value, i) => value !== b[i]).length
  expect(pixels(at(50)).includes(0)).toBe(false)
  expect(differs(pixels(at(50), 1), pixels(at(50), 1))).toBe(0)
  expect(differs(pixels(at(50), 1), pixels(at(50), 2))).toBeGreaterThan(30)
  expect(differs(pixels(at(20)), pixels(at(90)))).toBeGreaterThan(200)
  expect(differs(pixels(RICH), pixels({ ...RICH, earned: NO_MATERIALS }))).toBeGreaterThan(100)
  for (const state of [EMPTY, at(0, 0), at(100, 0), RICH]) {
    for (const [width, height] of [[24, 6], [60, 48], [200, 100]] as const) expect(renderPixels(state, COLD, 3.3, width, height).length).toBe(width * height)
  }
  const line = counterLine({ ...RICH, wear: { twigs: 0.1, logs: 0, trunks: 0 } }, 60).map(([text]) => text).join('')
  expect(line).toBe('river 1M/1M twigs 9k -1k logs 30 trunks 8')
  expect(frameCells(at(50), COLD, 1, 30, 10).length).toBe(4800)
  expect(quadrantOf([5, 5, 5, 5])).toEqual([0x20, 5, 5])
  expect(quadrantOf([0xffffff, 0xffffff, 0, 0])).toEqual([0x2580, 0xffffff, 0])
  expect(quadrantOf([0xffffff, 0, 0, 0xfefefe])[0]).toBe(0x259a)
})

test('even the tallest, richest scene fits what the panel accepts', () => {
  for (const percent of [10, 85, 100]) {
    const busy = { ...RICH, context: at(percent).context, pennants: 50, isParty: true }
    expect(damScene(busy, { hazel: 9, mimi: 9, pompom: 9, truffle: 9 }, 2.4).length).toBeLessThan(131_072)
  }
})

test('a check announces only what is new', () => {
  const was = { twigs: 10, logs: 2, trunks: 1 }
  expect(gainNote(was, was)).toBe(null)
  expect(gainNote(was, { twigs: 9, logs: 2, trunks: 1 })).toBe(null)
  expect(gainNote(was, { twigs: 14, logs: 2, trunks: 2 })).toBe('+4 twigs, +1 trunks')
})

test('tool figures are read from their own output', () => {
  expect(parseEconomizer('{"toolprune":{"applied":[1,2]},"microskills":{"injected_lines":5}}')).toEqual({ pruned: 2, facts: 5 })
  expect(graphNodes('{"nodes":[{},{}]}')).toBe(2)
})

test('bringing high water down wins a pennant, a full context loses half', () => {
  expect(nextPennants(2, 80, 40)).toBe(3)
  expect(nextPennants(2, 60, 20)).toBe(2)
  expect(nextPennants(2, 80, 70)).toBe(2)
  expect(nextPennants(2, null, 10)).toBe(2)
  expect(nextPennants(3, 95, 100)).toBe(1)
  expect(nextPennants(0, 100, 30)).toBe(0)
  const flags = (pennants: number) => damScene({ ...at(20), pennants }, COLD).split('class="flap"').length - 1
  expect(flags(0)).toBe(0)
  expect(flags(3)).toBe(3)
  expect(flags(50)).toBe(7)
})

test('calm water has fish, and a party is a brief one', () => {
  expect(damScene(at(10), COLD).includes('class="p jump"')).toBe(true)
  expect(damScene(at(60), COLD).includes('class="p jump"')).toBe(false)
  expect(damScene(at(20), COLD).includes('class="confetti"')).toBe(false)
  expect(damScene({ ...at(20), isParty: true }, COLD).includes('class="confetti"')).toBe(true)
})

test('every state draws cleanly, with no stray values', () => {
  for (const state of [EMPTY, at(0, 0), at(1), at(50), at(90), at(100), at(100, 0, { hazel: false, mimi: false, pompom: false, truffle: false })]) {
    const svg = damScene(state, COLD)
    expect(svg.includes('NaN')).toBe(false)
    expect(svg.includes('undefined')).toBe(false)
        expect(svg).toContain('prefers-reduced-motion')
  }
})

test('the alternative carries every figure the picture shows', () => {
  const alt = sceneAlt(at(50, 5_000))
  expect(alt).toContain('Context 50% full')
  expect(alt).toContain('Messages')
  expect(alt).toContain('50 twigs')
  expect(alt).toContain('5k tokens saved')
  expect(alt).toContain('0 floods averted')
  expect(sceneAlt(at(100, 0))).toContain('torrent')
  expect(sceneAlt(EMPTY)).toContain('not measured')
  expect(formatCount(1_234_000)).toBe('1.2M')
})

test('heat climbs through the levels and cools back to sleep', () => {
  expect([0, 1, 2, 3, 5, 6, 9].map(levelOf)).toEqual([0, 1, 1, 2, 2, 3, 3])
  expect(warm(COLD, ['truffle'], 20).truffle).toBe(9)
  expect(cool(warm(COLD, ['truffle'])).truffle).toBe(0)
  expect(cool(COLD).truffle).toBe(0)
})

test('a beaver wakes only for its own tool', () => {
  expect(usedBy('Bash', 'headroom perf')).toEqual(['hazel'])
  expect(usedBy('Skill', 'graphify')).toEqual(['mimi'])
  expect(usedBy('Skill', 'tokeneconomizer:status')).toEqual(['truffle'])
  expect(usedBy('Bash', 'sh scripts/te status')).toEqual(['truffle'])
  expect(usedBy('Edit', '')).toEqual(['pompom'])
  expect(usedBy('Skill', 'ponytail:ponytail')).toEqual(['pompom'])
  expect(usedBy('Bash', 'git status')).toEqual([])
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

const UP = { value: { value: { shape: 'stats-12', value: { ...EMPTY, proxy: 'up' } }, version: 1 } } as never
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

test('the pane holds the scene alone, with its figures as the alternative', async $ => {
  const desktop = await $.ui.mount({ ...PANE, surface: 'desktop' })
  expect(await desktop.drawn()).toMatchObject({ type: 'Svg' })
  await desktop.unmount()
  const terminal = await $.ui.mount({ ...PANE, surface: 'terminal' })
  expect(await terminal.drawn()).toMatchObject({ type: 'Raster', props: { columns: 60, rows: 40 } })
  await terminal.unmount()
})
