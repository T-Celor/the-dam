export type ProxyState = 'off' | 'starting' | 'up' | 'down'
export type ToolsPresent = { headroom: boolean; graphify: boolean; economizer: boolean; ponytail: boolean }
export type ContextStats = { percent: number; total: number; max: number }
export type Gains = { saved: number; pruned: number; facts: number; nodes: number }

export type Stats = Gains & {
  context: ContextStats | null
  tools: ToolsPresent
  proxy: ProxyState
  hasGraph: boolean
}

declare module 'claude-code' {
  interface PluginState {
    'the-dam': { stats: Shaped<Stats> }
  }
}
