// The deterministic half of a goldfish review, after the Elephant-Goldfish Model (Google, Apache 2.0): what a reviewer with
// no memory of the conversation is given, and which claims of a document the code no longer backs.
// The reviewing itself is the model's; this finds the split, the linked files and the drift without it.
use crate::graph::Index;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

const SPLIT_HEADERS: [&str; 4] = ["detailed implementation plan", "implementation plan", "work breakdown", "implementation steps"];
const MOST_SOURCE_BYTES: u64 = 2_000_000;
const MOST_MAP_FILES: usize = 12;

// The 1-based line of the header that starts the implementation plan; None where the document has only a specification.
pub fn split_line(text: &str) -> Option<usize> {
    text.lines().position(|line| {
        let head = line.trim_start();
        head.starts_with("##") && !head.starts_with("###") && {
            let title = head.trim_start_matches('#').trim().to_lowercase();
            SPLIT_HEADERS.iter().any(|name| title == *name || title.starts_with(&format!("{name}:")))
        }
    })
    .map(|index| index + 1)
}

// The local targets of the Markdown links of `text`, without fragments; anchors, URLs and mail links are left out.
fn link_targets(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("](") {
        rest = &rest[at + 2..];
        let Some(end) = rest.find(')') else { break };
        let target = rest[..end].split_whitespace().next().unwrap_or("").split('#').next().unwrap_or("");
        let is_remote = target.contains("://") || target.starts_with("mailto:");
        if !target.is_empty() && !is_remote && !found.iter().any(|known| known == target) {
            found.push(target.to_string());
        }
        rest = &rest[end..];
    }

    found
}

// The linked files that exist and stay inside `root` (symbolic links followed), as a reviewer may be given them.
pub fn linked_files(doc: &Path, text: &str, root: &Path) -> Vec<PathBuf> {
    let Ok(root) = root.canonicalize() else { return Vec::new() };
    let folder = doc.parent().unwrap_or(Path::new("."));
    let mut files: Vec<PathBuf> = link_targets(text)
        .iter()
        .filter_map(|target| folder.join(target).canonicalize().ok())
        .filter(|real| real.is_file() && real.starts_with(&root))
        .collect();
    files.sort();
    files.dedup();

    files
}

// The `code spans` of a document, once each.
fn code_spans(text: &str) -> Vec<String> {
    let mut spans = Vec::new();
    let mut in_fence = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        for (index, part) in line.split('`').enumerate() {
            if index % 2 == 1 && !part.is_empty() && !spans.iter().any(|known| known == part) {
                spans.push(part.to_string());
            }
        }
    }

    spans
}

fn looks_like_path(span: &str) -> bool {
    // A bare file name (`config.toml`) is as likely a user's file as this project's, a home path is never this project's: neither is checked.
    let has_ext = span.rsplit_once('.').is_some_and(|(stem, ext)| !stem.is_empty() && (1..=5).contains(&ext.len()) && ext.chars().all(|c| c.is_ascii_alphanumeric()) && ext.chars().any(|c| c.is_ascii_alphabetic()));
    !span.contains(char::is_whitespace) && !span.contains("://") && !span.starts_with(['-', '/', '~']) && !span.contains(['*', '<', '{', '$', '=', '(']) && span.contains('/') && (has_ext || span.ends_with('/'))
}

// A name a program would define: snake_case or camelCase or Pascal case with a join in it, not a plain English word.
fn looks_like_identifier(span: &str) -> bool {
    let long_enough = span.len() >= 5;
    let plain = span.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') && !span.starts_with(|c: char| c.is_ascii_digit());
    let joined = span.contains('_') || span.chars().skip(1).any(|c| c.is_ascii_uppercase()) && span.chars().any(|c| c.is_ascii_lowercase());

    long_enough && plain && joined
}

fn words_of(text: &str, into: &mut HashSet<String>) {
    let mut word = String::new();
    for c in text.chars().chain(std::iter::once(' ')) {
        if c.is_ascii_alphanumeric() || c == '_' {
            word.push(c);
        } else if !word.is_empty() {
            if word.len() >= 5 {
                into.insert(std::mem::take(&mut word));
            } else {
                word.clear();
            }
        }
    }
}

// Every identifier-like word of the code files `files` (the map's list of what the project's code is).
fn code_words(files: &[String]) -> HashSet<String> {
    let mut words = HashSet::new();
    for file in files {
        if fs::metadata(file).is_ok_and(|m| m.len() <= MOST_SOURCE_BYTES) {
            if let Ok(bytes) = fs::read(file) {
                words_of(&String::from_utf8_lossy(&bytes), &mut words);
            }
        }
    }

    words
}

pub struct Drift {
    pub paths: Vec<String>,
    pub names: Vec<String>,
}

// What `text` says in code spans that the project no longer has: files that are gone, names no code file contains.
// Paths are taken from the folder of the document, then from `root`.
pub fn drift_of(doc: &Path, text: &str, root: &Path, words: &HashSet<String>, map: Option<&Index>) -> Drift {
    let folder = doc.parent().unwrap_or(Path::new("."));
    let mut missing = Drift { paths: Vec::new(), names: Vec::new() };
    for span in code_spans(text) {
        if looks_like_path(&span) {
            let clean = span.trim_end_matches('/');
            let exists = |base: &Path| base.join(clean).exists();
            if !exists(folder) && !exists(root) {
                // The map knows where a file went.
                missing.paths.push(match map.and_then(|map| map.file_named(clean)) {
                    Some(now) => format!("{span} (now {now})"),
                    None => span,
                });
            }
        } else if looks_like_identifier(&span) && !map.is_some_and(|map| map.knows(&span)) && !words.contains(&span) {
            missing.names.push(match map.and_then(|map| map.spelled_as(&span)) {
                Some(now) => format!("{span} (now {now})"),
                None => span,
            });
        }
    }

    missing
}

fn prepare(doc: &str, extra: &[String]) -> Result<String, String> {
    let path = Path::new(doc);
    let text = fs::read_to_string(path).map_err(|error| format!("could not read {doc}: {error}"))?;
    let lines = text.lines().count();
    let root = std::env::current_dir().map_err(|error| error.to_string())?;
    let mut out = format!("document: {doc}\nlines: {lines}\n");
    match split_line(&text) {
        Some(at) if at > 1 => out.push_str(&format!("specification: lines 1-{}\nplan: lines {at}-{lines}\n", at - 1)),
        Some(_) => out.push_str(&format!("specification: none\nplan: lines 1-{lines}\n")),
        None => out.push_str(&format!("specification: lines 1-{lines}\nplan: none\n")),
    }
    // Fresh eyes read a section once: what was reviewed and has not changed is not sent again.
    let changed = crate::autoreview::changed_since_review(path, &text);
    if changed.is_empty() {
        out.push_str("changed since the last review: nothing; review nothing\n");
    } else if crate::autoreview::sections(&text).len() == changed.len() {
        out.push_str("changed since the last review: everything (first review)\n");
    } else {
        out.push_str(&format!("changed since the last review: {} (review only these sections)\n", changed.join(", ")));
    }
    out.push_str("context (give each reviewer these paths, nothing of the conversation):\n");
    let mut files = linked_files(path, &text, &root);
    for file in extra {
        if let Ok(real) = Path::new(file).canonicalize() {
            if real.is_file() && !files.contains(&real) {
                files.push(real);
            }
        }
    }
    if files.is_empty() {
        out.push_str("  none\n");
    }
    for file in files {
        out.push_str(&format!("  {}\n", file.display().to_string().trim_start_matches(r"\\?\")));
    }
    // The code the document names, found by the map: the source material a reviewer checks the claims against.
    if let Some(map) = crate::graph::index() {
        let mut defining: Vec<String> = Vec::new();
        for span in code_spans(&text).iter().filter(|span| looks_like_identifier(span)) {
            for file in map.defined_in(span) {
                if !defining.iter().any(|known| known == file) {
                    defining.push(file.to_string());
                }
            }
        }
        defining.truncate(MOST_MAP_FILES);
        if !defining.is_empty() {
            out.push_str(&format!("defining the code it names, from the map ({}):\n", defining.len()));
            for file in defining {
                out.push_str(&format!("  {file}\n"));
            }
        }
    }
    let outside: Vec<String> = link_targets(&text).into_iter().filter(|t| !path.parent().unwrap_or(Path::new(".")).join(t).canonicalize().is_ok_and(|real| root.canonicalize().is_ok_and(|r| real.starts_with(r)))).collect();
    if !outside.is_empty() {
        out.push_str(&format!("left out (missing, outside the folder; ask the user): {}\n", outside.join(", ")));
    }

    Ok(out)
}

fn drift_report(docs: &[String]) -> (String, bool) {
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let words = code_words(&crate::graph::source_files());
    let map = crate::graph::index();
    let mut out = String::new();
    let mut drifted = false;
    for doc in docs {
        let Ok(text) = fs::read_to_string(doc) else {
            out.push_str(&format!("{doc}: unreadable\n"));
            drifted = true;
            continue;
        };
        let found = drift_of(Path::new(doc), &text, &root, &words, map.as_ref());
        if found.paths.is_empty() && found.names.is_empty() {
            out.push_str(&format!("{doc}: no drift\n"));
            continue;
        }
        drifted = true;
        out.push_str(&format!("{doc}:\n"));
        for path in &found.paths {
            out.push_str(&format!("  file missing: {path}\n"));
        }
        for name in &found.names {
            out.push_str(&format!("  name not in the code: {name}\n"));
        }
    }

    (out, drifted)
}

pub fn run(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("prepare") if args.len() > 1 => match prepare(&args[1], &args[2..]) {
            Ok(out) => {
                print!("{out}");
                0
            }
            Err(error) => {
                eprintln!("{error}");
                1
            }
        },
        Some("drift") if args.len() > 1 => {
            let (out, drifted) = drift_report(&args[1..]);
            print!("{out}");
            i32::from(drifted)
        }
        // Records that the document, as it is now, has been reviewed.
        Some("mark") if args.len() > 1 => match crate::autoreview::mark(&args[1]) {
            Ok(out) => {
                print!("{out}");
                0
            }
            Err(error) => {
                eprintln!("{error}");
                1
            }
        },
        // The PostToolUse hook that says when a design document is ready for review.
        Some("hook") => {
            crate::autoreview::hook();
            0
        }
        _ => {
            eprintln!("usage: dam goldfish prepare DOCUMENT [CONTEXT_FILE...] | drift DOCUMENT... | mark DOCUMENT | hook");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_plan_header_and_only_that() {
        let text = "# Title\n\n## Goals\n\ntext\n\n## Implementation Plan\n\n1. a\n";
        assert_eq!(split_line(text), Some(7));
        assert_eq!(split_line("# T\n\n### Implementation plan\n"), None);
        assert_eq!(split_line("# T\n\n## Detailed Implementation Plan: v2\n"), Some(3));
        assert_eq!(split_line("# T\n\n## Goals\n"), None);
    }

    #[test]
    fn links_keep_local_targets_without_fragments() {
        let text = "[a](docs/a.md#top) [b](https://x.dev/b) [c](#here) [d](docs/a.md) [e](mailto:x@y.z) [f](d/f.md \"title\")";
        assert_eq!(link_targets(text), vec!["docs/a.md", "d/f.md"]);
    }

    #[test]
    fn spans_skip_fenced_blocks() {
        let text = "use `alpha_one` here\n```\n`inside_fence`\n```\nand `beta.rs`";
        assert_eq!(code_spans(text), vec!["alpha_one", "beta.rs"]);
    }

    #[test]
    fn tells_paths_and_names_from_words() {
        assert!(looks_like_path("src/main.rs") && looks_like_path(".github/ci.yml") && looks_like_path("docs/"));
        assert!(!looks_like_path("Cargo.toml") && !looks_like_path("~/.codex/config.toml") && !looks_like_path("v5.3.3") && !looks_like_path("a/1.2"));
        assert!(!looks_like_path("--flag") && !looks_like_path("a b.c") && !looks_like_path("https://x.y/z") && !looks_like_path("x*.rs"));
        assert!(looks_like_identifier("split_line") && looks_like_identifier("splitLine") && looks_like_identifier("SplitLine"));
        assert!(!looks_like_identifier("split") && !looks_like_identifier("Plan") && !looks_like_identifier("a_b"));
    }

    #[test]
    fn drift_names_what_the_code_lost() {
        let dir = std::env::temp_dir().join(format!("goldfish-drift-{}", std::process::id()));
        fs::create_dir_all(dir.join("src")).unwrap();
        fs::write(dir.join("src/main.rs"), "fn keep_me() {}").unwrap();
        let doc = dir.join("design.md");
        let text = "`src/main.rs` has `keep_me`; `src/gone.rs` had `lost_name`.";
        let words = code_words(&[dir.join("src/main.rs").to_string_lossy().into_owned()]);
        let found = drift_of(&doc, text, &dir, &words, None);
        assert_eq!(found.paths, vec!["src/gone.rs"]);
        assert_eq!(found.names, vec!["lost_name"]);
    }

    #[test]
    fn the_map_says_where_a_file_went_and_what_a_name_became() {
        use std::collections::HashMap;
        let map = Index {
            files: vec!["code/main.rs".into()],
            defined: HashMap::from([("splitLine".to_string(), vec![0])]),
            spelled: HashMap::from([("splitline".to_string(), "splitLine".to_string())]),
        };
        let dir = std::env::temp_dir().join("goldfish-map-none");
        let found = drift_of(&dir.join("d.md"), "`src/main.rs` and `split_line` and `splitLine`", &dir, &HashSet::new(), Some(&map));
        assert_eq!(found.paths, vec!["src/main.rs (now code/main.rs)"]);
        assert_eq!(found.names, vec!["split_line (now splitLine)"]);
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn linked_files_stay_inside_the_folder() {
        let dir = std::env::temp_dir().join(format!("goldfish-links-{}", std::process::id()));
        fs::create_dir_all(dir.join("docs")).unwrap();
        fs::write(dir.join("docs/a.md"), "a").unwrap();
        let doc = dir.join("design.md");
        let found = linked_files(&doc, "[a](docs/a.md#x) [gone](docs/none.md) [up](../outside.md)", &dir);
        assert_eq!(found.len(), 1);
        assert!(found[0].ends_with("a.md"));
        fs::remove_dir_all(&dir).ok();
    }
}
