// The dam's arithmetic, separate from its drawing. Every curve saturates, so no figure,
// however large, can blow up the picture, and no flood can erase what was earned.

export type Materials = { twigs: number; logs: number; trunks: number }
export type Sources = { saved: number; pruned: number; facts: number; nodes: number }

export const NO_MATERIALS: Materials = { twigs: 0, logs: 0, trunks: 0 }

// What a check found since the last one, or null when nothing new was earned.
export function gainNote(before: Materials, after: Materials): string | null {
  const parts = (['twigs', 'logs', 'trunks'] as const).filter(kind => after[kind] > before[kind]).map(kind => `+${after[kind] - before[kind]} ${kind}`)

  return parts.length > 0 ? parts.join(', ') : null
}

const clamp01 = (value: number) => Math.min(1, Math.max(0, value))
const approach = (value: number, scale: number) => 1 - Math.exp(-Math.max(0, value) / scale)

// 1 twig per 100 tokens compressed, 1 log per tool pruned or fact learned, 1 trunk per 50 graph nodes.
export function earnedFrom(sources: Sources): Materials {
  return {
    twigs: Math.floor(sources.saved / 100),
    logs: sources.pruned + sources.facts,
    trunks: Math.ceil(sources.nodes / 50),
  }
}

export function stockOf(earned: Materials, wear: Materials): Materials {
  return {
    twigs: Math.round(earned.twigs * (1 - wear.twigs)),
    logs: Math.round(earned.logs * (1 - wear.logs)),
    trunks: Math.round(earned.trunks * (1 - wear.trunks)),
  }
}

const WEIGHT = { core: 0.15, twigs: 0.45, logs: 0.2, trunks: 0.2 }

// 0 to 1. Ponytail alone gives 0.15; 10k twigs (1M tokens) fill their whole share.
export function strengthOf(hasCore: boolean, earned: Materials, wear: Materials): number {
  const stock = stockOf(earned, wear)

  return (
    (hasCore ? WEIGHT.core : 0) +
    WEIGHT.twigs * clamp01(Math.log10(1 + stock.twigs) / 4) +
    WEIGHT.logs * approach(stock.logs, 12) +
    WEIGHT.trunks * approach(stock.trunks, 6)
  )
}

// A stronger dam starts leaking later: at 60% of context when bare, 85% at full strength.
export function pressureOf(percent: number, strength: number): number {
  const threshold = 0.6 + 0.25 * strength

  return clamp01((percent / 100 - threshold) / (1 - threshold))
}

// The most a full-pressure leak can take of each, before the dam's strength reduces it.
const MAX_WEAR: Materials = { twigs: 0.4, logs: 0.25, trunks: 0.15 }
const LEAK_PER_SECOND = 0.02
const REPAIR_SECONDS = 600

// Wear climbs towards a ceiling set by pressure and strength, and is repaired once the water is back down.
export function nextWear(wear: Materials, pressure: number, strength: number, seconds: number): Materials {
  const step = (kind: keyof Materials) => {
    if (pressure <= 0) return wear[kind] * Math.exp(-seconds / REPAIR_SECONDS)
    const ceiling = MAX_WEAR[kind] * pressure * (1 - 0.6 * strength)
    if (wear[kind] >= ceiling) return wear[kind]

    return wear[kind] + (ceiling - wear[kind]) * (1 - Math.exp(-LEAK_PER_SECOND * seconds))
  }

  return { twigs: step('twigs'), logs: step('logs'), trunks: step('trunks') }
}

// What is drawn eases towards the real level, so cleaning the context drains the river instead of snapping it.
export function nextRiver(shown: number | null, percent: number): number {
  return shown === null ? percent : Math.abs(percent - shown) < 1 ? percent : shown + (percent - shown) * 0.5
}

// Only a weak dam is swept away by a full context; a strong one holds and leaks.
export function isOverwhelmed(percent: number, strength: number): boolean {
  return percent >= 100 && strength < 0.4
}

export type Shape = { height: number; topHalf: number; footHalf: number; size: number }

// The dam's outline grows with its strength, fast at first and more slowly later.
export function shapeOf(strength: number): Shape {
  const size = Math.pow(clamp01(strength), 0.8)

  return { size, height: 130 + 152 * size, topHalf: 110 + 62 * size, footHalf: 170 + 98 * size }
}
