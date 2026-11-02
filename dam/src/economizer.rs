// TokenEconomizer's hooks for Claude Code (Apache-2.0, Microsoft), rewritten from plugin/hook.py,
// controller.py, store.py, config.py, hosts.py and judge.py. Every Claude Code event is answered here, on
// the same state files; another agent's payload is answered `{}`.
use crate::config::plain_text;
use crate::context::Ctx;
use std::process::Command;
use crate::loopguard::{BackgroundJudge, EnvelopeTracker, Judge, Thresholds, TurnGuard};
use fancy_regex::Regex;
use serde_json::{json, Map, Value};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use std::sync::LazyLock;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const RESULT_CAP: usize = 20_000;
const TURN_HISTORY: usize = 200;
const LOCK_WAIT: Duration = Duration::from_secs(10);
const RESPONSE_KEYS: [&str; 10] = ["stdout", "stderr", "output", "aggregated_output", "formatted_output", "content", "result", "error", "message", "text_result_for_llm"];

static EXIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?im)^\s*(?:exit code|process exited with code)[:\s]+(-?\d+)").expect("fixed"));
static COPILOT_EXIT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<shellId: [^>]*?completed with exit code (-?\d+)>").expect("fixed"));

pub(crate) fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

fn truthy(value: Option<String>) -> bool {
    matches!(value.unwrap_or_default().trim().to_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

pub(crate) fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |elapsed| elapsed.as_secs_f64())
}

pub(crate) fn state_root() -> PathBuf {
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

fn python_json(value: &Value, indent: Option<usize>, depth: usize, out: &mut String, sort: bool) {
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
                python_json(item, indent, depth + 1, out, sort);
            }
            breakline(out, depth);
            out.push(']');
        }
        Value::Object(map) if map.is_empty() => out.push_str("{}"),
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            if sort {
                keys.sort_by(|a, b| a.chars().cmp(b.chars()));
            }
            out.push('{');
            for (n, key) in keys.iter().enumerate() {
                if n > 0 {
                    out.push_str(comma);
                }
                breakline(out, depth + 1);
                python_string(key, out);
                out.push_str(": ");
                python_json(&map[key.as_str()], indent, depth + 1, out, sort);
            }
            breakline(out, depth);
            out.push('}');
        }
    }
}

pub(crate) fn dumps(value: &Value, indent: Option<usize>) -> String {
    let mut out = String::new();
    python_json(value, indent, 0, &mut out, true);

    out
}

// json.dumps without sort_keys: the order the keys were inserted in.
pub(crate) fn dumps_ordered(value: &Value, indent: Option<usize>) -> String {
    let mut out = String::new();
    python_json(value, indent, 0, &mut out, false);

    out
}

pub struct Store {
    pub base: PathBuf,
}

impl Store {
    pub(crate) fn read(&self, name: &str) -> Option<Value> {
        serde_json::from_slice(&fs::read(self.base.join(name)).ok()?).ok()
    }

    pub(crate) fn read_lines(&self, name: &str) -> Vec<Value> {
        crate::trace::lines(&self.base.join(name)).iter().filter_map(|line| serde_json::from_str(line).ok()).collect()
    }

    // Through a temporary file, so that a reader never sees half a document.
    pub(crate) fn write(&self, name: &str, value: &Value) -> std::io::Result<()> {
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

    pub(crate) fn append(&self, name: &str, record: &Value) -> std::io::Result<()> {
        let target = self.base.join(name);
        if let Some(dir) = target.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut file = OpenOptions::new().create(true).append(true).open(target)?;

        writeln!(file, "{}", dumps(record, None))
    }

    // The same lock file as the Python's: a stuck holder costs at most the wait, then the work goes on.
    pub(crate) fn locked(&self, name: &str) -> Option<File> {
        self.locked_within(name, LOCK_WAIT)
    }

    pub(crate) fn locked_within(&self, name: &str, wait: Duration) -> Option<File> {
        fs::create_dir_all(&self.base).ok()?;
        let file = OpenOptions::new().create(true).read(true).append(true).open(self.base.join(format!(".{name}.lock"))).ok()?;
        let deadline = Instant::now() + wait;
        while file.try_lock().is_err() {
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        Some(file)
    }
}

fn take_chars(text: &str, count: usize) -> String {
    text.chars().take(count).collect()
}

pub(crate) fn python_truthy(value: &Value) -> bool {
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

pub(crate) fn pin_name(session: &str) -> String {
    let clean: String = session.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' }).take(200).collect();

    format!("{clean}.json")
}

pub struct Hook {
    payload: Value,
    session: String,
    ctx: Ctx,
}

impl std::ops::Deref for Hook {
    type Target = Ctx;

    fn deref(&self) -> &Ctx {
        &self.ctx
    }
}

#[cfg(windows)]
pub(crate) fn detach(command: &mut Command, breakaway: bool) {
    use std::os::windows::process::CommandExt;
    // CREATE_NEW_PROCESS_GROUP | CREATE_NO_WINDOW, and out of the agent's job when it allows that.
    command.creation_flags(0x0000_0200 | 0x0800_0000 | if breakaway { 0x0100_0000 } else { 0 });
}

#[cfg(unix)]
pub(crate) fn detach(command: &mut Command, _breakaway: bool) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
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
        crate::judge::spawn_job(self.hook, &self.hook.session, &job);

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

    fn cwd(&self) -> String {
        cwd_of(&self.payload)
    }

    fn session_doc(&self) -> Value {
        match self.store.read(&format!("sessions/{}.json", self.session)) {
            Some(doc @ Value::Object(_)) => doc,
            _ => json!({"session_id": self.session, "host": "claude", "started": now(), "cwd": self.cwd(), "turns": [], "nudges": 0, "aborts": 0, "judge_tokens": 0.0, "writer_tokens": 0.0}),
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

    fn session_end(&self) -> std::io::Result<Value> {
        // An end with no start and no session log is no session to learn from.
        let transcript = self.text("transcript_path");
        if !self.store.base.join("sessions").join(format!("{}.json", self.session)).exists() && (transcript.is_empty() || !Path::new(&transcript).exists()) {
            return Ok(json!({}));
        }
        let mut doc = self.session_doc();
        self.close_turn(&mut doc, false)?;
        doc["ended"] = json!(now());
        doc["end_reason"] = self.field("reason").clone();
        self.save(&mut doc)?;
        crate::fold::spawn_fold(self, std::slice::from_ref(&self.session));

        Ok(json!({}))
    }

    // The deny list's lines at session start, each said once: what `auto` removed from this checkout's sessions,
    // and a proposal still waiting (under `propose`, or when `auto` could not write the settings file).
    fn toolprune_notices(&self, seen: &mut Map<String, Value>) -> Vec<Value> {
        let (command, unit) = ("/tokeneconomizer:toolprune", "tool");
        let waiting = self.store.read("untool.json").unwrap_or(Value::Null);
        let mut mine = crate::denylist::applied_for(&self.store, &self.repo.root);
        mine.sort();
        let few = |items: &[String]| format!("{}{}", items.iter().take(4).cloned().collect::<Vec<_>>().join(", "), if items.len() > 4 { "..." } else { "" });
        let auto = self.config.text("toolprune") == "auto";
        let mut out = Vec::new();
        if auto && seen.get("untool_applied").and_then(Value::as_str).unwrap_or("") != mine.join(",") {
            if !mine.is_empty() {
                out.push(json!({"systemMessage": format!("TokenEconomizer removed {} {unit}(s) this repository's sessions never used ({}), from this session on. Review with `{command}`; `{command} restore` gives them back.", mine.len(), few(&mine))}));
            }
            seen.insert("untool_applied".to_string(), json!(mine.join(",")));
        }
        // What this checkout already applied, or gave back, is no news here.
        let kept = crate::denylist::kept_for(&self.store, &self.repo.root);
        let refused = waiting.get("auto_apply_refused").filter(|why| auto && python_truthy(why));
        let proposal: Vec<String> = if auto && refused.is_none() {
            Vec::new()
        } else {
            waiting.get("proposal").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).filter(|tool| !mine.iter().any(|item| item == tool) && !kept.iter().any(|item| item == tool)).map(str::to_string).collect()
        };
        if !proposal.is_empty() && seen.get("untool").and_then(Value::as_str) != Some(&proposal.join(",")) {
            let how = match refused {
                Some(why) => format!("they could not be applied ({}), and `{command} apply` tries again.", plain_text(why)),
                None => format!("nothing changes until you run `{command} apply`."),
            };
            out.push(json!({"systemMessage": format!("TokenEconomizer: {} {unit}(s) went unused in this repository's recent sessions ({}). Review with `{command}`; {how}", proposal.len(), few(&proposal))}));
            seen.insert("untool".to_string(), json!(proposal.join(",")));
        }

        out
    }

    fn session_start(&self) -> std::io::Result<Value> {
        let source = Some(self.text("source")).filter(|source| !source.is_empty()).unwrap_or_else(|| "startup".to_string());
        crate::session::ensure_launcher();
        let mut doc = self.session_doc();
        doc["source"] = json!(source);
        if doc.get("cwd").is_none() {
            doc["cwd"] = json!(self.cwd());
        }
        let mut seen = match self.store.read("notices.json") {
            Some(Value::Object(seen)) => seen,
            _ => Map::new(),
        };
        let seen_before = seen.clone();
        let mut outputs: Vec<Value> = Vec::new();

        if self.config.text("microskills") != "off" {
            let injection = self.store.read("injection.json").unwrap_or(Value::Null);
            let text = injection.get("rendered").and_then(Value::as_str).unwrap_or("");
            // A resumed conversation replays what the first start added; a cleared or compacted one has lost it.
            let fresh = matches!(source.as_str(), "startup" | "new" | "clear" | "compact");
            if !text.is_empty() && fresh {
                let facts = text.lines().filter(|line| line.starts_with("- ")).count();
                self.log("inject", json!({"chars": text.chars().count(), "facts": facts, "source": source}))?;
                if !self.config.shadow {
                    outputs.push(merge_context("SessionStart", Some(text), None));
                    doc["injected_chars"] = json!(text.chars().count());
                    doc["injected_text"] = json!(text);
                    if let Some(ids) = injection.get("rendered_ids") {
                        doc["injected_ids"] = ids.clone();
                    }
                    let digest = sha1_smol::Sha1::from(text).digest().to_string();
                    if seen.get("injection").and_then(Value::as_str) != Some(&digest) {
                        outputs.push(merge_context("", None, Some(&format!("TokenEconomizer: added {facts} learned fact(s) about {} to this session's context (review with `/tokeneconomizer:microskills`).", self.repo.label))));
                        seen.insert("injection".to_string(), json!(digest));
                    }
                }
            }
        }

        if self.config.text("toolprune") != "off" && !self.config.shadow {
            outputs.extend(self.toolprune_notices(&mut seen));
        }

        let calls_a_model = self.config.text("microskills").contains("distiller") || self.config.loopguard != "off";
        if let Some(pause) = crate::sidecall::paused(&self.root).filter(|_| calls_a_model && !self.config.shadow) {
            if seen.get("sidecall_paused") != pause.get("at") {
                outputs.push(merge_context("", None, Some("TokenEconomizer: the plugin's own model calls go through `claude -p`, which answered that it is not signed in, so they are paused; facts that need no model call are still learned, and the loop guard redirects only at its backstop. `/tokeneconomizer:config set microskills=on` (or `loopguard=on`) tries again.")));
                seen.insert("sidecall_paused".to_string(), pause["at"].clone());
            }
        }

        if seen != seen_before {
            self.store.write("notices.json", &Value::Object(seen))?;
        }
        self.save(&mut doc)?;
        crate::session::note_repo(self)?;
        crate::session::prune_pins(self);
        crate::fold::spawn_fold(self, &crate::fold::pending_sessions(self, &[&self.session]));

        // Contexts and notices concatenate, as the Python's merge_outputs does.
        let notices: Vec<&str> = outputs.iter().filter_map(|output| output.get("systemMessage").and_then(Value::as_str)).collect();
        let mut answer = Map::new();
        if let Some(context) = outputs.iter().find_map(|output| output.get("hookSpecificOutput")) {
            answer.insert("hookSpecificOutput".to_string(), context.clone());
        }
        if !notices.is_empty() {
            answer.insert("systemMessage".to_string(), json!(notices.join("\n")));
        }

        Ok(Value::Object(answer))
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
        let is_paused = crate::sidecall::paused(&self.root).is_some();
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
            "SessionStart" => self.session_start(),
            "UserPromptSubmit" => self.user_prompt(),
            "PostToolUse" | "PostToolUseFailure" => self.post_tool(),
            "Stop" => self.stop(),
            "SessionEnd" => self.session_end(),
            _ => Ok(json!({})),
        }
    }
}

// The directory the agent is in: the hook's own, else the project's, else this process's.
fn cwd_of(payload: &Value) -> String {
    let own = payload.get("cwd").filter(|cwd| python_truthy(cwd)).map(plain_text);

    own.or_else(|| env("CLAUDE_PROJECT_DIR")).or_else(|| std::env::current_dir().ok().map(|dir| dir.display().to_string())).unwrap_or_default()
}

// The hook for a Claude Code payload that names a session; the session is pinned to its repository by the first one.
fn native_hook(payload: &Value) -> Option<Hook> {
    if !is_claude(payload) {
        return None;
    }
    let session = plain_text(payload.get("session_id").filter(|id| python_truthy(id))?);
    let cwd = cwd_of(payload);
    let (ctx, unpinned) = crate::session::ctx_for(&session, Path::new(&cwd));
    let hook = Hook { payload: payload.clone(), session, ctx };
    // A disabled plugin writes nothing, not even a pin.
    if hook.config.enabled && unpinned {
        crate::session::pin(&hook.ctx, &hook.session, &cwd).ok()?;
    }

    Some(hook)
}

fn record_error(error: &str) {
    crate::session::append_error(&format!("--- {} pid={} (dam-economizer)\n{error}", crate::session::iso(now()), std::process::id()));
}

// A hook never fails the agent: whatever goes wrong, it answers `{}` and records why.
pub fn hook() {
    // The release build aborts on a panic: answer first, so that a bug never fails the agent.
    std::panic::set_hook(Box::new(|info| {
        record_error(&info.to_string());
        println!("{{}}");
        std::process::exit(0);
    }));
    let mut raw = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut raw);
    let mut out = std::io::stdout().lock();
    let payload: Value = serde_json::from_slice(&raw).unwrap_or(Value::Null);
    let answer = if truthy(env("TE_SIDECALL")) || truthy(env("TE_DISABLE")) || !payload.is_object() {
        json!({})
    } else {
        match native_hook(&payload) {
            Some(hook) if !hook.config.enabled => json!({}),
            Some(hook) => hook.handle().unwrap_or_else(|error| {
                record_error(&error.to_string());
                json!({})
            }),
            None => json!({}),
        }
    };
    let _ = writeln!(out, "{}", dumps(&answer, None));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

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
