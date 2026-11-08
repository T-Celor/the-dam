// Makes the advisor's review mandatory where it matters. At the end of a turn that changed several files, or code that
// touches security, money or data, the agent may not finish until a fresh reviewer (the `advisor`, or the goldfish
// reviewers) has been called in that turn. It is asked once per turn, so it can never loop. No model runs here.
use serde_json::Value;
use std::fs;
use std::io::Read;

// Distinct code files a turn may change before a review is required.
const MOST_FILES_UNREVIEWED: usize = 3;
const SENSITIVE: [&str; 11] = ["auth", "secur", "crypt", "passw", "secret", "token", "payment", "billing", "migrat", "permission", ".sql"];
const DOCS: [&str; 6] = ["md", "txt", "rst", "adoc", "csv", "log"];
const EDIT_TOOLS: [&str; 6] = ["write", "edit", "multiedit", "notebookedit", "apply_patch", "create"];

#[derive(Debug, Default, PartialEq)]
pub struct Turn {
    // Where the turn began: the line of the user's last prompt.
    pub id: usize,
    pub edited: Vec<String>,
    pub reviewed: bool,
}

// The file name of a path in either spelling: the same file is named in full by one tool and by its name alone by another.
fn base_name(path: &str) -> String {
    path.replace('\\', "/").rsplit('/').next().unwrap_or("").to_lowercase()
}

// Files a release bumps or a tool regenerates: a version number is not a change that wants a second pair of eyes.
const BOOKKEEPING: [&str; 9] = ["cargo.toml", "cargo.lock", "package-lock.json", "yarn.lock", "pnpm-lock.yaml", "plugin.json", "marketplace.json", ".gitignore", "go.sum"];

fn is_doc(name: &str) -> bool {
    BOOKKEEPING.contains(&base_name(name).as_str()) || name.rsplit_once('.').is_some_and(|(_, ext)| DOCS.iter().any(|doc| ext.eq_ignore_ascii_case(doc)))
}

fn patch_files(text: &str) -> Vec<String> {
    text.lines().filter_map(|line| line.trim().strip_prefix("*** Add File:").or_else(|| line.trim().strip_prefix("*** Update File:"))).map(|name| name.trim().to_string()).collect()
}

fn files_of(tool: &str, input: &Value) -> Vec<String> {
    let mut found = Vec::new();
    for key in ["file_path", "path", "filePath", "notebook_path"] {
        if let Some(name) = input[key].as_str() {
            found.push(name.to_string());
        }
    }
    for key in ["input", "patch", "command"] {
        if let Some(text) = input[key].as_str() {
            found.extend(patch_files(text));
        }
    }
    if tool == "bash" || tool == "shell" {
        if let Some(command) = input["command"].as_str().or_else(|| input["cmd"].as_str()) {
            found.extend(crate::loopguard::shell_edit_targets(command));
        }
    }

    found
}

// The last turn of a Claude Code transcript: its edits, and whether a fresh reviewer was called in it. Work done inside
// subagents (`isSidechain`) is not the turn's own.
pub fn scan(transcript: &str) -> Turn {
    let mut turn = Turn::default();
    for (at, line) in transcript.lines().enumerate() {
        let Ok(entry) = serde_json::from_str::<Value>(line) else { continue };
        if entry["isSidechain"].as_bool() == Some(true) {
            continue;
        }
        let content = &entry["message"]["content"];
        match entry["type"].as_str() {
            Some("user") => {
                let is_prompt = match content {
                    Value::String(text) => !text.trim().is_empty(),
                    Value::Array(blocks) => blocks.iter().any(|block| block["type"] == "text") && !blocks.iter().any(|block| block["type"] == "tool_result"),
                    _ => false,
                };
                if is_prompt && entry["isMeta"].as_bool() != Some(true) {
                    turn = Turn { id: at, ..Turn::default() };
                }
            }
            Some("assistant") => {
                for block in content.as_array().into_iter().flatten().filter(|block| block["type"] == "tool_use") {
                    let tool = block["name"].as_str().unwrap_or("").to_lowercase();
                    let input = &block["input"];
                    if matches!(tool.as_str(), "agent" | "task") {
                        let kind = input["subagent_type"].as_str().unwrap_or("").to_lowercase();
                        turn.reviewed |= kind.contains("advisor") || kind.contains("goldfish");
                    } else if EDIT_TOOLS.contains(&tool.as_str()) || tool == "bash" {
                        for name in files_of(&tool, input) {
                            if !is_doc(&name) && !turn.edited.iter().any(|known| base_name(known) == base_name(&name)) {
                                turn.edited.push(name);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    turn
}

// Why the agent may not finish yet; None when no review is needed or one happened.
pub fn reason(turn: &Turn) -> Option<String> {
    if turn.reviewed || turn.edited.is_empty() {
        return None;
    }
    let sensitive: Vec<&String> = turn.edited.iter().filter(|name| SENSITIVE.iter().any(|word| name.to_lowercase().contains(word))).collect();
    if sensitive.is_empty() && turn.edited.len() < MOST_FILES_UNREVIEWED {
        return None;
    }
    let base = |name: &str| name.replace('\\', "/").rsplit('/').next().unwrap_or("").to_string();
    let shown: Vec<String> = turn.edited.iter().take(5).map(|name| base(name)).collect();
    let more = if turn.edited.len() > 5 { format!(" +{} more", turn.edited.len() - 5) } else { String::new() };
    let why = if sensitive.is_empty() { format!("{} code files changed", turn.edited.len()) } else { format!("{} code files changed, including {}", turn.edited.len(), sensitive.iter().take(3).map(|name| base(name)).collect::<Vec<_>>().join(", ")) };

    Some(format!("Review required before you finish: {why} ({}{more}). Call the `advisor` agent now with three things and nothing else: the task in one sentence, the changed paths, and the test result. Fix its Blockers and Majors, or say in one line why you disagree, then finish. (The dam asks once per turn; DAM_REVIEW=off turns this off.)", shown.join(", ")))
}

fn state_file(session: &str) -> std::path::PathBuf {
    let safe: String = session.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();

    crate::economizer::state_root().join("goldfish").join(format!("gate-{safe}.txt"))
}

// `dam goldfish gate`: a Stop payload on stdin; a block decision on stdout when a review is owed, else nothing.
pub fn hook() {
    crate::watchdog(25);
    if std::env::var("DAM_REVIEW").is_ok_and(|value| matches!(value.to_lowercase().as_str(), "off" | "0")) {
        return;
    }
    let mut bytes = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut bytes);
    let text = String::from_utf8_lossy(&bytes);
    let Ok(input) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) else { return };
    // Already continuing because of a stop hook: never ask twice in a row.
    if input["stop_hook_active"].as_bool() == Some(true) {
        return;
    }
    let Some(path) = input["transcript_path"].as_str() else { return };
    let Ok(transcript) = fs::read_to_string(path) else { return };
    let turn = scan(&transcript);
    let Some(why) = reason(&turn) else { return };
    let session = input["session_id"].as_str().unwrap_or("session");
    let file = state_file(session);
    if fs::read_to_string(&file).is_ok_and(|seen| seen.trim() == turn.id.to_string()) {
        return;
    }
    if let Some(folder) = file.parent() {
        let _ = fs::create_dir_all(folder);
    }
    let _ = fs::write(&file, turn.id.to_string());
    print!("{}", serde_json::json!({"decision": "block", "reason": why}));
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn user(text: &str) -> String {
        json!({"type": "user", "message": {"role": "user", "content": text}}).to_string()
    }

    fn tool(name: &str, input: Value) -> String {
        json!({"type": "assistant", "message": {"role": "assistant", "content": [{"type": "tool_use", "id": "x", "name": name, "input": input}]}}).to_string()
    }

    fn result() -> String {
        json!({"type": "user", "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": "x", "content": "ok"}]}}).to_string()
    }

    fn lines(parts: &[String]) -> String {
        parts.join("\n")
    }

    #[test]
    fn three_code_files_need_a_review_and_two_do_not() {
        let edits = |n: usize| (0..n).flat_map(|i| [tool("Edit", json!({"file_path": format!("src/f{i}.rs")})), result()]).collect::<Vec<_>>();
        let mut parts = vec![user("do it")];
        parts.extend(edits(2));
        assert!(reason(&scan(&lines(&parts))).is_none());
        let mut parts = vec![user("do it")];
        parts.extend(edits(3));
        let why = reason(&scan(&lines(&parts))).expect("a review is owed");
        assert!(why.contains("3 code files") && why.contains("advisor"), "{why}");
    }

    #[test]
    fn a_sensitive_file_needs_one_alone_and_documents_need_none() {
        let sensitive = lines(&[user("x"), tool("Write", json!({"file_path": "app/auth/login.py"}))]);
        assert!(reason(&scan(&sensitive)).unwrap().contains("login.py"));
        let docs = lines(&[user("x"), tool("Write", json!({"file_path": "README.md"})), tool("Write", json!({"file_path": "a.md"})), tool("Write", json!({"file_path": "b.txt"})), tool("Write", json!({"file_path": "c.md"}))]);
        assert!(reason(&scan(&docs)).is_none());
    }

    #[test]
    fn a_review_in_the_turn_settles_it_and_a_new_prompt_starts_over() {
        let mut parts = vec![user("do it")];
        parts.extend((0..4).map(|i| tool("Write", json!({"file_path": format!("s/f{i}.rs")}))));
        parts.push(tool("Agent", json!({"subagent_type": "the-dam:advisor", "prompt": "review"})));
        assert!(scan(&lines(&parts)).reviewed);
        assert!(reason(&scan(&lines(&parts))).is_none());
        parts.push(user("next"));
        let fresh = scan(&lines(&parts));
        assert!(fresh.edited.is_empty() && !fresh.reviewed);
    }

    #[test]
    fn a_version_bump_is_not_a_change_to_review() {
        let parts = [user("x"), tool("Edit", json!({"file_path": ".claude-plugin/plugin.json"})), tool("Edit", json!({"file_path": "dam/Cargo.toml"})), tool("Edit", json!({"file_path": "dam/Cargo.lock"})), tool("Edit", json!({"file_path": "src/a.rs"}))];
        assert_eq!(scan(&lines(&parts)).edited, vec!["src/a.rs"]);
        assert!(reason(&scan(&lines(&parts))).is_none());
    }

    #[test]
    fn one_file_named_two_ways_counts_once() {
        let parts = [user("x"), tool("Edit", json!({"file_path": r"C:\repo\src\a.rs"})), tool("Bash", json!({"command": "sed -i s/a/b/ src/a.rs"})), tool("Edit", json!({"file_path": "/repo/src/b.rs"})), tool("Edit", json!({"file_path": "/repo/src/c.rs"}))];
        assert_eq!(scan(&lines(&parts)).edited.len(), 3);
        assert!(reason(&scan(&lines(&parts))).is_some());
        assert!(reason(&scan(&lines(&parts[..4]))).is_none(), "two files, one of them named twice");
    }

    #[test]
    fn shell_edits_patches_and_subagent_work_are_read() {
        let patch = "*** Begin Patch\n*** Add File: a/one.rs\n+x\n*** Update File: a/two.rs\n*** End Patch";
        let parts = [user("x"), tool("apply_patch", json!({"input": patch})), tool("Bash", json!({"command": "sed -i s/a/b/ src/three.py"}))];
        assert_eq!(scan(&lines(&parts)).edited, vec!["a/one.rs", "a/two.rs", "three.py"]);
        let side = json!({"type": "assistant", "isSidechain": true, "message": {"content": [{"type": "tool_use", "name": "Write", "input": {"file_path": "x.rs"}}]}}).to_string();
        assert!(scan(&lines(&[user("x"), side])).edited.is_empty());
    }
}
