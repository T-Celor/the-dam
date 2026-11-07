// Some managed setups (enterprise policies) block plugin hooks or modules, so nothing the dam adds reaches the model.
// Each channel leaves a heartbeat when it runs; `te doctor` reads them, `te context` prints what injection would have
// added, and `te fallback on` puts one standing instruction in the user's instructions file to fetch it: CLAUDE.md,
// Codex's AGENTS.md or Copilot's copilot-instructions.md. Off unless turned on.
pub use crate::beaver::Host;
use crate::context::Ctx;
use crate::economizer::state_root;
use crate::paths::abspath;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const HOOKS: &str = "hooks";
pub const MODULE: &str = "module";
const START: &str = "<!-- the-dam:fallback:start -->";
const END: &str = "<!-- the-dam:fallback:end -->";
const GRAPH: &str = ".dam-graph/graph.tsv";

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |at| at.as_secs())
}

fn beat_path(root: &Path, channel: &str) -> PathBuf {
    root.join("channels").join(channel)
}

// Best effort: a heartbeat that cannot be written only makes `te doctor` less sure.
pub fn beat(channel: &str) {
    let path = beat_path(&abspath(&state_root()), channel);
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = fs::write(path, now().to_string());
}

// Seconds since the channel last ran, None when it never has on this machine.
pub fn age(root: &Path, channel: &str) -> Option<u64> {
    let at: u64 = fs::read_to_string(beat_path(root, channel)).ok()?.trim().parse().ok()?;

    Some(now().saturating_sub(at))
}

fn managed_settings_paths() -> Vec<PathBuf> {
    if cfg!(windows) {
        let base = std::env::var("ProgramFiles").unwrap_or_else(|_| "C:\\Program Files".into());
        vec![PathBuf::from(base).join("ClaudeCode").join("managed-settings.json"), PathBuf::from("C:\\ProgramData\\ClaudeCode\\managed-settings.json")]
    } else if cfg!(target_os = "macos") {
        vec![PathBuf::from("/Library/Application Support/ClaudeCode/managed-settings.json")]
    } else {
        vec![PathBuf::from("/etc/claude-code/managed-settings.json")]
    }
}

fn home() -> PathBuf {
    std::env::var("USERPROFILE").or_else(|_| std::env::var("HOME")).map_or_else(|_| PathBuf::from("."), PathBuf::from)
}

// `$NAME` when set, else `~/<dot>`: each host's own folder.
pub fn claude_dir() -> PathBuf {
    host_dir("CLAUDE_CONFIG_DIR", ".claude")
}

fn host_dir(var: &str, dot: &str) -> PathBuf {
    std::env::var(var).ok().filter(|dir| !dir.is_empty()).map_or_else(|| home().join(dot), PathBuf::from)
}

// `key = true` (or false) under `[section]` of a TOML file; "" is the top level.
// shortcut: reads plain `key = value` lines only, no inline tables or dotted keys; use a TOML parser if Codex grows them.
fn toml_flag(text: &str, section: &str, key: &str) -> Option<bool> {
    let mut current = String::new();
    for line in text.lines().map(|line| line.split('#').next().unwrap_or("").trim()) {
        if let Some(name) = line.strip_prefix('[').and_then(|rest| rest.strip_suffix(']')) {
            current = name.trim().to_string();
        } else if let Some((name, value)) = line.split_once('=') {
            if current == section && name.trim() == key {
                return value.trim().parse().ok();
            }
        }
    }

    None
}

// What keeps a host's plugin hooks from running, as far as its files say. Copilot's `disableAllHooks` skips only the
// file that sets it, and its organization policies are not on disk, so for Copilot only the heartbeat tells.
pub fn blocking_policies(host: Host) -> Vec<String> {
    match host {
        Host::Claude => claude_policies(),
        Host::Codex => {
            let mut found = Vec::new();
            let requirements = PathBuf::from("/etc/codex/requirements.toml");
            if fs::read_to_string(&requirements).is_ok_and(|text| toml_flag(&text, "", "allow_managed_hooks_only") == Some(true)) {
                found.push(format!("allow_managed_hooks_only in {}", requirements.display()));
            }
            let config = host_dir("CODEX_HOME", ".codex").join("config.toml");
            let text = fs::read_to_string(&config).unwrap_or_default();
            for key in ["hooks", "codex_hooks"] {
                if toml_flag(&text, "features", key) == Some(false) {
                    found.push(format!("[features] {key} = false in {}", config.display()));
                }
            }
            found
        }
        Host::Copilot => Vec::new(),
    }
}

fn claude_policies() -> Vec<String> {
    let mut found = Vec::new();
    for path in managed_settings_paths() {
        let Some(settings) = fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str::<serde_json::Value>(text.trim_start_matches('\u{feff}')).ok()) else { continue };
        for key in ["disableAllHooks", "allowManagedHooksOnly"] {
            if settings.get(key) == Some(&serde_json::Value::Bool(true)) {
                found.push(format!("{key} in {}", path.display()));
            }
        }
    }

    found
}

fn graph_nudge(program: &str, built: bool) -> String {
    let first = if built { String::new() } else { format!(" The map is built on the first query (a few seconds); `\"{program}\" graph update` rebuilds it after edits.") };

    format!("A map of this project: its code, its documents and its settings. To find where something is defined or described, what it calls or what calls it, or how parts connect, run `\"{program}\" graph query \"<name or question>\"` first, before Grep, Glob or Read sweeps: it answers in a few hundred tokens.{first}")
}

// The one `dam` program running now, as the model would type it.
pub fn program() -> String {
    std::env::current_exe().map_or_else(|_| "dam".to_string(), |exe| exe.to_string_lossy().replace('\\', "/"))
}

// Out of git's sight without touching the project's own .gitignore, as the module does before a build.
fn exclude_graph(cwd: &Path) {
    let Ok(out) = std::process::Command::new("git").args(["rev-parse", "--git-path", "info/exclude"]).current_dir(cwd).output() else { return };
    if !out.status.success() {
        return;
    }
    let path = cwd.join(String::from_utf8_lossy(&out.stdout).trim());
    // A file it cannot read as text is left alone rather than replaced.
    let Ok(text) = read_existing(&path) else { return };
    if !text.contains(".dam-graph") {
        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let _ = fs::write(&path, format!("{}\n.dam-graph/\n", text.trim_end()).trim_start());
    }
}

// What the hooks and the module add to a session in `cwd`, as one text: learned facts, the map, Beaver's rules.
pub fn context(ctx: &Ctx, cwd: &Path) -> String {
    let mut parts = Vec::new();
    if ctx.config.text("microskills") != "off" {
        let injection = ctx.store.read("injection.json").unwrap_or_default();
        if let Some(text) = injection.get("rendered").and_then(|value| value.as_str()).filter(|text| !text.is_empty()) {
            parts.push(text.to_string());
        }
    }
    parts.extend(map_line(cwd));
    // Without hooks nothing condenses an output on its way back, so the model is shown how.
    let dam = program();
    parts.push(format!("Long command outputs: pipe them through `\"{dam}\" squeeze` (for example `cargo test 2>&1 | \"{dam}\" squeeze`). It keeps every line that differs and condenses the repeated ones, with the path of the full output."));
    if let Some(beaver) = crate::beaver::context_text() {
        parts.push(beaver);
    }

    parts.join("\n\n")
}

// The map's command, for a project worth mapping: what the module tells Claude Code, and Beaver's start hook the others.
pub fn map_line(cwd: &Path) -> Option<String> {
    if crate::graph::source_files().len() < 3 {
        return None;
    }
    exclude_graph(cwd);

    Some(graph_nudge(&program(), cwd.join(GRAPH).is_file()))
}

pub fn host_name(host: Host) -> &'static str {
    match host {
        Host::Claude => "claude",
        Host::Codex => "codex",
        Host::Copilot => "copilot",
    }
}

pub fn parse_host(name: &str) -> Option<Host> {
    [Host::Claude, Host::Codex, Host::Copilot].into_iter().find(|host| host_name(*host) == name)
}

// The heartbeat of a host's hooks; Claude Code's keeps its first name.
pub fn hooks_channel(host: Host) -> String {
    match host {
        Host::Claude => HOOKS.to_string(),
        other => format!("{HOOKS}-{}", host_name(other)),
    }
}

// The user-level instructions file each host reads in every project, hooks or not.
pub fn instructions_file(host: Host) -> PathBuf {
    match host {
        Host::Claude => host_dir("CLAUDE_CONFIG_DIR", ".claude").join("CLAUDE.md"),
        Host::Codex => host_dir("CODEX_HOME", ".codex").join("AGENTS.md"),
        Host::Copilot => host_dir("COPILOT_HOME", ".copilot").join("copilot-instructions.md"),
    }
}

// The program itself, not the `sh` launcher: Codex and Copilot may run commands in PowerShell on Windows.
fn block(host: Host, program: &str) -> String {
    // The model's shell may not carry the host's variables, so the host is named: its state is its own.
    let (tool, flag, off) = match host {
        Host::Claude => ("the Bash tool", String::new(), "`/the-dam:fallback off`".to_string()),
        other => ("your shell", format!(" --host {}", host_name(other)), format!("the dam's `fallback` skill, or `\"{program}\" te fallback off --host {}`", host_name(other))),
    };

    format!("{START}\nThe dam plugin cannot add its context in this environment (plugin hooks are blocked), so fetch it: at the start of each task in a project, run `\"{program}\" te context{flag}` once with {tool} (in PowerShell: `& \"{program}\" te context{flag}`), and follow what it prints (this repository's learned facts, the project map's query command, how to condense long outputs, the coding rules). Turn this off with {off}.\n{END}")
}

// `text` with the fallback block replaced, added (Some) or removed (None). Markers that do not pair up (one edited
// away, or two blocks) are refused rather than guessed: removing from a stray start would take the person's lines.
pub fn with_block(text: &str, block: Option<&str>) -> Result<String, String> {
    let kept = match (text.find(START), text.matches(START).count()) {
        (None, _) if !text.contains(END) => text.to_string(),
        (Some(start), 1) => match text[start..].find(END).map(|end| start + end) {
            Some(end) if text.matches(END).count() == 1 => format!("{}{}", text[..start].trim_end(), &text[end + END.len()..]),
            _ => return Err(format!("its `{START}` and `{END}` lines do not pair up: fix them by hand")),
        },
        _ => return Err(format!("its `{START}` and `{END}` lines do not pair up: fix them by hand")),
    };
    let kept = kept.trim_end();

    Ok(match block {
        Some(block) if kept.is_empty() => format!("{block}\n"),
        Some(block) => format!("{kept}\n\n{block}\n"),
        None if kept.is_empty() => String::new(),
        None => format!("{kept}\n"),
    })
}

// A file's text, "" when it does not exist; any other failure (not text, locked) is an error, never an empty file.
fn read_existing(path: &Path) -> Result<String, String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(format!("{}: {error}; left as it is", path.display())),
    }
}

pub fn is_on(host: Host) -> bool {
    fs::read_to_string(instructions_file(host)).is_ok_and(|text| text.contains(START))
}

// Turns the fallback on or off for `host`; the message says what changed.
pub fn set(host: Host, on: bool) -> Result<String, String> {
    let path = instructions_file(host);
    let text = read_existing(&path)?;
    let block = if on {
        let program = std::env::current_exe().map_err(|error| format!("this program's path is unknown: {error}"))?;
        Some(self::block(host, &program.to_string_lossy().replace('\\', "/")))
    } else {
        None
    };
    let next = with_block(&text, block.as_deref()).map_err(|why| format!("{}: {why}", path.display()))?;
    if next == text {
        return Ok(format!("fallback already {} ({})", if on { "on" } else { "off" }, path.display()));
    }
    // Whole or not at all: a crash midway never leaves the person's file cut short.
    crate::paths::write_atomic(&path, next.as_bytes()).map_err(|error| format!("{}: {error}", path.display()))?;

    Ok(if on {
        format!("fallback on: one instruction added to {}; new sessions fetch the dam's context with `te context`", path.display())
    } else {
        format!("fallback off: the dam's instruction removed from {}", path.display())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_block_is_added_once_replaced_and_removed_without_touching_the_rest() {
        let mine = "# My rules\n\nBe brief.\n";
        let on = with_block(mine, Some(&block(Host::Claude, "/a/te"))).unwrap();
        assert!(on.starts_with("# My rules\n\nBe brief.\n\n<!-- the-dam:fallback:start -->"));
        let again = with_block(&on, Some(&block(Host::Claude, "/b/te"))).unwrap();
        assert_eq!(again.matches(START).count(), 1);
        assert!(again.contains("\"/b/te\" te context`") && !again.contains("/a/te"));
        assert_eq!(with_block(&again, None).unwrap(), mine);
        assert_eq!(with_block("", None).unwrap(), "");
        assert_eq!(with_block("", Some("B")).unwrap(), "B\n");
    }

    #[test]
    fn markers_that_do_not_pair_up_are_refused_never_guessed() {
        // A start left alone, then the person's own lines: removing from it would take those lines.
        let stray = format!("# Mine\n{START}\nmy rule\n");
        assert!(with_block(&stray, None).is_err());
        assert!(with_block(&stray, Some("B")).is_err());
        assert!(with_block(&format!("{END}\nmine\n{START}\n"), None).is_err());
        let two = format!("{START}\n{END}\n{START}\n{END}\n");
        assert!(with_block(&two, None).is_err());
    }

    #[test]
    fn a_file_it_cannot_read_is_never_replaced() {
        let dir = crate::session::TempDir::new("dam-channels-").unwrap();
        let path = dir.0.join("CLAUDE.md");
        assert_eq!(read_existing(&path).unwrap(), "");
        fs::write(&path, [0xff, 0xfe, b'x', 0]).unwrap();
        assert!(read_existing(&path).is_err());
    }

    #[test]
    fn each_host_gets_its_own_file_and_way_out() {
        assert!(instructions_file(Host::Codex).ends_with(".codex/AGENTS.md") || std::env::var("CODEX_HOME").is_ok());
        assert!(instructions_file(Host::Copilot).ends_with(".copilot/copilot-instructions.md") || std::env::var("COPILOT_HOME").is_ok());
        let codex = block(Host::Codex, "/p/dam");
        assert!(codex.contains("with your shell") && codex.contains("fallback off --host codex") && !codex.contains("/the-dam:"));
        assert!(codex.contains("te context --host codex`") && !block(Host::Claude, "/p").contains("--host"));
        assert!(block(Host::Claude, "/p").contains("`/the-dam:fallback off`"));
        assert_eq!(parse_host("copilot"), Some(Host::Copilot));
        assert_eq!(parse_host("pi"), None);
        assert_eq!(hooks_channel(Host::Claude), "hooks");
        assert_eq!(hooks_channel(Host::Codex), "hooks-codex");
    }

    #[test]
    fn a_codex_flag_is_read_from_its_section() {
        let config = "model = \"x\"\n[features]\nhooks = false # off\n[other]\nhooks = true\n";
        assert_eq!(toml_flag(config, "features", "hooks"), Some(false));
        assert_eq!(toml_flag(config, "other", "hooks"), Some(true));
        assert_eq!(toml_flag("allow_managed_hooks_only = true\n", "", "allow_managed_hooks_only"), Some(true));
        assert_eq!(toml_flag(config, "features", "codex_hooks"), None);
    }

    #[test]
    fn a_heartbeat_reads_back_as_an_age() {
        let dir = crate::session::TempDir::new("dam-channels-").unwrap();
        assert_eq!(age(&dir.0, HOOKS), None);
        fs::create_dir_all(dir.0.join("channels")).unwrap();
        fs::write(dir.0.join("channels").join(HOOKS), (now() - 90).to_string()).unwrap();
        assert!(age(&dir.0, HOOKS).is_some_and(|age| (90..100).contains(&age)));
    }
}
