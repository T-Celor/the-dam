// The Codex and GitHub Copilot packaging: each manifest and hook file follows its host's conventions, and the hook
// programs answer each host's payload in that host's shape. No model, no network: run after `cargo build --release`
// with `node --test scripts/hosts.test.mjs`.
import test from 'node:test'
import assert from 'node:assert/strict'
import { spawnSync, execFileSync } from 'node:child_process'
import { existsSync, mkdtempSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'

const root = join(import.meta.dirname, '..')
const read = path => JSON.parse(readFileSync(join(root, path), 'utf8'))
const claude = read('.claude-plugin/plugin.json')
const codex = read('.codex-plugin/plugin.json')
const copilot = read('.github/plugin/plugin.json')

// From developers.openai.com/codex/hooks and docs.github.com/copilot/reference/copilot-cli-reference/cli-hooks-reference.
const CODEX_EVENTS = ['SessionStart', 'SessionEnd', 'SubagentStart', 'SubagentStop', 'PreToolUse', 'PermissionRequest', 'PostToolUse', 'PreModelRequest', 'PostModelRequest', 'PreCompact', 'PostCompact', 'UserPromptSubmit', 'Stop', 'Interrupt']
const COPILOT_EVENTS = ['sessionStart', 'sessionEnd', 'userPromptSubmitted', 'userPromptTransformed', 'preToolUse', 'postToolUse', 'postToolUseFailure', 'errorOccurred', 'agentStop', 'subagentStart', 'subagentStop', 'preCompact', 'permissionRequest', 'notification']
const SUBCOMMANDS = ['beaver', 'te', 'graph', 'squeeze']
const rel = path => path.replace(/^\.\//, '')

test('the Codex and Copilot files are what scripts/hosts.mjs writes from the one declaration', () => {
  const out = spawnSync(process.execPath, [join(root, 'scripts', 'hosts.mjs'), '--check'], { encoding: 'utf8' })
  assert.equal(out.status, 0, out.stderr)
})

test('the manifests name the same plugin at the same version', () => {
  for (const manifest of [codex, copilot]) {
    assert.equal(manifest.name, claude.name)
    assert.equal(manifest.version, claude.version)
    assert.match(manifest.name, /^[a-z0-9]+(-[a-z0-9]+)*$/)
  }
  assert.equal(read('.github/plugin/marketplace.json').plugins[0].version, claude.version)
})

test('every path a manifest names exists', () => {
  for (const path of [codex.skills, codex.hooks, codex.interface.composerIcon, codex.interface.logo]) {
    assert.match(path, /^\.\//, `${path}: Codex paths start with ./`)
    assert.ok(existsSync(join(root, path)), path)
  }
  for (const path of [copilot.skills, copilot.hooks]) assert.ok(existsSync(join(root, path)), path)
  const market = read('.github/plugin/marketplace.json')
  assert.ok(market.owner?.name && market.plugins.every(p => p.name && p.source))
  const agents = read('.agents/plugins/marketplace.json').plugins[0]
  assert.equal(agents.policy.installation, 'AVAILABLE')
  assert.match(agents.source.url, /github\.com\/T-Celor\/the-dam/)
})

test('each skill is an Agent Skill: its own folder, SKILL.md with a name and a description', () => {
  const skills = readdirSync(join(root, 'skills'), { withFileTypes: true }).filter(entry => entry.isDirectory())
  assert.ok(skills.length > 0)
  for (const { name } of skills) {
    const text = readFileSync(join(root, 'skills', name, 'SKILL.md'), 'utf8')
    const front = text.match(/^---\r?\n([\s\S]*?)\r?\n---/)?.[1] ?? ''
    assert.match(front, new RegExp(`^name: ${name}\\s*$`, 'm'), `${name}: name`)
    assert.match(front, /^description: .+/m, `${name}: description`)
  }
})

test('the Codex hooks use Codex events and reach the programs from PLUGIN_ROOT', () => {
  const { hooks } = read(rel(codex.hooks))
  for (const [event, groups] of Object.entries(hooks)) {
    assert.ok(CODEX_EVENTS.includes(event), event)
    for (const handler of groups.flatMap(group => group.hooks)) {
      assert.equal(handler.type, 'command')
      assert.ok(handler.timeout > 0 && handler.timeout <= 600)
      assert.match(handler.command, /^"\$PLUGIN_ROOT\/scripts\/dam-run" (beaver|te|graph|squeeze) [a-z]+ --host codex$/)
      // For `%COMSPEC% /C`: built-ins first, so neither a `cmd.exe` nor a `dam.exe` from the project ever runs.
      assert.match(handler.commandWindows, /^set NoDefaultCurrentDirectoryInExePath=1&& set "PATH=%PLUGIN_ROOT%\\bin\\windows-x64;%PATH%"&& dam\.exe [a-z]+ [a-z]+ --host codex$/)
      assert.ok(SUBCOMMANDS.includes(handler.command.split(' ')[1]))
    }
  }
})

test('the Copilot hooks are version 1, camelCase events, with bash and PowerShell', () => {
  const file = read(copilot.hooks)
  assert.equal(file.version, 1)
  for (const [event, handlers] of Object.entries(file.hooks)) {
    assert.ok(COPILOT_EVENTS.includes(event), event)
    for (const handler of handlers) {
      assert.equal(handler.type, 'command')
      assert.ok(handler.timeoutSec > 0)
      assert.match(handler.bash, new RegExp(`PLUGIN_ROOT.*/scripts/dam-run" (beaver|te|graph|squeeze) [a-z]+ --host copilot --event ${event} \\|\\| exit 0$`))
      assert.match(handler.powershell, /bin\/windows-x64\/dam\.exe'.*& \$exe [a-z]+ [a-z]+ --host copilot --event [a-zA-Z]+ \}$/)
    }
  }
})

// ---- the programs, as each host runs them ----

const ext = process.platform === 'win32' ? '.exe' : ''
const folder = { win32: 'windows-x64', linux: process.arch === 'arm64' ? 'linux-arm64' : 'linux-x64', darwin: process.arch === 'arm64' ? 'macos-arm64' : 'macos-x64' }[process.platform]
const dam = [join(root, 'dam', 'target', 'release', `dam${ext}`), join(root, 'bin', folder, `dam${ext}`)].find(existsSync)

function project() {
  const home = mkdtempSync(join(tmpdir(), 'dam-hosts-'))
  const repo = join(home, 'repo')
  mkdirSync(join(repo, 'src'), { recursive: true })
  for (const name of ['a', 'b', 'c']) writeFileSync(join(repo, 'src', `${name}.py`), `def ${name}_thing():\n    return 1\n`)
  execFileSync('git', ['init', '-q'], { cwd: repo, stdio: 'ignore' })
  execFileSync('git', ['-c', 'core.autocrlf=false', 'add', '.'], { cwd: repo, stdio: 'ignore' })
  return { home, repo }
}

const HOSTS = {
  claude: home => ({ CLAUDE_PLUGIN_ROOT: root }),
  codex: home => ({ PLUGIN_ROOT: root, PLUGIN_DATA: join(home, 'data'), CLAUDE_PLUGIN_ROOT: root, CLAUDE_PLUGIN_DATA: join(home, 'data') }),
  copilot: home => ({ PLUGIN_ROOT: root, COPILOT_PLUGIN_DATA: join(home, 'data') }),
}

function run(host, args, payload) {
  const { home, repo } = project()
  try {
    const env = { PATH: process.env.PATH, SYSTEMROOT: process.env.SYSTEMROOT ?? '', HOME: home, USERPROFILE: home, APPDATA: join(home, 'appdata'), TOKENECONOMIZER_HOME: join(home, 'state'), ...HOSTS[host](home) }
    const out = spawnSync(dam, ['beaver', ...args], { cwd: repo, env, input: JSON.stringify(payload), encoding: 'utf8', timeout: 30_000 })
    assert.equal(out.status, 0, out.stderr)
    return out.stdout
  } finally {
    rmSync(home, { recursive: true, force: true })
  }
}

const skip = dam ? false : 'no dam build: run `cargo build --release` in dam/'

test('Claude Code still gets plain rules and no map line at session start', { skip }, () => {
  const out = run('claude', ['activate'], { session_id: 's1', source: 'startup', hook_event_name: 'SessionStart' })
  assert.match(out, /^BEAVER MODE ACTIVE — level: full/)
  assert.doesNotMatch(out, /A map of this project/)
})

test('Codex gets the map, condensing and the rules as plain context at session start', { skip }, () => {
  const out = run('codex', ['activate'], { session_id: 's1', source: 'startup', hook_event_name: 'SessionStart', cwd: '.', model: 'gpt', transcript_path: null })
  assert.match(out, /A map of this project/)
  assert.match(out, /dam(\.exe)?" graph query/)
  // Condensing is the squeeze hook's job there, so the start text does not ask the model to pipe outputs.
  assert.doesNotMatch(out, /" squeeze/)
  assert.match(out, /BEAVER MODE ACTIVE — level: full/)
  // The full rules from skills/beaver, not the one-line stand-in for a plugin folder not found.
  assert.match(out, /## Find before you read/)
})

test('Copilot gets the same as additionalContext JSON', { skip }, () => {
  const out = JSON.parse(run('copilot', ['activate'], { sessionId: 's1', timestamp: 1, cwd: '.', source: 'new' }))
  assert.deepEqual(Object.keys(out), ['additionalContext'])
  assert.match(out.additionalContext, /A map of this project/)
  assert.match(out.additionalContext, /## Find before you read/)
})

test('each host gets a subagent its rules in its own shape', { skip }, () => {
  // A level is live only after a start in the same session, as in a real one.
  const both = (host, payload) => {
    const { home, repo } = project()
    try {
      const env = { PATH: process.env.PATH, SYSTEMROOT: process.env.SYSTEMROOT ?? '', HOME: home, USERPROFILE: home, APPDATA: join(home, 'appdata'), TOKENECONOMIZER_HOME: join(home, 'state'), ...HOSTS[host](home) }
      const go = (args, input) => spawnSync(dam, ['beaver', ...args], { cwd: repo, env, input: JSON.stringify(input), encoding: 'utf8', timeout: 30_000 }).stdout
      go(['activate'], { ...payload, source: 'startup' })
      return { sub: go(['subagent'], payload), track: go(['track'], { ...payload, prompt: '/beaver lite' }) }
    } finally {
      rmSync(home, { recursive: true, force: true })
    }
  }
  const codexRun = both('codex', { session_id: 's2', agent_type: 'worker', agent_id: 'a1' })
  assert.equal(JSON.parse(codexRun.sub).hookSpecificOutput.hookEventName, 'SubagentStart')
  assert.match(codexRun.track, /BEAVER MODE CHANGED — level: lite/)
  const copilotRun = both('copilot', { sessionId: 's2', agentName: 'worker' })
  assert.match(JSON.parse(copilotRun.sub).additionalContext, /BEAVER MODE ACTIVE/)
})

// ---- the hooks that do the mod's work outside Claude Code ----

function hook(host, words, payload) {
  const { home, repo } = project()
  try {
    const env = { PATH: process.env.PATH, SYSTEMROOT: process.env.SYSTEMROOT ?? '', HOME: home, USERPROFILE: home, APPDATA: join(home, 'appdata'), TOKENECONOMIZER_HOME: join(home, 'state'), TE_JUDGE_NO_SPAWN: '1', ...HOSTS[host](home) }
    const out = spawnSync(dam, words, { cwd: repo, env, input: JSON.stringify({ cwd: repo, ...payload }), encoding: 'utf8', timeout: 60_000 })
    assert.equal(out.status, 0, out.stderr)
    return JSON.parse(out.stdout || '{}')
  } finally {
    rmSync(home, { recursive: true, force: true })
  }
}

const LOG = Array.from({ length: 400 }, (_, i) => `2026-11-04 12:00:${i} INFO worker ${i} finished job ${i} in ${i} ms`).join('\n')

test('Copilot gets a long Bash output back condensed, as a modified result', { skip }, () => {
  const out = hook('copilot', ['squeeze', 'hook', '--host', 'copilot', '--event', 'postToolUse'], { sessionId: 's3', toolName: 'bash', toolArgs: { command: 'make' }, toolResult: { resultType: 'success', textResultForLlm: LOG } })
  assert.equal(out.modifiedResult.resultType, 'success')
  assert.ok(out.modifiedResult.textResultForLlm.length < LOG.length / 4)
})

test('Codex gets a long Bash output back condensed, through a block reason', { skip }, () => {
  const out = hook('codex', ['squeeze', 'hook', '--host', 'codex'], { session_id: 's3', hook_event_name: 'PostToolUse', tool_name: 'Bash', tool_input: { command: 'make' }, tool_response: LOG })
  assert.equal(out.decision, 'block')
  assert.ok(out.reason.length < LOG.length / 4)
})

test('Claude Code is left to the mod: the squeeze hook says nothing there', { skip }, () => {
  assert.deepEqual(hook('claude', ['squeeze', 'hook'], { session_id: 's3', hook_event_name: 'PostToolUse', tool_name: 'Bash', tool_input: { command: 'make' }, tool_response: { stdout: LOG } }), {})
})

test('TokenEconomizer answers each host in its own shape', { skip }, () => {
  for (const [host, words, payload] of [
    ['codex', ['te', 'hook', '--host', 'codex'], { session_id: 's4', hook_event_name: 'PostToolUse', tool_name: 'Bash', tool_input: { command: 'make' }, tool_response: 'ok' }],
    ['copilot', ['te', 'hook', '--host', 'copilot', '--event', 'postToolUse'], { sessionId: 's4', toolName: 'bash', toolArgs: { command: 'make' }, toolResult: { resultType: 'success', textResultForLlm: 'ok' } }],
  ]) {
    const out = hook(host, words, payload)
    assert.ok(!('systemMessage' in out) || host === 'codex', host)
    assert.ok(Object.keys(out).every(key => ['hookSpecificOutput', 'systemMessage', 'additionalContext', 'decision', 'reason'].includes(key)), JSON.stringify(out))
  }
})

test('Copilot gets the map on code its prompt names, in the prompt the model receives', { skip }, () => {
  const { home, repo } = project()
  try {
    const env = { PATH: process.env.PATH, SYSTEMROOT: process.env.SYSTEMROOT ?? '', HOME: home, USERPROFILE: home, TOKENECONOMIZER_HOME: join(home, 'state'), ...HOSTS.copilot(home) }
    assert.equal(spawnSync(dam, ['graph', 'update'], { cwd: repo, env, encoding: 'utf8' }).status, 0)
    const payload = { sessionId: 's5', cwd: repo, prompt: 'why does b_thing return 1?', transformedPrompt: '<user>why does b_thing return 1?</user>' }
    const out = JSON.parse(spawnSync(dam, ['squeeze', 'hook', '--host', 'copilot', '--event', 'userPromptTransformed'], { cwd: repo, env, input: JSON.stringify(payload), encoding: 'utf8' }).stdout)
    assert.ok(out.modifiedTransformedPrompt.startsWith(payload.transformedPrompt), out.modifiedTransformedPrompt)
    assert.match(out.modifiedTransformedPrompt, /b_thing\(\) src\/b\.py:1/)
  } finally {
    rmSync(home, { recursive: true, force: true })
  }
})
