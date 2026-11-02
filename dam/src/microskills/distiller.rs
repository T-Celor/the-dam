// TokenEconomizer's micro-skills (Apache-2.0, Microsoft), rewritten from microskills/distiller.py.
use super::lifecycle::{edited_files, Lifecycle};
use super::observe::{repository_top, Vocabulary};
use super::{hit, rx, split, strip, tail, take};
use crate::economizer::{dumps, dumps_ordered, python_truthy};
use crate::trace::CONVERSATION;
use fancy_regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::LazyLock;

use super::Runner;

type Row = Map<String, Value>;

const PROPOSE_EVERY: i64 = 8;
const WINDOW: usize = 8;
const MAX_CANDS: usize = 4;
const MIN_CITE: usize = 2;
const MAX_EXTRACTORS: usize = 12;
const RAW_SAMPLE: usize = 3;
const ABSTAIN_GATE: f64 = 0.5;
const BLOB_CAP: usize = 20_000;

static PYTEST: LazyLock<Regex> = LazyLock::new(|| rx(r"-m\s+pytest|\bpytest\b|\bpy\.test\b"));
static REPRO: LazyLock<Regex> = LazyLock::new(|| rx(r"python[0-9.]*\s+-c|/tmp/\S*\.py|reproduce|repro"));
static BADARG: LazyLock<Regex> = LazyLock::new(|| rx(r"unrecognized arguments|no such option|usage:|error: argument"));
static OTHER_TESTS: LazyLock<Regex> = LazyLock::new(|| rx(r"runtests\.py|unittest|nosetests|tox"));
static IMPORT_ERROR: LazyLock<Regex> = LazyLock::new(|| rx(r"ImportError|ModuleNotFoundError|No module named"));
static CHECK: LazyLock<Regex> = LazyLock::new(|| {
    rx(r"\b(?:pytest|py\.test|tox|nox|unittest|runtests\.py|jest|vitest|mocha|rspec|phpunit|ctest|tsc|eslint|prettier|ruff|flake8|pylint|mypy|pyright|golangci-lint|cmake|ninja|bazel)\b|(?:^|[\s;&|(])make\b|\b(?:npm|yarn|pnpm|bun)\s+(?:run\s+)?(?:test|build|lint|check|typecheck)\b|\b(?:cargo|go|mvn|gradle|gradlew|dotnet|mix|swift)\s+(?:test|build|check|vet|clippy|compile|package|verify)\b")
});
static TEST: LazyLock<Regex> = LazyLock::new(|| {
    rx(r"\b(?:pytest|py\.test|tox|nox|unittest|runtests\.py|jest|vitest|mocha|rspec|phpunit|ctest)\b|\b(?:make|npm|yarn|pnpm|bun|cargo|go|mvn|gradle|gradlew|dotnet|mix|swift|rake)\s+(?:run\s+)?test\b")
});
static MISSING_CMD: LazyLock<Regex> = LazyLock::new(|| rx(r"(?i)command not found|: not found\b|is not recognized as"));
static MISSING_DEP: LazyLock<Regex> = LazyLock::new(|| {
    rx(r"(?i)ModuleNotFoundError|No module named|ImportError|Cannot find module|ERR_MODULE_NOT_FOUND|cannot find package|could not find (?:package|crate|module|gem)|unresolved import|cannot load such file|undefined reference to|fatal error: [^\n]*: No such file")
});
static BAD_USAGE: LazyLock<Regex> = LazyLock::new(|| rx(r"(?mi)^usage:|unrecognized arguments?|no such option|unknown (?:option|flag|command)|invalid option|unexpected argument|error: argument"));
static EXIT: LazyLock<Regex> = LazyLock::new(|| rx(r"(?i)\bexit(?:ed with)?(?: code| status)?[:=\s]+(\d+)"));
static FAILED_OUT: LazyLock<Regex> = LazyLock::new(|| rx(r"(?m)Traceback \(most recent call last\)|^\w*Error\b"));
static ERROR_LINE: LazyLock<Regex> = LazyLock::new(|| rx(r"(?i)\b(?:error|failed|failure|fatal|exception|traceback|cannot|not found|denied|refused|panic)\b"));
static LEADING_CD: LazyLock<Regex> = LazyLock::new(|| rx(r"^cd \.\s*(?:&&|;)\s*"));
static HEX: LazyLock<Regex> = LazyLock::new(|| rx(r"0x[0-9a-fA-F]+"));
static PATH: LazyLock<Regex> = LazyLock::new(|| rx(r#"(?<![\w.~])/[^\s'"`:,)]+"#));
static NUMBER: LazyLock<Regex> = LazyLock::new(|| rx(r"\b\d+\b"));
static SPACES: LazyLock<Regex> = LazyLock::new(|| rx(r"\s+"));
static TESTBED: LazyLock<Regex> = LazyLock::new(|| rx(r"/testbed/\S+"));
static PAYLOAD: LazyLock<Regex> = LazyLock::new(|| rx(r"(?s)\{.*\}"));

// A pattern a model wrote: None where Python's re would not compile it.
fn user_rx(pattern: Option<&Value>) -> Option<Regex> {
    Regex::new(pattern?.as_str().filter(|pattern| !pattern.is_empty())?).ok()
}

fn truthy(value: Option<&Value>) -> bool {
    value.is_some_and(python_truthy)
}

// bool(dg.get(key, dg.get(fallback))): the second only where the first is absent.
fn flag(dg: &Row, key: &str, fallback: &str) -> bool {
    truthy(dg.get(key).or_else(|| dg.get(fallback)))
}

// str.splitlines.
fn splitlines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let (mut start, mut chars) = (0, text.char_indices().peekable());
    while let Some((at, c)) = chars.next() {
        if !matches!(c, '\n' | '\r' | '\u{b}' | '\u{c}' | '\u{1c}'..='\u{1e}' | '\u{85}' | '\u{2028}' | '\u{2029}') {
            continue;
        }
        lines.push(&text[start..at]);
        start = at + c.len_utf8();
        if c == '\r' && chars.peek().is_some_and(|(_, next)| *next == '\n') {
            start += 1;
            chars.next();
        }
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }

    lines
}

// ---- reading the session ----------------------------------------------------------------------

type Call = (String, Value, String);

// Ordered (tool lowercased, args, output) calls. The output is the result text, or its JSON where the log gives blocks.
fn load_calls(path: &Path) -> Vec<Call> {
    let Ok(bytes) = std::fs::read(path) else { return Vec::new() };
    let content = String::from_utf8_lossy(&bytes);
    if ["\"item.completed\"", "\"type\": \"item.completed\"", "\"tool.execution_start\"", "\"tool.execution_complete\""].iter().any(|marker| content.contains(marker)) {
        return crate::trace::load_trajectory(path).into_iter().map(|call| (call.tool.unwrap_or_default().to_lowercase(), call.args, call.result)).collect();
    }
    let key = |id: Option<&Value>| id.map_or_else(|| "null".to_string(), Value::to_string);
    let (mut order, mut uses, mut results): (Vec<String>, Vec<(String, (String, Value))>, Vec<(String, String)>) = (Vec::new(), Vec::new(), Vec::new());
    for line in content.lines().map(strip).filter(|line| !line.is_empty()) {
        let Ok(event) = serde_json::from_str::<Value>(line) else { continue };
        let Some(blocks) = event.get("message").filter(|message| message.is_object()).and_then(|_| event.get("message")?.get("content")?.as_array()) else { continue };
        if !CONVERSATION.contains(&event.get("type").and_then(Value::as_str).unwrap_or("assistant")) {
            continue;
        }
        for block in blocks.iter().filter(|block| block.is_object()) {
            match block.get("type").and_then(Value::as_str) {
                Some("tool_use") => {
                    let id = key(block.get("id"));
                    if uses.iter().any(|(known, _)| *known == id) {
                        continue;
                    }
                    let input = block.get("input").filter(|input| python_truthy(input)).cloned().unwrap_or_else(|| json!({}));
                    uses.push((id.clone(), (block.get("name").and_then(Value::as_str).unwrap_or("").to_string(), input)));
                    order.push(id);
                }
                Some("tool_result") => {
                    let text = match block.get("content") {
                        Some(Value::String(text)) => text.clone(),
                        other => dumps_ordered(other.unwrap_or(&Value::Null), None),
                    };
                    let id = key(block.get("tool_use_id"));
                    match results.iter_mut().find(|(known, _)| *known == id) {
                        Some(slot) => slot.1 = text,
                        None => results.push((id, text)),
                    }
                }
                _ => {}
            }
        }
    }

    order
        .iter()
        .map(|id| {
            let (name, input) = uses.iter().find(|(known, _)| known == id).map(|(_, found)| found.clone()).unwrap_or_default();
            let result = results.iter().find(|(known, _)| known == id).map(|(_, text)| text.clone()).unwrap_or_default();
            (name.to_lowercase(), input, result)
        })
        .collect()
}

fn bash_cmds(calls: &[Call]) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (tool, args, result) in calls {
        if ["bash", "shell", "stop_bash"].contains(&tool.as_str()) {
            let command = ["command", "cmd"].iter().filter_map(|key| args.get(*key)).find(|value| python_truthy(value)).and_then(Value::as_str).unwrap_or("");
            out.push((command.to_string(), result.clone()));
        }
    }

    out
}

// Dedup command shapes: drop long path/arg tails so the proposer sees FORMS.
fn templates<S: AsRef<str>>(cmds: impl IntoIterator<Item = S>, cap: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for cmd in cmds {
        let cmd = strip(cmd.as_ref());
        if cmd.is_empty() {
            continue;
        }
        let head = take(&SPACES.replace_all(cmd, " "), 80);
        let head = TESTBED.replace_all(&head, "<path>").into_owned();
        if !out.contains(&head) {
            out.push(head);
        }
        if out.len() >= cap {
            break;
        }
    }

    out
}

// A command or error line with the repository's own paths made relative and the home directory written `~`.
fn relative(text: &str, roots: &[String]) -> String {
    let mut text = text.to_string();
    for root in roots.iter().filter(|root| !root.is_empty()) {
        text = text.replace(&format!("{root}/"), "").replace(root.as_str(), ".");
    }
    let text = LEADING_CD.replace(&text, "").into_owned();

    match home() {
        Some(home) if !home.is_empty() && home != "/" => text.replace(&home, "~"),
        _ => text,
    }
}

// os.path.expanduser("~").
fn home() -> Option<String> {
    let var = |name: &str| std::env::var(name).ok();
    if !cfg!(windows) {
        return var("HOME");
    }

    var("USERPROFILE").or_else(|| Some(format!("{}\\{}", var("HOMEDRIVE").unwrap_or_default(), var("HOMEPATH")?.trim_start_matches(['\\', '/']))))
}

// The first line of a failing output that names the failure, normalized so two sessions that fail the same way read the same.
fn error_signature(out: &str, roots: &[String]) -> String {
    for line in splitlines(out) {
        if hit(&ERROR_LINE, line) || hit(&MISSING_CMD, line) {
            let line = relative(strip(line), roots);
            let line = HEX.replace_all(&line, "<hex>");
            let line = PATH.replace_all(&line, "<path>");
            let line = NUMBER.replace_all(&line, "N");

            return take(&split(&line).join(" "), 120);
        }
    }

    String::new()
}

fn command_failed(out: &str) -> bool {
    if let Ok(Some(caps)) = EXIT.captures(out) {
        return caps[1].chars().any(|digit| digit != '0');
    }

    hit(&MISSING_CMD, out) || hit(&MISSING_DEP, out) || hit(&FAILED_OUT, out)
}

fn strings(items: Vec<String>) -> Value {
    json!(items)
}

// The digest of the experiments' reading (Python/git or Node vocabularies).
fn build_digest(calls: &[Call], iid: &str) -> Row {
    let cmds = bash_cmds(calls);
    let cmd_blob = cmds.iter().map(|(c, _)| c.as_str()).collect::<Vec<_>>().join("\n");
    let out_blob = cmds.iter().map(|(_, o)| o.as_str()).collect::<Vec<_>>().join("\n");
    let test_cmds: Vec<&str> = cmds.iter().map(|(c, _)| c.as_str()).filter(|c| hit(&PYTEST, c) || hit(&OTHER_TESTS, c)).collect();
    let repro = cmds.iter().any(|(c, _)| hit(&REPRO, c) && !hit(&PYTEST, c));
    let edited: Vec<(String, Value)> = calls.iter().map(|(tool, args, _)| (tool.clone(), args.clone())).collect();
    let mut dg = Row::new();
    dg.insert("iid".into(), json!(iid));
    dg.insert("resolved".into(), Value::Null);
    dg.insert("edited_files".into(), json!(edited_files(&edited, true)));
    dg.insert("n_cmds".into(), json!(cmds.len()));
    dg.insert("cmd_templates".into(), strings(templates(cmds.iter().map(|(c, _)| c), 12)));
    dg.insert("test_invocations".into(), strings(templates(test_cmds, 12).into_iter().take(6).collect()));
    dg.insert("repro_written".into(), json!(repro));
    dg.insert("importerror".into(), json!(hit(&IMPORT_ERROR, &out_blob)));
    dg.insert("badarg".into(), json!(hit(&BADARG, &out_blob)));
    dg.insert("_cmd_blob".into(), json!(cmd_blob));
    dg.insert("_out_blob".into(), json!(out_blob));

    dg
}

// The digest the plugin's proposer reads: every edited file whatever its language, the build/test/lint commands run, the commands that failed with a normalized error line, and flags that hold in any ecosystem.
fn build_generic_digest(calls: &[Call], path: &Path, iid: &str) -> Row {
    let cmds = bash_cmds(calls);
    let start = crate::trace::load_trajectory(path).into_iter().filter_map(|call| call.cwd).find(|cwd| !cwd.is_empty());
    let mut roots: Vec<String> = Vec::new();
    for root in [start.clone(), start.as_deref().and_then(repository_top)].into_iter().flatten().filter(|root| !root.is_empty()) {
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    // Python sorts a set here; two different roots of one length would come in either order.
    roots.sort_by_key(|root| std::cmp::Reverse(root.chars().count()));
    let out_blob = cmds.iter().map(|(_, o)| o.as_str()).collect::<Vec<_>>().join("\n");
    let (mut failed, mut seen): (Vec<Value>, Vec<(String, String)>) = (Vec::new(), Vec::new());
    for (cmd, out) in &cmds {
        if !command_failed(out) {
            continue;
        }
        let row = (templates([relative(cmd, &roots)], 1).into_iter().next().unwrap_or_default(), error_signature(out, &roots));
        if !seen.contains(&row) {
            failed.push(json!({"cmd": row.0, "error": row.1}));
            seen.push(row);
        }
    }
    let edited: Vec<(String, Value)> = calls.iter().map(|(tool, args, _)| (tool.clone(), args.clone())).collect();
    let mut dg = Row::new();
    dg.insert("iid".into(), json!(iid));
    dg.insert("resolved".into(), Value::Null);
    dg.insert("edited_files".into(), json!(edited_files(&edited, false)));
    dg.insert("n_cmds".into(), json!(cmds.len()));
    dg.insert("cmd_templates".into(), strings(templates(cmds.iter().map(|(c, _)| relative(c, &roots)), 12)));
    dg.insert("check_commands".into(), strings(templates(cmds.iter().filter(|(c, _)| hit(&CHECK, c)).map(|(c, _)| relative(c, &roots)), 12).into_iter().take(6).collect()));
    dg.insert("failed_commands".into(), Value::Array(failed.into_iter().take(6).collect()));
    dg.insert("missing_command".into(), json!(hit(&MISSING_CMD, &out_blob)));
    dg.insert("missing_dependency".into(), json!(hit(&MISSING_DEP, &out_blob)));
    dg.insert("bad_usage".into(), json!(hit(&BAD_USAGE, &out_blob)));
    dg.insert("_ran_tests".into(), json!(cmds.iter().any(|(c, _)| hit(&TEST, c))));
    dg.insert("_cmd_blob".into(), json!(cmds.iter().map(|(c, _)| c.as_str()).collect::<Vec<_>>().join("\n")));
    dg.insert("_out_blob".into(), json!(out_blob));

    dg
}

// ---- detectors ---------------------------------------------------------------------------------

// Each reads either digest: a fact the experiments' proposer named keeps a meaning under the generic digest, and the reverse.
fn predicate(name: &str, dg: &Row) -> Option<bool> {
    Some(match name {
        "repro_written" => truthy(dg.get("repro_written")),
        "importerror" => flag(dg, "importerror", "missing_dependency"),
        "badarg" => flag(dg, "badarg", "bad_usage"),
        "ran_tests" => truthy(dg.get("test_invocations")) || truthy(dg.get("_ran_tests")),
        "ran_checks" => truthy(dg.get("check_commands")) || truthy(dg.get("test_invocations")),
        "command_failed" => truthy(dg.get("failed_commands")),
        "missing_command" => truthy(dg.get("missing_command")),
        "missing_dependency" => flag(dg, "missing_dependency", "importerror"),
        "bad_usage" => flag(dg, "bad_usage", "badarg"),
        "edited" => truthy(dg.get("edited_files")),
        _ => return None,
    })
}

fn blob<'a>(dg: &'a Row, key: &str) -> &'a str {
    dg.get(key).and_then(Value::as_str).unwrap_or("")
}

// A fact's scope or `fired` may name an authored feature, then a built-in predicate, then a regex over the commands.
fn lookup(name: Option<&Value>, dg: &Row, features: &Row) -> bool {
    let Some(name) = name.and_then(Value::as_str) else { return false };
    if let Some(feature) = features.get(name) {
        return python_truthy(feature);
    }
    if let Some(known) = predicate(name, dg) {
        return known;
    }

    user_rx(Some(&json!(name))).is_some_and(|re| hit(&re, blob(dg, "_cmd_blob")))
}

fn verdict(detector: &Row, dg: &Row) -> (Option<&'static str>, bool, bool) {
    let no_features = Row::new();
    let features = dg.get("features").and_then(Value::as_object).unwrap_or(&no_features);
    let fired_name = detector.get("fired").filter(|fired| python_truthy(fired)).or_else(|| detector.get("scope"));
    if !lookup(detector.get("scope"), dg, features) {
        return (None, false, false);
    }
    let fired = lookup(fired_name, dg, features);
    let text = format!("{}\n{}", blob(dg, "_cmd_blob"), blob(dg, "_out_blob"));
    if user_rx(detector.get("contradict")).is_some_and(|re| hit(&re, &text)) {
        return (Some("contradict"), true, true);
    }
    if user_rx(detector.get("confirm")).is_some_and(|re| hit(&re, &text)) {
        return (Some("confirm"), fired, true);
    }

    (None, fired, true)
}

// A proposer-authored feature over raw calls: cmd_regex (bool), out_regex (bool), cmd_count (int).
struct Extractor {
    kind: String,
    rx: Option<Regex>,
    spec: Row,
}

impl Extractor {
    fn new(spec: &Row) -> Option<Self> {
        spec.get("name")?;
        let kind = spec.get("kind").map_or("cmd_regex", |kind| kind.as_str().unwrap_or("")).to_string();

        Some(Extractor { kind, rx: user_rx(spec.get("pattern")), spec: spec.clone() })
    }

    fn compute(&self, cmds: &[(String, String)]) -> Value {
        let Some(re) = &self.rx else { return if self.kind == "cmd_count" { json!(0) } else { json!(false) } };
        match self.kind.as_str() {
            "out_regex" => json!(cmds.iter().any(|(_, out)| hit(re, out))),
            "cmd_count" => json!(cmds.iter().filter(|(cmd, _)| hit(re, cmd)).count()),
            _ => json!(cmds.iter().any(|(cmd, _)| hit(re, cmd))),
        }
    }
}

// An authored payload with a validated lifecycle.
struct Fact {
    claim: String,
    injection: String,
    spec: Row,
    life: Lifecycle,
}

impl Fact {
    // None where the spec has no string id, claim or injection, or no detector object.
    fn new(spec: Row) -> Option<Self> {
        let id = spec.get("id")?.as_str()?.to_string();
        let claim = spec.get("claim").map_or(Some(id.as_str()), Value::as_str)?.to_string();
        let injection = spec.get("injection").map_or(Some(claim.as_str()), Value::as_str)?.to_string();
        if spec.get("detector").is_some_and(|detector| !detector.is_object()) {
            return None;
        }
        let high = truthy(spec.get("high_stakes"));

        Some(Fact { claim, injection, spec, life: Lifecycle::new(&id, high, true) })
    }

    fn detector(&self) -> Row {
        self.spec.get("detector").and_then(Value::as_object).cloned().unwrap_or_default()
    }

    // Fold one finished-session digest into this fact's lifecycle.
    fn observe(&mut self, now: i64, dg: &Row, files: &BTreeSet<String>) {
        let (verdict, fired, exercised) = verdict(&self.detector(), dg);
        self.life.update(now, verdict, fired, exercised, files);
    }

    fn to_dict(&self) -> Value {
        json!({"spec": self.spec, "life": self.life.to_dict()})
    }
}

fn files_of(dg: &Row) -> BTreeSet<String> {
    dg.get("edited_files").and_then(Value::as_array).into_iter().flatten().filter_map(|file| file.as_str().map(str::to_string)).collect()
}

// Everything about a note except detector.contradict (the append-only surface).
fn frozen_view(spec: &Row) -> (Value, Value, bool, String) {
    let mut detector = spec.get("detector").and_then(Value::as_object).cloned().unwrap_or_default();
    detector.retain(|key, _| key != "contradict");

    (spec.get("claim").cloned().unwrap_or(Value::Null), spec.get("injection").cloned().unwrap_or(Value::Null), truthy(spec.get("high_stakes")), dumps(&Value::Object(detector), None))
}

// The model's answer: its extractors and facts; None where Python would raise on the shape.
fn parse_payload(raw: &str) -> Option<(Vec<Row>, Vec<Row>)> {
    let Ok(Some(found)) = PAYLOAD.find(raw) else { return Some((Vec::new(), Vec::new())) };
    let Ok(Value::Object(payload)) = serde_json::from_str::<Value>(found.as_str()) else { return Some((Vec::new(), Vec::new())) };
    let listed = |key: &str| -> Option<Vec<Row>> {
        match payload.get(key) {
            None | Some(Value::String(_) | Value::Object(_)) => Some(Vec::new()),
            Some(Value::Array(items)) => Some(items.iter().filter_map(|item| item.as_object().cloned()).collect()),
            Some(_) => None,
        }
    };
    let text = |row: &Row, key: &str| row.get(key).is_some_and(|value| value.as_str().is_some_and(|text| !text.is_empty()));
    let extractors = listed("extractors")?.into_iter().filter(|e| text(e, "name") && text(e, "pattern")).collect();
    let facts = listed("facts")?.into_iter().filter(|f| text(f, "id") && f.get("detector").is_some_and(|detector| detector.is_object() && python_truthy(detector))).collect();

    Some((extractors, facts))
}

// Recover [(cmd, out)] from a digest's stashed blobs.
fn reparse_cmds(dg: &Row) -> Vec<(String, String)> {
    let outs: Vec<&str> = blob(dg, "_out_blob").split('\n').collect();

    blob(dg, "_cmd_blob").split('\n').enumerate().map(|(n, cmd)| (cmd.to_string(), outs.get(n).copied().unwrap_or("").to_string())).collect()
}

pub struct Distiller {
    repo: String,
    generic: bool,
    propose_every: i64,
    now: i64,
    facts: Vec<(String, Fact)>,
    window: Vec<Row>,
    pub opt_tokens: f64,
    last_propose: i64,
    extractors: Vec<(String, Extractor)>,
    raw_window: Vec<Vec<(String, String)>>,
}

impl Distiller {
    // None where Python's load would raise.
    pub fn load(repo: &str, mech: &Path, vocab: Vocabulary, propose_every: i64) -> Option<Self> {
        let mut this = Distiller { repo: repo.to_string(), generic: vocab.generic, propose_every: if propose_every > 0 { propose_every } else { PROPOSE_EVERY }, now: 0, facts: Vec::new(), window: Vec::new(), opt_tokens: 0.0, last_propose: -1, extractors: Vec::new(), raw_window: Vec::new() };
        let Some(state) = super::load_state(&super::state_path(mech, "manual_state", repo))? else { return Some(this) };
        this.now = state.get("now").and_then(Value::as_i64).unwrap_or(0);
        this.opt_tokens = state.get("opt_tokens").and_then(Value::as_f64).unwrap_or(0.0);
        this.last_propose = state.get("last_propose").and_then(Value::as_i64).unwrap_or(-1);
        for (key, spec) in state.get("extractors").and_then(Value::as_object).into_iter().flatten() {
            this.extractors.push((key.clone(), Extractor::new(spec.as_object()?)?));
        }
        for (key, fact) in state.get("facts").and_then(Value::as_object).into_iter().flatten() {
            let mut loaded = Fact::new(fact.get("spec")?.as_object()?.clone())?;
            loaded.life = Lifecycle::from_dict(fact.get("life")?)?;
            this.facts.push((key.clone(), loaded));
        }
        let window: Vec<Row> = state.get("window").and_then(Value::as_array).into_iter().flatten().filter_map(|dg| dg.as_object().cloned()).collect();
        this.window = window[window.len().saturating_sub(WINDOW)..].to_vec();
        this.raw_window = state.get("raw_cmds").and_then(Value::as_array).into_iter().flatten().map(|cmds| cmds.as_array().into_iter().flatten().filter_map(Value::as_str).map(|cmd| (cmd.to_string(), String::new())).collect()).collect();

        Some(this)
    }

    pub fn save(&self, mech: &Path) -> bool {
        // The window goes with the facts: a proposer reloaded for each session must see the recent sessions. Raw blobs are capped.
        let window: Vec<Value> = self.window[self.window.len().saturating_sub(WINDOW)..]
            .iter()
            .map(|dg| {
                let mut dg = dg.clone();
                dg.insert("_cmd_blob".into(), json!(tail(blob(&dg, "_cmd_blob"), BLOB_CAP)));
                dg.insert("_out_blob".into(), json!(tail(blob(&dg, "_out_blob"), 2 * BLOB_CAP)));
                Value::Object(dg)
            })
            .collect();
        let raw = &self.raw_window[self.raw_window.len().saturating_sub(WINDOW)..];
        let state = json!({
            "repo": self.repo, "now": self.now, "opt_tokens": self.opt_tokens, "last_propose": self.last_propose,
            "extractors": self.extractors.iter().map(|(key, e)| (key.clone(), Value::Object(e.spec.clone()))).collect::<Row>(),
            "facts": self.facts.iter().map(|(key, f)| (key.clone(), f.to_dict())).collect::<Row>(),
            "window": window,
            "raw_cmds": raw.iter().map(|cmds| cmds.iter().map(|(cmd, _)| cmd.clone()).collect::<Vec<_>>()).collect::<Vec<_>>(),
        });

        super::save_state(&super::state_path(mech, "manual_state", &self.repo), &state)
    }

    // Firing-gated manual for the NEXT session: empty if nothing qualifies yet.
    pub fn render(&self) -> String {
        let lines: Vec<String> = self.facts.iter().filter(|(_, f)| f.life.injectable()).map(|(_, f)| format!("- {}", f.injection)).collect();
        if lines.is_empty() {
            return String::new();
        }
        let name = self.repo.rsplit('/').next().unwrap_or("");
        let tail = if self.generic { "\u{2014} learned from earlier sessions in this repository" } else { "\u{2014} learned from prior tasks in this repo" };

        format!("# repo operating manual ({name}) {tail}\n{}\n", lines.join("\n"))
    }

    fn sensed(&self, cmd: &str) -> bool {
        if self.generic { hit(&CHECK, cmd) } else { hit(&PYTEST, cmd) || hit(&REPRO, cmd) }
    }

    fn covered(&self, cmd: &str) -> bool {
        self.sensed(cmd) || self.extractors.iter().any(|(_, e)| e.rx.as_ref().is_some_and(|re| hit(re, cmd)))
    }

    // Fraction of commands matched by SOME extractor or built-in sense.
    fn coverage(&self, cmds: &[(String, String)]) -> f64 {
        if cmds.is_empty() {
            return 1.0;
        }

        cmds.iter().filter(|(cmd, _)| self.covered(cmd)).count() as f64 / cmds.len() as f64
    }

    // Uncovered command templates from the raw window, for extractor authoring.
    fn abstaining_excerpts(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for cmds in &self.raw_window[self.raw_window.len().saturating_sub(WINDOW)..] {
            for (cmd, _) in cmds {
                if self.covered(cmd) {
                    continue;
                }
                let line = take(strip(&SPACES.replace_all(cmd, " ")), 100);
                if !line.is_empty() && !out.contains(&line) {
                    out.push(line);
                }
            }
        }
        out.truncate(RAW_SAMPLE * 4);

        out
    }

    fn proposer_prompt(&self, excerpts: &[String], external: &[String]) -> String {
        let view: Vec<Value> = self.window.iter().map(|dg| Value::Object(dg.iter().filter(|(key, _)| !key.starts_with('_')).map(|(key, value)| (key.clone(), value.clone())).collect())).collect();
        let mut existing: Vec<(&str, &str)> = self.facts.iter().map(|(id, f)| (id.as_str(), f.claim.as_str())).collect();
        existing.sort();
        let known = if self.generic {
            let lines: Vec<String> = existing.iter().map(|(id, claim)| if claim.is_empty() { format!("  - {id}") } else { format!("  - {id}: {claim}") }).collect();
            format!("\n\nExisting facts, as `id: claim` (do NOT restate or paraphrase one under a NEW id; re-emit an id with ONLY detector.contradict changed to revise its contradiction condition, else use a NEW id for a fact not listed here):\n{}", if lines.is_empty() { "(none)".to_string() } else { lines.join("\n") })
        } else {
            let ids: Vec<&str> = existing.iter().map(|(id, _)| *id).collect();
            format!("\n\nExisting fact ids (re-emit an id with ONLY detector.contradict changed to revise its contradiction condition, else use a NEW id):\n{}", if ids.is_empty() { "(none)".to_string() } else { ids.join(", ") })
        };
        let mut features: Vec<&str> = self.extractors.iter().map(|(name, _)| name.as_str()).collect();
        features.sort_unstable();
        let mut prompt = format!(
            "{}\n\nRepo: {}{known}\n\nExisting feature names:\n{}\n\nTask digests (chronological):\n{}",
            if self.generic { PROPOSER_SYSTEM_GENERIC } else { PROPOSER_SYSTEM },
            self.repo,
            if features.is_empty() { "(none)".to_string() } else { features.join(", ") },
            dumps_ordered(&Value::Array(view), Some(1))
        );
        if !excerpts.is_empty() {
            let lines: Vec<String> = excerpts.iter().map(|cmd| format!("  $ {cmd}")).collect();
            prompt.push_str(&format!("\n\nRAW command excerpts from recent tasks the current extractors did NOT cover (author extractors to reach these if they hide a durable fact):\n{}", lines.join("\n")));
        }
        if !external.is_empty() {
            let lines: Vec<String> = external.iter().map(|claim| format!("  - {claim}")).collect();
            prompt.push_str(&format!("\n\nFacts ALREADY injected before every task by a separate layer (do NOT restate or paraphrase them; author only facts beyond these):\n{}", lines.join("\n")));
        }
        prompt.push_str("\n\nReturn the JSON object {extractors, facts}:");

        prompt
    }

    // The proposer tick, the only paid step. None where the Python would raise.
    fn propose(&mut self, runner: &Runner, external: &[String]) -> Option<()> {
        if self.window.is_empty() {
            return Some(());
        }
        let coverage: Vec<f64> = self.raw_window[self.raw_window.len().saturating_sub(WINDOW)..].iter().map(|cmds| self.coverage(cmds)).collect();
        let abstaining = !coverage.is_empty() && coverage.iter().sum::<f64>() / (coverage.len() as f64) < ABSTAIN_GATE;
        let excerpts: Vec<String> = if abstaining { self.abstaining_excerpts().into_iter().take(RAW_SAMPLE).collect() } else { Vec::new() };
        let prompt = self.proposer_prompt(&excerpts, external);
        // No answer is no tick: the next session asks again.
        let Some((raw, effective)) = runner(&prompt, "agent") else { return Some(()) };
        if let Some(effective) = effective.as_f64() {
            self.opt_tokens += effective;
        }
        let (extractors, facts) = parse_payload(&raw)?;
        for spec in extractors {
            self.admit_extractor(&spec);
        }
        for spec in facts.into_iter().take(MAX_CANDS) {
            self.admit(spec);
        }
        self.last_propose = self.now;

        Some(())
    }

    fn admit_extractor(&mut self, spec: &Row) -> bool {
        let Some(name) = spec.get("name").and_then(Value::as_str) else { return false };
        if self.extractors.iter().any(|(known, _)| known == name) || self.extractors.len() >= MAX_EXTRACTORS {
            return false;
        }
        let Some(extractor) = Extractor::new(spec).filter(|e| e.rx.is_some()) else { return false };
        // Retro-featurize the window so a same-tick fact can reference it.
        for dg in &mut self.window {
            let value = extractor.compute(&reparse_cmds(dg));
            let features = dg.entry("features").or_insert_with(|| json!({}));
            if let Some(features) = features.as_object_mut() {
                features.insert(name.to_string(), value);
            }
        }
        self.extractors.push((name.to_string(), extractor));

        true
    }

    // The window cites whose confirm regex fires under the CURRENT features.
    fn ground(&self, spec: &Row) -> usize {
        let detector = spec.get("detector").and_then(Value::as_object).cloned().unwrap_or_default();
        let cites = spec.get("cite").and_then(Value::as_array).cloned().unwrap_or_default();

        cites.iter().filter(|iid| self.window.iter().rev().find(|dg| dg.get("iid") == Some(*iid)).is_some_and(|dg| verdict(&detector, dg).0 == Some("confirm"))).count()
    }

    // New id = append; existing id = contradict-only revise.
    fn admit(&mut self, mut spec: Row) -> bool {
        let Some(id) = spec.get("id").and_then(Value::as_str).map(str::to_string) else { return false };
        let Some(at) = self.facts.iter().position(|(key, _)| *key == id) else {
            if self.ground(&spec) < MIN_CITE {
                return false;
            }
            spec.entry("rev").or_insert(json!(1));
            // A fresh fact replayed over the window enters with real lifecycle state.
            let Some(mut fact) = Fact::new(spec) else { return false };
            for (n, dg) in self.window.iter().enumerate() {
                let now = self.now - self.window.len() as i64 + n as i64;
                fact.observe(now, dg, &files_of(dg));
            }
            self.facts.push((id, fact));

            return true;
        };
        let old = &mut self.facts[at].1;
        // ONLY the contradiction condition may be revised; everything else is frozen.
        if frozen_view(&spec) != frozen_view(&old.spec) {
            return false;
        }
        let contradict = |row: &Row| row.get("detector").and_then(Value::as_object).and_then(|detector| detector.get("contradict")).cloned().unwrap_or(Value::Null);
        let new = contradict(&spec);
        if python_truthy(&new) && user_rx(Some(&new)).is_none() {
            return false;
        }
        if new == contradict(&old.spec) {
            return false;
        }
        // Contradiction only ever retires: mutate in place, keep the earned lifecycle, apply going forward.
        let detector = old.spec.entry("detector").or_insert_with(|| json!({}));
        if let Some(detector) = detector.as_object_mut() {
            detector.insert("contradict".into(), new);
        }
        let rev = old.spec.get("rev").and_then(Value::as_i64).unwrap_or(1);
        old.spec.insert("rev".into(), json!(rev + 1));

        true
    }

    // Fold in the finished session, then advance the clock (which may trigger a paid tick). None where the Python would raise.
    pub fn update(&mut self, path: &Path, iid: &str, runner: &Runner, external: &[String]) -> Option<()> {
        let calls = load_calls(path);
        let cmds = bash_cmds(&calls);
        let mut dg = if self.generic { build_generic_digest(&calls, path, iid) } else { build_digest(&calls, iid) };
        let features: Row = self.extractors.iter().map(|(name, e)| (name.clone(), e.compute(&cmds))).collect();
        dg.insert("features".into(), Value::Object(features));
        self.raw_window.push(cmds);
        if self.raw_window.len() > WINDOW {
            self.raw_window.drain(..self.raw_window.len() - WINDOW);
        }
        let files = files_of(&dg);
        for (_, fact) in &mut self.facts {
            fact.observe(self.now, &dg, &files);
        }
        self.window.push(dg);
        if self.window.len() > WINDOW {
            self.window.drain(..self.window.len() - WINDOW);
        }
        self.now += 1;
        if self.now - self.last_propose >= self.propose_every {
            self.propose(runner, external)?;
        }

        Some(())
    }

    pub fn debug_row(&self) -> Row {
        let mut row = Row::new();
        for (key, f) in &self.facts {
            row.insert(key.clone(), json!({"state": f.life.state, "fire": format!("{}/{}", f.life.fire_num, f.life.fire_den), "rate": super::round2(f.life.firing_rate()), "inject": f.life.injectable(), "claim": take(&f.claim, 60)}));
        }
        let mut names: Vec<&String> = self.extractors.iter().map(|(name, _)| name).collect();
        names.sort();
        row.insert("_extractors".into(), json!(names));
        for (key, f) in &self.facts {
            if let Some(entry) = row.get_mut(key).and_then(Value::as_object_mut) {
                entry.insert("rev".into(), f.spec.get("rev").cloned().unwrap_or(json!(1)));
            }
        }

        row
    }
}

const PROPOSER_SYSTEM: &str = r####"You mine a DURABLE, REPO-SPECIFIC operating manual for a coding agent from a window of finished-task digests in ONE repository. Each digest lists edited files, command templates, test invocations, and boolean flags — NOT raw logs.

Emit facts that a FUTURE task in this repo would want stated up front to avoid wasted commands: the canonical test invocation, whether a repro is redundant, a required env var, a build/reinstall step that's needed (or NOT needed), a lint/format hook that reformats, etc. Do NOT emit task-specific fixes or which lines to change — only durable interface facts true across tasks.

For EACH fact return an object with:
  id          short kebab slug
  claim       one sentence, what is true about this repo
  injection   the exact line to inject into the agent prefix. State it as a DECLARATIVE fact about the repo (where things are, what the canonical command is). NEVER phrase it as a prohibition on exploration or tool use — do NOT tell the agent to avoid `find`/`grep`/searching/reading, or to skip discovering files. A prior that suppresses exploration silently breaks later tasks on a growing codebase where the agent must discover accreted files. State what is true; never forbid a tool.
  high_stakes true if a wrong belief would break the run (test cmd, build step)
  detector    {scope, confirm, contradict?, fired?} where scope is a python regex over the command string (or one of: repro_written, importerror, badarg, ran_tests) selecting tasks where the fact is RELEVANT; confirm is a regex whose match in commands+output means the fact HELD this task; contradict (optional) a regex meaning the fact was FALSE this task; fired (optional) a predicate/regex meaning the waste the fact prevents actually occurred.
  cite        list of >=2 iids from the window where confirm matches (grounding)

Return ONLY a JSON list of <=4 such objects. Be conservative: a fact you cannot ground with a working detector + 2 citations is worse than no fact.

ADDITIONALLY: if a durable repo fact would need evidence NOT present in the digest flags, first author an EXTRACTOR that surfaces it, then reference the extractor's feature name as your detector scope/fired. Extractors are cheap host-run predicates over raw commands/outputs.
Return a JSON OBJECT: {"extractors": [{name, kind, pattern}], "facts": [...]}
  extractor.kind in {cmd_regex, out_regex, cmd_count}; pattern is a python regex.
  A fact's detector.scope/fired may name any extractor you define here or a built-in (repro_written, importerror, badarg, ran_tests).

REVISION: the CONTRADICTION condition of an existing note may be corrected — and nothing else. Re-emit a note with the SAME id and the SAME claim/injection/high_stakes and the SAME detector.scope/detector.confirm/detector.fired, changing ONLY detector.contradict, to fix or tighten the regex that marks the fact FALSE this task. The note KEEPS its earned firing history and the new condition applies going forward. Re-emitting an id while changing any other field is rejected; extractors remain append-only."####;

const PROPOSER_SYSTEM_GENERIC: &str = r####"You mine a DURABLE, REPO-SPECIFIC operating manual for a coding agent from a window of finished-session digests in ONE repository, whatever its language and toolchain. Each digest lists the files edited, command templates (paths relative to the repository), the build, test and lint commands run, the commands that failed with a normalized error line, and boolean flags — NOT raw logs or file contents. (Each session is called a task below.)

Emit facts that a FUTURE session in this repository would want stated up front to avoid wasted commands: how to build, run or check it here and the exact invocation that works; the toolchain (interpreter, package manager, compiler, version manager) and how it is reached; a setup, install, code-generation or rebuild step that is needed (or NOT needed); an environment variable, service or working directory a command needs; where generated, configured or vendored files live; a command that fails here and the variant that works; a hook or formatter that rewrites files. Tests are one such topic, not the focus. Do NOT emit task-specific fixes or which lines to change — only durable facts true across sessions.

For EACH fact return an object with:
  id          short kebab slug
  claim       one sentence, what is true about this repo
  injection   the exact line to inject into the agent prefix. State it as a DECLARATIVE fact about the repo (where things are, what the canonical command is). NEVER phrase it as a prohibition on exploration or tool use — do NOT tell the agent to avoid `find`/`grep`/searching/reading, or to skip discovering files. A prior that suppresses exploration silently breaks later tasks on a growing codebase where the agent must discover accreted files. State what is true; never forbid a tool.
  high_stakes true if a wrong belief would break the run (test cmd, build step)
  detector    {scope, confirm, contradict?, fired?} where scope is a python regex over the command string (or one of: ran_checks, ran_tests, command_failed, missing_command, missing_dependency, bad_usage, edited) selecting tasks where the fact is RELEVANT; confirm is a regex whose match in commands+output means the fact HELD this task; contradict (optional) a regex meaning the fact was FALSE this task; fired (optional) a predicate/regex meaning the waste the fact prevents actually occurred.
  cite        list of >=2 iids from the window where confirm matches (grounding)

Return ONLY a JSON list of <=4 such objects. Be conservative: a fact you cannot ground with a working detector + 2 citations is worse than no fact.

ADDITIONALLY: if a durable repo fact would need evidence NOT present in the digest flags, first author an EXTRACTOR that surfaces it, then reference the extractor's feature name as your detector scope/fired. Extractors are cheap host-run predicates over raw commands/outputs.
Return a JSON OBJECT: {"extractors": [{name, kind, pattern}], "facts": [...]}
  extractor.kind in {cmd_regex, out_regex, cmd_count}; pattern is a python regex.
  A fact's detector.scope/fired may name any extractor you define here or a built-in (ran_checks, ran_tests, command_failed, missing_command, missing_dependency, bad_usage, edited).

REVISION: the CONTRADICTION condition of an existing note may be corrected — and nothing else. Re-emit a note with the SAME id and the SAME claim/injection/high_stakes and the SAME detector.scope/detector.confirm/detector.fired, changing ONLY detector.contradict, to fix or tighten the regex that marks the fact FALSE this task. The note KEEPS its earned firing history and the new condition applies going forward. Re-emitting an id while changing any other field is rejected; extractors remain append-only."####;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_answer_is_read_from_the_first_brace_to_the_last() {
        let (extractors, facts) = parse_payload(r#"ok {"extractors": [{"name": "n", "pattern": "p"}], "facts": [{"id": "a", "detector": {"scope": "x"}}, {"id": "b"}]} done"#).expect("well formed");
        assert_eq!((extractors.len(), facts.len()), (1, 1));
        assert!(parse_payload("no json").is_some_and(|(e, f)| e.is_empty() && f.is_empty()));
        // A null list makes Python's loop raise.
        assert!(parse_payload(r#"{"extractors": null}"#).is_none());
    }

    #[test]
    fn lines_split_as_python_splits_them() {
        assert_eq!(splitlines("a\r\nb\u{85}c\n"), ["a", "b", "c"]);
        assert_eq!(splitlines("\nx"), ["", "x"]);
    }

    #[test]
    fn an_error_line_is_masked_to_its_shape() {
        assert_eq!(error_signature("ok\nError: cannot open /tmp/x.py at 0x1f line 12", &[]), "Error: cannot open <path> at <hex> line N");
    }
}
