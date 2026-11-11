// `te status` (Apache-2.0, Microsoft), rewritten from plugin/cli.py, accounting.py and denylist.py.
// It only reads: carrying state over from earlier keys stays with the session hooks.
use crate::config::Config;
use crate::economizer::{pin_name, Store};
use crate::loopguard::EnvelopeTracker;
use crate::context::Ctx;
use crate::paths::RepoId;
use serde_json::{json, Value};
use std::fs;
use std::path::Path;

const OVERHEAD_KEYS: [&str; 6] = ["optimizer_tokens", "untool_classify_tokens", "untool_judge_tokens", "codex_untool_adapt_tokens", "spiral_judge_tokens", "spiral_nudge_writer_tokens"];

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

// The repository a running session was pinned to, following any merge made since the pin.
pub(crate) fn pinned(root: &Path, session: &str) -> Option<RepoId> {
    let doc: Value = serde_json::from_slice(&fs::read(root.join("sessions").join(crate::hosts::name()).join(pin_name(session))).ok()?).ok()?;
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

// The last 2 MiB of the ledger, from a line start: the ledger is capped, the status needs only the recent end.
fn ledger_tail(path: &Path) -> Vec<u8> {
    use std::io::{Read, Seek, SeekFrom};
    const TAIL: u64 = 2 << 20;
    let Ok(mut file) = fs::File::open(path) else { return Vec::new() };
    let len = file.metadata().map_or(0, |meta| meta.len());
    let start = len.saturating_sub(TAIL);
    let mut bytes = Vec::new();
    if file.seek(SeekFrom::Start(start)).is_err() || file.read_to_end(&mut bytes).is_err() {
        return Vec::new();
    }
    if start > 0 {
        let cut = bytes.iter().position(|byte| *byte == b'\n').map_or(bytes.len(), |at| at + 1);
        bytes.drain(..cut);
    }

    bytes
}

pub(crate) fn gather(repo: &RepoId, root: &Path) -> Value {
    let repo_dir = root.join("repos").join(&repo.key);
    let store = Store { base: repo_dir.join(crate::hosts::name()) };
    let config = Config::load(root, &repo_dir);
    let injection = store.read("injection.json").unwrap_or(Value::Null);
    let untool = store.read("untool.json").unwrap_or(Value::Null);
    let tracker = EnvelopeTracker::load(&store.read("envelope.json").unwrap_or(Value::Null), &config.aggr, &config.thresholds);
    let log = ledger_tail(&store.base.join("ledger.jsonl"));
    let records: Vec<Value> = String::from_utf8_lossy(&log).lines().filter_map(|line| serde_json::from_str(line).ok()).filter(Value::is_object).collect();
    let injected = injection.get("rendered").and_then(Value::as_str).unwrap_or("").lines().filter(|line| line.starts_with("- ")).count();

    json!({
        "repo": repo.label,
        "repo_root": repo.root,
        "repo_key": repo.key,
        "identified_by": repo.how,
        "host": crate::hosts::name(),
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
            "applied": crate::denylist::applied_for(&store, &repo.root, crate::hosts::current()),
        },
        "loopguard": {"mode": config.loopguard, "envelope": tracker.envelope(), "ended_green_turns": tracker.resolved_turns.len()},
        "ledger": summarize(&records),
    })
}

// `te status --explain`: the saved state described in words, never a guess at why a past turn did not act.
fn explanations(root: &Path, repo: &RepoId) -> Value {
    let repo_dir = root.join("repos").join(&repo.key);
    let store = Store { base: repo_dir.join(crate::hosts::name()) };
    let config = Config::load(root, &repo_dir);
    if !config.enabled {
        return json!({"plugin": "plugin is disabled; hooks record nothing"});
    }
    let injection = store.read("injection.json").filter(Value::is_object).unwrap_or_else(|| json!({}));
    let untool = store.read("untool.json").filter(Value::is_object).unwrap_or_else(|| json!({}));
    let tracker = EnvelopeTracker::load(&store.read("envelope.json").unwrap_or(Value::Null), &config.aggr, &config.thresholds);
    let paused = crate::sidecall::paused(root).is_some();
    let mut notes = serde_json::Map::new();
    if config.shadow {
        notes.insert("plugin".into(), json!("shadow mode: new decisions are recorded, not applied"));
    }
    let (facts_detail, tools_detail) = ("see microskills with the same --host/--cwd", "see toolprune with the same --host/--cwd");
    let profile = config.text("microskills");
    let proposer = profile.contains("distiller");
    let skills = if profile == "off" {
        "off; cached facts are not injected".to_string()
    } else if truthy(injection.get("rendered")) {
        let base = if config.shadow { "cached facts present; injection suppressed in shadow mode" } else { "cached facts are available for session start" };
        if proposer && paused { format!("{base}; model proposer paused") } else { base.to_string() }
    } else if proposer && paused {
        format!("model proposer paused after a sign-in failure; {facts_detail}")
    } else if injection.as_object().is_some_and(serde_json::Map::is_empty) {
        "no learning snapshot recorded for this agent/repository yet".to_string()
    } else {
        let why = match injection.get("facts").and_then(Value::as_object) {
            None => "no fact details recorded in the last learning snapshot".to_string(),
            Some(facts) => {
                // Metadata is not a fact, and an unfamiliar state is not evidence of zero facts.
                let rows: Vec<&Value> = facts.iter().filter(|(key, _)| !matches!(key.as_str(), "_profile" | "_vocab" | "_task" | "_extractors")).map(|(_, row)| row).collect();
                let states = ["CANDIDATE", "ACTIVE", "RETIRED"];
                let state = |row: &Value| row.get("state").and_then(Value::as_str).map(str::to_string);
                if rows.iter().any(|row| !state(row).is_some_and(|found| states.contains(&found.as_str()))) {
                    "fact lifecycle details unavailable in the last learning snapshot".to_string()
                } else if rows.is_empty() {
                    "no facts tracked in the last learning snapshot".to_string()
                } else {
                    let count = |name: &str| rows.iter().filter(|row| state(row).as_deref() == Some(name)).count();
                    format!("{} candidate, {} active, {} retired; no facts rendered", count("CANDIDATE"), count("ACTIVE"), count("RETIRED"))
                }
            }
        };
        format!("{why}; {facts_detail}")
    };
    notes.insert("microskills".into(), json!(skills));
    let prune = config.text("toolprune");
    let tools = if prune == "off" {
        "off; previously applied rules are not restored automatically".to_string()
    } else {
        let proposal: Vec<&str> = untool.get("proposal").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
        let pending: Vec<&str> = untool.get("auto_pending").and_then(|held| held.get("items")).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
        let why = if !proposal.is_empty() {
            if config.shadow {
                "proposal recorded in shadow mode"
            } else if prune == "propose" {
                "proposal awaiting approval"
            } else if truthy(untool.get("auto_apply_refused")) {
                "automatic apply was refused"
            } else if proposal.iter().any(|tool| pending.contains(tool)) {
                "proposal held for the idle wait"
            } else {
                "unapplied proposal recorded"
            }
        } else if !crate::denylist::applied_for(&store, &repo.root, crate::hosts::current()).is_empty() {
            "deny rules recorded as applied for this checkout"
        } else if untool.as_object().is_some_and(serde_json::Map::is_empty) {
            "no tool-usage snapshot recorded for this agent/repository yet"
        } else {
            // A zero proposal can mean used, protected or kept tools, not only too little evidence.
            "no eligible proposal recorded"
        };
        format!("{why}; {tools_detail}")
    };
    notes.insert("toolprune".into(), json!(tools));
    let guard = if config.loopguard == "off" {
        "off".to_string()
    } else if tracker.envelope().is_none() {
        format!("learning baseline: {}/{} ended-green turns", tracker.resolved_turns.len(), tracker.min_samples())
    } else if paused {
        "judge calls paused after a sign-in failure; backstop remains active".to_string()
    } else {
        "baseline ready; per-turn reasons cannot be inferred from these totals".to_string()
    };
    notes.insert("loopguard".into(), json!(guard));

    Value::Object(notes)
}

fn listed(value: &Value) -> String {
    let items: Vec<&str> = value.as_array().map_or_else(Vec::new, |items| items.iter().filter_map(Value::as_str).collect());

    if items.is_empty() { "-".to_string() } else { items.join(", ") }
}

fn plain(value: &Value) -> String {
    crate::config::plain_text(value)
}

pub(crate) fn render(out: &Value, repo: &RepoId) -> String {
    let config = &out["config"];
    let ledger = &out["ledger"];
    let tokens = |key: &str| fmt_tokens(ledger[key].as_f64().unwrap_or(0.0));
    let mut lines = vec![
        format!("TokenEconomizer -- {} (claude)", repo.label),
        format!("  repository   {}, {}", repo.root, describe(repo)),
        format!("  state        {}", plain(&out["state"])),
        format!("  config       microskills={} toolprune={} loopguard={} repomap={} mode={} shadow={}", plain(&config["microskills"]), plain(&config["toolprune"]), plain(&config["loopguard"]), plain(&config["repomap"]), plain(&config["mode"]), plain(&config["shadow"])),
    ];
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
    for (name, why) in out.get("explanations").and_then(Value::as_object).into_iter().flatten() {
        lines.push(format!("  explain      {name}: {}", plain(why)));
    }

    lines.join("\n")
}

// Python's json.dumps(indent=2): the same text, with non-ASCII escaped.
pub(crate) fn pretty(value: &Value) -> String {
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

// `te status`: what the plugin has learned for the repository a context names, and what it did.
pub fn report(ctx: &Ctx, as_json: bool, explain: bool) -> String {
    let mut out = gather(&ctx.repo, &ctx.root);
    if explain {
        out["explanations"] = explanations(&ctx.root, &ctx.repo);
    }

    if as_json { pretty(&out) } else { render(&out, &ctx.repo) }
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
}
