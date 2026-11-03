// Side calls on the user's own Claude Code (Apache-2.0, Microsoft), rewritten from plugin/sidecall.py (the Claude parts).
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
    root.join("hosts").join("claude").join("sidecall.json")
}

// The pause since `claude -p` said it is not signed in ({"at", "reason"}), or None.
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
    append_error(&format!("--- {} sidecall claude: no reply (exit {code}) {tail}", iso(now())));
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

pub fn runner_for(config: &Config) -> Box<Runner> {
    let model = config.text("paid_model").to_string();

    Box::new(move |prompt, _model| claude_call(prompt, &model))
}
