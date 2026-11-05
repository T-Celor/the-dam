// The economizer under Codex and the GitHub Copilot CLI (Apache-2.0, Microsoft), rewritten from plugin/hosts.py,
// trace/codex.py and trace/copilot.py: each host's payload becomes the Claude-shaped one the economizer reads, its
// answer goes back in the host's own shape, and its session log becomes a Claude-shaped transcript for the fold.
use crate::channels::{host_name, parse_host, Host};
use crate::config::plain_text;
use crate::economizer::{env, python_truthy};
use fancy_regex::Regex;
use serde_json::{json, Map, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, OnceLock};

static CURRENT: OnceLock<Host> = OnceLock::new();
static PATCH_FILE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^\*\*\* (?:Update|Add|Delete) File:\s*(.+?)\s*$").expect("fixed"));

// The value after `flag` on this process's command line.
pub fn arg(flag: &str) -> Option<String> {
    let args: Vec<String> = std::env::args().collect();

    args.iter().position(|word| word == flag).and_then(|at| args.get(at + 1)).cloned()
}

// The host this process works for: the `--host` its hook entry or its spawner (`session::spawn`) gave, else what
// the environment says (the same reading as Beaver's), so one hook entry without the flag still answers its host.
pub fn current() -> Host {
    *CURRENT.get_or_init(|| arg("--host").and_then(|name| parse_host(&name)).unwrap_or_else(crate::beaver::host_from_env))
}

pub fn name() -> &'static str {
    host_name(current())
}

// How a notice names one of the dam's skills to the person: Codex calls a plugin's skill with `$`, the others with `/`.
pub fn skill(name: &str) -> String {
    match current() {
        Host::Codex => format!("$the-dam:{name}"),
        Host::Claude | Host::Copilot => format!("/the-dam:{name}"),
    }
}

// How a notice names this host's side-call command (sidecall.call_name).
pub fn call_name(host: Host) -> &'static str {
    match host {
        Host::Claude => "claude -p",
        Host::Codex => "codex exec",
        Host::Copilot => "copilot -p",
    }
}

// How a person stops a turn on each host, in its UI and in a headless run (Host.interrupt / end_run).
pub fn stop_hint(host: Host, headless: bool) -> &'static str {
    match (host, headless) {
        (_, true) => "Press Ctrl+C to end the run.",
        (Host::Claude, false) => "Press Esc to interrupt it.",
        (Host::Codex, false) => "Press Esc to interrupt it, then /stop to end a command it left running.",
        (Host::Copilot, false) => "Press Esc twice to interrupt it.",
    }
}

// Codex.headless: the rollout's first record names its front end, `exec` under `codex exec`.
pub fn codex_headless(transcript: &str) -> bool {
    // Its first line only: a long session's rollout runs to megabytes, and this is asked on a hook's clock.
    use std::io::BufRead;
    let first = std::fs::File::open(transcript).ok().and_then(|file| std::io::BufReader::new(file).lines().next()?.ok()).and_then(|line| serde_json::from_str::<Value>(&line).ok());
    let meta = first.as_ref().and_then(|record| record.get("payload")).unwrap_or(&Value::Null);

    meta.get("source").and_then(Value::as_str) == Some("exec") || meta.get("originator").and_then(Value::as_str) == Some("codex_exec")
}

pub fn copilot_home() -> PathBuf {
    env("COPILOT_HOME").map_or_else(|| PathBuf::from(env("HOME").or_else(|| env("USERPROFILE")).unwrap_or_default()).join(".copilot"), PathBuf::from)
}

// Copilot's camelCase hook events, by the economizer's names.
fn copilot_event(event: &str) -> &str {
    match event {
        "sessionStart" => "SessionStart",
        "userPromptSubmitted" => "UserPromptSubmit",
        "preToolUse" => "PreToolUse",
        "postToolUse" => "PostToolUse",
        "postToolUseFailure" => "PostToolUseFailure",
        "agentStop" => "Stop",
        "sessionEnd" => "SessionEnd",
        "subagentStart" => "SubagentStart",
        other => other,
    }
}

// Copilot's runtime tool names, by Claude Code's.
fn copilot_tool(name: &str) -> &str {
    match name {
        "bash" | "powershell" => "Bash",
        "view" => "Read",
        "create" => "Write",
        "edit" | "str_replace_editor" | "apply_patch" => "Edit",
        "grep" | "rg" => "Grep",
        "glob" => "Glob",
        "web_fetch" => "WebFetch",
        "task" => "Agent",
        other => other,
    }
}

// shortcut: words quoted only for whitespace and quotes, not every shell metacharacter as shlex.join does; enough for the loop guard's command signature.
fn join_argv(items: &[Value]) -> String {
    items.iter().map(plain_text).map(|word| if word.is_empty() || word.contains(|c: char| c.is_whitespace() || c == '\'' || c == '"') { format!("'{}'", word.replace('\'', "'\"'\"'")) } else { word }).collect::<Vec<_>>().join(" ")
}

fn patch_paths(patch: &str) -> Vec<String> {
    PATCH_FILE.captures_iter(patch).flatten().filter_map(|caps| Some(caps.get(1)?.as_str().to_string())).collect()
}

// Codex.tool_call: a shell call is Claude Code's Bash with a command string; apply_patch names the files it touches.
fn codex_tool(name: &str, args: &Value) -> (String, Value) {
    let low = name.to_lowercase();
    if ["bash", "shell", "exec_command", "local_shell", "unified_exec", "container.exec", "exec"].contains(&low.as_str()) {
        let command = args.get("command").or_else(|| args.get("cmd")).unwrap_or(&Value::Null);
        let command = match command {
            Value::Array(items) => join_argv(items),
            Value::Null => String::new(),
            other => plain_text(other),
        };
        return ("Bash".to_string(), json!({"command": command}));
    }
    if low == "apply_patch" {
        let patch = ["input", "patch", "command"].iter().find_map(|key| args.get(*key).and_then(Value::as_str)).unwrap_or("");
        let paths = patch_paths(patch);
        return ("apply_patch".to_string(), json!({"path": paths.first().cloned().unwrap_or_default(), "paths": paths}));
    }

    (name.to_string(), args.clone())
}

fn first<'a>(payload: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| payload.get(*key).filter(|value| !value.is_null()))
}

// Copilot hands tool arguments as a JSON string.
fn as_object(value: Option<&Value>) -> Value {
    match value {
        Some(Value::String(text)) => serde_json::from_str::<Value>(text).ok().filter(Value::is_object).unwrap_or_else(|| json!({})),
        Some(value @ Value::Object(_)) => value.clone(),
        _ => json!({}),
    }
}

fn normalize_codex(payload: &Value) -> Value {
    let mut out = payload.clone();
    // Codex stamps a prompt-turn with `turn_id` where Claude Code stamps `prompt_id`.
    if let Some(turn) = payload.get("turn_id").filter(|turn| python_truthy(turn)) {
        out["prompt_id"] = turn.clone();
    }
    if let Some(name) = payload.get("tool_name").and_then(Value::as_str) {
        let (name, args) = codex_tool(name, &as_object(payload.get("tool_input")));
        out["tool_name"] = json!(name);
        out["tool_input"] = args;
    }

    out
}

fn normalize_copilot(event: &str, payload: &Value) -> Value {
    let mut out = Map::new();
    out.insert("hook_event_name".to_string(), json!(event));
    let pairs = [("session_id", &["sessionId", "session_id"][..]), ("cwd", &["cwd"]), ("source", &["source"]), ("prompt", &["prompt", "initialPrompt"]), ("reason", &["reason", "stopReason"]), ("stop_hook_active", &["stop_hook_active", "stopHookActive"]), ("error", &["error"])];
    for (key, names) in pairs {
        if let Some(value) = first(payload, names) {
            // A failure's `error` may come as {message}; the economizer reads its text.
            let value = if key == "error" { value.get("message").cloned().unwrap_or_else(|| value.clone()) } else { value.clone() };
            out.insert(key.to_string(), value);
        }
    }
    // Only agentStop names the log; its place is fixed by the session id.
    let session = out.get("session_id").map(plain_text).unwrap_or_default();
    let transcript = first(payload, &["transcriptPath", "transcript_path"]).map(plain_text).filter(|path| !path.is_empty());
    let transcript = transcript.or_else(|| (!session.is_empty()).then(|| copilot_home().join("session-state").join(&session).join("events.jsonl").display().to_string()));
    if let Some(path) = transcript {
        out.insert("transcript_path".to_string(), json!(path));
    }
    if let Some(name) = first(payload, &["toolName", "tool_name"]).and_then(Value::as_str) {
        out.insert("tool_name".to_string(), json!(copilot_tool(name)));
        out.insert("tool_input".to_string(), as_object(first(payload, &["toolArgs", "tool_input"])));
    }
    // {resultType, textResultForLlm}: the shape `response_text` and `exit_code` read as Copilot's snake_case one.
    if let Some(result) = first(payload, &["toolResult", "tool_result"]) {
        let text = first(result, &["textResultForLlm", "text_result_for_llm"]).cloned().unwrap_or(Value::Null);
        let kind = first(result, &["resultType", "result_type"]).cloned().unwrap_or(Value::Null);
        out.insert("tool_response".to_string(), json!({"text_result_for_llm": text, "result_type": kind}));
    }

    Value::Object(out)
}

// The Claude-shaped payload for `host`'s, with `event` the hook's `--event` when the host's payload does not name it.
pub fn normalize(host: Host, event: Option<&str>, payload: &Value) -> Value {
    let named = first(payload, &["hook_event_name", "hookEventName"]).map(plain_text);
    let event = event.map(str::to_string).or(named).unwrap_or_default();
    match host {
        Host::Claude => payload.clone(),
        Host::Codex => {
            let mut out = normalize_codex(payload);
            out["hook_event_name"] = json!(event);
            out
        }
        Host::Copilot => normalize_copilot(copilot_event(&event), payload),
    }
}

// The economizer's answer in `host`'s shape: Claude Code and Codex share it; Copilot reads a top-level
// `additionalContext` on the events that honor one, and shows no `systemMessage`.
pub fn render(host: Host, event: &str, answer: &Value, relay: impl Fn(&str) -> String) -> Value {
    if host != Host::Copilot {
        return answer.clone();
    }
    if !matches!(event, "SessionStart" | "PostToolUse" | "PostToolUseFailure" | "SubagentStart") {
        return json!({});
    }
    let context = answer.get("hookSpecificOutput").and_then(|output| output.get("additionalContext")).and_then(Value::as_str);
    let notice = answer.get("systemMessage").and_then(Value::as_str);
    // A tool's notice rides with the context that already carries what matters; a start's notice is the model's to pass on.
    let context = match (context, notice) {
        (Some(context), Some(notice)) if event == "SessionStart" => Some(format!("{context}\n\n{}", relay(notice))),
        (None, Some(notice)) if event == "SessionStart" => Some(relay(notice)),
        (context, _) => context.map(str::to_string),
    };

    context.map_or_else(|| json!({}), |context| json!({"additionalContext": context}))
}

fn records(path: &Path) -> Vec<Value> {
    crate::trace::lines(path).iter().filter_map(|line| serde_json::from_str::<Value>(line).ok()).filter(Value::is_object).collect()
}

fn prompt_line(text: &str) -> Value {
    json!({"type": "user", "message": {"role": "user", "content": text}})
}

fn call_lines(id: &str, name: &str, args: &Value, result: &str, ok: bool, cwd: &Value) -> [Value; 2] {
    [
        json!({"type": "assistant", "cwd": cwd, "message": {"role": "assistant", "content": [{"type": "tool_use", "id": id, "name": name, "input": args}]}}),
        json!({"type": "user", "cwd": cwd, "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": id, "content": result, "is_error": !ok}]}}),
    ]
}

// _turn_starts: a rollout's `task_started` events open its turns, or where it has none its `turn_context` records.
fn codex_turn_start(rollout: &[Value]) -> impl Fn(&Value) -> bool {
    let started = |record: &Value| record.get("type").and_then(Value::as_str) == Some("event_msg") && record["payload"].get("type").and_then(Value::as_str) == Some("task_started");
    let by_task = rollout.iter().any(started);

    move |record: &Value| if by_task { started(record) } else { record.get("type").and_then(Value::as_str) == Some("turn_context") }
}

// codex_rollout_to_exec and codex_exec_calls in one: the rollout's prompt-turns and completed items as a Claude Code transcript.
pub fn codex_to_claude(rollout: &Path) -> Option<Vec<Value>> {
    if rollout.extension().is_some_and(|ext| ext == "zst") || !rollout.exists() {
        return None;
    }
    let records = records(rollout);
    let starts = codex_turn_start(&records);
    let mut cwd = Value::Null;
    let mut out = Vec::new();
    for (n, record) in records.iter().enumerate() {
        let payload = &record["payload"];
        if record.get("type").and_then(Value::as_str) == Some("session_meta") || record.get("type").and_then(Value::as_str) == Some("turn_context") {
            if let Some(dir) = payload.get("cwd").filter(|dir| dir.is_string()) {
                cwd = dir.clone();
            }
        }
        if starts(record) {
            out.push(prompt_line(""));
            continue;
        }
        if record.get("type").and_then(Value::as_str) != Some("event_msg") || payload.get("type").and_then(Value::as_str) != Some("item_completed") {
            continue;
        }
        let item = &payload["item"];
        let id = item.get("id").map_or_else(|| format!("codex-{n}"), plain_text);
        match item.get("type").and_then(Value::as_str) {
            Some("CommandExecution") => {
                let command = match &item["command"] {
                    Value::Array(items) => join_argv(items),
                    other => plain_text(other),
                };
                let output = item.get("aggregated_output").filter(|text| python_truthy(text)).map(plain_text).unwrap_or_else(|| format!("{}{}", item.get("stdout").map(plain_text).unwrap_or_default(), item.get("stderr").map(plain_text).unwrap_or_default()));
                let ok = item.get("exit_code").and_then(Value::as_i64) == Some(0);
                out.extend(call_lines(&id, "Bash", &json!({"command": command}), &output, ok, &cwd));
            }
            Some("FileChange") => {
                let paths: Vec<String> = match &item["changes"] {
                    Value::Object(changes) => changes.keys().cloned().collect(),
                    Value::Array(changes) => changes.iter().filter_map(|change| change.get("path").and_then(Value::as_str).map(str::to_string)).collect(),
                    _ => Vec::new(),
                };
                let ok = item.get("status").map(plain_text).is_none_or(|status| status.is_empty() || status.eq_ignore_ascii_case("completed"));
                out.extend(call_lines(&id, "apply_patch", &json!({"path": paths.first().cloned().unwrap_or_default(), "paths": paths}), "", ok, &cwd));
            }
            _ => {}
        }
    }

    Some(out)
}

// copilot_event_calls and copilot_prompt_count: `user.message` events and paired `tool.execution_*` ones.
pub fn copilot_to_claude(events: &Path) -> Option<Vec<Value>> {
    if !events.exists() {
        return None;
    }
    let mut cwd = Value::Null;
    let mut pending: Map<String, Value> = Map::new();
    let mut out = Vec::new();
    for event in records(events) {
        let data = event.get("data").filter(|data| data.is_object()).cloned().unwrap_or_else(|| json!({}));
        match event.get("type").and_then(Value::as_str) {
            Some("session.start") => cwd = data["context"].get("cwd").cloned().unwrap_or(Value::Null),
            Some("user.message") => out.push(prompt_line(&data.get("content").map(plain_text).unwrap_or_default())),
            Some("tool.execution_start") => {
                let id = data.get("toolCallId").map(plain_text).unwrap_or_default();
                pending.insert(id, json!({"name": data.get("toolName").map(plain_text).unwrap_or_default(), "args": as_object(data.get("arguments"))}));
            }
            Some("tool.execution_complete") => {
                let id = data.get("toolCallId").map(plain_text).unwrap_or_default();
                let Some(call) = pending.remove(&id) else { continue };
                let text = data["result"].get("content").and_then(Value::as_str).unwrap_or("");
                let ok = data.get("success").is_none_or(python_truthy);
                out.extend(call_lines(&id, copilot_tool(call["name"].as_str().unwrap_or("")), &call["args"], text, ok, &cwd));
            }
            _ => {}
        }
    }

    Some(out)
}

// The log the fold reads, Claude-shaped: Claude Code's own; another host's converted into `workdir`. None when unreadable.
pub fn claude_log(host: Host, transcript: &Path, workdir: &Path) -> Option<PathBuf> {
    let lines = match host {
        Host::Claude => return transcript.exists().then(|| transcript.to_path_buf()),
        Host::Codex => codex_to_claude(transcript)?,
        Host::Copilot => copilot_to_claude(transcript)?,
    };
    let path = workdir.join(format!("{}_trace.jsonl", host_name(host)));
    let mut file = std::fs::File::create(&path).ok()?;
    for line in lines {
        writeln!(file, "{line}").ok()?;
    }

    Some(path)
}

fn count(value: &Value, key: &str) -> i64 {
    value.get(key).and_then(|n| n.as_i64().or_else(|| n.as_f64().map(|f| f as i64))).unwrap_or(0)
}

// codex_session_usage: the last cumulative `token_count`, whose input is the grand total including both cache fields.
fn codex_usage(rollout: &Path) -> Value {
    let records = records(rollout);
    let model = records.iter().filter(|record| record.get("type").and_then(Value::as_str) == Some("turn_context")).find_map(|record| record["payload"].get("model").cloned()).unwrap_or(Value::Null);
    let totals: Vec<&Value> = records.iter().filter(|record| record["payload"].get("type").and_then(Value::as_str) == Some("token_count")).filter_map(|record| record["payload"]["info"].get("total_token_usage").filter(|total| python_truthy(total))).collect();
    let Some(total) = totals.last() else { return json!({"input_tokens": null, "cache_read_input_tokens": null, "cache_creation_input_tokens": null, "output_tokens": null, "requests": 0, "model": model}) };
    let (cached, written, reasoning) = (count(total, "cached_input_tokens"), count(total, "cache_write_input_tokens"), count(total, "reasoning_output_tokens"));

    json!({"input_tokens": (count(total, "input_tokens") - cached - written).max(0), "cache_read_input_tokens": cached, "cache_creation_input_tokens": written, "output_tokens": count(total, "output_tokens") + reasoning, "requests": totals.len(), "model": model})
}

// copilot_session_usage: `session.shutdown` per model; input is again the grand total.
fn copilot_usage(events: &Path) -> Value {
    let (mut fresh, mut read, mut written, mut output, mut requests) = (0, 0, 0, 0, 0);
    let mut models: Vec<(String, i64)> = Vec::new();
    for event in records(events).iter().filter(|event| event.get("type").and_then(Value::as_str) == Some("session.shutdown")) {
        for (model, metrics) in event["data"].get("modelMetrics").and_then(Value::as_object).into_iter().flatten() {
            let usage = &metrics["usage"];
            let (cache_read, cache_write) = (count(usage, "cacheReadTokens"), count(usage, "cacheWriteTokens"));
            fresh += (count(usage, "inputTokens") - cache_read - cache_write).max(0);
            read += cache_read;
            written += cache_write;
            output += count(usage, "outputTokens");
            let calls = count(&metrics["requests"], "count");
            requests += calls;
            match models.iter_mut().find(|(known, _)| known == model) {
                Some((_, total)) => *total += calls,
                None => models.push((model.clone(), calls)),
            }
        }
    }
    if models.is_empty() {
        return json!({"input_tokens": null, "cache_read_input_tokens": null, "cache_creation_input_tokens": null, "output_tokens": null, "requests": 0, "model": null});
    }
    let top = models.iter().map(|(_, calls)| *calls).max();
    let model = models.iter().find(|(_, calls)| Some(*calls) == top).map(|(model, _)| model.clone());

    json!({"input_tokens": fresh, "cache_read_input_tokens": read, "cache_creation_input_tokens": written, "output_tokens": output, "requests": requests, "model": model})
}

// The session's token breakdown, in the keys `pricing::effective` reads, from the host's own log.
pub fn session_usage(host: Host, transcript: &Path) -> Value {
    match host {
        Host::Claude => crate::trace::claude_session_usage(transcript),
        Host::Codex => codex_usage(transcript),
        Host::Copilot => copilot_usage(transcript),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn relay(text: &str) -> String {
        format!("relay:{text}")
    }

    #[test]
    fn codex_payloads_read_as_claude_codes() {
        let start = normalize(Host::Codex, None, &json!({"hook_event_name": "SessionStart", "session_id": "s", "transcript_path": null, "cwd": "/r", "source": "startup", "turn_id": null}));
        assert_eq!(start["hook_event_name"], "SessionStart");
        assert_eq!(start["source"], "startup");
        let prompt = normalize(Host::Codex, None, &json!({"hook_event_name": "UserPromptSubmit", "session_id": "s", "turn_id": "t1", "prompt": "go"}));
        assert_eq!(prompt["prompt_id"], "t1");
        let tool = normalize(Host::Codex, None, &json!({"hook_event_name": "PostToolUse", "session_id": "s", "turn_id": "t1", "tool_name": "Bash", "tool_input": {"command": ["cargo", "test", "a b"]}, "tool_response": {"output": "boom", "exit_code": 101}}));
        assert_eq!(tool["tool_input"]["command"], "cargo test 'a b'");
        assert_eq!(crate::economizer::exit_code(&tool["tool_response"]), Some(101));
        let patch = normalize(Host::Codex, None, &json!({"hook_event_name": "PostToolUse", "tool_name": "apply_patch", "tool_input": {"command": "*** Begin Patch\n*** Update File: src/a.rs\n@@\n"}}));
        assert_eq!(patch["tool_input"]["path"], "src/a.rs");
        let stop = normalize(Host::Codex, None, &json!({"hook_event_name": "Stop", "session_id": "s", "turn_id": "t1", "stop_hook_active": true}));
        assert_eq!(stop["stop_hook_active"], true);
        let end = normalize(Host::Codex, Some("SessionEnd"), &json!({"session_id": "s", "transcript_path": "/h/.codex/sessions/rollout-1.jsonl"}));
        assert_eq!(end["hook_event_name"], "SessionEnd");
    }

    #[test]
    fn copilot_payloads_read_as_claude_codes() {
        let start = normalize(Host::Copilot, Some("sessionStart"), &json!({"sessionId": "abc", "timestamp": 1, "cwd": "/r", "source": "new"}));
        assert_eq!(start["hook_event_name"], "SessionStart");
        assert_eq!(start["session_id"], "abc");
        assert!(start["transcript_path"].as_str().unwrap().replace('\\', "/").ends_with("session-state/abc/events.jsonl"));
        let prompt = normalize(Host::Copilot, Some("userPromptSubmitted"), &json!({"sessionId": "abc", "prompt": "go"}));
        assert_eq!((prompt["hook_event_name"].as_str(), prompt["prompt"].as_str()), (Some("UserPromptSubmit"), Some("go")));
        let tool = normalize(Host::Copilot, Some("postToolUse"), &json!({"sessionId": "abc", "toolName": "powershell", "toolArgs": "{\"command\": \"cargo test\"}", "toolResult": {"resultType": "success", "textResultForLlm": "fail\n<shellId: 3 completed with exit code 2>"}}));
        assert_eq!(tool["tool_name"], "Bash");
        assert_eq!(tool["tool_input"]["command"], "cargo test");
        assert_eq!(crate::economizer::exit_code(&tool["tool_response"]), Some(2));
        assert!(crate::economizer::response_text(&tool["tool_response"]).starts_with("fail"));
        let failed = normalize(Host::Copilot, Some("postToolUseFailure"), &json!({"sessionId": "abc", "toolName": "view", "toolArgs": {"path": "a.rs"}, "error": "no such file"}));
        assert_eq!((failed["tool_name"].as_str(), failed["error"].as_str()), (Some("Read"), Some("no such file")));
        assert!(failed.get("tool_response").is_none());
        let stop = normalize(Host::Copilot, Some("agentStop"), &json!({"sessionId": "abc", "transcriptPath": "/x/events.jsonl", "stopReason": "end_turn", "stop_hook_active": false}));
        assert_eq!((stop["hook_event_name"].as_str(), stop["transcript_path"].as_str()), (Some("Stop"), Some("/x/events.jsonl")));
        let end = normalize(Host::Copilot, Some("sessionEnd"), &json!({"sessionId": "abc", "reason": "complete"}));
        assert_eq!((end["hook_event_name"].as_str(), end["reason"].as_str()), (Some("SessionEnd"), Some("complete")));
    }

    #[test]
    fn answers_go_back_in_each_hosts_shape() {
        let answer = json!({"hookSpecificOutput": {"hookEventName": "SessionStart", "additionalContext": "facts"}, "systemMessage": "added"});
        assert_eq!(render(Host::Codex, "SessionStart", &answer, relay), answer);
        assert_eq!(render(Host::Copilot, "SessionStart", &answer, relay), json!({"additionalContext": "facts\n\nrelay:added"}));
        assert_eq!(render(Host::Copilot, "SessionStart", &json!({"systemMessage": "paused"}), relay), json!({"additionalContext": "relay:paused"}));
        let nudge = json!({"hookSpecificOutput": {"hookEventName": "PostToolUse", "additionalContext": "redirect"}, "systemMessage": "nudged"});
        assert_eq!(render(Host::Copilot, "PostToolUse", &nudge, relay), json!({"additionalContext": "redirect"}));
        assert_eq!(render(Host::Copilot, "PostToolUse", &json!({"systemMessage": "x"}), relay), json!({}));
        assert_eq!(render(Host::Copilot, "UserPromptSubmit", &nudge, relay), json!({}));
        assert_eq!(render(Host::Copilot, "Stop", &json!({}), relay), json!({}));
    }

    fn fixture(name: &str, lines: &[Value]) -> (crate::session::TempDir, PathBuf) {
        let dir = crate::session::TempDir::new("te-hosts-").unwrap();
        let path = dir.0.join(name);
        std::fs::write(&path, lines.iter().map(Value::to_string).collect::<Vec<_>>().join("\n")).unwrap();
        (dir, path)
    }

    #[test]
    fn a_codex_rollout_becomes_a_claude_transcript() {
        let (dir, path) = fixture("rollout-1.jsonl", &[
            json!({"type": "session_meta", "payload": {"id": "s", "cwd": "/r", "source": "exec"}}),
            json!({"type": "turn_context", "payload": {"cwd": "/r", "model": "gpt-5"}}),
            json!({"type": "event_msg", "payload": {"type": "task_started"}}),
            json!({"type": "event_msg", "payload": {"type": "item_completed", "item": {"type": "CommandExecution", "id": "c1", "command": ["cargo", "test"], "aggregated_output": "error[E0425]", "exit_code": 101}}}),
            json!({"type": "event_msg", "payload": {"type": "item_completed", "item": {"type": "FileChange", "id": "c2", "changes": {"src/a.rs": {"type": "update"}}, "status": "completed"}}}),
            json!({"type": "event_msg", "payload": {"type": "token_count", "info": {"total_token_usage": {"input_tokens": 1000, "cached_input_tokens": 600, "output_tokens": 50, "reasoning_output_tokens": 10}}}}),
        ]);
        assert!(codex_headless(path.to_str().unwrap()));
        let log = claude_log(Host::Codex, &path, &dir.0).unwrap();
        let calls = crate::trace::load_trajectory(&log);
        assert_eq!(calls.len(), 2);
        assert_eq!((calls[0].tool.as_deref(), calls[0].args["command"].as_str(), calls[0].success, calls[0].cwd.as_deref()), (Some("Bash"), Some("cargo test"), false, Some("/r")));
        assert_eq!(calls[1].args["path"], "src/a.rs");
        assert_eq!(crate::trace::claude_prompt_count(&log), 1);
        let usage = session_usage(Host::Codex, &path);
        assert_eq!((usage["input_tokens"].as_i64(), usage["cache_read_input_tokens"].as_i64(), usage["output_tokens"].as_i64(), usage["model"].as_str()), (Some(400), Some(600), Some(60), Some("gpt-5")));
    }

    #[test]
    fn a_copilot_session_log_becomes_a_claude_transcript() {
        let (dir, path) = fixture("events.jsonl", &[
            json!({"type": "session.start", "data": {"context": {"cwd": "/r"}}}),
            json!({"type": "user.message", "data": {"content": "fix it"}}),
            json!({"type": "tool.execution_start", "data": {"toolCallId": "t1", "toolName": "bash", "arguments": {"command": "npm test"}}}),
            json!({"type": "tool.execution_complete", "data": {"toolCallId": "t1", "success": false, "result": {"content": "1 failing"}}}),
            json!({"type": "session.shutdown", "data": {"modelMetrics": {"gpt-5": {"usage": {"inputTokens": 4000, "cacheReadTokens": 2400, "outputTokens": 80}, "requests": {"count": 4}}}}}),
        ]);
        let log = claude_log(Host::Copilot, &path, &dir.0).unwrap();
        let calls = crate::trace::load_trajectory(&log);
        assert_eq!((calls.len(), calls[0].tool.as_deref(), calls[0].result.as_str(), calls[0].success), (1, Some("Bash"), "1 failing", false));
        assert_eq!(crate::trace::claude_prompt_count(&log), 1);
        let usage = session_usage(Host::Copilot, &path);
        assert_eq!((usage["input_tokens"].as_i64(), usage["requests"].as_i64(), usage["model"].as_str()), (Some(1600), Some(4), Some("gpt-5")));
        assert!(claude_log(Host::Copilot, &dir.0.join("missing.jsonl"), &dir.0).is_none());
    }
}
