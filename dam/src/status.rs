// `te status` (Apache-2.0, Microsoft), rewritten from plugin/cli.py, accounting.py and denylist.py.
// It only reads: carrying state over from earlier keys stays with the session hooks.
use crate::config::Config;
use crate::economizer::{pin_name, state_root, Store};
use crate::loopguard::EnvelopeTracker;
use crate::paths::{abspath, realpath, repo_identity, RepoId};
use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

const OVERHEAD_KEYS: [&str; 6] = ["optimizer_tokens", "untool_classify_tokens", "untool_judge_tokens", "codex_untool_adapt_tokens", "spiral_judge_tokens", "spiral_nudge_writer_tokens"];
const HOSTS: [&str; 5] = ["claude", "codex", "copilot", "opencode", "pi"];

// A sum that stays a whole number until a fraction joins it, as Python's does.
#[derive(Default)]
struct Sum {
    whole: i64,
    fraction: f64,
    is_float: bool,
}

impl Sum {
    fn add(&mut self, value: Option<&Value>) {
        match value {
            Some(Value::Number(number)) => match number.as_i64() {
                Some(whole) => self.whole += whole,
                None => {
                    self.fraction += number.as_f64().unwrap_or(0.0);
                    self.is_float = true;
                }
            },
            Some(Value::Bool(true)) => self.whole += 1,
            _ => {}
        }
    }

    fn total(&self) -> f64 {
        self.whole as f64 + self.fraction
    }

    fn value(&self) -> Value {
        if self.is_float { json!(self.total()) } else { json!(self.whole) }
    }
}

fn is_number(value: Option<&Value>) -> bool {
    matches!(value, Some(Value::Number(_) | Value::Bool(_)))
}

fn whole(value: Option<&Value>) -> i64 {
    value.and_then(|item| item.as_i64().or_else(|| item.as_f64().map(|n| n as i64))).unwrap_or(0)
}

fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null | Value::Bool(false)) => false,
        Some(Value::Number(number)) => number.as_f64() != Some(0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
        Some(Value::Bool(true)) => true,
    }
}

pub fn summarize(records: &[Value]) -> Value {
    let kind = |record: &Value, name: &str| record.get("kind").and_then(Value::as_str) == Some(name);
    let sessions: Vec<&Value> = records.iter().filter(|record| kind(record, "session")).collect();
    let priced: Vec<&&Value> = sessions.iter().filter(|record| is_number(record.get("effective_tokens"))).collect();
    let (mut spent, mut overhead) = (Sum::default(), Sum::default());
    for record in &priced {
        spent.add(record.get("effective_tokens"));
    }
    for record in records.iter().filter(|record| kind(record, "session") || kind(record, "overhead")) {
        for key in OVERHEAD_KEYS {
            overhead.add(record.get("overhead").and_then(|row| row.get(key)));
        }
    }
    let both = spent.total() + overhead.total();
    let count = |key: &str| sessions.iter().map(|record| whole(record.get(key))).sum::<i64>();

    json!({
        "sessions": sessions.len(),
        "priced_sessions": priced.len(),
        "effective_tokens": spent.value(),
        "overhead_tokens": overhead.value(),
        "overhead_frac": if both != 0.0 { overhead.total() / both } else { 0.0 },
        "requests": count("requests"),
        "nudges": count("nudges"),
        "aborts": count("aborts"),
        "turns": count("turns"),
        "injected_sessions": sessions.iter().filter(|record| truthy(record.get("injected_chars"))).count(),
    })
}

fn fmt_tokens(number: f64) -> String {
    for (unit, size) in [("B", 1e9), ("M", 1e6), ("k", 1e3)] {
        if number.abs() >= size {
            return format!("{:.1}{unit}", number / size);
        }
    }

    format!("{number:.0}")
}

fn canon(path: &str) -> String {
    if path.is_empty() { String::new() } else { realpath(Path::new(path)).to_string_lossy().into_owned() }
}

// What the plugin added to this checkout's settings file.
fn applied_for(store: &Store, repo_root: &str) -> Value {
    let doc = store.read("untool_applied.json").unwrap_or(Value::Null);
    let mut by: Map<String, Value> = Map::new();
    if let Some(paths) = doc.get("by_path").and_then(Value::as_object) {
        for (path, items) in paths {
            by.insert(canon(path), if items.is_null() { json!([]) } else { items.clone() });
        }
    } else if truthy(doc.get("tools")) {
        by.insert(canon(doc.get("path").and_then(Value::as_str).unwrap_or("")), doc["tools"].clone());
    }
    let settings = Path::new(repo_root).join(".claude").join("settings.local.json");
    let wanted = canon(&settings.to_string_lossy());

    by.get(&wanted).filter(|items| truthy(Some(items))).or(by.get("")).filter(|items| truthy(Some(items))).cloned().unwrap_or_else(|| json!([]))
}

// The repository a running session was pinned to, following any merge made since the pin.
fn pinned(root: &Path, session: &str) -> Option<RepoId> {
    let doc: Value = serde_json::from_slice(&fs::read(root.join("sessions").join("claude").join(pin_name(session))).ok()?).ok()?;
    let field = |name: &str| doc.get(name).and_then(Value::as_str).unwrap_or("").to_string();
    let mut repo = RepoId { root: field("root"), key: field("key"), label: field("label"), remote: field("remote"), how: field("how") };
    if repo.key.is_empty() || repo.root.is_empty() {
        return None;
    }
    if root.join("repos").join(&repo.key).is_dir() {
        return Some(repo);
    }
    let index: Value = fs::read(root.join("index.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or(Value::Null);
    let since = doc.get("at").and_then(Value::as_f64).unwrap_or(0.0);
    let mut key = repo.key.clone();
    for _ in 0..8 {
        let Some(redirect) = index.get("redirects").and_then(|all| all.get(&key)) else { break };
        if redirect.get("at").and_then(Value::as_f64).unwrap_or(0.0) < since {
            break;
        }
        match redirect.get("to").and_then(Value::as_str).filter(|to| !to.is_empty()) {
            Some(to) => key = to.to_string(),
            None => return Some(repo),
        }
    }
    repo.key = key;

    Some(repo)
}

fn session_note(root: &Path, repo: &RepoId) -> Value {
    let session = ["TE_SESSION_ID", "CLAUDE_CODE_SESSION_ID"].iter().find_map(|name| std::env::var(name).ok().filter(|id| !id.is_empty()));
    match session.and_then(|id| pinned(root, &id)).filter(|pin| pin.key != repo.key) {
        Some(pin) => json!(format!("this claude session started in {} and is recorded under {} (`te --cwd {} status`)", pin.root, pin.label, pin.root)),
        None => Value::Null,
    }
}

fn describe(repo: &RepoId) -> String {
    match repo.how.as_str() {
        "remote" => format!("identified by its origin remote ({})", repo.remote),
        "path" => "identified by its path (a git repository with no remote)".to_string(),
        _ => "identified by its directory (not a git repository)".to_string(),
    }
}

fn newest_change(dir: &Path) -> Option<std::time::SystemTime> {
    let own = fs::metadata(dir).ok()?.modified().ok()?;
    let files = fs::read_dir(dir).ok()?.flatten().filter_map(|entry| entry.metadata().ok()?.modified().ok());

    files.chain([own]).max()
}

// The agent `te` means without `--host`. Only Claude Code's state is read here.
fn host_is_claude(repo: &RepoId, root: &Path, asked: Option<&str>) -> bool {
    if let Some(host) = asked {
        return host == "claude";
    }
    if let Some(host) = std::env::var("TE_HOST").ok().filter(|host| HOSTS.contains(&host.as_str())) {
        return host == "claude";
    }
    let marked = |name: &str| std::env::var(name).is_ok_and(|value| value == "1");
    if marked("CLAUDECODE") {
        return true;
    }
    if marked("COPILOT_CLI") {
        return false;
    }
    let mut recent = None;
    for host in HOSTS {
        if let Some(changed) = newest_change(&root.join("repos").join(&repo.key).join(host)) {
            if recent.is_none_or(|(_, newest)| changed > newest) {
                recent = Some((host, changed));
            }
        }
    }

    recent.is_none_or(|(host, _)| host == "claude")
}

fn gather(repo: &RepoId, root: &Path) -> Value {
    let repo_dir = root.join("repos").join(&repo.key);
    let store = Store { base: repo_dir.join("claude") };
    let config = Config::load(root, &repo_dir);
    let injection = store.read("injection.json").unwrap_or(Value::Null);
    let untool = store.read("untool.json").unwrap_or(Value::Null);
    let tracker = EnvelopeTracker::load(&store.read("envelope.json").unwrap_or(Value::Null), &config.aggr, &config.thresholds);
    let log = fs::read(store.base.join("ledger.jsonl")).unwrap_or_default();
    let records: Vec<Value> = String::from_utf8_lossy(&log).lines().filter_map(|line| serde_json::from_str(line).ok()).filter(Value::is_object).collect();
    let injected = injection.get("rendered").and_then(Value::as_str).unwrap_or("").lines().filter(|line| line.starts_with("- ")).count();

    json!({
        "repo": repo.label,
        "repo_root": repo.root,
        "repo_key": repo.key,
        "identified_by": repo.how,
        "host": "claude",
        "state": store.base.to_string_lossy(),
        "config": config.as_dict(),
        "warnings": config.warnings,
        "session_note": session_note(root, repo),
        "microskills": {"profile": config.text("microskills"), "injected_lines": injected, "sessions_folded": injection.get("sessions").cloned().unwrap_or(json!(0))},
        "toolprune": {
            "mode": config.text("toolprune"),
            "sessions": untool.get("sessions").cloned().unwrap_or(json!(0)),
            "prompts": untool.get("prompts").cloned().unwrap_or(json!(0)),
            "catalog_source": untool.get("catalog_source").cloned().unwrap_or(Value::Null),
            "proposal": untool.get("proposal").cloned().unwrap_or(json!([])),
            "applied": applied_for(&store, &repo.root),
        },
        "loopguard": {"mode": config.loopguard, "envelope": tracker.envelope(), "ended_green_turns": tracker.resolved_turns.len()},
        "ledger": summarize(&records),
    })
}

fn listed(value: &Value) -> String {
    let items: Vec<&str> = value.as_array().map_or_else(Vec::new, |items| items.iter().filter_map(Value::as_str).collect());

    if items.is_empty() { "-".to_string() } else { items.join(", ") }
}

fn plain(value: &Value) -> String {
    crate::config::plain_text(value)
}

fn render(out: &Value, repo: &RepoId, root: &Path) -> String {
    let config = &out["config"];
    let ledger = &out["ledger"];
    let tokens = |key: &str| fmt_tokens(ledger[key].as_f64().unwrap_or(0.0));
    let mut lines = vec![
        format!("TokenEconomizer -- {} (claude)", repo.label),
        format!("  repository   {}, {}", repo.root, describe(repo)),
        format!("  state        {}", plain(&out["state"])),
        format!("  config       microskills={} toolprune={} loopguard={} repomap={} mode={} shadow={}", plain(&config["microskills"]), plain(&config["toolprune"]), plain(&config["loopguard"]), plain(&config["repomap"]), plain(&config["mode"]), plain(&config["shadow"])),
    ];
    if config["repomap"] != "off" {
        let state: Value = fs::read(root.join("repos").join(&repo.key).join("repomap.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or(Value::Null);
        let cached = if truthy(state.get("map")) {
            let commit: String = state.get("commit").and_then(Value::as_str).filter(|commit| !commit.is_empty()).unwrap_or("?").chars().take(10).collect();
            format!("{} tokens cached for {commit}", whole(state.get("chars")) / 4)
        } else {
            format!("none yet ({})", state.get("error").and_then(Value::as_str).filter(|why| !why.is_empty()).unwrap_or("made after the first session"))
        };
        lines.push(format!("  repomap      {cached}"));
    }
    for warning in out["warnings"].as_array().into_iter().flatten() {
        lines.push(format!("  warning      {}", plain(warning)));
    }
    if !out["session_note"].is_null() {
        lines.push(format!("  note         {}", plain(&out["session_note"])));
    }
    lines.push(format!("  sessions     {} folded, {} effective tokens, plugin overhead {} ({:.1}%)", ledger["sessions"], tokens("effective_tokens"), tokens("overhead_tokens"), ledger["overhead_frac"].as_f64().unwrap_or(0.0) * 100.0));
    let skills = &out["microskills"];
    lines.push(format!("  microskills  {} fact(s) injected at session start ({} sessions learned from)", skills["injected_lines"], plain(&skills["sessions_folded"])));
    let prune = &out["toolprune"];
    let source = if truthy(Some(&prune["catalog_source"])) { plain(&prune["catalog_source"]) } else { "none yet".to_string() };
    lines.push(format!("  toolprune    {} sessions ({} prompts) observed, catalog: {source}; proposed tools: {}; applied: {}", plain(&prune["sessions"]), plain(&prune["prompts"]), listed(&prune["proposal"]), listed(&prune["applied"])));
    let guard = &out["loopguard"];
    let envelope = guard["envelope"].as_f64().filter(|size| *size != 0.0).map_or_else(|| "not learned yet".to_string(), |size| format!("{size:.0} tool calls"));
    lines.push(format!("  loopguard    envelope {envelope} ({} ended-green turns); {} redirect(s), {} still-stuck notice(s) over {} turns", guard["ended_green_turns"], ledger["nudges"], ledger["aborts"], ledger["turns"]));

    lines.join("\n")
}

// Python's json.dumps(indent=2): the same text, with non-ASCII escaped.
fn pretty(value: &Value) -> String {
    let text = serde_json::to_string_pretty(value).unwrap_or_default();
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            let mut units = [0u16; 2];
            for unit in c.encode_utf16(&mut units) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }

    out
}

// None hands the command back to the Python: another agent's state, or arguments not known here.
pub fn run(args: &[String]) -> Option<String> {
    let (mut cwd, mut host, mut as_json, mut asked) = (None::<PathBuf>, None::<&str>, false, false);
    let mut words = args.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--cwd" => cwd = Some(PathBuf::from(words.next()?)),
            "--host" => host = Some(words.next()?.as_str()),
            "--json" => as_json = true,
            "status" => asked = true,
            _ => return None,
        }
    }
    if !asked {
        return None;
    }
    let cwd = cwd.or_else(|| std::env::current_dir().ok())?;
    let root = abspath(&state_root());
    let repo = repo_identity(&cwd);
    if !host_is_claude(&repo, &root, host) {
        return None;
    }
    let out = gather(&repo, &root);

    Some(if as_json { pretty(&out) } else { render(&out, &repo, &root) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_ledger_adds_up_like_the_python() {
        let records = [
            json!({"kind": "session", "effective_tokens": 900, "nudges": 2, "turns": 3, "injected_chars": 5, "overhead": {"spiral_judge_tokens": 100}}),
            json!({"kind": "session", "aborts": 1, "overhead": {"not_a_key": 7}}),
            json!({"kind": "overhead", "effective_tokens": 5, "nudges": 9, "overhead": {"optimizer_tokens": 50}}),
        ];
        let sum = summarize(&records);
        assert_eq!(sum["sessions"], 2);
        assert_eq!(sum["priced_sessions"], 1);
        assert_eq!(sum["effective_tokens"], 900);
        assert_eq!(sum["overhead_tokens"], 150);
        assert_eq!(sum["nudges"], 2);
        assert_eq!(sum["aborts"], 1);
        assert_eq!(sum["injected_sessions"], 1);
        assert_eq!(fmt_tokens(1234.0), "1.2k");
        assert_eq!(fmt_tokens(512.0), "512");
    }

    #[test]
    fn what_is_not_known_here_goes_to_the_python() {
        assert_eq!(run(&["toolprune".to_string()]), None);
        assert_eq!(run(&["--host".to_string(), "codex".to_string(), "status".to_string()]), None);
    }
}
