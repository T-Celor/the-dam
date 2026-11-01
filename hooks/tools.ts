export type Located = {
  squeezer: string | null
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
    found.squeezer ? null : 'compressor',
    found.graphify ? null : 'graphify',
    found.hasEconomizer ? null : 'TokenEconomizer',
    found.hasPonytail ? null : 'Ponytail',
  ].filter(name => name !== null)
}

// Only what is missing, so a second run does nothing.
export function setupSteps(found: Located, home: string, isWindows: boolean, hasClone: boolean, root: string): Step[] {
  const claude = (...args: string[]) => (isWindows ? ['cmd', '/c', 'claude', ...args] : ['claude', ...args])
  const steps: Step[] = []
  // The plugin ships a Windows build; elsewhere it builds from its own source.
  if (!found.squeezer) steps.push({ label: 'compressor', argv: ['cargo', 'install', '--path', `${root}/squeeze`] })
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

const WORTH_SQUEEZING_CHARS = 6000
// Claude Code hands over at most this much of a longer output, which it saves itself and shows as a preview.
const MOST_INLINE_CHARS = 29_000
const WORTH_KEEPING_TOKENS = 200
type ToolRecord = { stdout?: unknown; content?: unknown; mode?: unknown; isImage?: unknown; backgroundTaskId?: unknown; persistedOutputPath?: unknown }

// Bash output only: Read and Edit need the file's exact bytes, and each Grep hit is an answer.
export function squeezableField(tool: string, record: ToolRecord): 'stdout' | null {
  const field = tool === 'Bash' ? 'stdout' : null
  if (!field || record.isImage || record.backgroundTaskId || record.persistedOutputPath) return null
  const text = record[field]

  return typeof text === 'string' && text.length >= WORTH_SQUEEZING_CHARS && text.length < MOST_INLINE_CHARS ? field : null
}

// Rough, as no tokenizer is at hand: about four characters per token.
const tokensIn = (text: string) => Math.ceil(text.length / 4)

export function squeezed(original: string, text: string): { text: string; before: number; after: number } | null {
  const [before, after] = [tokensIn(original), tokensIn(text)]

  return before - after >= WORTH_KEEPING_TOKENS ? { text, before, after } : null
}

// The squeeze is lossy, so the model is always told where the whole output is.
export function squeezeNote(before: number, after: number, path: string): string {
  return `\n\n[The dam condensed this output from about ${before} to ${after} tokens, and lines may be missing. Full output: ${path}. Read that file before concluding that something is absent.]`
}

export function parseEconomizer(stdout: string) {
  const { toolprune, microskills, ledger } = JSON.parse(stdout)

  return {
    pruned: toolprune?.applied?.length ?? 0,
    pending: toolprune?.proposal?.length ?? 0,
    facts: microskills?.injected_lines ?? 0,
    loops: (ledger?.nudges ?? 0) + (ledger?.aborts ?? 0),
    overhead: Math.round(ledger?.overhead_tokens ?? 0),
  }
}

const CODE_FILE = /\.(py|js|jsx|ts|tsx|java|kt|go|rs|c|h|cc|cpp|hpp|cs|rb|php|swift|scala|lua|sh)$/i
const WORTH_GRAPHING_FILES = 20

// A graph only pays for itself in a real project: `listing` is the output of `git ls-files`.
export function worthGraphing(listing: string): boolean {
  return listing.split('\n').filter(path => CODE_FILE.test(path.trim())).length >= WORTH_GRAPHING_FILES
}

export function graphNodes(text: string): number {
  return (JSON.parse(text) as { nodes?: unknown[] }).nodes?.length ?? 0
}
