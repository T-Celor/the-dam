// Each host's tool-pruning settings (Apache-2.0, Microsoft), rewritten from plugin/denylist.py: Claude Code's deny list in
// .claude/settings.local.json, Codex's capability toggles in .codex/config.toml, Copilot's disabled MCP servers in
// .github/copilot/settings.local.json. A host's file is written only for that host, and only where it reads it.
use crate::channels::Host;
use crate::economizer::{now, python_truthy, Store};
use crate::paths::realpath;
use fancy_regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

const APPLIED: &str = "untool_applied.json";

// Per settings file, in the order the record holds them.
type ByPath = Vec<(String, Vec<String>)>;

// _SETTINGS: the file each host reads, relative to the checkout.
fn settings_rel(host: Host) -> &'static str {
    match host {
        Host::Claude => ".claude/settings.local.json",
        Host::Codex => ".codex/config.toml",
        Host::Copilot => ".github/copilot/settings.local.json",
    }
}

pub fn settings_path(repo_root: &str, host: Host) -> PathBuf {
    settings_rel(host).split('/').fold(PathBuf::from(repo_root), |path, part| path.join(part))
}

// _UNITS: what one entry removes on each host.
pub fn unit(host: Host) -> &'static str {
    match host {
        Host::Claude => "tool",
        Host::Codex => "capability",
        Host::Copilot => "MCP server",
    }
}

// CODEX_TOGGLES: Codex capability -> (table, or None for the top level; key; TOML value). Each probed project-local on
// 0.153.1. `web` and `request_user_input` are a person's tools, `skills` takes effect user-level only: none is offered.
type Toggle = (Option<&'static str>, &'static str, &'static str);
pub const CODEX_TOGGLES: &[(&str, &[Toggle])] = &[
    ("collaboration", &[(None, "include_collaboration_mode_instructions", "false"), (Some("agents"), "enabled", "false"), (Some("features"), "multi_agent", "false")]),
    ("goals", &[(Some("features"), "goals", "false")]),
    ("image_media", &[(Some("features"), "view_image", "false")]),
];
const MARK: &str = "# tokeneconomizer";

pub fn codex_capabilities() -> Vec<String> {
    CODEX_TOGGLES.iter().map(|(name, _)| name.to_string()).collect()
}

fn canon(path: &str) -> String {
    if path.is_empty() { String::new() } else { realpath(Path::new(path)).to_string_lossy().into_owned() }
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value.and_then(Value::as_array).map_or_else(Vec::new, |items| items.iter().filter_map(Value::as_str).map(str::to_string).collect())
}

fn put(by: &mut ByPath, path: String, items: Vec<String>) {
    match by.iter_mut().find(|(known, _)| *known == path) {
        Some(entry) => entry.1 = items,
        None => by.push((path, items)),
    }
}

fn paths_of(map: Option<&Value>) -> ByPath {
    let mut by = Vec::new();
    for (path, items) in map.and_then(Value::as_object).into_iter().flatten() {
        put(&mut by, canon(path), strings(Some(items)));
    }

    by
}

// The record holds one file's entries as {"tools", "path"} up to 0.1.0; that shape is still read.
fn applied_by_path(doc: &Value) -> ByPath {
    if doc.get("by_path").is_some_and(Value::is_object) {
        return paths_of(doc.get("by_path"));
    }
    let mut by = Vec::new();
    if doc.get("tools").is_some_and(python_truthy) {
        put(&mut by, canon(doc.get("path").and_then(Value::as_str).unwrap_or("")), strings(doc.get("tools")));
    }

    by
}

fn listing(by: &ByPath) -> Value {
    Value::Object(by.iter().filter(|(_, items)| !items.is_empty()).map(|(path, items)| {
        let mut sorted = items.clone();
        sorted.sort();
        (path.clone(), json!(sorted))
    }).collect())
}

fn write_applied(store: &Store, by: &ByPath, kept: &ByPath) -> Result<(), String> {
    store.write(APPLIED, &json!({"by_path": listing(by), "kept": listing(kept), "updated": now()})).map_err(|e| e.to_string())
}

fn applied_at(store: &Store, path: &Path) -> Vec<String> {
    let by = applied_by_path(&store.read(APPLIED).unwrap_or(Value::Null));
    let find = |key: &str| by.iter().find(|(known, _)| known == key).map(|(_, items)| items.clone()).filter(|items| !items.is_empty());

    find(&canon(&path.to_string_lossy())).or_else(|| find("")).unwrap_or_default()
}

// What the plugin added to this checkout's settings file.
pub fn applied_for(store: &Store, repo_root: &str, host: Host) -> Vec<String> {
    applied_at(store, &settings_path(repo_root, host))
}

// What the person gave back in this checkout, which `auto` must leave alone.
pub fn kept_for(store: &Store, repo_root: &str, host: Host) -> Vec<String> {
    let kept = paths_of(store.read(APPLIED).unwrap_or(Value::Null).get("kept"));
    let wanted = canon(&settings_path(repo_root, host).to_string_lossy());

    kept.into_iter().find(|(path, _)| *path == wanted).map(|(_, items)| items).unwrap_or_default()
}

// Marks entries as given back (`restore`), or no longer (`apply` by name).
pub fn set_kept(store: &Store, repo_root: &str, host: Host, add: &[String], remove: &[String]) -> Result<(), String> {
    let doc = store.read(APPLIED).unwrap_or(Value::Null);
    let mut kept = paths_of(doc.get("kept"));
    let path = canon(&settings_path(repo_root, host).to_string_lossy());
    let mut now_kept: BTreeSet<String> = kept.iter().find(|(known, _)| *known == path).map(|(_, items)| items.iter().cloned().collect()).unwrap_or_default();
    now_kept.extend(add.iter().cloned());
    now_kept.retain(|item| !remove.contains(item));
    put(&mut kept, path, now_kept.into_iter().collect());

    write_applied(store, &applied_by_path(&doc), &kept)
}

fn record(store: &Store, items: &BTreeSet<String>, path: &Path) -> Result<(), String> {
    let doc = store.read(APPLIED).unwrap_or(Value::Null);
    let mut by = applied_by_path(&doc);
    by.retain(|(known, _)| !known.is_empty());
    put(&mut by, canon(&path.to_string_lossy()), items.iter().cloned().collect());

    write_applied(store, &by, &paths_of(doc.get("kept")))
}

// The settings object at `path`: empty when there is no file. One that is there but is not a JSON
// object is refused rather than written over -- it is the person's.
fn read_settings(path: &Path) -> Result<Map<String, Value>, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    };
    let text = String::from_utf8(bytes).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(format!("refusing to edit {}: it is not a JSON object", path.display())),
        Err(e) => Err(format!("refusing to edit {}: it is not valid JSON ({e}); fix it first", path.display())),
    }
}

// A write must not leave the repository through a symlink: a checkout decides what these paths are.
fn guard_target(repo_root: &str, path: &Path) -> Result<(), String> {
    let low = |p: PathBuf| if cfg!(windows) { PathBuf::from(p.to_string_lossy().to_lowercase()) } else { p };
    let root = low(realpath(Path::new(repo_root)));
    let parent = low(realpath(path.parent().unwrap_or(Path::new(""))));
    if fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) || !parent.starts_with(&root) {
        return Err(format!("refusing to write {}: it is, or goes through, a symlink that leads out of the repository", path.display()));
    }

    Ok(())
}

// json.dump(indent=2) with the keys in their order; non-ASCII stays as written (a person's settings keep their characters).
pub(crate) fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

pub(crate) fn write_json(path: &Path, value: &Value) -> std::io::Result<()> {
    crate::paths::write_atomic(path, (pretty(value) + "\n").as_bytes())
}

// `.git/info/exclude` is the repository's local-only ignore: a file meant to be per-user is never committed by accident.
fn exclude_from_git(repo_root: &str, rel: &str) -> Option<()> {
    // git says where its exclude file is (worktrees, `gitdir:` files): never a path read from the checkout by hand.
    let asked = std::process::Command::new("git").args(["-c", "core.fsmonitor=false", "rev-parse", "--git-path", "info/exclude"]).current_dir(repo_root).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok().filter(|run| run.status.success())?;
    let path = Path::new(repo_root).join(String::from_utf8_lossy(&asked.stdout).trim());
    let info = path.parent()?.to_path_buf();
    let linked = |p: &Path| fs::symlink_metadata(p).is_ok_and(|meta| meta.file_type().is_symlink());
    if linked(&info) || linked(&path) {
        return None;
    }
    let entry = format!("/{rel}");
    let entry = entry.as_str();
    let existing = if path.exists() { fs::read_to_string(&path).ok()? } else { String::new() };
    if existing.split_whitespace().any(|word| word == entry) {
        return Some(());
    }
    fs::create_dir_all(&info).ok()?;
    let mut file = OpenOptions::new().create(true).append(true).open(&path).ok()?;
    let gap = if existing.is_empty() || existing.ends_with('\n') { "" } else { "\n" };

    write!(file, "{gap}# added by TokenEconomizer (per-user tool deny list)\n{entry}\n").ok()
}

// What the host's file holds the list in: Claude Code's `permissions.deny`, Copilot's top-level `disabledMcpServers`.
fn list_key(host: Host) -> (Option<&'static str>, &'static str) {
    if host == Host::Copilot { (None, "disabledMcpServers") } else { (Some("permissions"), "deny") }
}

fn home_of(host: Host) -> PathBuf {
    match host {
        Host::Codex => codex_home(),
        _ => crate::hosts::copilot_home(),
    }
}

fn place(host: Host) -> &'static str {
    if host == Host::Codex { "project" } else { "folder" }
}

// Does the host read a project's file here? Codex reads `.codex/config.toml` and Copilot `.github/copilot` only in a
// trusted project (verified on 0.153.1 and 1.0.83); Claude Code reads its file anywhere. None: cannot tell.
pub fn trusted(host: Host, repo_root: &str) -> Option<bool> {
    trusted_in(host, repo_root, &home_of(host))
}

fn trusted_in(host: Host, repo_root: &str, home: &Path) -> Option<bool> {
    match host {
        Host::Claude => Some(true),
        Host::Codex => codex_project_trusted(repo_root, home),
        Host::Copilot => copilot_folder_trusted(Path::new(repo_root), home),
    }
}

// Writes `items` (tools, capabilities or MCP servers, by host) into this host's file; returns those newly added.
pub fn apply(repo_root: &str, items: &[String], store: &Store, host: Host) -> Result<Vec<String>, String> {
    apply_in(repo_root, items, store, host, &home_of(host), Some("codex"))
}

// `home` is where the host keeps its trust; `cli` the Codex that checks the edited file (None: not checked).
fn apply_in(repo_root: &str, items: &[String], store: &Store, host: Host, home: &Path, cli: Option<&str>) -> Result<Vec<String>, String> {
    let path = settings_path(repo_root, host);
    guard_target(repo_root, &path)?;
    // Upstream only warned; a file the host would ignore here is not written at all.
    let name = crate::channels::host_name(host);
    match trusted_in(host, repo_root, home) {
        Some(true) => {}
        Some(false) => return Err(format!("{name} reads {} only in a trusted {}, and this one is not: trust it ({name} asks when you start it here), then apply again", path.display(), place(host))),
        None => return Err(format!("cannot tell whether {name} trusts this {} (its configuration in {} is unreadable)", place(host), home.display())),
    }
    if host == Host::Codex {
        return apply_codex(repo_root, &path, items, store, cli);
    }
    let created = !path.exists();
    let (data, added) = with_items(&path, host, items)?;
    if added.is_empty() {
        return Ok(added);
    }
    drop(data);
    // Recorded before written, as `restore` writes before it forgets: whichever write fails, the record still covers every entry of the plugin's in the file.
    let mut all: BTreeSet<String> = applied_at(store, &path).into_iter().collect();
    all.extend(added.iter().cloned());
    record(store, &all, &path)?;
    // Read again right before the write: the person may have edited the file meanwhile.
    let (data, _) = with_items(&path, host, items)?;
    write_json(&path, &Value::Object(data)).map_err(|e| e.to_string())?;
    if created {
        exclude_from_git(repo_root, settings_rel(host));
    }

    Ok(added)
}

// The file's settings with `items` added to the host's list, and those that were not there yet.
fn with_items(path: &Path, host: Host, items: &[String]) -> Result<(Map<String, Value>, Vec<String>), String> {
    let mut data = read_settings(path)?;
    let refuse = |what: &str| format!("refusing to edit {}: {what} is not what a settings file holds there", path.display());
    let (outer, key) = list_key(host);
    let holder = match outer {
        Some(outer) => data.entry(outer).or_insert_with(|| json!({})).as_object_mut().ok_or_else(|| refuse(outer))?,
        None => &mut data,
    };
    let list = holder.entry(key).or_insert_with(|| json!([])).as_array_mut().ok_or_else(|| refuse(key))?;
    let added: Vec<String> = items.iter().filter(|item| !list.iter().any(|rule| rule.as_str() == Some(item.as_str()))).cloned().collect();
    list.extend(added.iter().map(|item| json!(item)));

    Ok((data, added))
}

// Removes entries this plugin added (all of them when `items` is empty); returns those removed. A restore needs no
// trust: it only takes back what the plugin wrote.
pub fn restore(repo_root: &str, store: &Store, items: &[String], host: Host) -> Result<Vec<String>, String> {
    let path = settings_path(repo_root, host);
    let mine: BTreeSet<String> = applied_at(store, &path).into_iter().collect();
    let target: BTreeSet<String> = if items.is_empty() { mine.clone() } else { mine.iter().filter(|item| items.contains(item)).cloned().collect() };
    if target.is_empty() {
        return Ok(Vec::new());
    }
    guard_target(repo_root, &path)?;
    if host == Host::Codex {
        return restore_codex(&path, store, &mine, &target);
    }
    let mut data = read_settings(&path)?;
    let refuse = |what: &str| format!("refusing to edit {}: {what} is not what a settings file holds there", path.display());
    let (outer, key) = list_key(host);
    let (mut removed, mut changed, mut drop_outer) = (Vec::new(), false, false);
    let holder = match outer {
        Some(outer) => match data.get_mut(outer) {
            Some(Value::Object(holder)) => Some(holder),
            Some(other) if python_truthy(other) => return Err(refuse(outer)),
            _ => None,
        },
        None => Some(&mut data),
    };
    if let Some(holder) = holder.filter(|holder| holder.contains_key(key)) {
        let listed = match holder.get(key) {
            Some(Value::Array(rules)) => rules.clone(),
            Some(other) if !python_truthy(other) => Vec::new(),
            _ => return Err(refuse(key)),
        };
        let (gone, kept): (Vec<Value>, Vec<Value>) = listed.into_iter().partition(|rule| rule.as_str().is_some_and(|name| target.contains(name)));
        removed = gone.iter().filter_map(Value::as_str).map(str::to_string).collect();
        if kept.is_empty() {
            holder.shift_remove(key);
        } else {
            holder.insert(key.to_string(), Value::Array(kept));
        }
        drop_outer = outer.is_some() && holder.is_empty();
        changed = true;
    }
    if let Some(outer) = outer.filter(|_| drop_outer) {
        data.shift_remove(outer);
    }
    if changed {
        write_json(&path, &Value::Object(data)).map_err(|e| e.to_string())?;
    }
    record(store, &mine.difference(&target).cloned().collect(), &path)?;

    Ok(removed)
}

// ---- Codex: marked lines in the project's .codex/config.toml (_apply_codex, _restore_codex) ----------------------
static HEADER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^\s*\[\s*([A-Za-z0-9_.\-"]+)\s*\]\s*(#.*)?$"#).expect("fixed"));
const KEY_PART: &str = r#"(?:[A-Za-z0-9_-]+|"[^"]*"|'[^']*')"#;
static TOML_KEY: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"^{KEY_PART}(?:\s*\.\s*{KEY_PART})*\s*=\s*\S")).expect("fixed"));
static TOML_HEADER: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"^\[\[?\s*{KEY_PART}(?:\s*\.\s*{KEY_PART})*\s*\]\]?\s*(?:#.*)?$")).expect("fixed"));
static PROJECT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^\s*\[projects\.(?:"([^"]+)"|'([^']+)')\]\s*$"#).expect("fixed"));
static TRUSTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^\s*trust_level\s*=\s*"trusted""#).expect("fixed"));

fn is_match(re: &Regex, text: &str) -> bool {
    re.is_match(text).unwrap_or(false)
}

fn content(line: &str) -> &str {
    line.trim_end_matches(['\r', '\n'])
}

fn any_header(line: &str) -> bool {
    line.trim_start().starts_with('[')
}

// _sections: the line ranges of the top level (None) and of each [table]; an [[array]] header is no table of ours.
fn sections(lines: &[String]) -> Vec<(Option<String>, usize, usize)> {
    let (mut out, mut current, mut start) = (Vec::new(), None, 0);
    // A line starting with `[` inside a value running over lines (`  [1, 2],`) is no header: only at depth 0.
    let mut depth = 0;
    for (at, line) in lines.iter().enumerate() {
        if depth == 0 && any_header(line) {
            out.push((current, start, at));
            current = Some(HEADER.captures(content(line)).ok().flatten().and_then(|caps| caps.get(1)).map_or_else(|| "[[array]]".to_string(), |name| name.as_str().to_string()));
            start = at + 1;
        } else if depth == 0 {
            depth += depth_change(line.split_once('=').map_or("", |(_, value)| value));
        } else {
            depth += depth_change(line);
        }
        depth = depth.max(0);
    }
    out.push((current, start, lines.len()));

    out
}

fn sets(line: &str, key: &str) -> bool {
    line.trim().strip_prefix(key).is_some_and(|rest| rest.trim_start().starts_with('='))
}

// _key_set: `key` under [table], or a top-level dotted `table.key`. A key the person set is theirs, whatever its value.
fn key_set(lines: &[String], table: Option<&str>, key: &str) -> bool {
    sections(lines).iter().any(|(at, a, b)| lines[*a..*b].iter().any(|line| match (table, at) {
        (None, None) => sets(line, key),
        (Some(table), Some(at)) if at == table => sets(line, key),
        (Some(table), None) => sets(line, &format!("{table}.{key}")),
        _ => false,
    }))
}

// _toml_insert: `key = value` into `table` (None: the top level), marked with `tag`. Lines keep their own endings, and
// no blank line is added, so that removing the marked lines gives the person's bytes back.
fn toml_insert(lines: &mut Vec<String>, newline: &str, table: Option<&str>, key: &str, value: &str, tag: &str) {
    if key_set(lines, table, key) {
        return;
    }
    // `features = { ... }`: an inline table cannot be extended, and a [features] beside it would make the file invalid.
    if table.is_some_and(|table| lines.iter().any(|line| sets(line, table))) {
        return;
    }
    // shortcut: a last line without a newline gets one, which `restore` keeps.
    if let Some(last) = lines.last_mut().filter(|last| !last.ends_with('\n')) {
        last.push_str(newline);
    }
    let line = format!("{key} = {value}  {tag}{newline}");
    let secs = sections(lines);
    let first_header = secs.get(1).map_or(lines.len(), |(_, a, _)| a - 1);
    match table {
        None => lines.insert(first_header, line),
        Some(table) => {
            let header = secs.iter().find(|(at, _, _)| at.as_deref() == Some(table)).map(|(_, a, _)| *a);
            let dotted = lines[..first_header].iter().any(|line| line.trim_start().strip_prefix(table).is_some_and(|rest| rest.starts_with('.')));
            if let Some(at) = header {
                lines.insert(at, line);
            } else if dotted {
                lines.insert(first_header, format!("{table}.{line}"));
            } else {
                lines.push(format!("[{table}]  {tag}{newline}"));
                lines.push(line);
            }
        }
    }
}

// _toml_remove: every line carrying one of `tags`; a [table] header the plugin added stays while a key of the
// person's is under it.
fn toml_remove(lines: &[String], tags: &[String]) -> Vec<String> {
    let marked = |line: &str| tags.iter().any(|tag| content(line).trim_end().ends_with(tag.as_str()));
    let mut keep = Vec::new();
    for (at, line) in lines.iter().enumerate() {
        let foreign_below = || lines[at + 1..].iter().take_while(|next| !any_header(next)).any(|next| !next.trim().is_empty() && !marked(next));
        if !marked(line) || (is_match(&HEADER, content(line)) && foreign_below()) {
            keep.push(line.clone());
        }
    }

    keep
}

// Brackets opened (+) and closed (-) outside strings and comments: what carries a value over to the next line.
fn depth_change(text: &str) -> i64 {
    let (mut depth, mut quote, mut escaped) = (0, None, false);
    for c in text.chars() {
        match quote {
            Some(q) => {
                if escaped {
                    escaped = false;
                } else if c == '\\' && q == '"' {
                    escaped = true;
                } else if c == q {
                    quote = None;
                }
            }
            None => match c {
                '"' | '\'' => quote = Some(c),
                '#' => break,
                '[' | '{' => depth += 1,
                ']' | '}' => depth -= 1,
                _ => {}
            },
        }
    }

    depth
}

// A file the line editor understands: headers, `key = value` lines (a value may run over lines in brackets), comments.
// Anything else, multi-line strings included, is refused rather than edited. Python checked with tomllib; no TOML
// parser is among this crate's dependencies, and Codex itself checks the result (`codex_config_error`).
fn toml_plain(text: &str) -> bool {
    if text.contains("\"\"\"") || text.contains("'''") {
        return false;
    }
    let mut depth = 0;
    for line in text.lines() {
        let line = line.trim();
        if depth == 0 {
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if line.starts_with('[') {
                if !is_match(&TOML_HEADER, line) {
                    return false;
                }
                continue;
            }
            if !is_match(&TOML_KEY, line) {
                return false;
            }
            depth += depth_change(line.split_once('=').map_or("", |(_, value)| value));
        } else {
            depth += depth_change(line);
        }
        if depth < 0 {
            return false;
        }
    }

    depth == 0
}

fn write_text(path: &Path, text: &str) -> std::io::Result<()> {
    crate::paths::write_atomic(path, text.as_bytes())
}

// _git_tracked: is `rel` committed here? Taken as committed when git cannot run, so that its file is left alone; git's
// fsmonitor is off, as a repository's own configuration could name a program for it.
fn git_tracked(repo_root: &str, rel: &str) -> bool {
    use std::process::{Command, Stdio};
    let run = Command::new("git").args(["-c", "core.fsmonitor=false", "ls-files", "--error-unmatch", "--", rel]).current_dir(repo_root).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();

    run.map_or(true, |status| status.success())
}

// codex_config_error: Codex's own complaint about the configuration it would load here, or None. `codex features list`
// loads the whole layered configuration and calls no model. Silent when the CLI is not installed or does not answer.
fn codex_config_error(repo_root: &str, cli: &str) -> Option<String> {
    use std::process::Stdio;
    crate::sidecall::which(cli)?;
    let mut command = crate::sidecall::command(cli).ok()?;
    command.args(["features", "list"]).current_dir(repo_root).env("TE_SIDECALL", "1").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().ok()?;
    // Drained while polling: a pipe nobody reads stalls the child at 64 KB, and the wait runs to the kill.
    fn drain(pipe: Option<impl std::io::Read + Send + 'static>) -> std::thread::JoinHandle<Vec<u8>> {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut buf);
            }
            buf
        })
    }
    let (stdout, stderr) = (drain(child.stdout.take()), drain(child.stderr.take()));
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child.try_wait().ok()? {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    if status.success() {
        return None;
    }
    let (stdout, stderr) = (stdout.join().unwrap_or_default(), stderr.join().unwrap_or_default());
    let said: String = String::from_utf8_lossy(if stderr.is_empty() { &stdout } else { &stderr }).trim().chars().take(500).collect();

    Some(if said.is_empty() { format!("exit {}", status.code().unwrap_or(-1)) } else { said })
}

fn split_lines(text: &str) -> Vec<String> {
    text.split_inclusive('\n').map(str::to_string).collect()
}

fn apply_codex(repo_root: &str, path: &Path, capabilities: &[String], store: &Store, cli: Option<&str>) -> Result<Vec<String>, String> {
    let created = !path.exists();
    // Claude Code's and Copilot's files are per-user by name; a project's .codex/config.toml is often the team's, committed.
    if !created && git_tracked(repo_root, settings_rel(Host::Codex)) {
        return Err(format!("{} is committed in this repository; the plugin will not edit a shared file. Add the toggles by hand, or keep them in your own copy of it.", path.display()));
    }
    let original = if created { String::new() } else { fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))? };
    if !toml_plain(&original) {
        return Err(format!("refusing to edit {}: it is not TOML this plugin can edit line by line; fix it, or add the toggles by hand", path.display()));
    }
    let newline = if original.contains("\r\n") { "\r\n" } else { "\n" };
    let mine: BTreeSet<String> = applied_at(store, path).into_iter().collect();
    let mut lines = split_lines(&original);
    let mut added = Vec::new();
    for capability in capabilities {
        let Some((_, toggles)) = CODEX_TOGGLES.iter().find(|(name, _)| name == capability) else { continue };
        if mine.contains(capability) || added.contains(capability) {
            continue;
        }
        for (table, key, value) in toggles.iter() {
            toml_insert(&mut lines, newline, *table, key, value, &format!("{MARK}:{capability}"));
        }
        added.push(capability.clone());
    }
    if added.is_empty() {
        return Ok(added);
    }
    let text = lines.concat();
    if !toml_plain(&text) {
        return Err(format!("refusing to write {}: the result would not parse as TOML", path.display()));
    }
    let mut all = mine.clone();
    all.extend(added.iter().cloned());
    record(store, &all, path)?;
    write_text(path, &text).map_err(|e| e.to_string())?;
    if let Some(error) = cli.and_then(|cli| codex_config_error(repo_root, cli)) {
        // The check took up to 30 s: take back only our marked lines from what the file holds now, not a stale copy.
        let tags: Vec<String> = added.iter().map(|capability| format!("{MARK}:{capability}")).collect();
        let _ = match fs::read_to_string(path) {
            Ok(now) => {
                let back = toml_remove(&split_lines(&now), &tags).concat();
                if created && back.trim().is_empty() { fs::remove_file(path) } else { write_text(path, &back) }
            }
            Err(_) => Ok(()),
        };
        record(store, &mine, path)?;
        return Err(format!("Codex rejected the edited {}, so it was put back: {error}", path.display()));
    }
    if created {
        exclude_from_git(repo_root, settings_rel(Host::Codex));
    }

    Ok(added)
}

fn restore_codex(path: &Path, store: &Store, mine: &BTreeSet<String>, target: &BTreeSet<String>) -> Result<Vec<String>, String> {
    match fs::read_to_string(path) {
        Ok(text) => {
            let tags: Vec<String> = target.iter().map(|capability| format!("{MARK}:{capability}")).collect();
            let kept = toml_remove(&split_lines(&text), &tags).concat();
            if kept.trim().is_empty() {
                fs::remove_file(path).map_err(|e| e.to_string())?;
            } else if kept != text {
                write_text(path, &kept).map_err(|e| e.to_string())?;
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    }
    record(store, &mine.difference(target).cloned().collect(), path)?;

    Ok(target.iter().cloned().collect())
}

pub fn codex_home() -> PathBuf {
    use crate::economizer::env;
    env("CODEX_HOME").map_or_else(|| PathBuf::from(env("HOME").or_else(|| env("USERPROFILE")).unwrap_or_default()).join(".codex"), PathBuf::from)
}

fn normcase(path: &Path) -> String {
    let text = path.to_string_lossy().into_owned();
    if cfg!(windows) { text.replace('/', "\\").to_lowercase() } else { text }
}

// codex_project_trusted: is this checkout a trusted project in the user's Codex config? None when it is unreadable.
fn codex_project_trusted(repo_root: &str, home: &Path) -> Option<bool> {
    let text = fs::read_to_string(home.join("config.toml")).ok()?;
    let root = Path::new(repo_root);
    let want = [normcase(&crate::paths::abspath(root)), normcase(&realpath(root))];
    let mut current: Option<String> = None;
    for line in text.lines() {
        if let Ok(Some(caps)) = PROJECT.captures(line) {
            // A double-quoted TOML key escapes its backslashes, as in a Windows path Codex wrote.
            current = caps.get(1).map(|path| path.as_str().replace("\\\\", "\\")).or_else(|| caps.get(2).map(|path| path.as_str().to_string())).map(|path| normcase(Path::new(&path)));
            continue;
        }
        if any_header(line) {
            current = None;
            continue;
        }
        if current.as_ref().is_some_and(|path| want.contains(path)) && is_match(&TRUSTED, line) {
            return Some(true);
        }
    }

    Some(false)
}

// copilot_trusted_folders: Copilot's `trustedFolders` (its config.json starts with `//` comment lines), or None.
fn copilot_trusted_folders(home: &Path) -> Option<Vec<String>> {
    let text = fs::read_to_string(home.join("config.json")).ok()?;
    let doc: Value = serde_json::from_str(&text.lines().filter(|line| !line.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n")).ok()?;

    Some(doc.get("trustedFolders").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect())
}

// copilot_folder_trusted: is `path` in or under a folder Copilot trusts? None when its config is unreadable.
fn copilot_folder_trusted(path: &Path, home: &Path) -> Option<bool> {
    let real = normcase(&realpath(path));
    let sep = std::path::MAIN_SEPARATOR;

    Some(copilot_trusted_folders(home)?.iter().map(|folder| normcase(&realpath(Path::new(folder)))).any(|folder| real == folder || real.starts_with(&format!("{}{sep}", folder.trim_end_matches(sep)))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_keeps_non_ascii_as_written() {
        assert!(pretty(&json!({"k": "é✓"})).contains("é✓"));
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dam-denylist-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn apply_and_restore_touch_only_their_own_rules() {
        let dir = scratch("own");
        let (repo, state) = (dir.join("repo"), Store { base: dir.join("state") });
        let root = repo.to_string_lossy().into_owned();
        let path = settings_path(&root, Host::Claude);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, r#"{"theme": "dark", "permissions": {"deny": ["Mine"], "allow": ["x"]}}"#).unwrap();

        let words = |list: &[&str]| list.iter().map(|word| word.to_string()).collect::<Vec<_>>();
        assert_eq!(apply(&root, &words(&["Mine", "A", "B"]), &state, Host::Claude).unwrap(), words(&["A", "B"]));
        assert_eq!(applied_for(&state, &root, Host::Claude), words(&["A", "B"]));
        assert_eq!(restore(&root, &state, &words(&["A"]), Host::Claude).unwrap(), words(&["A"]));
        assert_eq!(restore(&root, &state, &[], Host::Claude).unwrap(), words(&["B"]));
        let after: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(after, json!({"theme": "dark", "permissions": {"deny": ["Mine"], "allow": ["x"]}}));
        assert!(applied_for(&state, &root, Host::Claude).is_empty());
        set_kept(&state, &root, Host::Claude, &words(&["A"]), &[]).unwrap();
        assert_eq!(kept_for(&state, &root, Host::Claude), words(&["A"]));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_settings_file_that_is_not_an_object_is_left_alone() {
        let dir = scratch("refuse");
        let (repo, state) = (dir.join("repo"), Store { base: dir.join("state") });
        let root = repo.to_string_lossy().into_owned();
        let path = settings_path(&root, Host::Claude);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        for text in ["[1]", "{not json"] {
            fs::write(&path, text).unwrap();
            let error = apply(&root, &["A".to_string()], &state, Host::Claude).unwrap_err();
            assert!(error.starts_with("refusing to edit"), "{error}");
            assert_eq!(fs::read_to_string(&path).unwrap(), text);
        }
        let _ = fs::remove_dir_all(&dir);
    }

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|word| word.to_string()).collect()
    }

    // A checkout, its state, and a Codex home that trusts it (`trust`) or knows nothing of it.
    fn codex_fixture(name: &str, trust: bool) -> (PathBuf, String, Store, PathBuf) {
        let dir = scratch(name);
        let (repo, home) = (dir.join("repo"), dir.join("codex-home"));
        fs::create_dir_all(repo.join(".codex")).unwrap();
        fs::create_dir_all(&home).unwrap();
        let root = repo.to_string_lossy().into_owned();
        let project = if trust { format!("[projects.'{}']\ntrust_level = \"trusted\"\n", realpath(&repo).display()) } else { String::new() };
        fs::write(home.join("config.toml"), format!("model = \"gpt-5\"\n{project}")).unwrap();
        (dir.clone(), root, Store { base: dir.join("state") }, home)
    }

    #[test]
    fn codex_toggles_go_in_and_come_out_byte_for_byte() {
        let (dir, root, state, home) = codex_fixture("codex", true);
        let path = settings_path(&root, Host::Codex);
        let mine = "# the team's\r\nmodel = \"o4\"\r\n\r\n[features]\r\nweb_search = true\r\n\r\n[[skills.config]]\r\nname = \"x\"\r\n";
        fs::write(&path, mine).unwrap();
        let added = apply_in(&root, &words(&["collaboration", "goals", "image_media", "web"]), &state, Host::Codex, &home, None).unwrap();
        assert_eq!(added, words(&["collaboration", "goals", "image_media"]));
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("include_collaboration_mode_instructions = false  # tokeneconomizer:collaboration\r\n[features]"), "{text}");
        assert!(text.contains("[features]\r\nview_image = false  # tokeneconomizer:image_media\r\ngoals = false  # tokeneconomizer:goals\r\nmulti_agent = false"), "{text}");
        assert!(text.contains("[agents]  # tokeneconomizer:collaboration\r\nenabled = false") && toml_plain(&text));
        assert_eq!(applied_for(&state, &root, Host::Codex), words(&["collaboration", "goals", "image_media"]));
        assert!(apply_in(&root, &words(&["goals"]), &state, Host::Codex, &home, None).unwrap().is_empty());
        assert_eq!(restore(&root, &state, &words(&["goals"]), Host::Codex).unwrap(), words(&["goals"]));
        assert!(!fs::read_to_string(&path).unwrap().contains("tokeneconomizer:goals"));
        restore(&root, &state, &[], Host::Codex).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), mine);
        assert!(applied_for(&state, &root, Host::Codex).is_empty());
        // A key the person set stays theirs, and a file the plugin created goes when its last line does.
        fs::remove_file(&path).unwrap();
        apply_in(&root, &words(&["goals"]), &state, Host::Codex, &home, None).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "[features]  # tokeneconomizer:goals\ngoals = false  # tokeneconomizer:goals\n");
        restore(&root, &state, &[], Host::Codex).unwrap();
        assert!(!path.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn codex_refuses_an_unparsable_file_and_an_untrusted_project() {
        let (dir, root, state, home) = codex_fixture("codex-refuse", true);
        let path = settings_path(&root, Host::Codex);
        for text in ["notes = \"\"\"\nmulti\n\"\"\"\n", "[features\n", "just words\n"] {
            fs::write(&path, text).unwrap();
            let error = apply_in(&root, &words(&["goals"]), &state, Host::Codex, &home, None).unwrap_err();
            assert!(error.starts_with("refusing to edit"), "{error}");
            assert_eq!(fs::read_to_string(&path).unwrap(), text);
        }
        let (dir2, root2, state2, home2) = codex_fixture("codex-untrusted", false);
        let error = apply_in(&root2, &words(&["goals"]), &state2, Host::Codex, &home2, None).unwrap_err();
        assert!(error.contains("only in a trusted project"), "{error}");
        assert!(!settings_path(&root2, Host::Codex).exists() && applied_for(&state2, &root2, Host::Codex).is_empty());
        // No Codex config at all: trust cannot be told, and nothing is written.
        fs::remove_file(home2.join("config.toml")).unwrap();
        assert!(apply_in(&root2, &words(&["goals"]), &state2, Host::Codex, &home2, None).unwrap_err().starts_with("cannot tell"));
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&dir2);
    }

    #[test]
    fn codex_trust_reads_a_double_quoted_windows_key() {
        let home = scratch("codex-trust");
        let root = r"C:\Work\Repo";
        fs::write(home.join("config.toml"), "[projects.\"c:\\\\work\\\\repo\"]\ntrust_level = \"trusted\"\n[other]\ntrust_level = \"trusted\"\n").unwrap();
        assert_eq!(codex_project_trusted(root, &home), Some(cfg!(windows)));
        assert_eq!(codex_project_trusted(r"C:\Work\Other", &home), Some(false));
        let _ = fs::remove_dir_all(&home);
    }

    fn copilot_fixture(name: &str, trust: bool) -> (PathBuf, String, Store, PathBuf) {
        let dir = scratch(name);
        let (repo, home) = (dir.join("repo"), dir.join("copilot-home"));
        fs::create_dir_all(&repo).unwrap();
        fs::create_dir_all(&home).unwrap();
        let folders = if trust { json!([dir.to_string_lossy()]) } else { json!([]) };
        fs::write(home.join("config.json"), format!("// written by Copilot\n{}", json!({"trustedFolders": folders}))).unwrap();
        (dir.clone(), repo.to_string_lossy().into_owned(), Store { base: dir.join("state") }, home)
    }

    #[test]
    fn copilot_servers_are_disabled_and_given_back() {
        let (dir, root, state, home) = copilot_fixture("copilot", true);
        let path = settings_path(&root, Host::Copilot);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mine = json!({"model": "gpt-5", "disabledMcpServers": ["theirs"], "émoji": "ü"});
        fs::write(&path, mine.to_string()).unwrap();
        assert_eq!(apply_in(&root, &words(&["theirs", "github-mcp-server"]), &state, Host::Copilot, &home, None).unwrap(), words(&["github-mcp-server"]));
        let written: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(written["disabledMcpServers"], json!(["theirs", "github-mcp-server"]));
        assert!(written.get("permissions").is_none());
        assert_eq!(restore(&root, &state, &[], Host::Copilot).unwrap(), words(&["github-mcp-server"]));
        assert_eq!(serde_json::from_str::<Value>(&fs::read_to_string(&path).unwrap()).unwrap(), mine);
        fs::write(&path, "{broken").unwrap();
        assert!(apply_in(&root, &words(&["x"]), &state, Host::Copilot, &home, None).unwrap_err().starts_with("refusing to edit"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "{broken");
        let (dir2, root2, state2, home2) = copilot_fixture("copilot-untrusted", false);
        assert!(apply_in(&root2, &words(&["x"]), &state2, Host::Copilot, &home2, None).unwrap_err().contains("only in a trusted folder"));
        assert!(!settings_path(&root2, Host::Copilot).exists());
        // Nor is another host's file touched.
        assert!(!settings_path(&root, Host::Claude).exists() && !settings_path(&root, Host::Codex).exists());
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&dir2);
    }
}
