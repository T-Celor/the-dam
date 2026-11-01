// TokenEconomizer's hooks for Claude Code (Apache-2.0, Microsoft), rewritten from plugin/hook.py,
// controller.py, store.py, config.py, hosts.py and judge.py. The prompt-turn hooks run here on the
// same state files. Whatever is not ported yet goes to the Python: session start and end, other
// agents, a session not yet pinned to a repository, the repository map, the judge's own job.
use crate::loopguard::{BackgroundJudge, EnvelopeTracker, Judge, Thresholds, TurnGuard};
use fancy_regex::Regex;
use serde_json::{json, Map, Value};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const RESULT_CAP: usize = 20_000;
const TURN_HISTORY: usize = 200;
const LOCK_WAIT: Duration = Duration::from_secs(10);
const RESPONSE_KEYS: [&str; 10] = ["stdout", "stderr", "output", "aggregated_output", "formatted_output", "content", "result", "error", "message", "text_result_for_llm"];

static EXIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?im)^\s*(?:exit code|process exited with code)[:\s]+(-?\d+)").expect("fixed"));
static COPILOT_EXIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<shellId: [^>]*?completed with exit code (-?\d+)>").expect("fixed"));

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

fn truthy(value: Option<String>) -> bool {
    matches!(value.unwrap_or_default().trim().to_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |elapsed| elapsed.as_secs_f64())
}

fn state_root() -> PathBuf {
    if let Some(home) = env("TOKENECONOMIZER_HOME") {
        return PathBuf::from(home);
    }

    env("USERPROFILE").or_else(|| env("HOME")).map_or_else(|| PathBuf::from("."), PathBuf::from).join(".tokeneconomizer")
}

// json.dumps as Python writes it: sorted keys, ASCII only.
fn python_string(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            ' '..='~' => out.push(c),
            c => {
                let mut units = [0u16; 2];
                for unit in c.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
}

fn python_json(value: &Value, indent: Option<usize>, depth: usize, out: &mut String) {
    let breakline = |out: &mut String, depth: usize| {
        if let Some(size) = indent {
            out.push('\n');
            out.push_str(&" ".repeat(size * depth));
        }
    };
    let comma = if indent.is_some() { "," } else { ", " };
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        Value::Number(number) => out.push_str(&number.to_string()),
        Value::String(text) => python_string(text, out),
        Value::Array(items) if items.is_empty() => out.push_str("[]"),
        Value::Array(items) => {
            out.push('[');
            for (n, item) in items.iter().enumerate() {
                if n > 0 {
                    out.push_str(comma);
                }
                breakline(out, depth + 1);
                python_json(item, indent, depth + 1, out);
            }
            breakline(out, depth);
            out.push(']');
        }
        Value::Object(map) if map.is_empty() => out.push_str("{}"),
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by(|a, b| a.chars().cmp(b.chars()));
            out.push('{');
            for (n, key) in keys.iter().enumerate() {
                if n > 0 {
                    out.push_str(comma);
                }
                breakline(out, depth + 1);
                python_string(key, out);
                out.push_str(": ");
                python_json(&map[key.as_str()], indent, depth + 1, out);
            }
            breakline(out, depth);
            out.push('}');
        }
    }
}

fn dumps(value: &Value, indent: Option<usize>) -> String {
    let mut out = String::new();
    python_json(value, indent, 0, &mut out);

    out
}

struct Store {
    base: PathBuf,
}

impl Store {
    fn read(&self, name: &str) -> Option<Value> {
        serde_json::from_slice(&fs::read(self.base.join(name)).ok()?).ok()
    }

    // Through a temporary file, so that a reader never sees half a document.
    fn write(&self, name: &str, value: &Value) -> std::io::Result<()> {
        let target = self.base.join(name);
        if let Some(dir) = target.parent() {
            fs::create_dir_all(dir)?;
        }
        let temp = PathBuf::from(format!("{}.{}.tmp", target.display(), std::process::id()));
        fs::write(&temp, dumps(value, Some(2)))?;
        let moved = fs::rename(&temp, &target);
        if moved.is_err() {
            let _ = fs::remove_file(&temp);
        }

        moved
    }

    fn append(&self, name: &str, record: &Value) -> std::io::Result<()> {
        let target = self.base.join(name);
        if let Some(dir) = target.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut file = OpenOptions::new().create(true).append(true).open(target)?;

        writeln!(file, "{}", dumps(record, None))
    }

    // The same lock file as the Python's: a stuck holder costs at most the wait, then the work goes on.
    fn locked(&self, name: &str) -> Option<File> {
        fs::create_dir_all(&self.base).ok()?;
        let file = OpenOptions::new().create(true).read(true).append(true).open(self.base.join(format!(".{name}.lock"))).ok()?;
        let deadline = Instant::now() + LOCK_WAIT;
        while file.try_lock().is_err() {
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        Some(file)
    }
}

#[derive(Debug, PartialEq)]
pub struct Config {
    enabled: bool,
    mode: String,
    loopguard: String,
    aggr: String,
    nudges: i64,
    thresholds: Thresholds,
    repomap: String,
    shadow: bool,
}

fn canonical(key: &str) -> &str {
    match key {
        "spiral" => "loopguard",
        "spiral_aggr" => "loopguard_aggr",
        "spiral_nudges" => "loopguard_nudges",
        "spiral_thresholds" => "loopguard_thresholds",
        other => other,
    }
}

fn plain_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        other => other.to_string(),
    }
}

fn threshold_range(name: &str) -> Option<(bool, f64, Option<f64>)> {
    Some(match name {
        "min_turns" | "edit_churn" | "distinct_edit_files" | "error_outputs" | "envelope_min_samples" | "gate_min_turns" | "gate_error_outputs" => (true, 1.0, None),
        "stagnation" | "judge_confidence" => (false, 0.0, Some(1.0)),
        "envelope_pct" => (false, 1.0, Some(100.0)),
        "envelope_margin" | "backstop_mult" => (false, 0.1, None),
        _ => return None,
    })
}

impl Config {
    fn new() -> Self {
        Config { enabled: true, mode: "auto".to_string(), loopguard: "nudge".to_string(), aggr: "default".to_string(), nudges: 1, thresholds: Map::new(), repomap: "off".to_string(), shadow: false }
    }

    fn set_threshold(&mut self, name: &str, value: &Value) {
        let Some((is_int, low, high)) = threshold_range(name) else { return };
        if value.is_boolean() {
            return;
        }
        let number = value.as_f64().or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()));
        let Some(number) = number.filter(|n: &f64| n.is_finite()) else { return };
        if is_int && value.is_string() && number.fract() != 0.0 {
            return;
        }
        let number = if is_int { number.trunc() } else { number };
        if number < low || high.is_some_and(|top| number > top) {
            return;
        }
        let stored = if is_int { json!(number as i64) } else { json!(number) };
        self.thresholds.insert(name.to_string(), stored);
    }

    // Invalid values are ignored, as a typo in a config file must not break the agent's hooks.
    pub fn set(&mut self, key: &str, value: &Value) {
        if let Some(name) = key.strip_prefix("loopguard.").or(key.strip_prefix("spiral.")) {
            self.set_threshold(name, value);
            return;
        }
        let word = plain_text(value).trim().to_lowercase();
        let switch = |on_value: &str, aliases: bool, choices: &[&str]| -> Option<String> {
            let chosen = match word.as_str() {
                "on" | "true" | "yes" | "1" | "enable" | "enabled" => on_value.to_string(),
                "off" | "false" | "no" | "0" | "disable" | "disabled" => "off".to_string(),
                "guard" if aliases => "stop".to_string(),
                _ => plain_text(value).trim().to_string(),
            };
            choices.contains(&chosen.as_str()).then_some(chosen)
        };
        match canonical(key) {
            "loopguard_thresholds" => {
                for (name, item) in value.as_object().into_iter().flatten() {
                    self.set_threshold(name, item);
                }
            }
            "enabled" => self.enabled = value.as_bool().unwrap_or(matches!(word.as_str(), "1" | "true" | "yes" | "on")),
            "shadow" => self.shadow = value.as_bool().unwrap_or(matches!(word.as_str(), "1" | "true" | "yes" | "on")),
            "mode" if matches!(word.as_str(), "auto" | "interactive" | "headless") => self.mode = plain_text(value).trim().to_string(),
            "loopguard_aggr" if matches!(word.as_str(), "conservative" | "default" | "aggressive") => self.aggr = plain_text(value).trim().to_string(),
            "loopguard" => {
                if let Some(chosen) = switch("nudge", true, &["off", "nudge", "stop"]) {
                    self.loopguard = chosen;
                }
            }
            "repomap" => {
                if let Some(chosen) = switch("on", false, &["off", "on", "start"]) {
                    self.repomap = chosen;
                }
            }
            "loopguard_nudges" => {
                let number = value.as_i64().or_else(|| value.as_f64().map(|n| n as i64)).or_else(|| value.as_bool().map(i64::from)).or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()));
                if let Some(number) = number.filter(|n| *n >= 1) {
                    self.nudges = number;
                }
            }
            _ => {}
        }
    }

    fn apply_file(&mut self, path: &Path) {
        let Some(Value::Object(map)) = fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()) else { return };
        // Within a file a new name wins over its old one.
        for (key, value) in map.iter().filter(|(key, _)| canonical(key) != key.as_str()) {
            self.set(key, value);
        }
        for (key, value) in map.iter().filter(|(key, _)| canonical(key) == key.as_str()) {
            self.set(key, value);
        }
    }

    fn load(root: &Path, repo_dir: &Path) -> Self {
        let mut config = Config::new();
        config.apply_file(&root.join("config.json"));
        config.apply_file(&repo_dir.join("config.json"));
        for (variable, key) in [("TE_MODE", "mode"), ("TE_SHADOW", "shadow"), ("TE_SPIRAL", "loopguard"), ("TE_SPIRAL_AGGR", "loopguard_aggr"), ("TE_SPIRAL_NUDGES", "loopguard_nudges"), ("TE_LOOPGUARD", "loopguard"), ("TE_LOOPGUARD_AGGR", "loopguard_aggr"), ("TE_LOOPGUARD_NUDGES", "loopguard_nudges"), ("TE_REPOMAP", "repomap")] {
            if let Some(value) = env(variable) {
                config.set(key, &json!(value));
            }
        }
        for variable in ["TE_SPIRAL_THRESHOLDS", "TE_LOOPGUARD_THRESHOLDS"] {
            if let Some(value) = env(variable).and_then(|text| serde_json::from_str::<Value>(&text).ok()) {
                config.set("loopguard_thresholds", &value);
            }
        }
        config.enabled &= !truthy(env("TE_DISABLE"));

        config
    }
}

fn take_chars(text: &str, count: usize) -> String {
    text.chars().take(count).collect()
}

fn python_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64() != Some(0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => !items.is_empty(),
        Value::Object(map) => !map.is_empty(),
    }
}

pub fn response_text(response: &Value) -> String {
    let text = match response {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        Value::Array(items) => items.iter().map(|item| if item.is_object() { item.get("text").map_or(String::new(), plain_text) } else { plain_text(item) }).collect::<Vec<_>>().join(" "),
        Value::Object(map) => {
            let mut parts = Vec::new();
            for key in RESPONSE_KEYS {
                match map.get(key) {
                    Some(Value::String(text)) if !text.is_empty() => parts.push(text.clone()),
                    Some(Value::Array(items)) => parts.push(items.iter().filter(|item| item.is_object()).map(|item| item.get("text").map_or(String::new(), plain_text)).collect::<Vec<_>>().join(" ")),
                    _ => {}
                }
            }
            if parts.is_empty() {
                dumps(response, None)
            } else {
                parts.join("\n")
            }
        }
        other => plain_text(other),
    };

    take_chars(&text, RESULT_CAP)
}

fn first_exit(regex: &Regex, text: &str) -> Option<i64> {
    regex.captures(text).ok()??.get(1)?.as_str().parse().ok()
}

fn failure_response(payload: &Value) -> Option<Value> {
    if !payload.get("tool_response").unwrap_or(&Value::Null).is_null() || !payload.get("tool_result").unwrap_or(&Value::Null).is_null() {
        return None;
    }
    let error = payload.get("error").filter(|error| !error.is_null())?;
    let text = plain_text(error);
    let mut response = json!({"error": text, "result_type": "failure"});
    if let Some(code) = first_exit(&EXIT, &take_chars(&text, 400)) {
        response["exit_code"] = json!(code);
    }

    Some(response)
}

pub fn exit_code(response: &Value) -> Option<i64> {
    if let Value::Object(map) = response {
        for key in ["exit_code", "exitCode", "returncode", "status_code"] {
            if let Some(code) = map.get(key).and_then(Value::as_i64) {
                return Some(code);
            }
        }
        if let Some(code) = map.get("metadata").and_then(|meta| meta.get("exit_code")).and_then(Value::as_i64) {
            return Some(code);
        }
    }
    let text = match response {
        Value::String(text) => Some(text.as_str()),
        Value::Object(map) => ["output", "text_result_for_llm"].iter().find_map(|key| map.get(*key).and_then(Value::as_str)),
        _ => None,
    }?;
    if text.is_empty() {
        return None;
    }
    let count = text.chars().count();

    first_exit(&EXIT, &take_chars(text, 400)).or_else(|| first_exit(&COPILOT_EXIT, &text.chars().skip(count.saturating_sub(400)).collect::<String>()))
}

fn is_claude(payload: &Value) -> bool {
    if payload.get("turn_id").is_some() {
        return false;
    }
    let transcript = plain_text(payload.get("transcript_path").unwrap_or(&Value::Null));
    let transcript = if payload.get("transcript_path").is_none_or(Value::is_null) { String::new() } else { transcript };
    let slashed = transcript.replace('\\', "/");
    if slashed.rsplit('/').next().unwrap_or("").starts_with("rollout-") || slashed.contains("/.codex/") {
        return false;
    }
    if slashed.contains("/session-state/") && transcript.ends_with("events.jsonl") {
        return false;
    }

    !(transcript.is_empty() && env("COPILOT_PLUGIN_ROOT").is_some())
}

fn pin_name(session: &str) -> String {
    let clean: String = session.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' }).take(200).collect();

    format!("{clean}.json")
}

struct Repo {
    key: String,
    root: String,
}

// Only a pin whose repository directory exists: following a merge is the Python's.
fn pinned(root: &Path, session: &str) -> Option<Repo> {
    let path = root.join("sessions").join("claude").join(pin_name(session));
    let doc: Value = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    let key = doc.get("key")?.as_str().filter(|key| !key.is_empty())?.to_string();
    let repo_root = doc.get("root")?.as_str().filter(|text| !text.is_empty())?.to_string();

    root.join("repos").join(&key).is_dir().then_some(Repo { key, root: repo_root })
}

struct Hook {
    payload: Value,
    session: String,
    store: Store,
    config: Config,
    root: PathBuf,
    repo: Repo,
}

fn python_candidates() -> Vec<(String, Vec<&'static str>)> {
    let mut out = Vec::new();
    if let Some(chosen) = env("TE_PYTHON") {
        out.push((chosen, Vec::new()));
    }
    out.push(("python3".to_string(), Vec::new()));
    out.push(("python".to_string(), Vec::new()));
    if cfg!(windows) {
        out.push(("py".to_string(), vec!["-3"]));
    }

    out
}

// The first Python 3 that really runs: the Store alias on Windows is on PATH and runs nothing.
fn find_python() -> Option<(String, Vec<&'static str>)> {
    python_candidates().into_iter().find(|(program, args)| Command::new(program).args(args).args(["-c", "import sys; sys.exit(sys.version_info < (3, 10))"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|status| status.success()))
}

fn bootstrap() -> Option<PathBuf> {
    let root = env("CLAUDE_PLUGIN_ROOT").map(PathBuf::from).or_else(|| Some(std::env::current_exe().ok()?.parent()?.parent()?.to_path_buf()))?;

    Some(root.join("scripts").join("te_bootstrap.py"))
}

// What is not ported yet: the same hook, handed over whole.
fn run_python(raw: &[u8]) -> Option<Vec<u8>> {
    let (program, args) = find_python()?;
    let mut child = Command::new(program).args(args).arg(bootstrap()?).arg("hook").env("PYTHONUTF8", "1").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    child.stdin.take()?.write_all(raw).ok()?;
    let output = child.wait_with_output().ok()?;

    output.status.success().then_some(output.stdout)
}

#[cfg(windows)]
fn detach(command: &mut Command, breakaway: bool) {
    use std::os::windows::process::CommandExt;
    // CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW, and out of the agent's job when it allows that.
    command.creation_flags(0x0000_0200 | 0x0800_0000 | if breakaway { 0x0100_0000 } else { 0 });
}

#[cfg(unix)]
fn detach(command: &mut Command, _breakaway: bool) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

// The judge's own job stays in Python for now, in the background as before.
// shortcut: without the Python's WMI fallback, a job the agent's Windows job object kills simply never answers, and the guard's stale limit covers that.
fn spawn_judge(hook: &Hook, job: &str) -> Option<()> {
    if env("TE_JUDGE_NO_SPAWN").is_some() {
        return Some(());
    }
    let (program, args) = find_python()?;
    let script = bootstrap()?;
    fs::create_dir_all(&hook.store.base).ok()?;
    for breakaway in [true, false] {
        let log = OpenOptions::new().create(true).append(true).open(hook.store.base.join("judge.log")).ok()?;
        let mut command = Command::new(&program);
        command.args(&args).arg(&script).args(["cli", "judge", "--session", &hook.session, "--job", job, "--host", "claude", "--cwd", &hook.repo.root]);
        command.current_dir(&hook.repo.root).env("PYTHONUTF8", "1").stdin(Stdio::null()).stdout(log.try_clone().ok()?).stderr(log);
        detach(&mut command, breakaway);
        if command.spawn().is_ok() {
            return Some(());
        }
    }

    None
}

struct AsyncJudge<'a> {
    hook: &'a Hook,
    threshold: f64,
}

impl BackgroundJudge for AsyncJudge<'_> {
    fn poll(&mut self, guard: &TurnGuard) -> (bool, Option<Value>) {
        let Some(id) = guard.judge_pending.get("id").filter(|id| python_truthy(id)) else { return (false, None) };
        let result = self.hook.store.read(&format!("judge/{}.result.json", self.hook.session)).unwrap_or(Value::Null);
        if result.get("id") != Some(id) {
            return (false, None);
        }

        (true, result.get("verdict").filter(|verdict| python_truthy(verdict)).cloned())
    }

    fn start(&mut self, guard: &TurnGuard, digest: &str) -> Option<String> {
        let at = now();
        let job = format!("{}:{}:{}:{at:.3}", guard.turn_key, guard.segment, guard.judge_calls);
        let request = json!({"id": job, "session_id": self.hook.session, "turn_key": guard.turn_key, "segment": guard.segment, "digest": digest, "threshold": self.threshold, "at": at});
        self.hook.store.write(&format!("judge/{}.request.json", self.hook.session), &request).ok()?;
        spawn_judge(self.hook, &job);

        Some(job)
    }
}

struct NoVerdict;

impl BackgroundJudge for NoVerdict {
    fn poll(&mut self, _guard: &TurnGuard) -> (bool, Option<Value>) {
        (false, None)
    }

    fn start(&mut self, _guard: &TurnGuard, _digest: &str) -> Option<String> {
        None
    }
}

fn merge_context(event: &str, context: Option<&str>, notice: Option<&str>) -> Value {
    let mut out = Map::new();
    if let Some(text) = context {
        out.insert("hookSpecificOutput".to_string(), json!({"hookEventName": event, "additionalContext": text}));
    }
    if let Some(text) = notice {
        out.insert("systemMessage".to_string(), json!(text));
    }

    Value::Object(out)
}

fn relay(text: &str) -> String {
    format!("TokenEconomizer, a plugin the user runs, has a notice for them, and their main view does not show plugin messages. Put this line, word for word, in your next message to the user, and again in your final reply:\n\n{text}\n\nIt is for the user to act on; it does not change your task.")
}

impl Hook {
    fn field(&self, key: &str) -> &Value {
        self.payload.get(key).unwrap_or(&Value::Null)
    }

    fn text(&self, key: &str) -> String {
        match self.field(key) {
            Value::Null => String::new(),
            value if !python_truthy(value) => String::new(),
            value => plain_text(value),
        }
    }

    fn session_doc(&self) -> Value {
        match self.store.read(&format!("sessions/{}.json", self.session)) {
            Some(doc @ Value::Object(_)) => doc,
            _ => {
                let cwd = Some(self.text("cwd")).filter(|cwd| !cwd.is_empty()).or_else(|| env("CLAUDE_PROJECT_DIR")).or_else(|| std::env::current_dir().ok().map(|dir| dir.display().to_string())).unwrap_or_default();
                json!({"session_id": self.session, "host": "claude", "started": now(), "cwd": cwd, "turns": [], "nudges": 0, "aborts": 0, "judge_tokens": 0.0, "writer_tokens": 0.0})
            }
        }
    }

    fn save(&self, doc: &mut Value) -> std::io::Result<()> {
        let transcript = self.text("transcript_path");
        if !transcript.is_empty() {
            doc["transcript"] = json!(transcript);
        }
        doc["updated"] = json!(now());
        self.store.write(&format!("sessions/{}.json", self.session), doc)
    }

    fn log(&self, kind: &str, fields: Value) -> std::io::Result<()> {
        let mut record = fields.as_object().cloned().unwrap_or_default();
        record.insert("kind".to_string(), json!(kind));
        record.insert("at".to_string(), json!(now()));
        record.insert("session_id".to_string(), json!(self.session));
        record.insert("shadow".to_string(), json!(self.config.shadow));
        self.store.append("events.jsonl", &Value::Object(record))
    }

    fn tracker(&self) -> EnvelopeTracker {
        EnvelopeTracker::load(&self.store.read("envelope.json").unwrap_or(Value::Null), &self.config.aggr, &self.config.thresholds)
    }

    fn close_turn(&self, doc: &mut Value, stopped_normally: bool) -> std::io::Result<Option<Value>> {
        let data = doc.get("turn").cloned().unwrap_or(Value::Null);
        if !python_truthy(&data) {
            return Ok(None);
        }
        let guard = TurnGuard::from_value(&data);
        let summary = guard.summary(stopped_normally);
        if guard.cum_turns != 0 && stopped_normally {
            let lock = self.store.locked("envelope");
            let mut tracker = self.tracker();
            tracker.update(guard.cum_turns, guard.resolved_proxy(stopped_normally), guard.whole.snapshot());
            self.store.write("envelope.json", &tracker.to_saved())?;
            drop(lock);
        }
        let mut turns = doc.get("turns").and_then(Value::as_array).cloned().unwrap_or_default();
        let from = turns.len().saturating_sub(TURN_HISTORY - 1);
        turns.drain(..from);
        turns.push(summary.clone());
        doc["turns"] = Value::Array(turns);
        let add = |doc: &mut Value, key: &str, amount: f64| {
            let total = doc.get(key).and_then(Value::as_f64).unwrap_or(0.0) + amount;
            doc[key] = json!(total);
        };
        add(doc, "judge_tokens", guard.judge_tokens);
        add(doc, "writer_tokens", guard.writer_tokens);
        doc["turn"] = Value::Null;

        Ok(Some(summary))
    }

    fn user_prompt(&self) -> std::io::Result<Value> {
        if self.config.loopguard == "off" {
            return Ok(json!({}));
        }
        let mut doc = self.session_doc();
        self.close_turn(&mut doc, false)?;
        doc["turn"] = TurnGuard::new(&self.text("prompt_id")).to_value();
        self.save(&mut doc)?;

        Ok(json!({}))
    }

    fn stop(&self) -> std::io::Result<Value> {
        if self.config.loopguard == "off" || python_truthy(self.field("stop_hook_active")) {
            return Ok(json!({}));
        }
        let mut doc = self.session_doc();
        if let Some(summary) = self.close_turn(&mut doc, true)? {
            self.log("turn", summary)?;
        }
        self.save(&mut doc)?;

        Ok(json!({}))
    }

    fn is_headless(&self) -> bool {
        match self.config.mode.as_str() {
            "headless" => true,
            "interactive" => false,
            _ => match std::env::var("CLAUDE_CODE_SESSION_ATTENDED").unwrap_or_default().trim() {
                "0" => true,
                "1" => false,
                _ => std::env::var("CLAUDE_CODE_ENTRYPOINT").unwrap_or_default().starts_with("sdk"),
            },
        }
    }

    fn post_tool(&self) -> std::io::Result<Value> {
        if self.config.loopguard == "off" || python_truthy(self.field("agent_id")) {
            return Ok(json!({}));
        }
        let event = Some(self.text("hook_event_name")).filter(|name| !name.is_empty()).unwrap_or_else(|| "PostToolUse".to_string());
        let mut doc = self.session_doc();
        let key = self.text("prompt_id");
        let mut data = doc.get("turn").cloned().unwrap_or(Value::Null);
        let open_key = data.get("turn_key").and_then(Value::as_str).unwrap_or("").to_string();
        if python_truthy(&data) && !key.is_empty() && !open_key.is_empty() && open_key != key {
            self.close_turn(&mut doc, false)?;
            data = Value::Null;
        }
        let mut guard = if python_truthy(&data) { TurnGuard::from_value(&data) } else { TurnGuard::new(&key) };
        let name = plain_text(self.field("tool_name"));
        let name = if self.field("tool_name").is_null() { String::new() } else { name };
        let args = if self.field("tool_input").is_object() { self.field("tool_input").clone() } else { json!({}) };
        let response = failure_response(&self.payload).filter(python_truthy).unwrap_or_else(|| self.field("tool_response").clone());
        let tracker = self.tracker();
        let envelope = tracker.envelope();
        let mut thr = crate::loopguard::pinned_thresholds(&tracker, &self.config.thresholds);
        thr.insert("gate_over_envelope".to_string(), json!(false));
        let number = |thr: &Thresholds, key: &str| thr.get(key).and_then(Value::as_f64).unwrap_or(0.0);
        let gate = number(&thr, "gate_min_turns").max(number(&thr, "min_turns"));
        thr.insert("gate_min_turns".to_string(), json!(gate));
        let threshold = number(&thr, "judge_confidence");
        let is_paused = fs::read(self.root.join("hosts").join("claude").join("sidecall.json")).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok()).is_some_and(|pause| pause.get("at").is_some_and(python_truthy));
        let mut asynchronous = AsyncJudge { hook: self, threshold };
        let mut silent = NoVerdict;
        let judge = match (envelope, is_paused) {
            (None, _) => Judge::None,
            (_, true) => Judge::Background(&mut silent),
            _ => Judge::Background(&mut asynchronous),
        };
        let decision = guard.fold(&name, &args, &response_text(&response), exit_code(&response), envelope, &thr, judge, self.config.nudges);
        let mut out = json!({});
        if let Some((action, reason)) = decision {
            let short = reason.split(':').next().unwrap_or("");
            if action == "nudge" {
                let body = guard.nudge_body();
                guard.after_nudge();
                doc["nudges"] = json!(doc.get("nudges").and_then(Value::as_i64).unwrap_or(0) + 1);
                self.log("nudge", json!({"reason": reason, "tool_calls": guard.cum_turns, "envelope": envelope}))?;
                if !self.config.shadow {
                    out = merge_context(&event, Some(&body), Some(&format!("TokenEconomizer nudged this turn ({short}): the same failure kept recurring.")));
                }
            } else if action == "abort" && !guard.abort_delivered {
                guard.abort_delivered = true;
                doc["aborts"] = json!(doc.get("aborts").and_then(Value::as_i64).unwrap_or(0) + 1);
                self.log("abort", json!({"reason": reason, "tool_calls": guard.cum_turns, "envelope": envelope, "delivered": "notice"}))?;
                if !self.config.shadow {
                    let hint = if self.is_headless() { "Press Ctrl+C to end the run." } else { "Press Esc to interrupt it." };
                    let text = format!("TokenEconomizer: this turn still looks stuck after a redirect ({short}, {} tool calls). {hint}", guard.cum_turns);
                    out = merge_context(&event, Some(&relay(&text)), Some(&text));
                }
            }
        }
        doc["turn"] = guard.to_value();
        self.save(&mut doc)?;

        Ok(out)
    }

    fn handle(&self) -> std::io::Result<Value> {
        match self.text("hook_event_name").as_str() {
            "UserPromptSubmit" => self.user_prompt(),
            "PostToolUse" | "PostToolUseFailure" => self.post_tool(),
            "Stop" => self.stop(),
            _ => Ok(json!({})),
        }
    }
}

// The hook this program answers itself, or None for one that goes to the Python.
fn native_hook(payload: &Value) -> Option<Hook> {
    let event = payload.get("hook_event_name")?.as_str()?;
    if !matches!(event, "UserPromptSubmit" | "PostToolUse" | "PostToolUseFailure" | "Stop") || !is_claude(payload) {
        return None;
    }
    let session = plain_text(payload.get("session_id").filter(|id| python_truthy(id))?);
    let root = state_root();
    let repo = pinned(&root, &session)?;
    let repo_dir = root.join("repos").join(&repo.key);
    let config = Config::load(&root, &repo_dir);
    if event == "UserPromptSubmit" && config.repomap == "on" {
        return None;
    }

    Some(Hook { payload: payload.clone(), session, store: Store { base: repo_dir.join("claude") }, config, root, repo })
}

fn record_error(error: &str) {
    let root = state_root();
    let _ = fs::create_dir_all(&root);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(root.join("errors.log")) {
        let _ = writeln!(file, "--- at={:.0} pid={} (dam-economizer)\n{error}", now(), std::process::id());
    }
}

// A hook never fails the agent: whatever goes wrong, it answers `{}` and records why.
pub fn hook() {
    let mut raw = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut raw);
    let mut out = std::io::stdout().lock();
    if truthy(env("TE_SIDECALL")) || truthy(env("TE_DISABLE")) {
        let _ = writeln!(out, "{{}}");
        return;
    }
    let payload: Value = serde_json::from_slice(&raw).unwrap_or(Value::Null);
    let answer = match payload.is_object().then(|| native_hook(&payload)).flatten() {
        Some(hook) if !hook.config.enabled => json!({}),
        Some(hook) => hook.handle().unwrap_or_else(|error| {
            record_error(&error.to_string());
            json!({})
        }),
        None => {
            match run_python(&raw) {
                Some(answer) => {
                    let _ = out.write_all(&answer);
                }
                None => {
                    let _ = writeln!(out, "{{}}");
                }
            }
            return;
        }
    };
    let _ = writeln!(out, "{}", dumps(&answer, None));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_is_written_as_python_writes_it() {
        let value = json!({"b": [1, 2.5, "é😀"], "a": {"x": null}, "c": []});
        assert_eq!(dumps(&value, None), r#"{"a": {"x": null}, "b": [1, 2.5, "\u00e9\ud83d\ude00"], "c": []}"#);
        assert_eq!(dumps(&json!({"a": [1]}), Some(2)), "{\n  \"a\": [\n    1\n  ]\n}");
    }

    #[test]
    fn a_tool_response_reads_whatever_its_shape() {
        assert_eq!(response_text(&json!({"stdout": "out", "stderr": "err"})), "out\nerr");
        assert_eq!(response_text(&json!([{"text": "a"}, {"text": "b"}])), "a b");
        assert_eq!(response_text(&json!({"filePath": "é"})), r#"{"filePath": "\u00e9"}"#);
        assert_eq!(response_text(&Value::Null), "");
    }

    #[test]
    fn exit_codes_are_found_where_each_shape_keeps_them() {
        assert_eq!(exit_code(&json!({"exit_code": 3})), Some(3));
        assert_eq!(exit_code(&json!({"exit_code": true})), None);
        assert_eq!(exit_code(&json!({"stdout": "x"})), None);
        assert_eq!(exit_code(&json!("Exit code 2\nboom")), Some(2));
        let failed = failure_response(&json!({"error": "Exit code 7\nno"})).unwrap();
        assert_eq!(exit_code(&failed), Some(7));
    }

    #[test]
    fn settings_read_like_the_python() {
        let mut config = Config::new();
        config.set("spiral", &json!("guard"));
        assert_eq!(config.loopguard, "stop");
        config.set("loopguard", &json!(true));
        assert_eq!(config.loopguard, "nudge");
        config.set("loopguard", &json!("banana"));
        assert_eq!(config.loopguard, "nudge");
        config.set("loopguard.min_turns", &json!("7"));
        config.set("spiral.stagnation", &json!(2.0));
        assert_eq!(config.thresholds, *json!({"min_turns": 7}).as_object().unwrap());
        config.set("loopguard_nudges", &json!(0));
        assert_eq!(config.nudges, 1);
    }

    #[test]
    fn other_agents_go_to_the_python() {
        assert!(is_claude(&json!({"transcript_path": "C:\\Users\\x\\.claude\\a.jsonl"})));
        assert!(!is_claude(&json!({"turn_id": "t"})));
        assert!(!is_claude(&json!({"transcript_path": "/h/.codex/rollout-1.jsonl"})));
        assert!(!is_claude(&json!({"transcript_path": "/h/session-state/s/events.jsonl"})));
    }
}
