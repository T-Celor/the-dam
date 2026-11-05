// One declaration for three hosts. `.claude-plugin/plugin.json` (what the plugin is) and `hooks/hooks.json` (what runs
// when) are the only ones written by hand; this writes the Codex and GitHub Copilot manifests, marketplaces and hook
// files from them. `node scripts/hosts.mjs` writes them; `--check` fails when one is out of date (the tests run it).
// Not symlinks: Git on Windows checks a link out as a text file unless Developer Mode is on, and each host's schema differs.
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs'
import { dirname, join } from 'node:path'

const root = join(import.meta.dirname, '..')
const read = path => JSON.parse(readFileSync(join(root, path), 'utf8'))
const plugin = read('.claude-plugin/plugin.json')
const claudeHooks = read('hooks/hooks.json').hooks

// The `dam` subcommands of hooks/hooks.json whose hooks answer any host: Beaver, and TokenEconomizer (`dam te hook`,
// which reads each host's payload and session log through dam/src/hosts.rs).
const PORTABLE = ['beaver', 'te']
// What the mod does under Claude Code, done by a hook elsewhere: condensing outputs and the map's answers
// (dam/src/answers.rs). Copilot ignores what its prompt hook prints, so its map answers for a prompt go through the
// later userPromptTransformed, which may rewrite what the model receives.
const MOD_HOOKS = [
  { event: 'PostToolUse', matcher: 'Bash|mcp__.*', words: 'squeeze hook', timeout: 30, hosts: ['codex', 'copilot'] },
  { event: 'UserPromptSubmit', words: 'squeeze hook', timeout: 30, hosts: ['codex'] },
  { event: 'UserPromptTransformed', words: 'squeeze hook', timeout: 30, hosts: ['copilot'] },
]
const COPILOT_EVENT = { SessionStart: 'sessionStart', UserPromptSubmit: 'userPromptSubmitted', SubagentStart: 'subagentStart', SessionEnd: 'sessionEnd', PreToolUse: 'preToolUse', PostToolUse: 'postToolUse', PostToolUseFailure: 'postToolUseFailure', Stop: 'agentStop', SubagentStop: 'subagentStop', PreCompact: 'preCompact', UserPromptTransformed: 'userPromptTransformed' }
const CODEX_EVENTS = new Set(['SessionStart', 'SessionEnd', 'SubagentStart', 'SubagentStop', 'PreToolUse', 'PermissionRequest', 'PostToolUse', 'PreCompact', 'PostCompact', 'UserPromptSubmit', 'Stop'])

// Each portable handler of hooks/hooks.json as { event, matcher, words (what follows `dam`), timeout, statusMessage }.
const handlers = Object.entries(claudeHooks).flatMap(([event, groups]) =>
  groups.flatMap(group => group.hooks.flatMap(hook => {
    const run = hook.command.match(/^"\$\{CLAUDE_PLUGIN_ROOT\}\/scripts\/dam-run" ([a-z]+) (.+)$/)
    if (!run || !PORTABLE.includes(run[1])) return []
    return [{ event, matcher: group.matcher, words: `${run[1]} ${run[2]}`, timeout: hook.timeout, statusMessage: hook.statusMessage, hosts: ['codex', 'copilot'] }]
  }))).concat(MOD_HOOKS)
// The host named on every line, and Copilot's event too, as its camelCase payloads do not carry it.
const words = (h, host, event) => `${h.words} --host ${host}${host === 'copilot' ? ` --event ${event}` : ''}`

const meta = { name: plugin.name, version: plugin.version, description: plugin.description, author: plugin.author, homepage: plugin.homepage, repository: plugin.repository, license: plugin.license, keywords: plugin.keywords }
const display = 'The dam'

// Codex runs a hook in sh, or on Windows `commandWindows` in cmd or PowerShell: no quotes inside, which PowerShell would strip.
const codexHooks = {}
for (const h of handlers.filter(h => CODEX_EVENTS.has(h.event) && h.hosts.includes('codex'))) {
  const line = words(h, 'codex')
  const handler = {
    type: 'command',
    command: `"$PLUGIN_ROOT/scripts/dam-run" ${line}`,
    // Codex runs it with `%COMSPEC% /C`. cmd.exe looks for a bare program name in the project first, so a repository
    // could ship its own `cmd.exe` or `dam.exe`: the line starts with built-ins only, the first of which ends that search.
    // It begins with no quote, which /C would strip; the quoted `set` keeps a `&` in the path from splitting the line.
    commandWindows: String.raw`set NoDefaultCurrentDirectoryInExePath=1&& set "PATH=%PLUGIN_ROOT%\bin\windows-x64;%PATH%"&& ` + `dam.exe ${line}`,
    timeout: h.timeout,
    ...(h.statusMessage && { statusMessage: h.statusMessage }),
  }
  ;(codexHooks[h.event] ??= []).push({ ...(h.matcher && { matcher: h.matcher }), hooks: [handler] })
}

// Copilot takes one flat entry per handler, a bash and a PowerShell line; VS Code's Copilot sets only CLAUDE_PLUGIN_ROOT.
const copilotHooks = {}
for (const h of handlers.filter(h => COPILOT_EVENT[h.event] && h.hosts.includes('copilot'))) {
  const event = COPILOT_EVENT[h.event]
  const line = words(h, 'copilot', event)
  ;(copilotHooks[event] ??= []).push({
    type: 'command',
    bash: `"\${PLUGIN_ROOT:-$CLAUDE_PLUGIN_ROOT}/scripts/dam-run" ${line} || exit 0`,
    powershell: `$root = if ($env:PLUGIN_ROOT) { $env:PLUGIN_ROOT } else { $env:CLAUDE_PLUGIN_ROOT }; $exe = Join-Path $root 'bin/windows-x64/dam.exe'; if (Test-Path $exe) { & $exe ${line} }`,
    timeoutSec: h.timeout,
  })
}

const files = {
  'hooks/codex.json': { description: `Generated by scripts/hosts.mjs from hooks/hooks.json: the hooks that answer Codex.`, hooks: codexHooks },
  'hooks/copilot.json': { version: 1, hooks: copilotHooks },
  '.codex-plugin/plugin.json': {
    ...meta,
    skills: './skills/',
    hooks: './hooks/codex.json',
    interface: {
      displayName: display,
      shortDescription: 'Fewer tokens, every line kept',
      longDescription: 'A local map of the project, lossless condensing of long outputs and Beaver\'s smallest-change rules, all on this machine.',
      developerName: plugin.author.name,
      category: 'Productivity',
      capabilities: ['Instructions', 'Lifecycle hooks'],
      websiteURL: plugin.homepage,
      defaultPrompt: ['Use Beaver mode for this task.', 'Review this diff for over-engineering.', 'Ask the project map where this is defined.'],
      brandColor: '#2E7D8C',
      composerIcon: './assets/icon.png',
      logo: './assets/icon.png',
    },
  },
  '.github/plugin/plugin.json': { ...meta, category: 'productivity', skills: 'skills/', hooks: 'hooks/copilot.json' },
  '.github/plugin/marketplace.json': {
    name: plugin.name,
    description: plugin.description,
    owner: plugin.author,
    plugins: [{ name: plugin.name, description: plugin.description, version: plugin.version, source: './', category: 'productivity', skills: 'skills/', hooks: 'hooks/copilot.json' }],
  },
  '.agents/plugins/marketplace.json': {
    name: plugin.name,
    interface: { displayName: display },
    plugins: [{ name: plugin.name, source: { source: 'url', url: `${plugin.repository}.git`, ref: 'main' }, policy: { installation: 'AVAILABLE', authentication: 'ON_INSTALL' }, category: 'Productivity' }],
  },
}

const check = process.argv.includes('--check')
const stale = []
for (const [path, value] of Object.entries(files)) {
  const text = `${JSON.stringify(value, null, 2)}\n`
  const full = join(root, path)
  let now = null
  try { now = readFileSync(full, 'utf8').replace(/\r\n/g, '\n') } catch {}
  if (now === text) continue
  if (check) { stale.push(path); continue }
  mkdirSync(dirname(full), { recursive: true })
  writeFileSync(full, text)
  console.log(`wrote ${path}`)
}
if (stale.length) {
  console.error(`out of date, run \`node scripts/hosts.mjs\`: ${stale.join(', ')}`)
  process.exit(1)
}
