// What the map takes from a project's documents and settings: Markdown and reStructuredText headings,
// the links between documents and the code names a document quotes; top-level keys and sections of
// YAML, TOML and JSON. No model and no network, as for the code.
use fancy_regex::Regex;
use std::sync::LazyLock;

const MOST_NAME_CHARS: usize = 120;
const MOST_KEYS: usize = 200;
const MOST_JSON_BYTES: usize = 512 * 1024;
// Lock files and generated listings say nothing a person looks for.
const SKIPPED_NAMES: [&str; 6] = ["package-lock.json", "yarn.lock", "pnpm-lock.yaml", "composer.lock", "cargo.lock", "poetry.lock"];

static HEADING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^ {0,3}(#{1,6})\s+(.+?)\s*#*\s*$").expect("fixed"));
static LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\]\(\s*<?([^)\s>#]+)").expect("fixed"));
static QUOTED: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`([A-Za-z_][A-Za-z0-9_]{2,})(?:\(\))?`").expect("fixed"));
static YAML_KEY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^["']?([A-Za-z0-9_][A-Za-z0-9_.-]*)["']?\s*:"#).expect("fixed"));
static TOML_TABLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*\[\[?\s*([^\]]+?)\s*\]\]?\s*(?:#.*)?$").expect("fixed"));
static TOML_KEY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^["']?([A-Za-z0-9_][A-Za-z0-9_.-]*)["']?\s*="#).expect("fixed"));

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum DocKind {
    Markdown,
    Rst,
    Yaml,
    Toml,
    Json,
}

pub fn doc_kind(path: &str) -> Option<DocKind> {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    if SKIPPED_NAMES.contains(&name.as_str()) || name.contains(".min.") {
        return None;
    }
    Some(match name.rsplit('.').next()? {
        "md" | "markdown" | "mdx" => DocKind::Markdown,
        "rst" => DocKind::Rst,
        "yaml" | "yml" => DocKind::Yaml,
        "toml" => DocKind::Toml,
        "json" => DocKind::Json,
        _ => return None,
    })
}

#[derive(Default, Debug, PartialEq)]
pub struct DocScan {
    // Headings and table names, with their line.
    pub sections: Vec<(String, usize)>,
    // Top-level keys, with their line.
    pub keys: Vec<(String, usize)>,
    // Paths the document links to, as written.
    pub links: Vec<String>,
    // Code names the document quotes, once each.
    pub mentions: Vec<String>,
}

fn short(text: &str) -> String {
    text.trim().chars().take(MOST_NAME_CHARS).collect()
}

fn captured(regex: &Regex, line: &str, group: usize) -> Option<String> {
    regex.captures(line).ok().flatten().and_then(|found| found.get(group).map(|part| part.as_str().to_string()))
}

fn prose(text: &str, kind: DocKind, out: &mut DocScan) {
    let lines: Vec<&str> = text.lines().collect();
    let mut in_fence = false;
    for (n, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let heading = match kind {
            DocKind::Markdown => captured(&HEADING, line, 2),
            // A title is the line its underline follows: `====`, `----`, `~~~~` at least as long.
            _ => lines.get(n + 1).filter(|under| {
                let under = under.trim_end();
                under.len() >= line.trim_end().len().max(3) && under.chars().all(|c| matches!(c, '=' | '-' | '~' | '^' | '*' | '#')) && !line.trim().is_empty()
            }).map(|_| line.to_string()),
        };
        if let Some(heading) = heading.filter(|heading| !heading.trim().is_empty()) {
            out.sections.push((short(&heading.replace('`', "")), n + 1));
        }
        for found in LINK.captures_iter(line).flatten() {
            let target = &found[1];
            if !target.contains("://") && !target.starts_with("mailto:") {
                out.links.push(target.to_string());
            }
        }
        for found in QUOTED.captures_iter(line).flatten() {
            let name = found[1].to_string();
            if !out.mentions.contains(&name) {
                out.mentions.push(name);
            }
        }
    }
}

fn settings(text: &str, kind: DocKind, out: &mut DocScan) {
    if kind == DocKind::Json {
        if text.len() > MOST_JSON_BYTES {
            return;
        }
        let Ok(serde_json::Value::Object(map)) = serde_json::from_str::<serde_json::Value>(text) else { return };
        for key in map.keys().take(MOST_KEYS) {
            let quoted = format!("\"{key}\"");
            let line = text.lines().position(|line| line.contains(&quoted)).map_or(1, |n| n + 1);
            out.keys.push((short(key), line));
        }
        return;
    }
    let mut in_table = false;
    for (n, line) in text.lines().enumerate() {
        if line.trim_start().starts_with('#') || out.keys.len() + out.sections.len() >= MOST_KEYS {
            continue;
        }
        if kind == DocKind::Toml {
            if let Some(table) = captured(&TOML_TABLE, line, 1) {
                in_table = true;
                out.sections.push((short(&table), n + 1));
            } else if let Some(key) = captured(&TOML_KEY, line, 1).filter(|_| !in_table) {
                out.keys.push((short(&key), n + 1));
            }
        } else if let Some(key) = captured(&YAML_KEY, line, 1) {
            // Only the top level: an indented key belongs to the one above it.
            out.keys.push((short(&key), n + 1));
        }
    }
}

pub fn scan_doc(text: &str, kind: DocKind) -> DocScan {
    let mut out = DocScan::default();
    match kind {
        DocKind::Markdown | DocKind::Rst => prose(text, kind, &mut out),
        DocKind::Yaml | DocKind::Toml | DocKind::Json => settings(text, kind, &mut out),
    }

    out
}

// `../ops/release.md` from `docs/guide/a.md` is `docs/ops/release.md`.
pub fn resolve_link(from: &str, target: &str) -> String {
    let mut parts: Vec<&str> = if target.starts_with('/') { Vec::new() } else { from.rsplit_once('/').map_or(Vec::new(), |(dir, _)| dir.split('/').collect()) };
    for part in target.trim_start_matches('/').split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }

    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_gives_headings_links_and_quoted_names() {
        let text = "# The dam\n\nSee [ops](../ops/release.md#steps) and [site](https://x.y).\n\n```sh\n# not a heading\n```\n\n## Release `steps` ##\nCall `apply_deny()` then `run`.\n";
        let scan = scan_doc(text, DocKind::Markdown);
        assert_eq!(scan.sections, [("The dam".to_string(), 1), ("Release steps".to_string(), 9)]);
        assert_eq!(scan.links, ["../ops/release.md"]);
        assert_eq!(scan.mentions, ["steps", "apply_deny", "run"]);
    }

    #[test]
    fn rst_titles_are_underlined_lines() {
        let scan = scan_doc("Install\n=======\n\ntext\n\nUse it\n------\n", DocKind::Rst);
        assert_eq!(scan.sections, [("Install".to_string(), 1), ("Use it".to_string(), 6)]);
    }

    #[test]
    fn settings_give_their_top_level_keys_and_tables() {
        let yaml = scan_doc("# c\nport: 8080\nserver:\n  host: x\n\"name\": dam\n", DocKind::Yaml);
        assert_eq!(yaml.keys, [("port".to_string(), 2), ("server".to_string(), 3), ("name".to_string(), 5)]);
        let toml = scan_doc("title = \"x\"\n[package]\nname = \"dam\"\n[[bin]]\n", DocKind::Toml);
        assert_eq!(toml.keys, [("title".to_string(), 1)]);
        assert_eq!(toml.sections, [("package".to_string(), 2), ("bin".to_string(), 4)]);
        let json = scan_doc("{\n  \"scripts\": {},\n  \"name\": \"x\"\n}", DocKind::Json);
        assert_eq!(json.keys, [("scripts".to_string(), 2), ("name".to_string(), 3)]);
        assert_eq!(scan_doc("[1, 2]", DocKind::Json), DocScan::default());
    }

    #[test]
    fn links_resolve_from_the_document() {
        assert_eq!(resolve_link("docs/guide/a.md", "../ops/release.md"), "docs/ops/release.md");
        assert_eq!(resolve_link("README.md", "./docs/a.md"), "docs/a.md");
        assert_eq!(resolve_link("docs/a.md", "/CHANGELOG.md"), "CHANGELOG.md");
    }

    #[test]
    fn lock_files_and_other_formats_stay_out() {
        assert_eq!(doc_kind("web/package-lock.json"), None);
        assert_eq!(doc_kind("notes.txt"), None);
        assert_eq!(doc_kind("docs/A.MD"), Some(DocKind::Markdown));
    }
}
