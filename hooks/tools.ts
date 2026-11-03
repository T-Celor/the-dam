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
  return `A code graph of this project is ready. To find where something is defined, what it calls or what calls it, or how parts connect, run \`"${program}" query "<name or question>"\` first, before Grep, Glob or Read sweeps: it answers in a few hundred tokens. When a prompt or a Grep names code, its answer for those names is added for you.`
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

const GRAPH_QUERY = /dam-graph(?:\.exe)?"?\s+query\s+(.+)/

// The question a Bash command asks the code graph, or null for any other command.
export function graphQuestion(command: string): string | null {
  return command.match(GRAPH_QUERY)?.[1]?.trim().slice(0, 160) ?? null
}

const WORD = /[A-Za-z_][A-Za-z0-9_]*(?:\(\))?/g
const MOST_NAMES = 3

// The names in a prompt that look like code rather than words: snake_case, camelCase, PascalCase with two capitals, or a call.
// Only these get the graph's answer, which is exact for a name and noisy for a sentence.
export function codeNames(text: string): string[] {
  const names: string[] = []
  for (const found of text.match(WORD) ?? []) {
    const name = found.replace(/\(\)$/, '')
    const isCode = found.endsWith('()') || /[a-z0-9]_[a-z0-9]/i.test(name) || /[a-z][A-Z]/.test(name) || /^[A-Z][a-z0-9]+[A-Z]/.test(name)
    if (isCode && name.length >= 4 && !names.includes(name)) names.push(name)
    if (names.length === MOST_NAMES) break
  }

  return names
}

// The name a Grep looks for, when its pattern is one identifier (`\bname\b`, `name(`, `def name`), else null.
export function grepName(pattern: string): string | null {
  const name = pattern.replace(/\\b|\\\(|\(|\^|\$/g, '').replace(/^(def|fn|function|class|struct|interface|type|const|let|var)\s+/, '').trim()

  return /^[A-Za-z_][A-Za-z0-9_]{3,}$/.test(name) ? name : null
}

// The entries of a graph answer about exactly `name`: where it is defined, what it calls and what calls it.
export function exactEntries(answer: string, name: string): string[] {
  return answer.split(/\n\s*\n/).filter(entry => {
    const head = entry.trimStart()

    return head.startsWith(`${name}(`) || head.startsWith(`${name} `)
  })
}

// What the model reads about the names a prompt or a search named, in place of finding them itself.
export function graphFacts(entries: string[], program: string): string {
  return `The project's code graph on these names (\`"${program}" query "<question>"\` asks it more):\n\n${entries.join('\n\n')}`
}

// The number of nodes in dam-graph's table.
export function graphNodes(text: string): number {
  return text.split('\n').filter(line => line.startsWith('N\t')).length
}
