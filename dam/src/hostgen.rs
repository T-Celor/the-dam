// One declaration for three hosts. `.claude-plugin/plugin.json` (what the plugin is) and `hooks/hooks.json` (what runs
// when) are the only files written by hand; `dam hosts` writes the Codex and GitHub Copilot manifests, marketplaces and
// hook files from them, and `dam hosts --check` fails when one is out of date (the tests run it).
// Not symlinks: Git on Windows checks a link out as a text file unless Developer Mode is on, and each host's schema differs.
use serde_json::{json, Map, Value};
use std::path::Path;

// The `dam` subcommands of hooks/hooks.json whose hooks answer any host: Beaver, and TokenEconomizer (`dam te hook`,
// which reads each host's payload and session log through hosts.rs).
const PORTABLE: [&str; 2] = ["beaver", "te"];
const CLAUDE_RUN: &str = "\"${CLAUDE_PLUGIN_ROOT}/scripts/dam-run\" ";
const CODEX_EVENTS: [&str; 11] = ["SessionStart", "SessionEnd", "SubagentStart", "SubagentStop", "PreToolUse", "PermissionRequest", "PostToolUse", "PreCompact", "PostCompact", "UserPromptSubmit", "Stop"];
const COPILOT_EVENTS: [(&str, &str); 11] = [
    ("SessionStart", "sessionStart"),
    ("UserPromptSubmit", "userPromptSubmitted"),
    ("SubagentStart", "subagentStart"),
    ("SessionEnd", "sessionEnd"),
    ("PreToolUse", "preToolUse"),
    ("PostToolUse", "postToolUse"),
    ("PostToolUseFailure", "postToolUseFailure"),
    ("Stop", "agentStop"),
    ("SubagentStop", "subagentStop"),
    ("PreCompact", "preCompact"),
    ("UserPromptTransformed", "userPromptTransformed"),
];

struct Handler {
    event: String,
    matcher: Option<String>,
    // What follows `dam`.
    words: String,
    timeout: Value,
    status: Option<String>,
    codex: bool,
    copilot: bool,
}

// What the mod does under Claude Code, done by a hook elsewhere: condensing outputs and the map's answers (answers.rs).
// Copilot ignores what its prompt hook prints, so its map answers for a prompt go through the later
// userPromptTransformed, which may rewrite what the model receives.
fn mod_hooks() -> Vec<Handler> {
    let hook = |event: &str, matcher: Option<&str>, codex: bool, copilot: bool| Handler { event: event.into(), matcher: matcher.map(Into::into), words: "squeeze hook".into(), timeout: json!(30), status: None, codex, copilot };

    vec![hook("PostToolUse", Some("Bash|mcp__.*"), true, true), hook("UserPromptSubmit", None, true, false), hook("UserPromptTransformed", None, false, true)]
}

// Each portable handler of hooks/hooks.json, then the mod's.
fn handlers(claude: &Value) -> Vec<Handler> {
    let mut found = Vec::new();
    for (event, groups) in claude["hooks"].as_object().into_iter().flatten() {
        for group in groups.as_array().into_iter().flatten() {
            for hook in group["hooks"].as_array().into_iter().flatten() {
                let Some((program, args)) = hook["command"].as_str().and_then(|line| line.strip_prefix(CLAUDE_RUN)).and_then(|rest| rest.split_once(' ')) else { continue };
                if !PORTABLE.contains(&program) || args.is_empty() {
                    continue;
                }
                found.push(Handler {
                    event: event.clone(),
                    matcher: group["matcher"].as_str().map(Into::into),
                    words: format!("{program} {args}"),
                    timeout: hook["timeout"].clone(),
                    status: hook["statusMessage"].as_str().map(Into::into),
                    codex: true,
                    copilot: true,
                });
            }
        }
    }
    found.extend(mod_hooks());

    found
}

// The host named on every line, and Copilot's event too, as its camelCase payloads do not carry it.
fn words(handler: &Handler, host: &str, event: &str) -> String {
    let event = if host == "copilot" { format!(" --event {event}") } else { String::new() };

    format!("{} --host {host}{event}", handler.words)
}

fn pick(from: &Value, keys: &[&str]) -> Map<String, Value> {
    keys.iter().filter_map(|key| from.get(*key).map(|value| ((*key).to_string(), value.clone()))).collect()
}

fn codex_hooks(handlers: &[Handler]) -> Value {
    let mut events: Map<String, Value> = Map::new();
    for handler in handlers.iter().filter(|h| h.codex && CODEX_EVENTS.contains(&h.event.as_str())) {
        let line = words(handler, "codex", "");
        let mut hook = json!({
            "type": "command",
            "command": format!("\"$PLUGIN_ROOT/scripts/dam-run\" {line}"),
            // Codex runs it with `%COMSPEC% /C`. cmd.exe looks for a bare program name in the project first, so a repository
            // could ship its own `cmd.exe` or `dam.exe`: the line starts with built-ins only, the first of which ends that search.
            // It begins with no quote, which /C would strip; the quoted `set` keeps a `&` in the path from splitting the line.
            "commandWindows": format!(r#"set NoDefaultCurrentDirectoryInExePath=1&& set "PATH=%PLUGIN_ROOT%\bin\windows-x64;%PATH%"&& dam.exe {line}"#),
        });
        if !handler.timeout.is_null() {
            hook["timeout"] = handler.timeout.clone();
        }
        if let Some(status) = &handler.status {
            hook["statusMessage"] = json!(status);
        }
        let mut group = Map::new();
        if let Some(matcher) = &handler.matcher {
            group.insert("matcher".into(), json!(matcher));
        }
        group.insert("hooks".into(), json!([hook]));
        events.entry(handler.event.clone()).or_insert_with(|| json!([])).as_array_mut().expect("an array").push(Value::Object(group));
    }

    Value::Object(events)
}

// Copilot takes one flat entry per handler, a bash and a PowerShell line; VS Code's Copilot sets only CLAUDE_PLUGIN_ROOT.
fn copilot_hooks(handlers: &[Handler]) -> Value {
    let mut events: Map<String, Value> = Map::new();
    for handler in handlers.iter().filter(|h| h.copilot) {
        let Some((_, event)) = COPILOT_EVENTS.iter().find(|(claude, _)| *claude == handler.event) else { continue };
        let line = words(handler, "copilot", event);
        let mut hook = json!({
            "type": "command",
            "bash": format!("\"${{PLUGIN_ROOT:-$CLAUDE_PLUGIN_ROOT}}/scripts/dam-run\" {line} || exit 0"),
            "powershell": format!("$root = if ($env:PLUGIN_ROOT) {{ $env:PLUGIN_ROOT }} else {{ $env:CLAUDE_PLUGIN_ROOT }}; $exe = Join-Path $root 'bin/windows-x64/dam.exe'; if (Test-Path $exe) {{ & $exe {line} }}"),
        });
        if !handler.timeout.is_null() {
            hook["timeoutSec"] = handler.timeout.clone();
        }
        events.entry((*event).to_string()).or_insert_with(|| json!([])).as_array_mut().expect("an array").push(hook);
    }

    Value::Object(events)
}

fn read(root: &Path, path: &str) -> Result<Value, String> {
    let text = std::fs::read_to_string(root.join(path)).map_err(|error| format!("{path}: {error}"))?;

    serde_json::from_str(&text).map_err(|error| format!("{path}: {error}"))
}

// Every generated file, as (path from the plugin's folder, content).
pub fn files(root: &Path) -> Result<Vec<(&'static str, Value)>, String> {
    let plugin = read(root, ".claude-plugin/plugin.json")?;
    let handlers = handlers(&read(root, "hooks/hooks.json")?);
    let meta = pick(&plugin, &["name", "version", "description", "author", "homepage", "repository", "license", "keywords"]);
    let with = |extra: Value| -> Value {
        let mut all = meta.clone();
        all.extend(extra.as_object().cloned().unwrap_or_default());
        Value::Object(all)
    };
    let (name, description, version) = (&plugin["name"], &plugin["description"], &plugin["version"]);
    let display = "The dam";

    Ok(vec![
        ("hooks/codex.json", json!({"description": "Generated by `dam hosts` from hooks/hooks.json: the hooks that answer Codex.", "hooks": codex_hooks(&handlers)})),
        ("hooks/copilot.json", json!({"version": 1, "hooks": copilot_hooks(&handlers)})),
        (
            ".codex-plugin/plugin.json",
            with(json!({
                "skills": "./skills/",
                "hooks": "./hooks/codex.json",
                "interface": {
                    "displayName": display,
                    "shortDescription": "Fewer tokens, every line kept",
                    "longDescription": "A local map of the project, lossless condensing of long outputs and Beaver's smallest-change rules, all on this machine.",
                    "developerName": plugin["author"]["name"],
                    "category": "Productivity",
                    "capabilities": ["Instructions", "Lifecycle hooks"],
                    "websiteURL": plugin["homepage"],
                    "defaultPrompt": ["Use Beaver mode for this task.", "Review this diff for over-engineering.", "Ask the project map where this is defined."],
                    "brandColor": "#2E7D8C",
                    "composerIcon": "./assets/icon.png",
                    "logo": "./assets/icon.png",
                },
            })),
        ),
        (".github/plugin/plugin.json", with(json!({"category": "productivity", "skills": "skills/", "hooks": "hooks/copilot.json"}))),
        (
            ".github/plugin/marketplace.json",
            json!({
                "name": name,
                "description": description,
                "owner": plugin["author"],
                "plugins": [{"name": name, "description": description, "version": version, "source": "./", "category": "productivity", "skills": "skills/", "hooks": "hooks/copilot.json"}],
            }),
        ),
        (
            ".agents/plugins/marketplace.json",
            json!({
                "name": name,
                "interface": {"displayName": display},
                "plugins": [{"name": name, "source": {"source": "url", "url": format!("{}.git", plugin["repository"].as_str().unwrap_or_default()), "ref": "main"}, "policy": {"installation": "AVAILABLE", "authentication": "ON_INSTALL"}, "category": "Productivity"}],
            }),
        ),
    ])
}

fn text_of(value: &Value) -> String {
    format!("{}\n", serde_json::to_string_pretty(value).unwrap_or_default())
}

// The files that are not what `files` gives; with `write`, they are written first.
pub fn sync(root: &Path, write: bool) -> Result<Vec<String>, String> {
    let mut changed = Vec::new();
    for (path, value) in files(root)? {
        let text = text_of(&value);
        let now = std::fs::read_to_string(root.join(path)).ok().map(|now| now.replace("\r\n", "\n"));
        if now.as_deref() == Some(text.as_str()) {
            continue;
        }
        if write {
            crate::paths::write_atomic(&root.join(path), text.as_bytes()).map_err(|error| format!("{path}: {error}"))?;
        }
        changed.push(path.to_string());
    }

    Ok(changed)
}

// `dam hosts [--check]`: exit 1 with the stale files named when `--check` finds any.
pub fn run(check: bool) -> i32 {
    let Some(root) = crate::plugin_root() else {
        eprintln!("the plugin's folder was not found");
        return 2;
    };
    match sync(&root, !check) {
        Ok(changed) if check && !changed.is_empty() => {
            eprintln!("out of date, run `dam hosts`: {}", changed.join(", "));
            1
        }
        Ok(changed) => {
            for path in changed {
                println!("wrote {path}");
            }
            0
        }
        Err(error) => {
            eprintln!("{error}");
            2
        }
    }
}
