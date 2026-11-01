export type Materials = { twigs: number; logs: number; trunks: number }
export type ProxyState = 'off' | 'starting' | 'up' | 'down'
export type CritterId = 'hazel' | 'mimi' | 'pompom' | 'truffle'
// How recently each beaver's tool was used: rises with use, cools with time.
export type Heat = Record<CritterId, number>
export type ToolsPresent = Record<CritterId, boolean>
export type ContextStats = { percent: number; total: number; max: number; layers: Array<{ name: string; tokens: number }> }

export type Stats = {
  context: ContextStats | null
  saved: number
  tools: ToolsPresent
  proxy: ProxyState
  hasGraph: boolean
  // Floods averted in a row in this project; a flood wipes them.
  pennants: number
  isParty: boolean
  // What the tools' figures earned, and the share of each currently washed away.
  earned: Materials
  wear: Materials
  // The level the picture shows, easing towards the real one.
  river: number | null
}

declare module 'claude-code' {
  interface PluginState {
    'the-dam': { stats: Shaped<Stats>; heat: Shaped<Heat> }
  }
}
