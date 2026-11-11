// Which model does what, on the same ladder in each host: the balanced model runs the session, the most capable one advises,
// the smallest does the workers' tasks.
//   Claude Code   Sonnet 5.5 / Opus 5.5 / Haiku 5.5
//   Codex         gpt-6.1-sol / gpt-6-astra / gpt-6-luna   (ids from learn.chatgpt.com/docs/models)
// A plugin cannot set the session model or the advisor (only the person's own settings can), so `dam te models install` writes
// them there, remembering what was in place, and `uninstall` puts it back. Claude Code's workers are pinned by the module.
use crate::beaver::Host;
use serde_json::{json, Map, Value};
use std::path::PathBuf;

pub const MAIN: &str = "claude-sonnet-5-5";
pub const ADVISOR: &str = "claude-opus-5-5";
pub const WORKER: &str = "claude-haiku-5-5";
const KEYS: [(&str, &str); 2] = [("model", MAIN), ("advisorModel", ADVISOR)];

pub const CODEX_MAIN: &str = "gpt-6.1-sol";
pub const CODEX_ADVISOR: &str = "gpt-6-astra";
pub const CODEX_WORKER: &str = "gpt-6-luna";
// Marks a file the dam wrote, so `uninstall` removes those and never one the person wrote.
const MARK: &str = "# managed by the dam (/the-dam:models)";

fn settings_path(host: Host) -> PathBuf {
    match host {
        Host::Codex => crate::channels::codex_dir().join("config.toml"),
        _ => crate::channels::claude_dir().join("settings.json"),
    }
}

fn backup_path(host: Host) -> PathBuf {
    let name = if host == Host::Codex { "models-backup-codex.json" } else { "models-backup.json" };
    crate::economizer::state_root().join(name)
}

// ---- Claude Code: settings.json ----

// `settings` with the two models set (`backup` gets what they were), or put back from `backup`. Other keys are left as they are.
pub fn with_models(settings: &str, on: bool, backup: &mut Map<String, Value>) -> Result<String, String> {
    let mut doc: Map<String, Value> = if settings.trim().is_empty() { Map::new() } else { serde_json::from_str(settings).map_err(|error| format!("not JSON the dam can edit ({error}): left as it is"))? };
    for (key, model) in KEYS {
        if on {
            // Kept once: a second install must not replace the person's own choice with ours.
            backup.entry(key.to_string()).or_insert_with(|| doc.get(key).cloned().unwrap_or(Value::Null));
            doc.insert(key.to_string(), json!(model));
        } else if let Some(was) = backup.remove(key) {
            // Only what is still ours: a model the person changed since is theirs.
            if doc.get(key).and_then(Value::as_str) == Some(model) {
                if was.is_null() {
                    doc.remove(key);
                } else {
                    doc.insert(key.to_string(), was);
                }
            }
        }
    }

    Ok(format!("{}\n", serde_json::to_string_pretty(&Value::Object(doc)).unwrap_or_default()))
}

// ---- Codex: config.toml, edited line by line ----

fn is_header(line: &str) -> bool {
    line.trim_start().starts_with('[')
}

// Whether `line` sets `key` (and not a longer key that starts with it, such as model_reasoning_effort for model).
fn sets(line: &str, key: &str) -> bool {
    line.trim_start().strip_prefix(key).is_some_and(|rest| rest.trim_start().starts_with('='))
}

// Whether `line` is the header `[name]`, ignoring surrounding whitespace and a trailing comment.
fn header_is(line: &str, name: &str) -> bool {
    line.trim().strip_prefix('[').and_then(|rest| rest.split_once(']')).is_some_and(|(inside, after)| inside.trim() == name && (after.trim().is_empty() || after.trim_start().starts_with('#')))
}

// The lines of `table` (None:those before the first header) as (first, end), or None where the table is not there.
fn region(lines: &[String], table: Option<&str>) -> Option<(usize, usize)> {
    let first_header = lines.iter().position(|line| is_header(line)).unwrap_or(lines.len());
    let Some(name) = table else { return Some((0, first_header)) };
    let at = lines.iter().position(|line| header_is(line, name))?;
    let end = lines[at + 1..].iter().position(|line| is_header(line)).map_or(lines.len(), |next| at + 1 + next);

    Some((at + 1, end))
}

// Sets `key = "value"` in `table`, or puts back what was there.
fn edit(lines: &mut Vec<String>, table: Option<&str>, key: &str, value: &str, on: bool, backup: &mut Map<String, Value>, slot: &str) {
    let ours = format!("{key}=\"{value}\"");
    let found = region(lines, table).and_then(|(first, end)| (first..end).find(|&i| sets(&lines[i], key)));
    if on {
        backup.entry(slot.to_string()).or_insert_with(|| found.map_or(Value::Null, |i| json!(lines[i])));
        let line = format!("{key} = \"{value}\"");
        match (found, region(lines, table)) {
            (Some(i), _) => lines[i] = line,
            (None, Some((first, _))) => lines.insert(first, line),
            (None, None) => {
                if lines.last().is_some_and(|last| !last.trim().is_empty()) {
                    lines.push(String::new());
                }
                lines.push(format!("[{}]", table.unwrap_or_default()));
                lines.push(line);
            }
        }
    } else if let Some(was) = backup.remove(slot) {
        // Only a line that is still ours: one the person changed since is theirs.
        let Some(i) = found.filter(|&i| lines[i].replace(' ', "") == ours) else { return };
        match was.as_str() {
            Some(raw) => lines[i] = raw.to_string(),
            None => {
                lines.remove(i);
            }
        }
    }
}

pub fn with_codex_models(config: &str, on: bool, backup: &mut Map<String, Value>) -> Result<String, String> {
    // Plain `key = value` files only, as for the tool pruning: anything else is left as the person wrote it.
    if config.contains("\"\"\"") || config.contains("'''") {
        return Err("it holds multi-line strings, which the dam does not edit: left as it is".to_string());
    }
    let mut lines: Vec<String> = config.lines().map(String::from).collect();
    let top = region(&lines, None).map_or(0, |(_, end)| end);
    if lines[..top].iter().any(|line| line.trim_start().starts_with("agents.") || line.trim_start().starts_with("agents ") || line.trim_start().starts_with("agents=")) {
        return Err("it sets `agents` as a dotted key, which the dam does not edit: left as it is".to_string());
    }
    edit(&mut lines, None, "model", CODEX_MAIN, on, backup, "model");
    edit(&mut lines, Some("agents"), "default_subagent_model", CODEX_WORKER, on, backup, "default_subagent_model");

    Ok(format!("{}\n", lines.join("\n")))
}

fn codex_agent(name: &str) -> (PathBuf, String) {
    let path = crate::channels::codex_dir().join("agents").join(format!("{name}.toml"));
    let body = if name == "advisor" {
        format!("{MARK}\nname = \"advisor\"\ndescription = \"Senior advisor for decisions that are hard to reverse: architecture, a difficult bug, a security-sensitive change, or two findings that disagree. Read-only.\"\nmodel = \"{CODEX_ADVISOR}\"\nmodel_reasoning_effort = \"high\"\ndeveloper_instructions = \"\"\"\nYou are a senior advisor with no memory of the conversation: you know only what you are given and what you read. You read, you never edit. To review a change, read the changed files and the tests, not the summary, and report what is wrong first, one line each: Blocker, Major or Nit, file:line, what to do. To decide, give one recommendation, the reason in two or three lines, and the main risk. If the evidence is not enough, say what is missing instead of guessing. At most 12 lines, no preamble.\n\"\"\"\n")
    } else {
        format!("{MARK}\nname = \"worker\"\ndescription = \"Fast, cheap worker for scoped tasks that need no deep reasoning: searching files, running a command and summarising its output, a described mechanical edit.\"\nmodel = \"{CODEX_WORKER}\"\nmodel_reasoning_effort = \"low\"\ndeveloper_instructions = \"\"\"\nYou are a worker. Do exactly the task you are given, nothing more. Report in at most 8 lines: what you found or changed, the file paths, and anything you could not do. If you changed code, end with one line saying what a reviewer should check first. No preamble.\n\"\"\"\n")
    };

    (path, body)
}

// The two agent files: written where none is, removed only where the dam wrote it.
fn codex_agents(on: bool) -> Vec<String> {
    let mut notes = Vec::new();
    for name in ["advisor", "worker"] {
        let (path, body) = codex_agent(name);
        let existing = std::fs::read_to_string(&path).ok();
        match (on, existing) {
            (true, None) => {
                if crate::paths::write_atomic(&path, body.as_bytes()).is_ok() {
                    notes.push(format!("agent {name} ({})", if name == "advisor" { CODEX_ADVISOR } else { CODEX_WORKER }));
                }
            }
            (true, Some(text)) if text.starts_with(MARK) => {
                let _ = crate::paths::write_atomic(&path, body.as_bytes());
            }
            (false, Some(text)) if text.starts_with(MARK) => {
                let _ = std::fs::remove_file(&path);
            }
            _ => {}
        }
    }

    notes
}

// ---- both ----

pub fn show(host: Host) -> String {
    let path = settings_path(host);
    match host {
        Host::Codex => {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            let value = |key: &str| text.lines().find(|line| sets(line, key)).and_then(|line| line.split_once('=')).map_or("not set".to_string(), |(_, v)| v.trim().trim_matches('"').to_string());
            format!("session model   {} (wanted: {CODEX_MAIN})\nsubagents       {} (wanted: {CODEX_WORKER})\nadvisor         the `advisor` agent, {CODEX_ADVISOR}; the `worker` agent is {CODEX_WORKER}\nconfig          {}", value("model"), value("default_subagent_model"), path.display())
        }
        Host::Copilot => "Copilot CLI: the plugin's `advisor` and `worker` custom agents ship with it (agents/, Opus 5.5 and Haiku 5.5). The session model's settings key is not documented well enough to edit safely; use `--model` or `/model`.".to_string(),
        Host::Claude => {
            let doc: Value = std::fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or(Value::Null);
            let now = |key: &str| doc[key].as_str().unwrap_or("not set").to_string();
            format!("session model   {} (wanted: {MAIN})\nadvisor         {} (wanted: {ADVISOR})\nworkers         {WORKER}, pinned by the plugin's module for Claude Code's built-in subagents; the plugin's `worker` agent uses it too\nsettings        {}", now("model"), now("advisorModel"), path.display())
        }
    }
}

// `dam te models install|uninstall`.
pub fn set(host: Host, on: bool) -> Result<String, String> {
    if host == Host::Copilot {
        return Err("Copilot CLI is not supported: use `--model` or `/model`".to_string());
    }
    let (path, backup_file) = (settings_path(host), backup_path(host));
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("{}: {error}; left as it is", path.display())),
    };
    let mut backup: Map<String, Value> = std::fs::read_to_string(&backup_file).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    let next = if host == Host::Codex { with_codex_models(&text, on, &mut backup) } else { with_models(&text, on, &mut backup) }.map_err(|why| format!("{}: {why}", path.display()))?;
    crate::paths::write_atomic(&backup_file, serde_json::to_string_pretty(&Value::Object(backup)).unwrap_or_default().as_bytes()).map_err(|error| format!("{}: {error}", backup_file.display()))?;
    crate::paths::write_atomic(&path, next.as_bytes()).map_err(|error| format!("{}: {error}", path.display()))?;
    let agents = if host == Host::Codex { codex_agents(on) } else { Vec::new() };
    let (main, advisor) = if host == Host::Codex { (CODEX_MAIN, CODEX_ADVISOR) } else { (MAIN, ADVISOR) };
    let extra = if agents.is_empty() { String::new() } else { format!(" and {}", agents.join(", ")) };

    Ok(if on {
        format!("session model {main}, advisor {advisor} written to {}{extra}; they apply from the next {} start (a --model on the command line still wins)", path.display(), if host == Host::Codex { "Codex" } else { "Claude Code" })
    } else {
        format!("the models the dam set were put back in {}", path.display())
    })
}

// The first session after the plugin is installed, in a host whose hooks run here: the models are set once (Claude Code's
// module does the same for itself). Returns a line for the person where it did something.
pub fn first_session(host: Host) -> Option<String> {
    if host != Host::Codex {
        return None;
    }
    let flag = crate::economizer::state_root().join("hosts").join("codex").join("models-applied");
    if flag.exists() {
        return None;
    }
    let done = set(host, true).ok()?;
    let _ = crate::paths::write_atomic(&flag, b"1");

    Some(format!("The dam set the models: {done}. The `models` skill with `uninstall` puts the old settings back."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_models_are_set_and_put_back_and_the_rest_is_kept() {
        let mut backup = Map::new();
        let on = with_models("{\"theme\": \"dark\", \"model\": \"opus\"}", true, &mut backup).unwrap();
        let doc: Value = serde_json::from_str(&on).unwrap();
        assert_eq!((doc["model"].as_str(), doc["advisorModel"].as_str(), doc["theme"].as_str()), (Some(MAIN), Some(ADVISOR), Some("dark")));
        // A second install keeps the first backup.
        let again = with_models(&on, true, &mut backup).unwrap();
        assert_eq!(backup["model"], "opus");
        let off: Value = serde_json::from_str(&with_models(&again, false, &mut backup).unwrap()).unwrap();
        assert_eq!((off["model"].as_str(), off.get("advisorModel"), off["theme"].as_str()), (Some("opus"), None, Some("dark")));
    }

    #[test]
    fn a_model_the_person_changed_since_is_left_alone() {
        let mut backup = Map::new();
        with_models("{}", true, &mut backup).unwrap();
        let off: Value = serde_json::from_str(&with_models("{\"model\": \"haiku\", \"advisorModel\": \"claude-opus-5-5\"}", false, &mut backup).unwrap()).unwrap();
        assert_eq!(off["model"], "haiku");
        assert!(off.get("advisorModel").is_none());
        assert!(with_models("{ nope", true, &mut Map::new()).is_err());
    }

    #[test]
    fn codex_gets_its_models_in_the_right_tables_and_gives_the_old_ones_back() {
        let mut backup = Map::new();
        let config = "model = \"gpt-5\"\nmodel_reasoning_effort = \"high\"\n\n[agents]\nmax_concurrent_threads_per_session = 4\n\n[mcp_servers.x]\ncommand = \"x\"\n";
        let on = with_codex_models(config, true, &mut backup).unwrap();
        assert!(on.starts_with("model = \"gpt-6.1-sol\"\nmodel_reasoning_effort = \"high\""), "{on}");
        assert!(on.contains("[agents]\ndefault_subagent_model = \"gpt-6-luna\"\nmax_concurrent_threads_per_session = 4"), "{on}");
        assert!(on.contains("[mcp_servers.x]\ncommand = \"x\""));
        assert_eq!(on.lines().filter(|line| line.starts_with("model = ")).count(), 1);
        let off = with_codex_models(&on, false, &mut backup).unwrap();
        assert!(off.starts_with("model = \"gpt-5\"\nmodel_reasoning_effort = \"high\""), "{off}");
        assert!(!off.contains("gpt-6"), "{off}");
    }

    #[test]
    fn codex_without_the_keys_gets_them_and_loses_them_again() {
        let mut backup = Map::new();
        let on = with_codex_models("[projects.x]\ntrust_level = \"trusted\"\n", true, &mut backup).unwrap();
        assert!(on.starts_with("model = \"gpt-6.1-sol\"\n[projects.x]"), "{on}");
        assert!(on.ends_with("[agents]\ndefault_subagent_model = \"gpt-6-luna\"\n"), "{on}");
        let off = with_codex_models(&on, false, &mut backup).unwrap();
        assert!(!off.contains("gpt-6") && off.contains("trust_level"), "{off}");
        // A model the person changed since stays.
        let mut backup = Map::new();
        with_codex_models("", true, &mut backup).unwrap();
        assert!(with_codex_models("model = \"gpt-7\"\n", false, &mut backup).unwrap().contains("gpt-7"));
        assert!(with_codex_models("x = \"\"\"a\"\"\"", true, &mut Map::new()).is_err());
        assert!(with_codex_models("agents.default_subagent_model = \"x\"", true, &mut Map::new()).is_err());
    }

    #[test]
    fn a_table_header_with_a_comment_is_still_the_table() {
        let on = with_codex_models("[agents]  # note\nmax_threads = 2\n", true, &mut Map::new()).unwrap();
        assert_eq!(on.matches("[agents]").count(), 1, "{on}");
        assert!(on.contains("default_subagent_model"), "{on}");
    }
}
