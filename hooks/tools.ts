export type Located = {
  headroom: string | null
  graphify: string | null
  python: string | null
  teBoot: string | null
  hasEconomizer: boolean
  hasPonytail: boolean
}

export type Step = { label: string; argv: readonly string[] }

const TE_REPO = 'https://github.com/microsoft/TokenEconomizer.git'
const PONYTAIL_REPO = 'DietrichGebert/ponytail'

// uv tool's bin folder is often missing from the PATH of an app-launched session.
export function candidates(name: string, home: string | undefined): string[] {
  return home ? [`${home}/.local/bin/${name}.exe`, `${home}/.local/bin/${name}`, name] : [name]
}

// Read from Claude's own plugin files: `claude` is a .cmd shim on Windows, slow and awkward to spawn.
export function parseInstalled(text: string): boolean[] {
  const { plugins } = JSON.parse(text)
  const ids = Object.keys(plugins ?? {})

  return ['tokeneconomizer@', 'ponytail@'].map(prefix => ids.some(id => id.startsWith(prefix)))
}

export function teBootFrom(marketplaces: string): string | null {
  const location = JSON.parse(marketplaces).tokeneconomizer?.installLocation

  return location ? `${String(location).replace(/\\/g, '/')}/plugin/scripts/te_bootstrap.py` : null
}

export function missingNames(found: Located): string[] {
  return [
    found.headroom ? null : 'Headroom',
    found.graphify ? null : 'graphify',
    found.hasEconomizer ? null : 'TokenEconomizer',
    found.hasPonytail ? null : 'Ponytail',
  ].filter(name => name !== null)
}

// Only what is missing, so a second run does nothing.
export function setupSteps(found: Located, home: string, isWindows: boolean, hasClone: boolean): Step[] {
  const claude = (...args: string[]) => (isWindows ? ['cmd', '/c', 'claude', ...args] : ['claude', ...args])
  const steps: Step[] = []
  if (!found.headroom)
    steps.push({ label: 'Headroom', argv: ['uv', 'tool', 'install', '--python', '3.13', 'headroom-ai[all]'] })
  if (!found.graphify) {
    steps.push({ label: 'graphify', argv: ['uv', 'tool', 'install', 'graphifyy'] })
    steps.push({
      label: 'skill graphify',
      argv: [candidates('graphify', home)[isWindows ? 0 : 1] ?? 'graphify', 'install', '--platform', 'claude'],
    })
  }
  if (!found.python) steps.push({ label: 'Python', argv: ['uv', 'python', 'install', '3.12'] })
  if (!found.hasEconomizer) {
    if (!hasClone)
      steps.push({ label: 'clone TokenEconomizer', argv: ['git', 'clone', '--depth', '1', TE_REPO, `${home}/TokenEconomizer`] })
    steps.push({ label: 'marketplace TokenEconomizer', argv: claude('plugin', 'marketplace', 'add', `${home}/TokenEconomizer`) })
    steps.push({ label: 'plugin TokenEconomizer', argv: claude('plugin', 'install', 'tokeneconomizer@tokeneconomizer') })
  }
  if (!found.hasPonytail) {
    steps.push({ label: 'marketplace Ponytail', argv: claude('plugin', 'marketplace', 'add', PONYTAIL_REPO) })
    steps.push({ label: 'plugin Ponytail', argv: claude('plugin', 'install', 'ponytail@ponytail') })
  }

  return steps
}

export const GRAPH_NUDGE =
  'A graphify knowledge graph of this project exists at graphify-out/graph.json. ' +
  'For questions about architecture or how code connects, run `graphify query "<question>"` ' +
  'before broad Grep or Read sweeps; it costs far fewer tokens.'

export const PROXY = 'http://127.0.0.1:8787'
const WORTH_SQUEEZING_CHARS = 6000
const WORTH_KEEPING_TOKENS = 200
// Headroom prices the saving with this until the session has named its own model.
const FALLBACK_MODEL = 'claude-sonnet-4-5'

type ToolRecord = { stdout?: unknown; content?: unknown; mode?: unknown; isImage?: unknown; backgroundTaskId?: unknown }

// Bash output and Grep lines only: Read and Edit need the file's exact bytes.
export function squeezableField(tool: string, record: ToolRecord): 'stdout' | 'content' | null {
  const field = tool === 'Bash' ? 'stdout' : tool === 'Grep' ? 'content' : null
  if (!field || record.isImage || record.backgroundTaskId) return null
  if (tool === 'Grep' && record.mode !== 'content') return null
  const text = record[field]

  return typeof text === 'string' && text.length >= WORTH_SQUEEZING_CHARS ? field : null
}

export function compressRequest(text: string, model = FALLBACK_MODEL): string {
  return JSON.stringify({
    model,
    messages: [
      { role: 'user', content: 'run' },
      { role: 'assistant', content: [{ type: 'tool_use', id: 't1', name: 'Bash', input: {} }] },
      { role: 'user', content: [{ type: 'tool_result', tool_use_id: 't1', content: text }] },
    ],
  })
}

export function squeezed(responseText: string): { text: string; before: number; after: number } | null {
  const body = JSON.parse(responseText)
  const text = body.messages?.[2]?.content?.[0]?.content
  if (typeof text !== 'string' || !(body.tokens_saved >= WORTH_KEEPING_TOKENS)) return null

  return { text, before: body.tokens_before, after: body.tokens_after }
}

// The squeeze is lossy, so the model is always told where the whole output is.
export function squeezeNote(before: number, after: number, path: string): string {
  return `\n\n[Headroom condensed this output from ${before} to ${after} tokens, and lines may be missing. Full output: ${path}. Read that file before concluding that something is absent.]`
}

export function parseEconomizer(stdout: string): { pruned: number; facts: number } {
  const { toolprune, microskills } = JSON.parse(stdout)

  return { pruned: toolprune?.applied?.length ?? 0, facts: microskills?.injected_lines ?? 0 }
}

export function graphNodes(text: string): number {
  return (JSON.parse(text) as { nodes?: unknown[] }).nodes?.length ?? 0
}
