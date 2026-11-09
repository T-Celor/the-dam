export type ToolsPresent = { squeezer: boolean; graphify: boolean; economizer: boolean; beaver: boolean }
export type ContextStats = { percent: number; total: number; max: number }
export type Gains = { saved: number; pruned: number; facts: number; loops: number; nodes: number }
// What the tools printed, in tokens, what compression took out of it, every token the model's requests counted,
// and what those requests did not carry: each saved token once per request it stayed out of, and the pruned tools' schemas.
// `cost` is the session's requests priced as fresh-input tokens (input 1, cache read 0.1, cache write 1.25, output 5).
export type Flow = { saved: number; seen: number; spent: number; avoided?: number; cost?: number; costUsd?: number | null }

export type Stats = Gains & {
  // Tools TokenEconomizer will prune once idle, and what its own work cost in tokens.
  pending: number
  overhead: number
  context: ContextStats | null
  tools: ToolsPresent
  isCompressing: boolean
  hasGraph: boolean
  // Beaver's level in this session, null when it is off or not started.
  level: string | null
  // Compression in this session and in this project; `saved` above is every project's.
  session: Flow
  project: Flow
}

// One line of the session's journal: when, which tool (its icon), what it did.
// `count`: how many of a thing the entry adds to the session's totals (learned facts), where it is not one.
export type JournalEntry = { at: number; icon: string; text: string; count?: number }

declare module 'claude-code' {
  interface PluginState {
    'the-dam': { stats: Shaped<Stats>; journal: Shaped<JournalEntry[]> }
  }
}
