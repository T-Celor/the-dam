// Makes the goldfish review and the documents' drift happen by themselves, and cheaply: a design document that changed
// is flagged once, with only the sections that changed since it was last reviewed (the rest was already read by fresh
// eyes), and a session start says once when a document names files the code no longer has. No model runs here.
use crate::goldfish::{drift_of, split_line};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const NOT_DESIGNS: [&str; 6] = ["readme", "changelog", "contributing", "goldfish_review_report", "concepts_brief", "license"];
const NAME_HINTS: [&str; 5] = ["design", "spec", "rfc", "proposal", "architecture"];
const SHORTEST_DESIGN: usize = 800;
const MOST_DOCS: usize = 20;

fn digest(text: &str) -> String {
    format!("{:x}", md5::compute(text.as_bytes()))
}

// The sections of a Markdown document: its top (before the first `##`) and each `##` section, with a digest of its text.
pub fn sections(text: &str) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    let mut title = "(top)".to_string();
    let mut body = String::new();
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
        }
        let heading = !in_fence && line.starts_with("## ");
        if heading {
            if !body.trim().is_empty() || title != "(top)" {
                found.push((title.clone(), digest(&body)));
            }
            title = line.trim_start_matches('#').trim().to_string();
            body.clear();
        }
        body.push_str(line);
        body.push('\n');
    }
    found.push((title, digest(&body)));

    found
}

fn state_file(state: &Path, doc: &Path) -> PathBuf {
    let full = doc.canonicalize().unwrap_or_else(|_| doc.to_path_buf());

    state.join("goldfish").join(format!("{}.json", digest(&full.to_string_lossy())))
}

fn load(state: &Path, doc: &Path) -> Value {
    fs::read_to_string(state_file(state, doc)).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_else(|| json!({}))
}

fn save(file: &Path, state: &Value) {
    if let Some(folder) = file.parent() {
        let _ = fs::create_dir_all(folder);
    }
    let _ = crate::paths::write_atomic(file, state.to_string().as_bytes());
}

// A document's state is read and rewritten by several hooks that may run at once: each takes this lock for its read-modify-write.
fn lock_of(file: &Path) -> PathBuf {
    file.with_extension("lock")
}

// The titles of the sections whose text is not what was last reviewed (all of them before a first review).
pub fn changed_sections(text: &str, state: &Value) -> Vec<String> {
    let known: BTreeMap<&str, &str> = state["sections"].as_object().map(|map| map.iter().filter_map(|(title, sum)| Some((title.as_str(), sum.as_str()?))).collect()).unwrap_or_default();

    sections(text).into_iter().filter(|(title, sum)| known.get(title.as_str()) != Some(&sum.as_str())).map(|(title, _)| title).collect()
}

pub fn changed_since_review(doc: &Path, text: &str) -> Vec<String> {
    changed_sections(text, &load(&crate::economizer::state_root(), doc))
}

// A document the review is for: a Markdown file that reads as a design, finished enough to be read by someone else.
pub fn is_design(path: &Path, text: &str) -> bool {
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    let is_markdown = path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("md"));
    let skipped = path.components().any(|part| matches!(part.as_os_str().to_str(), Some("node_modules" | "target" | ".git")));
    if !is_markdown || skipped || text.len() < SHORTEST_DESIGN || NOT_DESIGNS.iter().any(|name| stem == *name) {
        return false;
    }
    let looks_like_one = split_line(text).is_some() || NAME_HINTS.iter().any(|hint| stem.contains(hint));
    // A design with a plan is done being written; one without is taken for done when it has the sections of a spec.
    let finished = split_line(text).is_some() || sections(text).len() >= 5;

    looks_like_one && finished
}

// Records that the document, as it is now, has been reviewed.
pub fn mark(doc: &str) -> Result<String, String> {
    mark_in(&crate::economizer::state_root(), doc)
}

fn mark_in(state: &Path, doc: &str) -> Result<String, String> {
    let path = Path::new(doc);
    let text = fs::read_to_string(path).map_err(|error| format!("could not read {doc}: {error}"))?;
    let all = sections(&text);
    let map: serde_json::Map<String, Value> = all.iter().map(|(title, sum)| (title.clone(), json!(sum))).collect();
    let file = state_file(state, path);
    crate::paths::with_lock(&lock_of(&file), || save(&file, &json!({"path": doc, "sections": map, "nudged": digest(&text)})));

    Ok(format!("{doc}: {} sections recorded as reviewed\n", all.len()))
}

// The paths a tool call names: file paths in its input (a `.md` string, or an `*** Add File:` line of a patch).
fn paths_in(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => {
            // A shell command that writes a document (`cat > docs/design.md <<EOF`, `tee`, `Set-Content`): the Write tool is not the only way in.
            if text.contains('>') || text.contains("tee ") || text.contains("Out-File") || text.contains("Set-Content") {
                for token in text.split_whitespace() {
                    let token = token.trim_matches(|c: char| matches!(c, '\'' | '"' | ';' | '>' | '(' | ')' | '`'));
                    if token.to_lowercase().ends_with(".md") && !token.starts_with('-') && !out.iter().any(|known| known == token) {
                        out.push(token.to_string());
                    }
                }
            }
            for line in text.lines() {
                let line = line.trim();
                let named = line.strip_prefix("*** Add File:").or_else(|| line.strip_prefix("*** Update File:")).map(str::trim);
                let candidate = named.unwrap_or(if text.lines().count() == 1 { line } else { "" });
                if candidate.to_lowercase().ends_with(".md") && !candidate.contains(char::is_whitespace) && !out.iter().any(|known| known == candidate) {
                    out.push(candidate.to_string());
                }
            }
        }
        Value::Array(items) => items.iter().for_each(|item| paths_in(item, out)),
        Value::Object(map) => map.values().for_each(|item| paths_in(item, out)),
        _ => {}
    }
}

// What to tell the agent after a tool call that wrote design documents: once per version of a document, and only the
// sections that changed since its last review. `mode` is auto (do it), ask (offer it) or off.
pub fn nudge(input: &Value, mode: &str) -> Option<String> {
    nudge_in(&crate::economizer::state_root(), input, mode)
}

fn nudge_in(state_dir: &Path, input: &Value, mode: &str) -> Option<String> {
    if mode == "off" {
        return None;
    }
    let cwd = input["cwd"].as_str().filter(|cwd| !cwd.is_empty()).map(PathBuf::from).or_else(|| std::env::current_dir().ok())?;
    let mut paths = Vec::new();
    for key in ["tool_input", "toolInput", "toolArgs", "input"] {
        match &input[key] {
            Value::String(text) => paths_in(&serde_json::from_str(text).unwrap_or_else(|_| json!(text)), &mut paths),
            other => paths_in(other, &mut paths),
        }
    }
    let session = input["session_id"].as_str().or_else(|| input["sessionId"].as_str()).unwrap_or("");
    let mut lines = Vec::new();
    for path in paths {
        let doc = if Path::new(&path).is_absolute() { PathBuf::from(&path) } else { cwd.join(&path) };
        let Ok(text) = fs::read_to_string(&doc) else { continue };
        if !is_design(&doc, &text) {
            continue;
        }
        let file = state_file(state_dir, &doc);
        let flagged = crate::paths::with_lock(&lock_of(&file), || {
            let mut state = load(state_dir, &doc);
            let changed = changed_sections(&text, &state);
            // Once per version, and once per document in a session: the fixes made after a review are not a new draft.
            if changed.is_empty() || state["nudged"].as_str() == Some(digest(&text).as_str()) || (!session.is_empty() && state["session"].as_str() == Some(session)) {
                return None;
            }
            let what = if state["sections"].is_null() { "never reviewed".to_string() } else { format!("changed since its review: {}", changed.join(", ")) };
            state["nudged"] = json!(digest(&text));
            state["session"] = json!(session);
            state["path"] = json!(doc.to_string_lossy());
            // The review is owed until `prepare` (its first step) has run for this version.
            state["started"] = json!(false);
            state["denied"] = json!(0);
            save(&file, &state);

            Some(what)
        });
        let Some(what) = flagged else { continue };
        let name = doc.strip_prefix(&cwd).unwrap_or(&doc).to_string_lossy().replace('\\', "/");
        lines.push(if mode == "ask" {
            format!("Design document {name} is written ({what}). Offer the user a goldfish review, /the-dam:goldfish {name}: it uses fresh sub-agents, so it costs tokens; only the changed sections are reviewed.")
        } else {
            format!("Design document {name} is written ({what}). Before you implement anything from it, run /the-dam:goldfish {name} now, without asking: it reviews only the changed sections (the rest was already reviewed). Skip it only if the user said they are still drafting.")
        });
    }

    (!lines.is_empty()).then(|| lines.join("\n"))
}

// The review of this document has begun: the goldfish skill ran `prepare`.
pub fn review_started(doc: &Path) {
    let state_dir = crate::economizer::state_root();
    let file = state_file(&state_dir, doc);
    crate::paths::with_lock(&lock_of(&file), || {
        let mut state = load(&state_dir, doc);
        if state["path"].is_null() {
            return;
        }
        state["started"] = json!(true);
        save(&file, &state);
    });
}

// Code may not be written from a design that was flagged in this session and has not been sent to review. Asked twice at
// most, so a host that cannot run the review (no sub-agent tool) is never stuck. Returns the reason to refuse the edit.
pub fn pending_review(input: &Value, state_dir: &Path, mode: &str) -> Option<String> {
    if mode != "auto" {
        return None;
    }
    let tool = input["tool_name"].as_str()?.to_lowercase();
    let targets: Vec<String> = match tool.as_str() {
        "write" | "edit" | "multiedit" => vec![input["tool_input"]["file_path"].as_str()?.to_lowercase()],
        // A shell command that writes code is the same as an edit.
        "bash" | "powershell" => crate::loopguard::shell_edit_targets(input["tool_input"]["command"].as_str()?).into_iter().map(|name| name.to_lowercase()).collect(),
        _ => return None,
    };
    // Writing documents is how the design gets written and revised.
    let is_doc = |target: &String| [".md", ".txt", ".rst"].iter().any(|ext| target.ends_with(ext));
    if targets.is_empty() || targets.iter().all(is_doc) {
        return None;
    }
    let session = input["session_id"].as_str().or_else(|| input["sessionId"].as_str()).filter(|session| !session.is_empty())?;
    for entry in fs::read_dir(state_dir.join("goldfish")).ok()?.flatten() {
        let file = entry.path();
        if file.extension().is_none_or(|ext| ext != "json") {
            continue;
        }
        let owed = crate::paths::with_lock(&lock_of(&file), || {
            let mut state = fs::read_to_string(&file).ok().and_then(|text| serde_json::from_str::<Value>(&text).ok())?;
            let doc = state["path"].as_str().map(PathBuf::from)?;
            if state["session"].as_str() != Some(session) || state["started"].as_bool() != Some(false) || state["denied"].as_i64().unwrap_or(0) >= 2 || !doc.is_file() {
                return None;
            }
            state["denied"] = json!(state["denied"].as_i64().unwrap_or(0) + 1);
            save(&file, &state);

            Some(doc)
        });
        let Some(doc) = owed else { continue };
        let name = doc.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();

        return Some(format!("Not yet: the design {name} has not been reviewed. Run /the-dam:goldfish on it first (it takes a minute and reviews only what changed), then write the code. If the review cannot run here, repeat the edit and it will go through."));
    }

    None
}

// Whether a (normalized) tool call writes: an edit tool, a patch, or a shell command.
pub fn writes(input: &Value) -> bool {
    matches!(input["tool_name"].as_str().unwrap_or("").to_lowercase().as_str(), "write" | "edit" | "multiedit" | "apply_patch" | "bash" | "powershell")
}

pub fn mode() -> String {
    std::env::var("DAM_GOLDFISH").ok().map(|value| value.to_lowercase()).filter(|value| matches!(value.as_str(), "auto" | "ask" | "off")).unwrap_or_else(|| "auto".to_string())
}

// `dam goldfish hook`: a PostToolUse payload on stdin; the host's context field on stdout, or nothing. Never fails the agent.
pub fn hook() {
    crate::watchdog(25);
    let mut bytes = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut bytes);
    let text = String::from_utf8_lossy(&bytes);
    let Ok(input) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) else { return };
    // Codex and Copilot run this after every tool: only a write counts (a read of a design is not a draft of it).
    let host = crate::beaver::host();
    if host != crate::beaver::Host::Claude && !writes(&crate::hosts::normalize(host, Some("PostToolUse"), &input)) {
        return;
    }
    let Some(note) = nudge(&input, &mode()) else { return };
    crate::journal::event(host, input["session_id"].as_str().or_else(|| input["sessionId"].as_str()).unwrap_or(""), "📐", "design document flagged for goldfish review");
    let out = match host {
        crate::beaver::Host::Copilot => json!({"additionalContext": note}),
        _ => json!({"hookSpecificOutput": {"hookEventName": "PostToolUse", "additionalContext": note}}),
    };
    print!("{out}");
}

// The documents a session start checks: the README and what is under `docs/`.
fn session_docs(root: &Path) -> Vec<PathBuf> {
    let mut found = vec![root.join("README.md")];
    let mut folders = vec![(root.join("docs"), 0)];
    while let Some((folder, depth)) = folders.pop() {
        let Ok(entries) = fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_type().is_ok_and(|kind| kind.is_symlink()) && path.is_dir() {
                continue;
            }
            if path.is_dir() {
                if depth < 6 && folders.len() < 40 {
                    folders.push((path, depth + 1));
                }
            } else if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("md")) {
                found.push(path);
            }
        }
    }
    found.sort();
    found.retain(|path| path.is_file());
    found.truncate(MOST_DOCS);

    found
}

// One line when the README or a document under `docs/` names a file that is gone from a folder that still exists (or
// that the map finds elsewhere), and only when that is news: the same list is not said twice.
pub fn drift_note(root: &Path) -> Option<String> {
    let map = crate::graph::index_if_built();
    let mut gone: Vec<String> = Vec::new();
    for doc in session_docs(root) {
        let Ok(text) = fs::read_to_string(&doc) else { continue };
        let name = doc.strip_prefix(root).unwrap_or(&doc).to_string_lossy().replace('\\', "/");
        for entry in drift_of(&doc, &text, root, &std::collections::HashSet::new(), map.as_ref()).paths {
            let span = entry.split(" (now ").next().unwrap_or(&entry);
            let first = span.trim_start_matches("./").split('/').next().unwrap_or("");
            // A path into a folder this project does not have (a user's `.codex/`) is not this project's claim.
            if entry.contains("(now ") || root.join(first).exists() {
                gone.push(format!("{name}: {entry}"));
            }
        }
    }
    if gone.is_empty() {
        return None;
    }
    let file = crate::economizer::state_root().join("goldfish").join(format!("drift-{}.txt", digest(&root.to_string_lossy())));
    let sum = digest(&gone.join("\n"));
    if fs::read_to_string(&file).is_ok_and(|seen| seen.trim() == sum) {
        return None;
    }
    if let Some(folder) = file.parent() {
        let _ = fs::create_dir_all(folder);
    }
    let _ = fs::write(&file, &sum);
    let shown: Vec<&str> = gone.iter().take(3).map(String::as_str).collect();

    Some(format!("Documents out of date: {}{}. Fix them when you touch that area; `dam goldfish drift <document>` lists all.", shown.join("; "), if gone.len() > 3 { format!("; +{} more", gone.len() - 3) } else { String::new() }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "# Retry\n\nIntro text that is long enough.\n\n## Goals\n\nGoal one.\n\n## Implementation Plan\n\n1. Edit the file.\n";

    #[test]
    fn sections_split_on_second_level_headings_outside_fences() {
        let titles: Vec<String> = sections("# T\n\ntop\n\n## A\n\na\n\n```\n## not a heading\n```\n\n## B\n\nb\n").into_iter().map(|(title, _)| title).collect();
        assert_eq!(titles, vec!["(top)", "A", "B"]);
    }

    #[test]
    fn only_edited_sections_count_as_changed() {
        let before = sections(DOC);
        let saved = json!({"sections": before.iter().map(|(t, s)| (t.clone(), json!(s))).collect::<serde_json::Map<_, _>>()});
        assert!(changed_sections(DOC, &saved).is_empty());
        let edited = DOC.replace("Goal one.", "Goal one, changed.");
        assert_eq!(changed_sections(&edited, &saved), vec!["Goals"]);
        assert_eq!(changed_sections(DOC, &json!({})).len(), 3);
    }

    #[test]
    fn a_design_is_a_finished_markdown_document_that_reads_as_one() {
        let long = format!("{DOC}{}", "More text. ".repeat(80));
        assert!(is_design(Path::new("docs/retry.md"), &long));
        assert!(!is_design(Path::new("README.md"), &long));
        assert!(!is_design(Path::new("docs/retry.md"), DOC));
        assert!(!is_design(Path::new("src/a.rs"), &long));
        assert!(!is_design(Path::new("notes.md"), &long.replace("## Implementation Plan", "## Next")));
    }

    #[test]
    fn paths_come_from_inputs_and_patches() {
        let mut found = Vec::new();
        paths_in(&json!({"file_path": "docs/design.md", "content": "text\nmore"}), &mut found);
        paths_in(&json!({"input": "*** Begin Patch\n*** Add File: a/spec.md\n+x\n*** End Patch"}), &mut found);
        assert_eq!(found, vec!["docs/design.md", "a/spec.md"]);
    }

    #[test]
    fn a_written_design_is_flagged_once_with_its_changed_sections() {
        let dir = std::env::temp_dir().join(format!("goldfish-auto-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
                let home = dir.join("home");
        let doc = dir.join("design.md");
        fs::write(&doc, format!("{DOC}{}", "More text. ".repeat(80))).unwrap();
        let input = json!({"cwd": dir.to_str().unwrap(), "tool_input": {"file_path": doc.to_str().unwrap()}});
        let first = nudge_in(&home, &input, "auto").expect("a first note");
        assert!(first.contains("never reviewed") && first.contains("without asking"), "{first}");
        assert!(nudge_in(&home, &input, "auto").is_none(), "said twice");
        assert!(nudge_in(&home, &input, "off").is_none());
        mark_in(&home, doc.to_str().unwrap()).unwrap();
        let text = fs::read_to_string(&doc).unwrap().replace("Goal one.", "Goal one, changed.");
        fs::write(&doc, text).unwrap();
        let again_same_session = nudge_in(&home, &json!({"cwd": dir.to_str().unwrap(), "session_id": "s1", "tool_input": {"file_path": doc.to_str().unwrap()}}), "ask");
        assert!(again_same_session.is_some(), "a first note in session s1");
        let text2 = fs::read_to_string(&doc).unwrap().replace("Goal one, changed.", "Goal one, changed twice.");
        fs::write(&doc, text2).unwrap();
        assert!(nudge_in(&home, &json!({"cwd": dir.to_str().unwrap(), "session_id": "s1", "tool_input": {"file_path": doc.to_str().unwrap()}}), "ask").is_none(), "not twice in one session");
        let again = nudge_in(&home, &input, "ask").expect("a note after an edit");
        assert!(again.contains("Goals") && !again.contains("Implementation Plan,") && again.contains("Offer the user"), "{again}");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn code_waits_for_the_review_of_a_design_flagged_in_the_session() {
        let dir = std::env::temp_dir().join(format!("goldfish-pending-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let home = dir.join("home");
        let doc = dir.join("design.md");
        fs::write(&doc, format!("{DOC}{}", "More text. ".repeat(80))).unwrap();
        let flagged = json!({"cwd": dir.to_str().unwrap(), "session_id": "s9", "tool_input": {"file_path": doc.to_str().unwrap()}});
        assert!(nudge_in(&home, &flagged, "auto").is_some());
        let edit = |file: &str, session: &str| json!({"tool_name": "Write", "session_id": session, "tool_input": {"file_path": file}});
        // Documents and other sessions are free; code is refused, twice at most.
        assert!(pending_review(&edit("notes.md", "s9"), &home, "auto").is_none());
        assert!(pending_review(&edit("src/a.rs", "other"), &home, "auto").is_none());
        assert!(pending_review(&edit("src/a.rs", "s9"), &home, "ask").is_none());
        assert!(pending_review(&edit("src/a.rs", "s9"), &home, "auto").unwrap().contains("design.md"));
        assert!(pending_review(&edit("src/a.rs", "s9"), &home, "auto").is_some());
        assert!(pending_review(&edit("src/a.rs", "s9"), &home, "auto").is_none(), "a third time goes through");
        // Once the review has begun nothing is owed.
        let again = json!({"cwd": dir.to_str().unwrap(), "session_id": "s10", "tool_input": {"file_path": doc.to_str().unwrap()}});
        fs::write(&doc, fs::read_to_string(&doc).unwrap().replace("Goal one.", "Goal one, v2.")).unwrap();
        assert!(nudge_in(&home, &again, "auto").is_some());
        let mut state = load(&home, &doc);
        state["started"] = json!(true);
        save(&state_file(&home, &doc), &state);
        assert!(pending_review(&edit("src/a.rs", "s10"), &home, "auto").is_none());
        fs::remove_dir_all(&dir).ok();
    }
}
