export type Located = {
  squeezer: string | null
  graph: string | null
  beaver: string | null
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
    found.squeezer && found.graph && found.beaver && found.economizer ? null : 'dam',
  ].filter(name => name !== null)
}

// Only what is missing, so a second run does nothing.
export function setupSteps(found: Located, root: string): Step[] {
  const steps: Step[] = []
  // The plugin ships a Windows build; elsewhere it builds from its own source.
  if (!found.squeezer || !found.graph || !found.beaver || !found.economizer) steps.push({ label: 'dam', argv: ['cargo', 'install', '--path', `${root}/dam`] })

  return steps
}

export function graphNudge(program: string): string {
  return `A map of this project is ready: its code, its documents and its settings. To find where something is defined or described, what it calls or what calls it, or how parts connect, run \`"${program}" graph query "<name or question>"\` first, before Grep, Glob or Read sweeps: it answers in a few hundred tokens. When a prompt or a Grep names code, the map's answer for those names is added for you.`
}

const WORTH_SQUEEZING_CHARS = 2000
// Claude Code hands over at most this much of a longer output, which it saves itself and shows as a preview.
const MOST_INLINE_CHARS = 29_000
type ToolRecord = { stdout?: unknown; content?: unknown; text?: unknown; mode?: unknown; isImage?: unknown; backgroundTaskId?: unknown; persistedOutputPath?: unknown }

// An MCP tool's result as plain text: a string, or text blocks only; null for anything else (images, structured data).
export function mcpText(result: unknown): string | null {
  if (typeof result === 'string') return result
  const isText = (block: unknown) => typeof block === 'object' && block !== null && (block as { type?: unknown }).type === 'text' && typeof (block as { text?: unknown }).text === 'string'
  if (!Array.isArray(result) || result.length === 0 || !result.every(isText)) return null

  return result.map(block => (block as { text: string }).text).join('\n')
}

// The condensed text back in the result's own shape.
export function withMcpText(result: unknown, text: string): unknown {
  return typeof result === 'string' ? text : [{ type: 'text', text }]
}

// Bash and PowerShell output, Grep's matching lines, and MCP tools' text (handed over as `{ text }`).
// Read and Edit need the file's exact bytes; a subagent's report and WebFetch's answer are short prose with nothing repeated.
// An output Claude Code saved to a file is read back from it (`persistedOutputPath`), whatever its size.
export function squeezableField(tool: string, record: ToolRecord): 'stdout' | 'content' | 'text' | null {
  const field = tool === 'Bash' || tool === 'PowerShell' ? 'stdout' : tool === 'Grep' && record.mode === 'content' ? 'content' : tool.startsWith('mcp__') ? 'text' : null
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
export function savedPreview(condensed: string, before: number, after: number, saved: string, original: string, standout: string[], isWeb = false): string {
  const head = condensed.slice(0, PREVIEW_CHARS)
  const cut = head.length < condensed.length ? head.slice(0, head.lastIndexOf('\n') + 1) || head : head
  const lines = standout.length && !isWeb ? `\nLines that stand out:\n${standout.join('\n')}\n` : ''
  const how = isWeb ? `turned this web page into Markdown, from about ${before} to ${after} tokens` : `condensed it from about ${before} to ${after} tokens without dropping any line`

  return `[Output too long to show. The dam ${how}, into ${saved}: read that file for the rest. Original: ${original}.]\n${lines}\nFirst lines:\n${cut}`
}

// Whether a condensed output is small enough for Claude Code to show inline.
export function fitsInline(text: string): boolean {
  return text.length < MOST_INLINE_CHARS
}

// Rough, as no tokenizer is at hand: about four characters per token.
const tokensIn = (text: string) => Math.ceil(text.length / 4)

// What a Bash, PowerShell, Grep or MCP call printed, in tokens, whether or not it was condensed; 0 for other tools.
export function outputTokens(tool: string, record: ToolRecord & { persistedOutputSize?: unknown }): number {
  if (tool.startsWith('mcp__')) return typeof record.text === 'string' ? tokensIn(record.text) : 0
  if (tool !== 'Bash' && tool !== 'PowerShell' && tool !== 'Grep') return 0
  if (typeof record.persistedOutputSize === 'number') return Math.ceil(record.persistedOutputSize / 4)
  const text = tool === 'Grep' ? record.content : record.stdout

  return typeof text === 'string' ? tokensIn(text) : 0
}

export function parseEconomizer(stdout: string) {
  const { toolprune, microskills, ledger } = JSON.parse(stdout) ?? {}
  const count = (list: unknown) => (Array.isArray(list) ? list.length : 0)
  const num = (value: unknown) => (typeof value === 'number' && Number.isFinite(value) ? value : 0)

  return {
    pruned: count(toolprune?.applied),
    pending: count(toolprune?.proposal),
    facts: num(microskills?.injected_lines),
    loops: num(ledger?.nudges) + num(ledger?.aborts),
    overhead: Math.round(num(ledger?.overhead_tokens)),
  }
}

const MAPPED_FILE = /\.(py|js|jsx|ts|tsx|java|kt|go|rs|c|h|cc|cpp|hpp|cs|rb|php|swift|scala|lua|sh|md|markdown|mdx|rst|ya?ml|toml|json)$/i
const WORTH_GRAPHING_FILES = 20

// A map only pays for itself in a real project, of code or of documents: `listing` is the output of `git ls-files`.
export function worthGraphing(listing: string): boolean {
  return listing.split('\n').filter(path => MAPPED_FILE.test(path.trim())).length >= WORTH_GRAPHING_FILES
}

// `dam graph query`, or the separate `dam-graph` program of versions before 0.21.
const GRAPH_QUERY = /dam(?:-graph(?:\.exe)?"?|(?:\.exe)?"?\s+graph)\s+query\s+(.+)/

// The question a Bash command asks the code graph, or null for any other command.
export function graphQuestion(command: string): string | null {
  return command.match(GRAPH_QUERY)?.[1]?.trim().slice(0, 160) ?? null
}

// The number of nodes in dam-graph's table.
export function graphNodes(text: string): number {
  return text.split('\n').filter(line => line.startsWith('N\t')).length
}
