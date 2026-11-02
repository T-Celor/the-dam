// Reading a Claude Code session log (Apache-2.0, Microsoft), rewritten from trace/claude.py,
// calls.py and jsonl.py. Only the conversation's own entries carry its calls: Claude Code also writes
// `api-request-blob` entries that repeat earlier calls.
use fancy_regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

const USAGE_KEYS: [&str; 4] = ["input_tokens", "cache_read_input_tokens", "cache_creation_input_tokens", "output_tokens"];
pub const CONVERSATION: [&str; 2] = ["assistant", "user"];

static RTK_PREFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^rtk\s+(?:proxy\s+)?(?=\S)").expect("fixed"));

#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub tool: Option<String>,
    pub args: Value,
    pub result: String,
    pub success: bool,
    pub cwd: Option<String>,
}

// Non-empty, stripped lines; nothing when the file cannot be read.
pub fn lines(path: &Path) -> Vec<String> {
    let Ok(bytes) = fs::read(path) else { return Vec::new() };

    String::from_utf8_lossy(&bytes).lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_string).collect()
}

fn is_truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null | Value::Bool(false)) => false,
        Some(Value::Number(number)) => number.as_f64() != Some(0.0),
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Array(items)) => !items.is_empty(),
        Some(Value::Object(map)) => !map.is_empty(),
        Some(Value::Bool(true)) => true,
    }
}

pub fn claude_calls(lines: &[String]) -> Vec<Call> {
    let (mut order, mut seen): (Vec<String>, HashMap<String, (Option<String>, Value, Option<String>)>) = (Vec::new(), HashMap::new());
    let mut results: HashMap<String, (String, bool)> = HashMap::new();
    for line in lines {
        let Ok(event) = serde_json::from_str::<Value>(line) else { continue };
        if !event.is_object() || !CONVERSATION.contains(&event.get("type").and_then(Value::as_str).unwrap_or("assistant")) {
            continue;
        }
        let Some(content) = event.get("message").and_then(|message| message.get("content")).and_then(Value::as_array) else { continue };
        for block in content.iter().filter(|block| block.is_object()) {
            match block.get("type").and_then(Value::as_str) {
                Some("tool_use") => {
                    let id = block.get("id").map_or_else(|| "null".to_string(), Value::to_string);
                    if seen.contains_key(&id) {
                        continue;
                    }
                    let name = block.get("name").and_then(Value::as_str).map(str::to_string);
                    let args = block.get("input").cloned().unwrap_or_else(|| json!({}));
                    let cwd = event.get("cwd").and_then(Value::as_str).map(str::to_string);
                    seen.insert(id.clone(), (name, args, cwd));
                    order.push(id);
                }
                Some("tool_result") => {
                    let id = block.get("tool_use_id").map_or_else(|| "null".to_string(), Value::to_string);
                    let text = match block.get("content") {
                        Some(Value::String(text)) => text.clone(),
                        Some(Value::Array(items)) => items.iter().filter(|item| item.is_object()).map(|item| item.get("text").and_then(Value::as_str).unwrap_or("")).collect::<Vec<_>>().join(" "),
                        _ => String::new(),
                    };
                    results.insert(id, (text, !is_truthy(block.get("is_error"))));
                }
                _ => {}
            }
        }
    }

    order.iter().map(|id| {
        let (tool, args, cwd) = seen[id].clone();
        let (result, success) = results.get(id).cloned().unwrap_or((String::new(), true));
        Call { tool, args, result, success, cwd }
    }).collect()
}

// Only Claude Code's logs: an empty answer for anything else, as for a log with no call in it.
pub fn load_trajectory(path: &Path) -> Vec<Call> {
    let lines = lines(path);
    if lines.iter().any(|line| line.contains("\"tool_use\"") || line.contains("\"type\": \"assistant\"") || line.contains("\"type\":\"assistant\"")) {
        claude_calls(&lines)
    } else {
        Vec::new()
    }
}

pub fn strip_rtk_prefix(command: &str) -> String {
    RTK_PREFIX.replace(command.trim_start(), "").into_owned()
}

fn is_prompt(event: &Value) -> bool {
    if event.get("type").and_then(Value::as_str) != Some("user") || ["isMeta", "isSidechain", "isCompactSummary"].iter().any(|key| is_truthy(event.get(*key))) {
        return false;
    }
    let content = event.get("message").and_then(|message| message.get("content"));
    let text = match content {
        Some(Value::Array(items)) => {
            let blocks: Vec<&Value> = items.iter().filter(|block| block.is_object()).collect();
            let kind = |block: &Value, name: &str| block.get("type").and_then(Value::as_str) == Some(name);
            if blocks.iter().any(|block| kind(block, "tool_result")) {
                return false;
            }
            blocks.iter().filter(|block| kind(block, "text")).map(|block| block.get("text").and_then(Value::as_str).unwrap_or("")).collect::<Vec<_>>().join(" ")
        }
        Some(Value::String(text)) => text.clone(),
        _ => return false,
    };

    !text.starts_with("<local-command") && !text.starts_with("[Request interrupted")
}

pub fn claude_prompt_count(path: &Path) -> i64 {
    lines(path).iter().filter_map(|line| serde_json::from_str::<Value>(line).ok()).filter(is_prompt).count() as i64
}

pub fn claude_subagent_files(path: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(path.with_extension("").join("subagents")) else { return Vec::new() };
    let mut files: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).filter(|file| file.extension().is_some_and(|ext| ext == "jsonl")).collect();
    files.sort();

    files
}

// De-duplicated by message id: the log writes one entry per content block, each repeating its usage.
pub fn claude_session_usage(path: &Path) -> Value {
    let mut files = vec![path.to_path_buf()];
    files.extend(claude_subagent_files(path));
    let (mut by_id, mut models): (HashMap<String, [i64; 4]>, Vec<(String, i64)>) = (HashMap::new(), Vec::new());
    let mut anonymous = 0;
    for file in &files {
        for line in lines(file).iter().filter(|line| line.contains("\"usage\"")) {
            let Ok(event) = serde_json::from_str::<Value>(line) else { continue };
            if event.get("type").and_then(Value::as_str) != Some("assistant") {
                continue;
            }
            let message = event.get("message").unwrap_or(&Value::Null);
            let Some(usage) = message.get("usage").filter(|usage| usage.is_object()) else { continue };
            let id = match message.get("id").filter(|id| is_truthy(Some(id))) {
                Some(Value::String(id)) => id.clone(),
                Some(other) => other.to_string(),
                None => {
                    anonymous += 1;
                    format!("anon-{}-{anonymous}", file.display())
                }
            };
            by_id.insert(id, USAGE_KEYS.map(|key| usage.get(key).and_then(|n| n.as_i64().or_else(|| n.as_f64().map(|f| f as i64))).unwrap_or(0)));
            if let Some(model) = message.get("model").and_then(Value::as_str).filter(|model| !model.is_empty()) {
                match models.iter_mut().find(|(known, _)| known == model) {
                    Some((_, count)) => *count += 1,
                    None => models.push((model.to_string(), 1)),
                }
            }
        }
    }
    let mut out = Map::new();
    for (n, key) in USAGE_KEYS.iter().enumerate() {
        out.insert((*key).to_string(), json!(by_id.values().map(|usage| usage[n]).sum::<i64>()));
    }
    out.insert("requests".to_string(), json!(by_id.len()));
    // The first of the most used, as Python's max keeps.
    let top = models.iter().map(|(_, count)| *count).max();
    out.insert("model".to_string(), json!(models.iter().find(|(_, count)| Some(*count) == top).map(|(model, _)| model.clone())));
    out.insert("files".to_string(), json!(files.iter().filter(|file| file.exists()).count()));

    Value::Object(out)
}

fn tool_usage(path: &Path) -> BTreeMap<String, i64> {
    let mut counts = BTreeMap::new();
    for name in load_trajectory(path).into_iter().filter_map(|call| call.tool).filter(|name| !name.is_empty()) {
        *counts.entry(name).or_insert(0) += 1;
    }

    counts
}

// A tool a subagent used is in use: denying it would take it from the subagent too.
pub fn claude_tool_usage(path: &Path) -> BTreeMap<String, i64> {
    let mut counts = tool_usage(path);
    for file in claude_subagent_files(path) {
        for (name, count) in tool_usage(&file) {
            *counts.entry(name).or_insert(0) += count;
        }
    }

    counts
}

pub fn claude_deferred_tools(path: &Path) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for line in lines(path).iter().filter(|line| line.contains("\"deferred_tools_delta\"")) {
        let Ok(event) = serde_json::from_str::<Value>(line) else { continue };
        let listed = |key: &str| event.get("attachment").and_then(|attachment| attachment.get(key)).and_then(Value::as_array).cloned().unwrap_or_default();
        names.extend(listed("addedNames").iter().filter_map(Value::as_str).map(str::to_string));
        for name in listed("removedNames").iter().filter_map(Value::as_str) {
            names.remove(name);
        }
    }

    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calls_pair_with_their_results_once() {
        let log = [
            r#"{"type":"assistant","cwd":"/r","message":{"content":[{"type":"tool_use","id":"1","name":"Bash","input":{"command":"ls"}}]}}"#,
            r#"{"type":"api-request-blob","message":{"content":[{"type":"tool_use","id":"9","name":"Twice","input":{}}]}}"#,
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"1","is_error":true,"content":[{"text":"a"},{"text":"b"}]}]}}"#,
        ].map(str::to_string);
        let calls = claude_calls(&log);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].tool.as_deref(), Some("Bash"));
        assert_eq!(calls[0].result, "a b");
        assert!(!calls[0].success);
        assert_eq!(calls[0].cwd.as_deref(), Some("/r"));
    }

    #[test]
    fn only_what_the_person_typed_is_a_prompt() {
        assert!(is_prompt(&json!({"type": "user", "message": {"content": "go"}})));
        assert!(!is_prompt(&json!({"type": "user", "isMeta": true, "message": {"content": "go"}})));
        assert!(!is_prompt(&json!({"type": "user", "message": {"content": "<local-command-stdout>x"}})));
        assert!(!is_prompt(&json!({"type": "user", "message": {"content": [{"type": "tool_result"}]}})));
    }

    #[test]
    fn the_rtk_wrapper_is_not_the_command() {
        assert_eq!(strip_rtk_prefix("rtk proxy pytest -q"), "pytest -q");
        assert_eq!(strip_rtk_prefix("rtk"), "rtk");
    }
}
