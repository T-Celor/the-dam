// Beaver: the lazy senior developer rules of Ponytail (MIT, https://github.com/DietrichGebert/ponytail), its hooks
// rewritten for Claude Code: `activate` at session start, `subagent` for each subagent, `track` for each prompt.
// What Ponytail called its settings (PONYTAIL_* variables, ~/.config/ponytail, `/ponytail`) still works.
// A hook must never block the session, so stdin gets a second and every failure is silent.
use crate::json;
use crate::scan::{self, lang_of};
use std::collections::hash_map::DefaultHasher;
use std::collections::BTreeMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, OnceLock};
use std::time::Duration;

const LEVELS: [&str; 4] = ["off", "lite", "full", "ultra"];
const DEFAULT_LEVEL: &str = "full";
const MAP_BUDGET: usize = 2000;
const MOST_MAP_FILES: usize = 4000;
const MOST_MAP_BYTES: u64 = 65_536;
// Claude Code gives the start hook 15 seconds; a first read of a large project from a cold disk can take more.
const MAP_TIME: std::time::Duration = std::time::Duration::from_secs(3);

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

// BEAVER_<name>, else Ponytail's PONYTAIL_<name>.
fn setting(name: &str) -> Option<String> {
    env(&format!("BEAVER_{name}")).or_else(|| env(&format!("PONYTAIL_{name}")))
}

fn home() -> PathBuf {
    env("USERPROFILE").or_else(|| env("HOME")).map_or_else(|| PathBuf::from("."), PathBuf::from)
}

fn claude_dir() -> PathBuf {
    env("CLAUDE_CONFIG_DIR").map_or_else(|| home().join(".claude"), PathBuf::from)
}

fn config_path() -> PathBuf {
    let dir = if let Some(xdg) = env("XDG_CONFIG_HOME") {
        PathBuf::from(xdg)
    } else if cfg!(windows) {
        env("APPDATA").map_or_else(|| home().join("AppData").join("Roaming"), PathBuf::from)
    } else {
        home().join(".config")
    };

    dir.join("beaver").join("config.json")
}

fn ponytail_config_path() -> PathBuf {
    config_path().parent().and_then(Path::parent).unwrap_or(Path::new(".")).join("ponytail").join("config.json")
}

fn configured_level(path: &Path) -> Option<String> {
    fs::read_to_string(path).ok().and_then(|text| json::field(text.trim_start_matches('\u{feff}'), "defaultMode")).and_then(|value| normal_level(&value))
}

fn normal_level(text: &str) -> Option<String> {
    let level = text.trim().to_lowercase();

    LEVELS.contains(&level.as_str()).then_some(level)
}

fn default_level() -> String {
    setting("DEFAULT_MODE")
        .and_then(|value| normal_level(&value))
        .or_else(|| configured_level(&config_path()))
        .or_else(|| configured_level(&ponytail_config_path()))
        .unwrap_or_else(|| DEFAULT_LEVEL.to_string())
}

// Refuses (file untouched) when the existing config does not parse: rewriting it would erase its other keys.
fn write_default_level(level: &str) -> Result<(), String> {
    let path = config_path();
    // Only a missing file is empty: an unreadable one (UTF-16, no access) is left as it is.
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("{} cannot be read ({error}): left as it is", path.display())),
    };
    let text = text.trim_start_matches('\u{feff}');
    let mut entries = if text.trim().is_empty() { Vec::new() } else { json::entries(text).ok_or_else(|| format!("{} is not JSON the dam can edit: left as it is", path.display()))? };
    entries.retain(|(key, _)| key != "\"defaultMode\"");
    entries.push(("\"defaultMode\"".to_string(), json::quote(level)));
    let body: Vec<String> = entries.iter().map(|(key, value)| format!("  {key}: {value}")).collect();
    crate::paths::write_atomic(&path, format!("{{\n{}\n}}", body.join(",\n")).as_bytes()).map_err(|error| format!("{}: {error}", path.display()))
}

fn shared_state() -> PathBuf {
    claude_dir().join(".beaver-active")
}

// The hook's session, from its input: each session keeps its own level, even beside another in the same project.
static SESSION: OnceLock<String> = OnceLock::new();
// A session's level outlives it by this long at most, so the folder does not grow for ever.
const LEVEL_KEPT: Duration = Duration::from_secs(7 * 24 * 3600);

fn levels_dir() -> PathBuf {
    claude_dir().join("beaver-modes")
}

fn project_state() -> Option<PathBuf> {
    if let Some(session) = SESSION.get().filter(|session| !session.is_empty()) {
        let name: String = session.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' }).take(200).collect();
        return Some(levels_dir().join(format!("session-{name}")));
    }
    let project = env("CLAUDE_PROJECT_DIR")?;
    let mut hasher = DefaultHasher::new();
    project.trim().hash(&mut hasher);
    SESSION.get().map(String::as_str).unwrap_or("").hash(&mut hasher);

    Some(levels_dir().join(format!("{:016x}", hasher.finish())))
}

fn prune_levels() {
    let Ok(entries) = fs::read_dir(levels_dir()) else { return };
    for entry in entries.flatten() {
        let age = entry.metadata().and_then(|meta| meta.modified()).ok().and_then(|at| at.elapsed().ok());
        if age.is_some_and(|age| age > LEVEL_KEPT) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn set_level(level: &str) {
    for path in project_state().into_iter().chain([shared_state()]) {
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let _ = fs::write(path, level);
    }
}

fn clear_level() {
    for path in project_state().into_iter().chain([shared_state()]) {
        let _ = fs::remove_file(path);
    }
}

// A session's level, as its hooks set it, else the default: what a status line shows.
pub fn level_of(session: &str) -> String {
    let name: String = session.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') { c } else { '_' }).take(200).collect();
    let live = fs::read_to_string(levels_dir().join(format!("session-{name}"))).ok().map(|text| text.trim().to_string()).filter(|level| !level.is_empty());

    live.unwrap_or_else(default_level)
}

fn live_level() -> Option<String> {
    let text = fs::read_to_string(project_state().unwrap_or_else(shared_state)).ok()?;
    let level = text.trim().to_string();

    (!level.is_empty()).then_some(level)
}

// `| **lite** |` table rows and `- lite: "..."` examples belong to one level; every other line stays.
pub fn filter_skill(body: &str, level: &str) -> String {
    let body = body.strip_prefix("---").and_then(|rest| rest.split_once("\n---")).map_or(body, |(_, rest)| rest).trim_start_matches(['-', '\r', '\n', ' ']);
    let is_other_level = |label: &str| normal_level(label).is_some_and(|found| found != level);
    let keep = |line: &&str| {
        if let Some(rest) = line.strip_prefix('|') {
            let cell = rest.split('|').next().unwrap_or("").trim();
            if let Some(label) = cell.strip_prefix("**").and_then(|c| c.strip_suffix("**")) {
                return !is_other_level(label);
            }
        }
        if let Some((label, rest)) = line.strip_prefix('-').and_then(|tail| tail.split_once(':')) {
            if rest.trim_start().starts_with('"') {
                return !is_other_level(label);
            }
        }
        true
    };

    body.lines().filter(keep).collect::<Vec<_>>().join("\n")
}

fn skill_path() -> PathBuf {
    crate::plugin_root().unwrap_or_default().join("skills").join("beaver").join("SKILL.md")
}

fn instructions(level: &str) -> String {
    let level = normal_level(level).unwrap_or_else(|| DEFAULT_LEVEL.to_string());
    match fs::read_to_string(skill_path()) {
        Ok(body) => format!("BEAVER MODE ACTIVE — level: {level}\n\n{}", filter_skill(&body, &level)),
        Err(_) => format!("BEAVER MODE ACTIVE — level: {level}\n\nYou are a lazy senior developer: the smallest change that fully solves the task, and never cut validation, error handling, security or anything the user asked for."),
    }
}

fn is_public_def(line: &str, name: &str, path: &str) -> bool {
    !name.starts_with('_')
        && match lang_of(path) {
            Some(scan::Lang::Python) => !line.starts_with([' ', '\t']),
            Some(scan::Lang::Script) => line.starts_with("export"),
            Some(scan::Lang::Rust) => line.starts_with("pub "),
            Some(scan::Lang::Go) => line.starts_with("func ") || line.starts_with("type ") && name.starts_with(char::is_uppercase),
            Some(scan::Lang::Java | scan::Lang::CSharp | scan::Lang::Kotlin) => line.trim_start().split_whitespace().any(|word| matches!(word, "class" | "interface" | "enum" | "record")),
            _ => false,
        }
}

fn is_skipped_map_file(path: &str) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    let dirs = &parts[..parts.len() - 1];
    let file = parts[parts.len() - 1];
    let skipped_dir = dirs.iter().any(|d| matches!(*d, "node_modules" | "dist" | "build" | "out" | "vendor" | "venv" | ".venv" | "__pycache__" | "coverage" | ".next" | "target" | "migrations" | "versions" | "fixtures" | "test" | "tests" | "__tests__" | "spec"));

    skipped_dir || file.starts_with("test_") || [".test.", ".spec.", ".gen.", ".min.", ".d.", "_test."].iter().any(|mark| file.contains(mark))
}

// What already exists in the project, so that "reuse first" costs no search: one line per folder.
pub fn codebase_map(files: &[String], root: &Path) -> String {
    let is_shared = |file: &str| {
        let lower = file.to_lowercase();
        ["util", "helper", "lib", "common", "shared", "service", "component", "hook", "core", "model", "schema", "api"].iter().any(|word| lower.contains(word))
    };
    let mut wanted: Vec<&String> = files.iter().filter(|file| !is_skipped_map_file(file)).take(MOST_MAP_FILES).collect();
    wanted.sort_by_key(|file| (!is_shared(file), file.to_string()));
    let mut folders: Vec<(String, Vec<Vec<String>>)> = Vec::new();
    let mut places: BTreeMap<String, usize> = BTreeMap::new();
    let started = std::time::Instant::now();
    for file in wanted {
        if started.elapsed() > MAP_TIME {
            break;
        }
        let Some(lang) = lang_of(file) else { continue };
        let path = root.join(file);
        if !fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() <= MOST_MAP_BYTES) {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else { continue };
        let lines: Vec<&[u8]> = bytes.split(|&b| b == b'\n').collect();
        let mut names = Vec::new();
        for def in scan::scan(&bytes, lang).defs {
            let line = String::from_utf8_lossy(lines.get(def.line - 1).copied().unwrap_or_default());
            if is_public_def(&line, &def.name, file) && !names.contains(&def.name) {
                names.push(def.name);
            }
        }
        if names.is_empty() {
            continue;
        }
        let folder = file.rsplit_once('/').map_or(".", |(dir, _)| dir).to_string();
        let at = *places.entry(folder.clone()).or_insert_with(|| {
            folders.push((folder, Vec::new()));
            folders.len() - 1
        });
        folders[at].1.push(names);
    }
    let (mut lines, mut used, mut skipped) = (Vec::new(), 0, 0);
    for (folder, per_file) in &folders {
        let each = if per_file.len() > 8 { 1 } else { 3 };
        let mut names: Vec<&str> = Vec::new();
        for name in per_file.iter().flat_map(|file| file.iter().take(each)) {
            if !names.contains(&name.as_str()) {
                names.push(name);
            }
        }
        let more = if names.len() > 30 { ", ..." } else { "" };
        let line = format!("{folder}/: {}{more}", names[..names.len().min(30)].join(", "));
        if used + line.len() > MAP_BUDGET {
            skipped += 1;
        } else {
            used += line.len() + 1;
            lines.push(line);
        }
    }
    if lines.is_empty() {
        return String::new();
    }
    let tail = if skipped > 0 { format!("\n({skipped} more folders not listed)") } else { String::new() };

    format!("Codebase map (what already exists; reuse it, read a file only when you need its details):\n{}{tail}", lines.join("\n"))
}

// On Windows a wrapper can swallow the piped JSON so that stdin never ends: a second, then whatever arrived.
fn read_stdin() -> String {
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = std::io::stdin().read_to_end(&mut bytes);
        let _ = send.send(bytes);
    });
    let bytes = receive.recv_timeout(Duration::from_secs(1)).unwrap_or_default();
    let input = String::from_utf8_lossy(&bytes).trim_start_matches('\u{feff}').to_string();
    // Copilot spells it sessionId.
    let _ = SESSION.set(json::field(&input, "session_id").or_else(|| json::field(&input, "sessionId")).unwrap_or_default());

    input
}

fn activate() {
    let input = read_stdin();
    prune_levels();
    // Compaction runs this again inside one conversation, which keeps its live level.
    let level = if json::field(&input, "source").as_deref() == Some("compact") { live_level().unwrap_or_else(|| "off".to_string()) } else { default_level() };
    let host = host();
    // Part of the installation: the first Codex session sets its models, whatever Beaver's level (Claude Code's module does
    // that for itself).
    let models = crate::models::first_session(host).or_else(|| crate::journal::first_session(host));
    if level == "off" {
        clear_level();
        return;
    }
    set_level(&level);
    let mut text = start_text(&level);
    if let Some(done) = models {
        text = format!("{done}\n\n{text}");
    }
    // Said once per list: a document that names files the code no longer has.
    if let Some(note) = std::env::current_dir().ok().and_then(|root| crate::autoreview::drift_note(&root)) {
        text = format!("{note}\n\n{text}");
    }
    if host != Host::Claude {
        // Where `te doctor --host` tells a blocked start hook from one that never had a session.
        crate::channels::beat(&crate::channels::hooks_channel(host));
        // No module runs there to give the map's command; `dam te hook` gives the facts, `dam squeeze hook` condenses.
        if let Some(map) = crate::channels::map_line(&std::env::current_dir().unwrap_or_default()) {
            text = format!("{map}\n\n{text}");
        }
    }
    print!("{}", start_output(host, &text));
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Host {
    Claude,
    Codex,
    Copilot,
}

// As Ponytail tells them apart: Copilot sets COPILOT_PLUGIN_DATA (VS Code's Copilot only a CLAUDE_PLUGIN_ROOT under
// .vscode/agent-plugins), Codex PLUGIN_DATA beside its CLAUDE_* aliases, Claude Code neither.
fn host_from(copilot_data: Option<String>, claude_root: Option<String>, plugin_data: Option<String>) -> Host {
    let vscode = claude_root.is_some_and(|root| root.replace('\\', "/").contains(".vscode/agent-plugins/"));
    if copilot_data.is_some() || vscode {
        Host::Copilot
    } else if plugin_data.is_some() {
        Host::Codex
    } else {
        Host::Claude
    }
}

// The host this program runs for: `--host` on its command line first (`hosts::current`), else the environment.
pub fn host() -> Host {
    crate::hosts::current()
}

// As the environment tells: what a hook entry without `--host` relies on.
pub fn host_from_env() -> Host {
    host_from(env("COPILOT_PLUGIN_DATA"), env("CLAUDE_PLUGIN_ROOT"), env("PLUGIN_DATA"))
}

// Claude Code and Codex take a start hook's plain text as context; Copilot reads only `additionalContext`.
fn start_output(host: Host, text: &str) -> String {
    match host {
        Host::Copilot => format!("{{\"additionalContext\":{}}}", json::quote(text)),
        Host::Claude | Host::Codex => text.to_string(),
    }
}

// What the session start adds at `level`: the rules, then the codebase map unless BEAVER_MAP=0.
fn start_text(level: &str) -> String {
    let output = instructions(level);
    if setting("MAP").as_deref() == Some("0") {
        return output;
    }
    let map = codebase_map(&crate::graph::source_files(), Path::new("."));

    if map.is_empty() { output } else { format!("{output}\n\n{map}") }
}

// The session start's text at the default level, for `te context` where hooks cannot add it; None when Beaver is off.
pub fn context_text() -> Option<String> {
    let level = default_level();

    (level != "off").then(|| start_text(&level))
}

// BEAVER_SUBAGENT_MATCHER (or PONYTAIL_SUBAGENT_MATCHER) as words separated by |, any of which may appear in the agent's type.
// shortcut: not a full regular expression as upstream, add one if `^` or `.*` scoping is ever needed.
fn matches_agent(matcher: &str, agent: &str) -> bool {
    let agent = agent.to_lowercase();

    agent.is_empty() || matcher.split('|').any(|word| agent.contains(&word.trim_matches(['^', '$']).to_lowercase()))
}

fn subagent() {
    let input = read_stdin();
    let Some(level) = live_level().filter(|level| level != "off") else { return };
    if let Some(matcher) = setting("SUBAGENT_MATCHER") {
        if !matches_agent(&matcher, &json::field(&input, "agent_type").unwrap_or_default()) {
            return;
        }
    }
    let rules = json::quote(&instructions(&level));
    if host() == Host::Copilot {
        print!("{{\"additionalContext\":{rules}}}");
    } else {
        print!("{{\"hookSpecificOutput\":{{\"hookEventName\":\"SubagentStart\",\"additionalContext\":{rules}}}}}");
    }
}

// What a prompt asks of the level, or None when it asks nothing.
pub fn answer_prompt(prompt: &str, live: Option<&str>, default: &str) -> Option<PromptAction> {
    let prompt = prompt.trim().to_lowercase();
    let bare = prompt.trim_end_matches(['.', '!', '?', ' ', '\t', '\n']);
    if bare == "stop beaver" || bare == "stop ponytail" || bare == "normal mode" {
        return Some(PromptAction::Off);
    }
    let mut parts = prompt.split_whitespace();
    let command = parts.next()?;
    let command = command.strip_prefix(['/', '@', '$'])?;
    if !matches!(command, "beaver" | "the-dam:beaver" | "ponytail" | "ponytail:ponytail" | "the-dam:ponytail") {
        return None;
    }
    match parts.next().unwrap_or("") {
        "default" => parts.next().and_then(normal_level).map(PromptAction::Default),
        "off" => Some(PromptAction::Off),
        "" => match live.filter(|level| *level != "off") {
            Some(level) => Some(PromptAction::Report(level.to_string())),
            None => Some(PromptAction::Set(if default == "off" { DEFAULT_LEVEL.to_string() } else { default.to_string() })),
        },
        word @ ("lite" | "full" | "ultra") => Some(PromptAction::Set(word.to_string())),
        _ => None,
    }
}

#[derive(Debug, PartialEq)]
pub enum PromptAction {
    Off,
    Default(String),
    Report(String),
    Set(String),
}

fn track() {
    let prompt = json::field(&read_stdin(), "prompt").unwrap_or_default();
    match answer_prompt(&prompt, live_level().as_deref(), &default_level()) {
        Some(PromptAction::Off) => {
            // Kept as "off", not deleted: a deleted file would read as the default level in a status line.
            set_level("off");
            crate::journal::event(host(), SESSION.get().map_or("", String::as_str), "🦫", "Beaver off");
            print!("BEAVER MODE OFF");
        }
        Some(PromptAction::Default(level)) => {
            match write_default_level(&level) {
                Ok(()) => print!("BEAVER DEFAULT SET — new sessions start in {level}."),
                Err(why) => print!("BEAVER DEFAULT NOT SET — {why}"),
            }
        }
        Some(PromptAction::Report(level)) => print!("BEAVER MODE ACTIVE — level: {level}"),
        Some(PromptAction::Set(level)) => {
            set_level(&level);
            crate::journal::event(host(), SESSION.get().map_or("", String::as_str), "🦫", &format!("Beaver set to {level}"));
            print!("BEAVER MODE CHANGED — level: {level}");
        }
        None => {}
    }
}

pub fn run(args: &[String]) {
    match args.first().map(String::as_str) {
        Some("activate") => activate(),
        Some("subagent") => subagent(),
        Some("track") => track(),
        _ => {
            eprintln!("usage: dam beaver activate | subagent | track");
            std::process::exit(2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_skill_keeps_only_its_level() {
        let body = "---\nname: x\n---\n# Title\n| **lite** | a |\n| **full** | b |\n| **Rule** | c |\n- lite: \"x\"\n- full: \"y\"\n- Full: a plain rule\n- other: \"z\"\n";
        let out = filter_skill(body, "full");
        assert!(out.starts_with("# Title"));
        assert!(!out.contains("name: x") && !out.contains("| a |") && !out.contains("\"x\""));
        for kept in ["| b |", "| c |", "\"y\"", "a plain rule", "\"z\""] {
            assert!(out.contains(kept), "{kept}");
        }
    }

    #[test]
    fn prompts_ask_what_upstream_lets_them() {
        let set = |w: &str| Some(PromptAction::Set(w.to_string()));
        assert_eq!(answer_prompt("/beaver ultra", None, "full"), set("ultra"));
        assert_eq!(answer_prompt("/the-dam:beaver lite", None, "full"), set("lite"));
        assert_eq!(answer_prompt("Stop beaver.", None, "full"), Some(PromptAction::Off));
        assert_eq!(answer_prompt("/ponytail ultra", None, "full"), set("ultra"));
        assert_eq!(answer_prompt("/the-dam:ponytail lite", None, "full"), set("lite"));
        assert_eq!(answer_prompt("$PONYTAIL", None, "lite"), set("lite"));
        assert_eq!(answer_prompt("/ponytail", None, "off"), set("full"));
        assert_eq!(answer_prompt("/ponytail", Some("ultra"), "full"), Some(PromptAction::Report("ultra".to_string())));
        assert_eq!(answer_prompt("/ponytail off", None, "full"), Some(PromptAction::Off));
        assert_eq!(answer_prompt("Stop ponytail!", None, "full"), Some(PromptAction::Off));
        assert_eq!(answer_prompt("/ponytail default lite", None, "full"), Some(PromptAction::Default("lite".to_string())));
        for ordinary in ["add a normal mode toggle", "/ponytail-review", "/beaver-review", "the beaver builds dams", "/ponytail banana", "fix the ponytail", "", "/ponytail default review"] {
            assert_eq!(answer_prompt(ordinary, None, "full"), None, "{ordinary}");
        }
    }

    #[test]
    fn each_host_is_told_apart_and_answered_in_its_shape() {
        let some = |s: &str| Some(s.to_string());
        assert_eq!(host_from(None, some("C:/Users/x/.claude/plugins/cache/the-dam"), None), Host::Claude);
        assert_eq!(host_from(None, some("/h/.codex/plugins/the-dam"), some("/h/.codex/data")), Host::Codex);
        assert_eq!(host_from(some("/h/.copilot/data"), None, some("/x")), Host::Copilot);
        assert_eq!(host_from(None, some("C:\\Users\\x\\.vscode\\agent-plugins\\the-dam"), None), Host::Copilot);
        assert_eq!(start_output(Host::Codex, "rules"), "rules");
        assert_eq!(start_output(Host::Copilot, "a \"b\""), r#"{"additionalContext":"a \"b\""}"#);
    }

    #[test]
    fn scoping_is_by_words() {
        assert!(matches_agent("explore|general", "general-purpose"));
        assert!(matches_agent("^general$", "general"));
        assert!(!matches_agent("explore", "planner"));
        assert!(matches_agent("explore", ""));
    }

    #[test]
    fn the_map_lists_public_names_by_folder_and_skips_tests() {
        let root = std::env::temp_dir().join(format!("dam-map-{}", std::process::id()));
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("tests")).unwrap();
        fs::write(root.join("src/a.py"), "def visible():\n    def inner(): pass\n\ndef _hidden(): pass\nclass Thing:\n    def method(self): pass\n").unwrap();
        fs::write(root.join("src/b.rs"), "pub fn open_it() {}\nfn private_one() {}\n").unwrap();
        fs::write(root.join("tests/t.py"), "def nope(): pass\n").unwrap();
        let files = ["src/a.py", "src/b.rs", "tests/t.py"].map(String::from);
        let map = codebase_map(&files, &root);
        assert!(map.contains("src/: visible, Thing, open_it"), "{map}");
        assert!(!map.contains("nope") && !map.contains("_hidden") && !map.contains("private_one") && !map.contains("inner"));
        fs::remove_dir_all(root).unwrap();
    }
}
