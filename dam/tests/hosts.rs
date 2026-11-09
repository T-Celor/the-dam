// The Codex and GitHub Copilot packaging: each manifest and hook file follows its host's conventions, and the hook
// programs answer each host's payload in that host's shape. No model, no network. Runs with `cargo test`.
use dam::session::TempDir;
use fancy_regex::Regex;
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("the plugin's folder")
}

fn read(path: &str) -> Value {
    serde_json::from_str(&std::fs::read_to_string(root().join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn matches(pattern: &str, text: &str) -> bool {
    Regex::new(pattern).expect("a pattern").is_match(text).expect("a match")
}

// From developers.openai.com/codex/hooks and docs.github.com/copilot/reference/copilot-cli-reference/cli-hooks-reference.
const CODEX_EVENTS: [&str; 14] = ["SessionStart", "SessionEnd", "SubagentStart", "SubagentStop", "PreToolUse", "PermissionRequest", "PostToolUse", "PreModelRequest", "PostModelRequest", "PreCompact", "PostCompact", "UserPromptSubmit", "Stop", "Interrupt"];
const COPILOT_EVENTS: [&str; 14] = ["sessionStart", "sessionEnd", "userPromptSubmitted", "userPromptTransformed", "preToolUse", "postToolUse", "postToolUseFailure", "errorOccurred", "agentStop", "subagentStart", "subagentStop", "preCompact", "permissionRequest", "notification"];
const SUBCOMMANDS: [&str; 5] = ["beaver", "te", "graph", "squeeze", "goldfish"];

#[test]
fn the_codex_and_copilot_files_are_what_dam_hosts_writes_from_the_one_declaration() {
    let stale = dam::hostgen::sync(root(), false).unwrap();
    assert!(stale.is_empty(), "out of date, run `dam hosts`: {stale:?}");
}

#[test]
fn the_manifests_name_the_same_plugin_at_the_same_version() {
    let claude = read(".claude-plugin/plugin.json");
    for manifest in [read(".codex-plugin/plugin.json"), read(".github/plugin/plugin.json")] {
        assert_eq!(manifest["name"], claude["name"]);
        assert_eq!(manifest["version"], claude["version"]);
        assert!(matches(r"^[a-z0-9]+(-[a-z0-9]+)*$", manifest["name"].as_str().unwrap()));
    }
    assert_eq!(read(".github/plugin/marketplace.json")["plugins"][0]["version"], claude["version"]);
}

#[test]
fn every_path_a_manifest_names_exists() {
    let codex = read(".codex-plugin/plugin.json");
    for path in [&codex["skills"], &codex["hooks"], &codex["interface"]["composerIcon"], &codex["interface"]["logo"]] {
        let path = path.as_str().unwrap();
        assert!(path.starts_with("./"), "{path}: Codex paths start with ./");
        assert!(root().join(path).exists(), "{path}");
    }
    let copilot = read(".github/plugin/plugin.json");
    for path in [&copilot["skills"], &copilot["hooks"]] {
        assert!(root().join(path.as_str().unwrap()).exists(), "{path}");
    }
    let market = read(".github/plugin/marketplace.json");
    assert!(market["owner"]["name"].is_string());
    assert!(market["plugins"].as_array().unwrap().iter().all(|p| p["name"].is_string() && p["source"].is_string()));
    let agents = &read(".agents/plugins/marketplace.json")["plugins"][0];
    assert_eq!(agents["policy"]["installation"], "AVAILABLE");
    assert!(agents["source"]["url"].as_str().unwrap().contains("github.com/T-Celor/the-dam"));
}

#[test]
fn each_skill_is_an_agent_skill_with_a_name_and_a_description() {
    let mut count = 0;
    for entry in std::fs::read_dir(root().join("skills")).unwrap().flatten().filter(|e| e.path().is_dir()) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(entry.path().join("SKILL.md")).unwrap_or_else(|e| panic!("{name}: {e}"));
        let front = text.trim_start_matches('\u{feff}').strip_prefix("---").and_then(|rest| rest.split("\n---").next()).unwrap_or_default().replace("\r\n", "\n");
        assert!(front.lines().any(|line| line.trim_end() == format!("name: {name}")), "{name}: name");
        assert!(front.lines().any(|line| line.starts_with("description: ") && line.len() > "description: ".len()), "{name}: description");
        count += 1;
    }
    assert!(count > 0);
}

#[test]
fn the_codex_hooks_use_codex_events_and_reach_the_programs_from_plugin_root() {
    let hooks = read("hooks/codex.json");
    for (event, groups) in hooks["hooks"].as_object().unwrap() {
        assert!(CODEX_EVENTS.contains(&event.as_str()), "{event}");
        for handler in groups.as_array().unwrap().iter().flat_map(|group| group["hooks"].as_array().unwrap()) {
            assert_eq!(handler["type"], "command");
            let timeout = handler["timeout"].as_u64().unwrap();
            assert!((1..=600).contains(&timeout));
            let command = handler["command"].as_str().unwrap();
            assert!(matches(r#"^"\$PLUGIN_ROOT/scripts/dam-run" (beaver|te|graph|squeeze|goldfish) [a-z]+ --host codex$"#, command), "{command}");
            // For `%COMSPEC% /C`: built-ins first, so neither a `cmd.exe` nor a `dam.exe` from the project ever runs.
            let windows = handler["commandWindows"].as_str().unwrap();
            assert!(matches(r#"^set NoDefaultCurrentDirectoryInExePath=1&& set "PATH=%PLUGIN_ROOT%\\bin\\windows-x64;%PATH%"&& dam\.exe [a-z]+ [a-z]+ --host codex$"#, windows), "{windows}");
            assert!(SUBCOMMANDS.contains(&command.split(' ').nth(1).unwrap()));
        }
    }
}

#[test]
fn the_copilot_hooks_are_version_1_camel_case_events_with_bash_and_powershell() {
    let file = read(read(".github/plugin/plugin.json")["hooks"].as_str().unwrap());
    assert_eq!(file["version"], 1);
    for (event, handlers) in file["hooks"].as_object().unwrap() {
        assert!(COPILOT_EVENTS.contains(&event.as_str()), "{event}");
        for handler in handlers.as_array().unwrap() {
            assert_eq!(handler["type"], "command");
            assert!(handler["timeoutSec"].as_u64().unwrap() > 0);
            let bash = handler["bash"].as_str().unwrap();
            assert!(matches(&format!(r#"PLUGIN_ROOT.*/scripts/dam-run" (beaver|te|graph|squeeze|goldfish) [a-z]+ --host copilot --event {event} \|\| exit 0$"#), bash), "{bash}");
            let powershell = handler["powershell"].as_str().unwrap();
            assert!(matches(r"bin/windows-x64/dam\.exe'.*& \$exe [a-z]+ [a-z]+ --host copilot --event [a-zA-Z]+ \}$", powershell), "{powershell}");
        }
    }
}

// ---- the programs, as each host runs them ----

struct Project {
    home: TempDir,
    repo: PathBuf,
}

fn git(repo: &Path, args: &[&str]) {
    let status = Command::new("git").args(args).current_dir(repo).stdout(Stdio::null()).stderr(Stdio::null()).status().expect("git");
    assert!(status.success(), "git {args:?}");
}

fn project() -> Project {
    let home = TempDir::new("dam-hosts-").unwrap();
    let repo = home.0.join("repo");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    for name in ["a", "b", "c"] {
        std::fs::write(repo.join("src").join(format!("{name}.py")), format!("def {name}_thing():\n    return 1\n")).unwrap();
    }
    git(&repo, &["init", "-q"]);
    git(&repo, &["-c", "core.autocrlf=false", "add", "."]);

    Project { home, repo }
}

fn host_env(host: &str, home: &Path) -> Vec<(&'static str, String)> {
    let data = home.join("data").to_string_lossy().into_owned();
    let plugin = root().to_string_lossy().into_owned();
    match host {
        "claude" => vec![("CLAUDE_PLUGIN_ROOT", plugin)],
        "codex" => vec![("PLUGIN_ROOT", plugin.clone()), ("PLUGIN_DATA", data.clone()), ("CLAUDE_PLUGIN_ROOT", plugin), ("CLAUDE_PLUGIN_DATA", data)],
        _ => vec![("PLUGIN_ROOT", plugin), ("COPILOT_PLUGIN_DATA", data)],
    }
}

// Runs `dam` as `host` would, in the project, with `input` on stdin; (exit success, stdout, stderr).
fn run_in(project: &Project, host: &str, args: &[&str], input: &Value, extra: &[(&str, &str)]) -> (bool, String, String) {
    let home = &project.home.0;
    let mut command = Command::new(env!("CARGO_BIN_EXE_dam"));
    command.args(args).current_dir(&project.repo).env_clear().stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for key in ["PATH", "SYSTEMROOT"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command.env("HOME", home).env("USERPROFILE", home).env("APPDATA", home.join("appdata")).env("TOKENECONOMIZER_HOME", home.join("state"));
    for (key, value) in host_env(host, home).iter().map(|(k, v)| (*k, v.as_str())).chain(extra.iter().copied()) {
        command.env(key, value);
    }
    let mut child = command.spawn().expect("dam");
    let mut stdin = child.stdin.take().unwrap();
    let body = input.to_string();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(body.as_bytes());
    });
    let out = child.wait_with_output().expect("dam ran");
    let _ = writer.join();

    (out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned())
}

fn run(host: &str, args: &[&str], input: Value) -> String {
    let project = project();
    let (ok, out, err) = run_in(&project, host, args, &input, &[]);
    assert!(ok, "{err}");

    out
}

#[test]
fn claude_code_still_gets_plain_rules_and_no_map_line_at_session_start() {
    let out = run("claude", &["beaver", "activate"], json!({"session_id": "s1", "source": "startup", "hook_event_name": "SessionStart"}));
    assert!(out.starts_with("BEAVER MODE ACTIVE — level: full"), "{out}");
    assert!(!out.contains("A map of this project"));
}

#[test]
fn codex_gets_the_map_condensing_and_the_rules_as_plain_context_at_session_start() {
    let out = run("codex", &["beaver", "activate"], json!({"session_id": "s1", "source": "startup", "hook_event_name": "SessionStart", "cwd": ".", "model": "gpt", "transcript_path": null}));
    assert!(out.contains("A map of this project"), "{out}");
    assert!(matches(r#"dam(\.exe)?" graph query"#, &out));
    // Condensing is the squeeze hook's job there, so the start text does not ask the model to pipe outputs.
    assert!(!out.contains("\" squeeze"));
    assert!(out.contains("BEAVER MODE ACTIVE — level: full"));
    // The full rules from skills/beaver, not the one-line stand-in for a plugin folder not found.
    assert!(out.contains("## Work"));
}

#[test]
fn copilot_gets_the_same_as_additional_context_json() {
    let out: Value = serde_json::from_str(&run("copilot", &["beaver", "activate"], json!({"sessionId": "s1", "timestamp": 1, "cwd": ".", "source": "new"}))).unwrap();
    assert_eq!(out.as_object().unwrap().keys().collect::<Vec<_>>(), ["additionalContext"]);
    let text = out["additionalContext"].as_str().unwrap();
    assert!(text.contains("A map of this project") && text.contains("## Work"));
}

// A level is live only after a start in the same session, as in a real one.
fn start_then(host: &str, payload: Value) -> (String, String) {
    let project = project();
    let with = |extra: Value| -> Value {
        let mut all = payload.clone();
        all.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        all
    };
    let go = |args: &[&str], input: Value| run_in(&project, host, args, &input, &[]).1;
    go(&["beaver", "activate"], with(json!({"source": "startup"})));

    (go(&["beaver", "subagent"], payload.clone()), go(&["beaver", "track"], with(json!({"prompt": "/beaver lite"}))))
}

#[test]
fn each_host_gets_a_subagent_its_rules_in_its_own_shape() {
    let (sub, track) = start_then("codex", json!({"session_id": "s2", "agent_type": "worker", "agent_id": "a1"}));
    assert_eq!(serde_json::from_str::<Value>(&sub).unwrap()["hookSpecificOutput"]["hookEventName"], "SubagentStart");
    assert!(track.contains("BEAVER MODE CHANGED — level: lite"), "{track}");
    let (sub, _) = start_then("copilot", json!({"sessionId": "s2", "agentName": "worker"}));
    assert!(serde_json::from_str::<Value>(&sub).unwrap()["additionalContext"].as_str().unwrap().contains("BEAVER MODE ACTIVE"));
}

// ---- the hooks that do the mod's work outside Claude Code ----

fn hook(host: &str, words: &[&str], payload: Value) -> Value {
    let project = project();
    let mut input = json!({"cwd": project.repo.to_string_lossy()});
    input.as_object_mut().unwrap().extend(payload.as_object().unwrap().clone());
    let (ok, out, err) = run_in(&project, host, words, &input, &[("TE_JUDGE_NO_SPAWN", "1")]);
    assert!(ok, "{err}");

    serde_json::from_str(if out.trim().is_empty() { "{}" } else { &out }).unwrap_or_else(|e| panic!("{e}: {out}"))
}

fn log() -> String {
    (0..400).map(|i| format!("2026-11-04 12:00:{i} INFO worker {i} finished job {i} in {i} ms")).collect::<Vec<_>>().join("\n")
}

#[test]
fn copilot_gets_a_long_bash_output_back_condensed_as_a_modified_result() {
    let log = log();
    let out = hook("copilot", &["squeeze", "hook", "--host", "copilot", "--event", "postToolUse"], json!({"sessionId": "s3", "toolName": "bash", "toolArgs": {"command": "make"}, "toolResult": {"resultType": "success", "textResultForLlm": log}}));
    assert_eq!(out["modifiedResult"]["resultType"], "success");
    assert!(out["modifiedResult"]["textResultForLlm"].as_str().unwrap().len() < log.len() / 4);
}

#[test]
fn codex_gets_a_long_bash_output_back_condensed_through_a_block_reason() {
    let log = log();
    let out = hook("codex", &["squeeze", "hook", "--host", "codex"], json!({"session_id": "s3", "hook_event_name": "PostToolUse", "tool_name": "Bash", "tool_input": {"command": "make"}, "tool_response": log}));
    assert_eq!(out["decision"], "block");
    assert!(out["reason"].as_str().unwrap().len() < log.len() / 4);
}

#[test]
fn claude_code_is_left_to_the_mod_the_squeeze_hook_says_nothing_there() {
    let out = hook("claude", &["squeeze", "hook"], json!({"session_id": "s3", "hook_event_name": "PostToolUse", "tool_name": "Bash", "tool_input": {"command": "make"}, "tool_response": {"stdout": log()}}));
    assert_eq!(out, json!({}));
}

#[test]
fn token_economizer_answers_each_host_in_its_own_shape() {
    let cases = [
        ("codex", vec!["te", "hook", "--host", "codex"], json!({"session_id": "s4", "hook_event_name": "PostToolUse", "tool_name": "Bash", "tool_input": {"command": "make"}, "tool_response": "ok"})),
        ("copilot", vec!["te", "hook", "--host", "copilot", "--event", "postToolUse"], json!({"sessionId": "s4", "toolName": "bash", "toolArgs": {"command": "make"}, "toolResult": {"resultType": "success", "textResultForLlm": "ok"}})),
    ];
    for (host, words, payload) in cases {
        let out = hook(host, &words, payload);
        assert!(out.get("systemMessage").is_none() || host == "codex", "{host}");
        assert!(out.as_object().unwrap().keys().all(|key| ["hookSpecificOutput", "systemMessage", "additionalContext", "decision", "reason"].contains(&key.as_str())), "{out}");
    }
}

#[test]
fn copilot_gets_the_map_on_code_its_prompt_names_in_the_prompt_the_model_receives() {
    let project = project();
    let (ok, _, err) = run_in(&project, "copilot", &["graph", "update"], &json!({}), &[]);
    assert!(ok, "{err}");
    let payload = json!({"sessionId": "s5", "cwd": project.repo.to_string_lossy(), "prompt": "why does b_thing return 1?", "transformedPrompt": "<user>why does b_thing return 1?</user>"});
    let (_, out, _) = run_in(&project, "copilot", &["squeeze", "hook", "--host", "copilot", "--event", "userPromptTransformed"], &payload, &[]);
    let out: Value = serde_json::from_str(&out).unwrap();
    let prompt = out["modifiedTransformedPrompt"].as_str().unwrap();
    assert!(prompt.starts_with(payload["transformedPrompt"].as_str().unwrap()), "{prompt}");
    assert!(matches(r"b_thing\(\) src/b\.py:1", prompt), "{prompt}");
}
