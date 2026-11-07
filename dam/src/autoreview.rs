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
    let _ = fs::write(file, state.to_string());
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
    save(&state_file(state, path), &json!({"path": doc, "sections": map, "nudged": digest(&text)}));

    Ok(format!("{doc}: {} sections recorded as reviewed\n", all.len()))
}

// The paths a tool call names: file paths in its input (a `.md` string, or an `*** Add File:` line of a patch).
fn paths_in(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => {
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
    let mut lines = Vec::new();
    for path in paths {
        let doc = if Path::new(&path).is_absolute() { PathBuf::from(&path) } else { cwd.join(&path) };
        let Ok(text) = fs::read_to_string(&doc) else { continue };
        if !is_design(&doc, &text) {
            continue;
        }
        let mut state = load(state_dir, &doc);
        let changed = changed_sections(&text, &state);
        if changed.is_empty() || state["nudged"].as_str() == Some(digest(&text).as_str()) {
            continue;
        }
        state["nudged"] = json!(digest(&text));
        save(&state_file(state_dir, &doc), &state);
        let name = doc.strip_prefix(&cwd).unwrap_or(&doc).to_string_lossy().replace('\\', "/");
        let what = if state["sections"].is_null() { "never reviewed".to_string() } else { format!("changed since its review: {}", changed.join(", ")) };
        lines.push(if mode == "ask" {
            format!("Design document {name} is written ({what}). Offer the user a goldfish review, /the-dam:goldfish {name}: it uses fresh sub-agents, so it costs tokens; only the changed sections are reviewed.")
        } else {
            format!("Design document {name} is written ({what}). Run /the-dam:goldfish {name} now, without asking: it reviews only the changed sections (the rest was already reviewed) and records the result. Skip it only if the user said they are still drafting.")
        });
    }

    (!lines.is_empty()).then(|| lines.join("\n"))
}

fn mode() -> String {
    std::env::var("DAM_GOLDFISH").ok().map(|value| value.to_lowercase()).filter(|value| matches!(value.as_str(), "auto" | "ask" | "off")).unwrap_or_else(|| "auto".to_string())
}

// `dam goldfish hook`: a PostToolUse payload on stdin; the host's context field on stdout, or nothing. Never fails the agent.
pub fn hook() {
    crate::watchdog(25);
    let mut bytes = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut bytes);
    let text = String::from_utf8_lossy(&bytes);
    let Ok(input) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) else { return };
    let Some(note) = nudge(&input, &mode()) else { return };
    let out = match crate::beaver::host() {
        crate::beaver::Host::Copilot => json!({"additionalContext": note}),
        _ => json!({"hookSpecificOutput": {"hookEventName": "PostToolUse", "additionalContext": note}}),
    };
    print!("{out}");
}

// The documents a session start checks: the README and what is under `docs/`.
fn session_docs(root: &Path) -> Vec<PathBuf> {
    let mut found = vec![root.join("README.md")];
    let mut folders = vec![root.join("docs")];
    while let Some(folder) = folders.pop() {
        let Ok(entries) = fs::read_dir(&folder) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if folders.len() < 40 {
                    folders.push(path);
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
        let again = nudge_in(&home, &input, "ask").expect("a note after an edit");
        assert!(again.contains("Goals") && !again.contains("Implementation Plan,") && again.contains("Offer the user"), "{again}");
        fs::remove_dir_all(&dir).ok();
    }
}
