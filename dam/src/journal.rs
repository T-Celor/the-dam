// What the dam did, where the Claude Code module is not there to count and show it: each condensed output's tokens
// per host and session, a status line for Copilot (Codex's takes built-in items only), and the journal any host's
// `journal` skill prints.
use crate::channels::{host_name, Host};
use crate::context::Ctx;
use crate::economizer::state_root;
use crate::paths::abspath;
use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

const UNKNOWN_SESSION: &str = "unknown";

fn flow_dir(root: &Path, host: Host) -> PathBuf {
    root.join("hosts").join(host_name(host)).join("flow")
}

fn file_name(session: &str) -> String {
    // Injective: every other byte is `_xx` (hex), and a `_` that a hex pair follows is `_5f`, so ids made of safe characters
    // keep their names and no two ids share a file.
    let bytes = session.as_bytes();
    let mut name = String::new();
    for (i, &b) in bytes.iter().enumerate() {
        let hex_pair_follows = bytes.get(i + 1..i + 3).is_some_and(|pair| pair.iter().all(u8::is_ascii_hexdigit));
        if b == b'_' && hex_pair_follows {
            name.push_str("_5f");
        } else if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.') {
            name.push(b as char);
        } else {
            name.push_str(&format!("_{b:02x}"));
        }
    }
    name.truncate(120);

    format!("{}.json", if name.is_empty() { UNKNOWN_SESSION } else { &name })
}

const FLOW_KEPT: std::time::Duration = std::time::Duration::from_secs(30 * 24 * 3600);

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |at| at.as_secs())
}

#[derive(Debug, Default, PartialEq)]
pub struct Flow {
    pub seen: u64,
    pub saved: u64,
    pub outputs: u64,
}

fn read_flow(path: &Path) -> Flow {
    let doc: Value = fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or(Value::Null);
    let number = |key: &str| doc.get(key).and_then(Value::as_u64).unwrap_or(0);

    Flow { seen: number("seen"), saved: number("saved"), outputs: number("outputs") }
}

fn add_at(root: &Path, host: Host, session: &str, before: usize, after: usize) {
    let dir = flow_dir(root, host);
    let path = dir.join(file_name(session));
    // The lock sits beside the folder, not in it: every file in it is read as a count.
    crate::paths::with_lock(&dir.with_file_name("flow.lock"), || {
        let was = read_flow(&path);
        let doc = json!({"seen": was.seen + before as u64, "saved": was.saved + before.saturating_sub(after) as u64, "outputs": was.outputs + 1, "at": now()});
        // A count that cannot be written only makes the status line show less: never fail the hook for it.
        let _ = crate::paths::write_atomic(&path, doc.to_string().as_bytes());
    });
}

// One condensed output's tokens, before and after.
pub fn add_saving(host: Host, session: &str, before: usize, after: usize) {
    add_at(&abspath(&state_root()), host, session, before, after);
}

fn events_file(root: &Path, host: Host) -> PathBuf {
    root.join("hosts").join(host_name(host)).join("events.jsonl")
}

const MOST_EVENT_LINES: usize = 300;

// One thing the dam did, for `te journal`: what Claude Code's module lists beside the chat. Claude Code keeps its own, so
// nothing is written there. Never fails the hook.
pub fn event(host: Host, session: &str, icon: &str, text: &str) {
    if host == Host::Claude {
        return;
    }
    let path = events_file(&abspath(&state_root()), host);
    let line = json!({"at": now(), "session": session, "icon": icon, "text": text}).to_string();
    crate::paths::with_lock(&path.with_file_name("events.lock"), || {
        let mut lines: Vec<String> = fs::read_to_string(&path).map(|old| old.lines().map(String::from).collect()).unwrap_or_default();
        lines.push(line);
        let from = lines.len().saturating_sub(MOST_EVENT_LINES);
        let _ = crate::paths::write_atomic(&path, format!("{}\n", lines[from..].join("\n")).as_bytes());
    });
}

// The latest session's events, newest first, as `HH:MM  icon text` (UTC).
fn event_lines(root: &Path, host: Host, most: usize) -> Vec<String> {
    let events: Vec<Value> = fs::read_to_string(events_file(root, host)).unwrap_or_default().lines().filter_map(|line| serde_json::from_str(line).ok()).collect();
    let session = events.last().and_then(|e| e["session"].as_str()).unwrap_or("").to_string();

    events.iter().rev().filter(|e| e["session"].as_str() == Some(session.as_str())).take(most).map(|e| {
        let at = e["at"].as_u64().unwrap_or(0) % 86_400;
        format!("{:02}:{:02}  {} {}", at / 3600, at % 3600 / 60, e["icon"].as_str().unwrap_or(""), e["text"].as_str().unwrap_or(""))
    }).collect()
}

// The session's count, and the host's since it was first counted.
fn totals(root: &Path, host: Host, session: Option<&str>) -> (Flow, Flow, Option<String>) {
    let dir = flow_dir(root, host);
    let mut all = Flow::default();
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let modified = entry.metadata().and_then(|meta| meta.modified()).ok();
        // Best effort: a count untouched for a month is a finished session's.
        if modified.and_then(|at| at.elapsed().ok()).is_some_and(|age| age > FLOW_KEPT) {
            let _ = fs::remove_file(&path);
            continue;
        }
        let flow = read_flow(&path);
        all.seen += flow.seen;
        all.saved += flow.saved;
        all.outputs += flow.outputs;
        if let Some(at) = modified.filter(|at| newest.as_ref().is_none_or(|(best, _)| at > best)) {
            newest = Some((at, path));
        }
    }
    // The session asked for, else the one counted last: a status line's payload may not name it.
    let path = session.filter(|session| !session.is_empty()).map(|session| dir.join(file_name(session))).or_else(|| newest.map(|(_, path)| path));
    let name = path.as_ref().and_then(|path| path.file_stem()).map(|stem| stem.to_string_lossy().into_owned());

    (path.map(|path| read_flow(&path)).unwrap_or_default(), all, name)
}

pub fn count(tokens: u64) -> String {
    match tokens {
        0..=999 => tokens.to_string(),
        1_000..=999_999 => format!("{:.1}k", tokens as f64 / 1e3),
        _ => format!("{:.1}M", tokens as f64 / 1e6),
    }
}

fn facts_in(cwd: &Path) -> usize {
    let injection = Ctx::for_cwd(cwd).store.read("injection.json").unwrap_or_default();

    injection.get("rendered").and_then(Value::as_str).map_or(0, |text| text.lines().filter(|line| line.starts_with("- ")).count())
}

fn line(session: &Flow, level: &str, facts: usize) -> String {
    let saved = if session.outputs == 0 { "💧 nothing to condense yet".to_string() } else { format!("💧 {} tokens saved on {} output(s)", count(session.saved), session.outputs) };

    format!("{saved} · 🦫 {level} · 🧠 {facts} fact(s)")
}

// The places the map holds for `cwd` (the node lines of its graph), as the status line under Claude Code counts them.
fn map_places(cwd: &Path) -> Option<usize> {
    let text = fs::read_to_string(cwd.join(".dam-graph").join("graph.tsv")).ok()?;

    Some(text.lines().filter(|line| line.starts_with("N\t")).count())
}

// The status line as Claude Code's module draws it, for the hosts that cannot run the module:
// `🦫 full · 💧 5k/20k (25%) · 🧠 3 · 🗺️ 400`.
pub fn status_text(host: Host, session: Option<&str>, cwd: &Path, transcript: Option<&Path>, native: Option<String>) -> String {
    status_text_with(host, session, cwd, transcript, native, None)
}

// The same eight slots, in the same order, as Claude Code's module: 🦫 level · 💧 saved/spent · 💸 cost · 🪵 tools pruned ·
// 🌀 loops · 🧠 facts · 🗺️ places · 🌊 context. `–` where the host has nothing to measure (tool pruning is Claude Code's).
pub fn status_text_with(host: Host, session: Option<&str>, cwd: &Path, transcript: Option<&Path>, native: Option<String>, context: Option<f64>) -> String {
    let (flow, _, name) = totals(&abspath(&state_root()), host, session);
    let id = session.map(str::to_string).or(name).unwrap_or_default();
    let level = crate::beaver::level_of(&id);
    let saved = share(flow.saved, flow.seen);
    let places = map_places(cwd).map_or("–".to_string(), |n| n.to_string());
    // The loop guard's own count for the session, as Claude Code's module reads it.
    let doc = Ctx::for_cwd(cwd).store.read(&format!("sessions/{id}.json")).unwrap_or_default();
    let loops = doc.get("nudges").and_then(Value::as_u64).unwrap_or(0) + doc.get("aborts").and_then(Value::as_u64).unwrap_or(0);
    let fill = context.map_or("–".to_string(), |p| format!("{}%", p.round()));

    format!("🦫 {level} · 💧 {saved} · 💸 {} · 🪵 – · 🌀 {loops} · 🧠 {} · 🗺️ {places} · 🌊 {fill}", native.unwrap_or_else(|| session_cost(host, transcript)), facts_in(cwd))
}

// `saved/spent (share)` as Claude Code's module writes it: `seen` is what the outputs weighed before condensing, so what was
// spent is `seen - saved` and the share is `saved/seen`.
fn share(saved: u64, seen: u64) -> String {
    if seen == 0 {
        return count(saved);
    }

    format!("{}/{} ({:.1}%)", count(saved), count(seen.saturating_sub(saved)), saved as f64 * 100.0 / seen as f64)
}

#[cfg(test)]
mod share_tests {
    use super::*;

    #[test]
    fn the_share_is_saved_over_what_the_outputs_weighed() {
        assert_eq!(share(1000, 1500), "1.0k/500 (66.7%)");
        assert_eq!(share(0, 0), "0");
    }
}

// What the session's requests cost, priced as fresh-input tokens (pricing.rs), from the host's own log; `–` before it has any.
// Copilot's log carries usage only when the session ends.
fn session_cost(host: Host, transcript: Option<&Path>) -> String {
    let usage = transcript.filter(|path| path.exists()).map(|path| crate::hosts::session_usage(host, path));

    if let Some(dollars) = usage.as_ref().and_then(crate::pricing::usd) {
        return format!("${dollars:.2}");
    }

    usage.and_then(|usage| crate::pricing::effective(&usage).0).map_or("–".to_string(), |cost| count(cost as u64))
}

// `dam te statusline --host copilot`: Copilot's status line command, its JSON payload on stdin.
pub fn status_line(host: Host, payload: &str) -> String {
    let payload: Value = serde_json::from_str(payload).unwrap_or(Value::Null);
    let text = |keys: &[&str]| keys.iter().find_map(|key| payload.get(*key).and_then(Value::as_str)).map(str::to_string);
    let cwd = text(&["cwd", "workingDirectory"]).map_or_else(|| std::env::current_dir().unwrap_or_default(), PathBuf::from);

    let session = text(&["sessionId", "session_id"]);
    let log = session.as_ref().map(|id| crate::hosts::copilot_home().join("session-state").join(id).join("events.jsonl"));

    // Copilot counts in premium requests, and its status line payload carries the session's in `cost.total_premium_requests`
    // (per third-party status lines, not checked against a live payload; `ai_used`, whose unit is unknown, is not read).
    // Without it the token pricing shows.
    let requests = payload["cost"]["total_premium_requests"].as_f64();
    let native = requests.map(|n| format!("{} req", if n.fract() == 0.0 { format!("{n:.0}") } else { format!("{n:.1}") }));

    // The context window's fill, if the payload carries it (per third-party status lines, not checked live).
    let context = payload["context_window"]["used_percentage"].as_f64();

    status_text_with(host, session.as_deref(), &cwd, log.as_deref(), native, context)
}

// The first Copilot session after the install: its status line is set once, as Claude Code's is by the module.
pub fn first_session(host: Host) -> Option<String> {
    if host != Host::Copilot {
        return None;
    }
    let flag = state_root().join("hosts").join("copilot").join("statusline-applied");
    if flag.exists() {
        // Ours already: kept pointing at this binary, which an update or a move of the plugin may have changed. Removed: left removed.
        if fs::read_to_string(copilot_settings()).is_ok_and(|text| text.contains("\" te statusline --host copilot")) {
            let _ = set_status_line(true);
        }
        return None;
    }
    let done = set_status_line(true);
    let _ = crate::paths::write_atomic(&flag, b"1");

    done.ok().map(|done| format!("The dam set Copilot's status line: {done}. `dam te statusline uninstall --host copilot` removes it."))
}

// `dam te journal`: what the dam did in this host's latest session and since it was installed.
pub fn journal(host: Host, cwd: &Path) -> String {
    let (session, all, name) = totals(&abspath(&state_root()), host, None);
    let mut lines = vec![format!("The dam under {}", host_name(host))];
    if host == Host::Claude {
        lines.push("Under Claude Code, `/the-dam` shows the full journal beside the chat; this counts only what hooks condensed.".to_string());
    }
    lines.push(format!("  this session   {}", line(&session, &crate::beaver::level_of(name.as_deref().unwrap_or("")), facts_in(cwd))));
    let share = if all.seen == 0 { String::new() } else { format!(" ({:.0}% of what they printed)", all.saved as f64 * 100.0 / all.seen as f64) };
    lines.push(format!("  all sessions   {} tokens saved on {} output(s){share}", count(all.saved), all.outputs));
    let events = event_lines(&abspath(&state_root()), host, 20);
    if !events.is_empty() {
        lines.push("  events, newest first (UTC):".to_string());
        lines.extend(events.into_iter().map(|line| format!("    {line}")));
    }

    lines.join("\n")
}

// ---- Copilot's status line setting ----

fn copilot_settings() -> PathBuf {
    crate::hosts::copilot_home().join("settings.json")
}

fn ours(program: &str) -> String {
    format!("\"{program}\" te statusline --host copilot")
}

// `settings` with the status line set to the dam's (Some) or the dam's removed (None); another one is left alone.
pub fn with_status_line(settings: &str, command: Option<&str>) -> Result<String, String> {
    let mut doc: Map<String, Value> = if settings.trim().is_empty() { Map::new() } else { serde_json::from_str(settings).map_err(|error| format!("not JSON the dam can edit ({error}): left as it is"))? };
    let current = doc.get("statusLine");
    // Ours is exactly what `ours` writes: a quoted program, then these words, and nothing after them.
    let is_ours = current.and_then(|line| line.get("command")).and_then(Value::as_str).is_some_and(|line| line.starts_with('"') && line.ends_with("\" te statusline --host copilot"));
    if current.is_some() && !is_ours {
        return Err("it already has a status line of its own: left as it is".to_string());
    }
    match command {
        Some(command) => doc.insert("statusLine".to_string(), json!({"type": "command", "command": command})),
        None => doc.remove("statusLine"),
    };

    Ok(format!("{}\n", serde_json::to_string_pretty(&Value::Object(doc)).unwrap_or_default()))
}

// `dam te statusline install|uninstall --host copilot`.
pub fn set_status_line(on: bool) -> Result<String, String> {
    let path = copilot_settings();
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("{}: {error}; left as it is", path.display())),
    };
    let next = with_status_line(&text, on.then(|| ours(&crate::channels::program())).as_deref()).map_err(|why| format!("{}: {why}", path.display()))?;
    if next.trim() == text.trim() {
        return Ok(format!("status line already {} ({})", if on { "installed" } else { "removed" }, path.display()));
    }
    crate::paths::write_atomic(&path, next.as_bytes()).map_err(|error| format!("{}: {error}", path.display()))?;

    Ok(if on {
        format!("status line installed in {}; it shows from Copilot's next start (where it does not, turn on Copilot's experimental features: its status line is one)", path.display())
    } else {
        format!("status line removed from {}", path.display())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn savings_add_up_per_session_and_for_the_host() {
        let dir = crate::session::TempDir::new("dam-journal-").unwrap();
        add_at(&dir.0, Host::Copilot, "s1", 1000, 100);
        add_at(&dir.0, Host::Copilot, "s1", 500, 400);
        add_at(&dir.0, Host::Copilot, "../evil", 10, 5);
        add_at(&dir.0, Host::Codex, "s1", 9000, 1);
        let (session, all, _) = totals(&dir.0, Host::Copilot, Some("s1"));
        assert_eq!(session, Flow { seen: 1500, saved: 1000, outputs: 2 });
        assert_eq!(all.saved, 1005);
        // A session id is a file name, never a path.
        assert!(flow_dir(&dir.0, Host::Copilot).join(".._2fevil.json").is_file());
        assert_ne!(file_name("a/b"), file_name("ab"));
        assert_ne!(file_name("a/b"), file_name("a_2fb"));
        assert_eq!(file_name("sess-1.x_y"), "sess-1.x_y.json");
        assert!(line(&session, "full", 3).starts_with("💧 1.0k tokens saved on 2 output(s) · 🦫 full · 🧠 3 fact(s)"));
    }

    #[test]
    fn copilot_settings_get_our_line_and_lose_it_and_another_one_is_kept() {
        let mine = "{\n  \"theme\": \"dark\"\n}";
        let on = with_status_line(mine, Some(&ours("/p/dam"))).unwrap();
        let doc: Value = serde_json::from_str(&on).unwrap();
        assert_eq!(doc["theme"], "dark");
        assert_eq!(doc["statusLine"]["command"], "\"/p/dam\" te statusline --host copilot");
        let off: Value = serde_json::from_str(&with_status_line(&on, None).unwrap()).unwrap();
        assert_eq!(off, json!({"theme": "dark"}));
        assert!(with_status_line("{\"statusLine\": {\"type\": \"command\", \"command\": \"mine.sh\"}}", Some("x")).is_err());
        assert!(with_status_line("{\"statusLine\": \"mine.sh\"}", Some("x")).is_err());
        assert!(with_status_line("{\"statusLine\": {\"type\": \"x\"}}", None).is_err());
        assert!(with_status_line("{ not json", Some("x")).is_err());
        assert!(with_status_line("", Some("x")).unwrap().contains("statusLine"));
    }

    #[test]
    fn the_journal_lists_the_latest_sessions_events_newest_first() {
        let root = std::env::temp_dir().join(format!("dam-events-{}", std::process::id()));
        let file = events_file(&root, Host::Codex);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "{\"at\":3600,\"session\":\"old\",\"icon\":\"💧\",\"text\":\"x\"}\n{\"at\":3660,\"session\":\"s\",\"icon\":\"🗺️\",\"text\":\"first\"}\nnot json\n{\"at\":7260,\"session\":\"s\",\"icon\":\"🦫\",\"text\":\"second\"}\n").unwrap();
        assert_eq!(event_lines(&root, Host::Codex, 5), vec!["02:01  🦫 second", "01:01  🗺️ first"]);
        assert_eq!(event_lines(&root, Host::Copilot, 5), Vec::<String>::new());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn counts_untouched_for_a_month_are_deleted() {
        let dir = crate::session::TempDir::new("dam-journal-old-").unwrap();
        add_at(&dir.0, Host::Copilot, "old", 100, 10);
        add_at(&dir.0, Host::Copilot, "new", 100, 10);
        let old = flow_dir(&dir.0, Host::Copilot).join(file_name("old"));
        let at = std::time::SystemTime::now() - std::time::Duration::from_secs(40 * 24 * 3600);
        fs::OpenOptions::new().write(true).open(&old).unwrap().set_modified(at).unwrap();
        let (_, all, _) = totals(&dir.0, Host::Copilot, None);
        assert_eq!(all.outputs, 1);
        assert!(!old.exists());
    }

    #[test]
    fn counts_read_short() {
        assert_eq!(count(999), "999");
        assert_eq!(count(12_345), "12.3k");
        assert_eq!(count(2_500_000), "2.5M");
    }
}

#[cfg(test)]
mod native_cost {
    use super::*;

    #[test]
    fn copilots_own_premium_requests_show_as_its_cost() {
        let line = status_line(Host::Copilot, r#"{"sessionId":"x","cwd":".","cost":{"total_premium_requests":3}}"#);
        assert!(line.contains("💸 3 req"), "{line}");
        assert!(status_line(Host::Copilot, r#"{"sessionId":"x","cwd":"."}"#).contains("💸 –"));
    }
}
