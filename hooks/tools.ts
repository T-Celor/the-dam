export type Located = {
  squeezer: string | null
  graph: string | null
  ponytail: string | null
  economizer: string | null
}

export type Step = { label: string; argv: readonly string[] }


// The build the plugin ships, then one built here: cargo's folder is often missing from an app-launched session's PATH.
export function candidates(name: string, root: string, home: string | undefined): string[] {
  const folders = ['windows-x64', 'linux-x64', 'linux-arm64', 'macos-arm64', 'macos-x64']
  const built = folders.flatMap(folder => (folder.startsWith('windows') ? [`${root}/bin/${folder}/${name}.exe`] : [`${root}/bin/${folder}/${name}`]))

  return [...built, ...(home ? [`${home}/.cargo/bin/${name}.exe`, `${home}/.cargo/bin/${name}`] : []), name]
}

export function missingNames(found: Located): string[] {
  return [
    found.squeezer && found.graph && found.ponytail && found.economizer ? null : 'dam',
  ].filter(name => name !== null)
}

// Only what is missing, so a second run does nothing.
export function setupSteps(found: Located, root: string): Step[] {
  const steps: Step[] = []
  // The plugin ships a Windows build; elsewhere it builds from its own source.
  if (!found.squeezer || !found.graph || !found.ponytail || !found.economizer) steps.push({ label: 'dam', argv: ['cargo', 'install', '--path', `${root}/dam`] })

  return steps
}

export function graphNudge(program: string): string {
  return `A code graph of this project is ready. For questions about architecture or how code connects, run \`"${program}" query "<question>"\` before broad Grep or Read sweeps; it costs far fewer tokens.`
}

const WORTH_SQUEEZING_CHARS = 6000
// Claude Code hands over at most this much of a longer output, which it saves itself and shows as a preview.
const MOST_INLINE_CHARS = 29_000
const WORTH_KEEPING_TOKENS = 200
type ToolRecord = { stdout?: unknown; content?: unknown; mode?: unknown; isImage?: unknown; backgroundTaskId?: unknown; persistedOutputPath?: unknown }

// Bash output and Grep's matching lines. Read and Edit need the file's exact bytes.
// An output Claude Code saved to a file is read back from it (`persistedOutputPath`), whatever its size.
export function squeezableField(tool: string, record: ToolRecord): 'stdout' | 'content' | null {
  const field = tool === 'Bash' ? 'stdout' : tool === 'Grep' && record.mode === 'content' ? 'content' : null
  if (!field || record.isImage || record.backgroundTaskId) return null
  if (field === 'stdout' && typeof record.persistedOutputPath === 'string') return field
  const text = record[field]

  return typeof text === 'string' && text.length >= WORTH_SQUEEZING_CHARS && text.length < MOST_INLINE_CHARS ? field : null
}

const STANDOUT = /\b(error|errors|failed|failure|fatal|panic|panicked|exception|traceback|warning)\b/i
const MOST_STANDOUT_LINES = 10
const MOST_STANDOUT_CHARS = 200

// The lines of a long output a reader must not miss, for the preview Claude Code shows of a saved output.
export function standoutLines(text: string): string[] {
  const found: string[] = []
  for (const line of text.split('\n')) {
    if (found.length === MOST_STANDOUT_LINES) break
    const trimmed = line.trim()
    if (trimmed && STANDOUT.test(trimmed) && !found.includes(trimmed)) found.push(trimmed.slice(0, MOST_STANDOUT_CHARS))
  }

  return found
}

const PREVIEW_CHARS = 2000

// What the model sees of an output still too long once condensed, in place of Claude Code's own preview:
// where the condensed file is, the lines that stand out, and its first lines.
export function savedPreview(condensed: string, before: number, after: number, saved: string, original: string, standout: string[]): string {
  const head = condensed.slice(0, PREVIEW_CHARS)
  const cut = head.length < condensed.length ? head.slice(0, head.lastIndexOf('\n') + 1) || head : head
  const lines = standout.length ? `\nLines that stand out:\n${standout.join('\n')}\n` : ''

  return `[Output too long to show. The dam condensed it from about ${before} to ${after} tokens without dropping any line, into ${saved}: read that file for the rest. Original: ${original}.]\n${lines}\nFirst lines:\n${cut}`
}

// Whether a condensed output is small enough for Claude Code to show inline.
export function fitsInline(text: string): boolean {
  return text.length < MOST_INLINE_CHARS
}

// Rough, as no tokenizer is at hand: about four characters per token.
const tokensIn = (text: string) => Math.ceil(text.length / 4)

export function squeezed(original: string, text: string): { text: string; before: number; after: number } | null {
  const [before, after] = [tokensIn(original), tokensIn(text)]

  return before - after >= WORTH_KEEPING_TOKENS ? { text, before, after } : null
}

// The model is told how to read the result, and where the original is if it prefers that.
export function squeezeNote(before: number, after: number, path: string): string {
  return `\n\n[The dam condensed this output from about ${before} to ${after} tokens without dropping any line. Original: ${path}.]`
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

// The number of nodes in dam-graph's table.
export function graphNodes(text: string): number {
  return text.split('\n').filter(line => line.startsWith('N\t')).length
}
