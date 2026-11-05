// Side calls on the user's own agent CLI (Apache-2.0, Microsoft), rewritten from plugin/sidecall.py (the Claude, Codex and Copilot parts).
// `claude -p` with no tools and none of the person's MCP servers, under TE_SIDECALL=1 so its hooks do nothing.
use crate::config::Config;
use crate::economizer::{dumps, env, now, state_root};
use crate::paths::abspath;
use crate::session::{append_error, iso, TempDir};
use fancy_regex::Regex;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{LazyLock, OnceLock};
use std::time::{Duration, Instant};

pub const TIMEOUT_S: u64 = 180;

// (reply, effective tokens when known); an Err is what Python raises (a timeout, no such program).
pub type Runner = dyn Fn(&str, &str) -> Result<(String, Option<f64>), String>;

static SIGNED_OUT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"To authenticate|/login\b|\b[Nn]ot (?:logged|signed) in").expect("fixed"));
static HELP: OnceLock<String> = OnceLock::new();

struct Output {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn reader<R: Read + Send + 'static>(pipe: Option<R>) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

fn run_capture(mut command: Command, input: Option<&str>, timeout: Duration) -> Result<Output, String> {
    command.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(|error| error.to_string())?;
    if let (Some(text), Some(mut stdin)) = (input, child.stdin.take()) {
        let data = text.as_bytes().to_vec();
        std::thread::spawn(move || {
            let _ = stdin.write_all(&data);
        });
    }
    let (out, err) = (reader(child.stdout.take()), reader(child.stderr.take()));
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(|error| error.to_string())? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("Command timed out after {} seconds", timeout.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(20));
    };

    Ok(Output { code: status.code(), stdout: out.join().unwrap_or_default(), stderr: err.join().unwrap_or_default() })
}

fn pause_path(root: &Path) -> PathBuf {
    root.join("hosts").join(crate::hosts::name()).join("sidecall.json")
}

// The pause since this host's CLI (`claude -p`, `codex exec`, `copilot -p`) said it is not signed in ({"at", "reason"}), or None.
pub fn paused(root: &Path) -> Option<Value> {
    let doc: Value = serde_json::from_slice(&std::fs::read(pause_path(root)).ok()?).ok()?;

    (doc.is_object() && doc.get("at").is_some_and(crate::economizer::python_truthy)).then_some(doc)
}

pub fn resume(root: &Path) -> bool {
    std::fs::remove_file(pause_path(root)).is_ok()
}

fn last_chars(text: &str, count: usize) -> String {
    let chars: Vec<char> = text.chars().collect();

    chars[chars.len().saturating_sub(count)..].iter().collect()
}

// A call that came back empty leaves a line in errors.log; one that said it is not signed in pauses the side calls.
fn no_reply(output: &Output) {
    let tail = last_chars(&output.stderr.split_whitespace().collect::<Vec<_>>().join(" "), 300);
    let code = output.code.map_or("?".to_string(), |code| code.to_string());
    append_error(&format!("--- {} sidecall {}: no reply (exit {code}) {tail}", iso(now()), crate::hosts::name()));
    if SIGNED_OUT.is_match(&format!("{}{}", output.stderr, output.stdout)).unwrap_or(false) {
        let path = pause_path(&abspath(&state_root()));
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(path, dumps(&json!({"at": now(), "reason": last_chars(&tail, 200)}), None));
    }
}

pub fn which(name: &str) -> Option<PathBuf> {
    let extensions: Vec<String> = if cfg!(windows) { env("PATHEXT").unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_string()).split(';').map(str::to_string).collect() } else { vec![String::new()] };

    std::env::split_paths(&std::env::var_os("PATH")?).find_map(|dir| extensions.iter().map(|ext| dir.join(format!("{name}{ext}"))).find(|candidate| candidate.is_file()))
}

static SHIM_TARGET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#""%~?dp0%?\\([^"]+)"\s*%\*"#).expect("fixed"));
static SHIM_PROG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"SET "_prog=([^"%]+)""#).expect("fixed"));

// What the npm shim at `path` runs: [program, script] or [executable], or None when it cannot be read so.
fn shim_argv(path: &Path) -> Option<Vec<String>> {
    let text = std::fs::read_to_string(path).ok()?;
    let target = SHIM_TARGET.captures_iter(&text).flatten().last()?.get(1)?.as_str().replace('\\', std::path::MAIN_SEPARATOR_STR);
    let base = path.parent()?;
    let target = base.join(target);
    if !target.is_file() {
        return None;
    }
    let target_text = target.to_string_lossy().into_owned();
    if target_text.to_lowercase().ends_with(".exe") {
        return Some(vec![target_text]);
    }
    let name = SHIM_PROG.captures(&text).ok().flatten().and_then(|found| found.get(1)).map_or("node", |found| found.as_str());
    let beside = base.join(format!("{name}.exe"));
    let program = if beside.is_file() { Some(beside) } else { which(name) }?;

    Some(vec![program.to_string_lossy().into_owned(), target_text])
}

// The agent CLI `cli` (a name or a path) as a command started with no shell in between. On Windows an npm-installed
// CLI is a `.cmd` shim that runs through cmd.exe, which parses every argument again: a newline ends the command and
// `%VAR%` expands, while a side call's prompt is session content. A batch file that is not such a shim is not run.
pub fn command(cli: &str) -> Result<Command, String> {
    if !cfg!(windows) {
        return Ok(Command::new(cli));
    }
    let found = if Path::new(cli).is_file() { Some(PathBuf::from(cli)) } else { which(cli) };
    let Some(found) = found.filter(|path| path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| ext.eq_ignore_ascii_case("cmd") || ext.eq_ignore_ascii_case("bat"))) else {
        return Ok(Command::new(cli));
    };
    let argv = shim_argv(&found).ok_or_else(|| format!("{}: a batch file that is not an npm shim, whose arguments cmd.exe would parse again", found.display()))?;
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]);

    Ok(command)
}

// `claude` on PATH, else the native installer's `~/.local/bin/claude`, else the bare name.
pub fn claude_cli() -> String {
    if let Some(found) = which("claude") {
        return found.to_string_lossy().into_owned();
    }
    let home = env("HOME").or_else(|| env("USERPROFILE")).unwrap_or_default();
    let exe = PathBuf::from(home).join(".local").join("bin").join(if cfg!(windows) { "claude.exe" } else { "claude" });

    if exe.is_file() { exe.to_string_lossy().into_owned() } else { "claude".to_string() }
}

// shortcut: one Claude Code per process, so `--help` is read once whatever path asked for it.
fn claude_help(cli: &str) -> &'static str {
    HELP.get_or_init(|| {
        let Ok(mut command) = command(cli) else { return String::new() };
        command.arg("--help");
        run_capture(command, None, Duration::from_secs(30)).map(|output| output.stdout).unwrap_or_default()
    })
}

// A side call is not a session of the person's, and stays out of their history.
pub fn claude_flags(cli: &str) -> Vec<String> {
    if claude_help(cli).contains("--no-session-persistence") { vec!["--no-session-persistence".to_string()] } else { Vec::new() }
}

// `--strict-mcp-config` with an empty `--mcp-config` file: a side call needs none of the person's MCP servers.
pub fn claude_no_mcp(cli: &str, cwd: &Path) -> std::io::Result<Vec<String>> {
    if !claude_help(cli).contains("--strict-mcp-config") {
        return Ok(Vec::new());
    }
    let path = cwd.join("mcp.json");
    std::fs::write(&path, r#"{"mcpServers": {}}"#)?;

    Ok(vec!["--strict-mcp-config".to_string(), "--mcp-config".to_string(), path.to_string_lossy().into_owned()])
}

pub fn claude_call(prompt: &str, model: &str) -> Result<(String, Option<f64>), String> {
    let cli = claude_cli();
    let dir = TempDir::new("te-side-").map_err(|error| error.to_string())?;
    let model = if model.is_empty() { "haiku" } else { model };
    let mut command = command(&cli)?;
    command.args(["-p", "--output-format", "json", "--max-turns", "1", "--tools", ""]).args(claude_no_mcp(&cli, &dir.0).map_err(|error| error.to_string())?).args(["--model", model]).args(claude_flags(&cli));
    command.env("TE_SIDECALL", "1").current_dir(&dir.0);
    let output = run_capture(command, Some(prompt), Duration::from_secs(TIMEOUT_S))?;
    let data: Value = serde_json::from_str(if output.stdout.is_empty() { "{}" } else { &output.stdout }).ok().filter(Value::is_object).unwrap_or_else(|| json!({}));
    let reply = data.get("result").filter(|reply| crate::economizer::python_truthy(reply));
    if reply.is_none() {
        no_reply(&output);
    }
    let mut usage = data.get("usage").filter(|usage| usage.is_object()).cloned().unwrap_or_else(|| json!({}));
    usage["model"] = json!(data.get("modelUsage").and_then(Value::as_object).and_then(|used| used.keys().next()).map_or_else(|| model.to_string(), Clone::clone));
    let (effective, _) = crate::pricing::effective(&usage);

    Ok((reply.map_or_else(String::new, crate::config::plain_text), effective))
}

// Codex's usage, converted as the rollout's is: its input is the grand total including both cache fields.
fn codex_effective(usage: &Value, model: &str) -> Option<f64> {
    let count = |key: &str| usage.get(key).and_then(Value::as_i64).unwrap_or(0);
    let (cached, written) = (count("cached_input_tokens"), count("cache_write_input_tokens"));
    let row = json!({"input_tokens": (count("input_tokens") - cached - written).max(0), "cache_read_input_tokens": cached, "cache_creation_input_tokens": written, "output_tokens": count("output_tokens") + count("reasoning_output_tokens"), "model": if model.is_empty() { "codex" } else { model }});

    crate::pricing::effective(&row).0
}

// sidecall.codex_call: `codex exec` read-only, without hooks or a saved session, from an empty directory.
pub fn codex_call(prompt: &str, model: &str) -> Result<(String, Option<f64>), String> {
    let dir = TempDir::new("te-side-").map_err(|error| error.to_string())?;
    let mut command = command("codex")?;
    // A read-only sandbox still lets it read and run commands, and the person's MCP servers are outside the sandbox:
    // none of them, and a reply that used a tool anyway is dropped (the session text it reads could steer it).
    command.args(["exec", "--json", "--disable", "hooks", "--ephemeral", "-s", "read-only", "--skip-git-repo-check", "-c", "mcp_servers={}"]);
    if !model.is_empty() {
        command.args(["-m", model]);
    }
    // `--`: a prompt that starts with `-` is not read as a flag.
    command.arg("--").arg(prompt).env("TE_SIDECALL", "1").current_dir(&dir.0);
    let output = run_capture(command, None, Duration::from_secs(TIMEOUT_S))?;
    let (mut text, mut usage, mut used_a_tool) = (String::new(), None, false);
    for event in output.stdout.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()) {
        let item = event["item"].get("type").and_then(Value::as_str);
        match event.get("type").and_then(Value::as_str) {
            Some("item.started" | "item.completed") if !matches!(item, Some("agent_message" | "reasoning" | "todo_list") | None) => used_a_tool = true,
            Some("item.completed") if item == Some("agent_message") => {
                text = event["item"].get("text").and_then(Value::as_str).filter(|reply| !reply.is_empty()).map_or(text, str::to_string);
            }
            Some("turn.completed") => usage = event.get("usage").filter(|found| found.is_object()).cloned().or(usage),
            _ => {}
        }
    }
    if used_a_tool {
        text.clear();
    }
    if text.is_empty() {
        no_reply(&output);
    }

    Ok((text, usage.and_then(|usage| codex_effective(&usage, model))))
}

// Everything the Copilot CLI offers (1.0.83 names), so the completion is offered nothing.
const COPILOT_TOOLS: [&str; 24] = ["bash", "read_bash", "write_bash", "stop_bash", "list_bash", "view", "grep", "rg", "glob", "edit", "create", "str_replace", "apply_patch", "web_fetch", "web_search", "ask_user", "session_store_sql", "sql", "task", "skill", "list_agents", "read_agent", "write_agent", "fetch_copilot_cli_documentation"];

// copilot_model: `paid_model`, else a concrete model the environment names, else Copilot's own default.
fn copilot_model(paid: &str) -> String {
    if !paid.is_empty() {
        return paid.to_string();
    }
    let named = ["COPILOT_PROVIDER_MODEL_ID", "COPILOT_MODEL"].iter().filter_map(|key| env(key)).map(|value| value.trim().to_string()).find(|value| !value.is_empty() && !value.eq_ignore_ascii_case("auto"));
    let base = env("COPILOT_PROVIDER_BASE_URL").unwrap_or_default();
    let byok = || if base.contains("openrouter.ai") { "anthropic/claude-haiku-4.5".to_string() } else if !base.is_empty() && env("COPILOT_PROVIDER_TYPE").unwrap_or_default().eq_ignore_ascii_case("anthropic") { "claude-haiku-4-5".to_string() } else { String::new() };

    named.unwrap_or_else(byok)
}

// The person's MCP servers, which a completion needs none of (trace.copilot_mcp_servers, less the built-in one).
fn copilot_mcp_servers() -> Vec<String> {
    let path = crate::hosts::copilot_home().join("mcp-config.json");
    let doc: Value = std::fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or(Value::Null);
    let mut names: Vec<String> = doc.get("mcpServers").and_then(Value::as_object).into_iter().flatten().map(|(name, _)| name.clone()).collect();
    names.sort();

    names
}

// sidecall.copilot_call: `copilot -p` as a pure text completion, every tool and MCP server refused.
pub fn copilot_call(prompt: &str, model: &str) -> Result<(String, Option<f64>), String> {
    let dir = TempDir::new("te-side-").map_err(|error| error.to_string())?;
    let model = copilot_model(model);
    let mut command = command("copilot")?;
    command.args(["-p", prompt, "--output-format", "json", "--no-color", "--no-auto-update", "--no-custom-instructions", "--disable-builtin-mcps"]);
    for server in copilot_mcp_servers().iter().filter(|server| *server != "github-mcp-server") {
        command.args(["--disable-mcp-server", server]);
    }
    for tool in COPILOT_TOOLS {
        command.args(["--deny-tool", tool, "--excluded-tools", tool]);
    }
    // The permission kinds too, so a tool newer than the list above is still refused.
    command.args(["--deny-tool", "shell", "--deny-tool", "write"]);
    if !model.is_empty() {
        command.args(["--model", &model]);
    }
    command.env("TE_SIDECALL", "1").current_dir(&dir.0);
    let output = run_capture(command, None, Duration::from_secs(TIMEOUT_S))?;
    let text = output.stdout.lines().filter_map(|line| serde_json::from_str::<Value>(line).ok()).filter(|event| event.get("type").and_then(Value::as_str) == Some("assistant.message")).filter_map(|event| event["data"].get("content").and_then(Value::as_str).filter(|reply| !reply.is_empty()).map(str::to_string)).next_back().unwrap_or_default();
    if text.is_empty() {
        no_reply(&output);
    }

    // shortcut: no usage read back from the call's own session log, so the judge charges its estimate; read `session.shutdown` if Copilot's spend must be exact.
    Ok((text, None))
}

// Each host's calls go through that host's own CLI, never another agent's.
pub fn runner_for(config: &Config) -> Box<Runner> {
    let model = config.text("paid_model").to_string();

    match crate::hosts::current() {
        crate::channels::Host::Claude => Box::new(move |prompt, _model| claude_call(prompt, &model)),
        crate::channels::Host::Codex => Box::new(move |prompt, _model| codex_call(prompt, &model)),
        crate::channels::Host::Copilot => Box::new(move |prompt, _model| copilot_call(prompt, &model)),
    }
}
