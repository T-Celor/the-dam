// The map comes first. A search for code names (a Grep for `charge_customer` or `post(`) is answered from the map before
// it runs: where each name is defined, what calls it. The search is refused once, with the answer as the reason; asked
// again, it runs. Where the map has no such name the search is left alone. Built where it is missing and the project is
// small enough. No model runs here.
use serde_json::{json, Value};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

// Searches answered from the map, and so refused once, in one session.
const MOST_ANSWERED: usize = 3;

// The code names a search pattern holds: `post\(|charge_customer` has two.
pub fn names_in(pattern: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for word in pattern.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_')) {
        let first_ok = word.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
        if word.len() >= 4 && first_ok && !names.iter().any(|known| known == word) {
            names.push(word.to_string());
        }
    }
    names.truncate(6);

    names
}

fn seen_file(state_dir: &Path, session: &str) -> PathBuf {
    let safe: String = session.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();

    state_dir.join("goldfish").join(format!("mapfirst-{safe}.txt"))
}

// The map's answer to refuse this tool call with, or None to let it run.
pub fn decide(input: &Value, state_dir: &Path) -> Option<String> {
    let tool = input["tool_name"].as_str()?.to_lowercase();
    // Code is not written from a design that was never sent to review.
    if matches!(tool.as_str(), "write" | "edit" | "multiedit") {
        return crate::autoreview::pending_review(input, state_dir, &crate::autoreview::mode());
    }
    let command = input["tool_input"]["command"].as_str().unwrap_or("");
    let shell = matches!(tool.as_str(), "bash" | "powershell");
    if shell {
        if let Some(reason) = crate::autoreview::pending_review(input, state_dir, &crate::autoreview::mode()) {
            return Some(reason);
        }
    }
    let explores = match tool.as_str() {
        "grep" | "glob" => true,
        "read" => input["tool_input"]["file_path"].as_str().is_some_and(|path| crate::scan::lang_of(path).is_some()),
        _ => shell && is_exploring(command),
    };
    // Having asked the map is having complied: nothing more is owed for the session.
    if shell && (command.contains("graph query") || command.contains("graph facts")) {
        if let (Some(cwd), Some(session)) = (input["cwd"].as_str(), input["session_id"].as_str()) {
            let _ = std::env::set_current_dir(cwd);
            let file = seen_file(state_dir, session);
            let seen = fs::read_to_string(&file).unwrap_or_default();
            if !seen.lines().any(|line| line == "overview") {
                if let Some(folder) = file.parent() {
                    let _ = fs::create_dir_all(folder);
                }
                let _ = fs::write(&file, format!("{seen}overview\n"));
            }
        }
        return None;
    }
    if !explores {
        return None;
    }
    let cwd = input["cwd"].as_str().filter(|cwd| !cwd.is_empty()).map(PathBuf::from)?;
    std::env::set_current_dir(&cwd).ok()?;
    let session = input["session_id"].as_str().or_else(|| input["sessionId"].as_str()).unwrap_or("session");
    let file = seen_file(state_dir, session);
    let seen = fs::read_to_string(&file).unwrap_or_default();
    let remember = |key: &str| {
        if let Some(folder) = file.parent() {
            let _ = fs::create_dir_all(folder);
        }
        let _ = fs::write(&file, format!("{seen}{key}\n"));
    };
    // A search for code names the map knows is answered by the map.
    if tool == "grep" {
        let names = names_in(input["tool_input"]["pattern"].as_str().unwrap_or(""));
        // At most MOST_ANSWERED refusals a session: a model that keeps varying its pattern is not asking the map, and each refusal costs a turn.
        let answered = seen.lines().filter(|line| line.starts_with("a:")).count();
        if !names.is_empty() && answered < MOST_ANSWERED && crate::graph::ensure_built() {
            if let Some(answer) = crate::answers::ask_graph(&cwd, &names) {
                let key = format!("a:{:x}", md5::compute(names.join(" ").as_bytes()));
                if !seen.lines().any(|line| line == key) {
                    remember(&key);

                    return Some(format!("Answered from the map before searching:
{answer}
Use this. If it does not show what you need, run the same search again and it will run."));
                }
            }
        }
    }
    // The first exploration of a session, whatever the tool (a listing, a search, a read, `ls`, `cat`), is met by the map's overview once.
    if seen.lines().any(|line| line == "overview") || !crate::graph::ensure_built() {
        return None;
    }
    let overview = crate::beaver::codebase_map(&crate::graph::source_files(), Path::new("."));
    if overview.is_empty() {
        return None;
    }
    remember("overview");

    Some(format!("The map first. This is what the project holds, folder by folder, with the names each file defines:
{overview}
For where a name is defined and what calls it, run `\"{}\" graph query \"<name or question>\"`, then read only the files it points to. If you still need what you just asked for, repeat the call and it will run.", crate::channels::program()))
}

// Whether a shell command is only looking around (listing, reading, searching), as opposed to building or changing something.
fn is_exploring(command: &str) -> bool {
    if command.contains('>') {
        return false;
    }
    const LOOKING: [&str; 16] = ["ls", "dir", "find", "tree", "cat", "type", "head", "tail", "grep", "rg", "egrep", "get-childitem", "gci", "get-content", "select-string", "sls"];
    command.split(['&', ';', '|', '\n']).map(str::trim).filter(|part| !part.is_empty() && !part.starts_with("cd ")).any(|part| {
        let first = part.split_whitespace().next().unwrap_or("").replace('\\', "/").to_lowercase();
        let first = first.rsplit('/').next().unwrap_or("").to_string();
        LOOKING.contains(&first.as_str()) || (first == "git" && part.split_whitespace().nth(1).is_some_and(|sub| matches!(sub, "ls-files" | "grep")))
    })
}

// `dam graph first`: a PreToolUse payload (Grep, Glob, Write, Edit) on stdin; a refusal with the map's answer on stdout, or nothing.
pub fn hook() {
    crate::watchdog(25);
    if crate::beaver::host() != crate::beaver::Host::Claude {
        return;
    }
    if std::env::var("DAM_MAPFIRST").is_ok_and(|value| matches!(value.to_lowercase().as_str(), "off" | "0")) {
        return;
    }
    let mut bytes = Vec::new();
    let _ = std::io::stdin().read_to_end(&mut bytes);
    let text = String::from_utf8_lossy(&bytes);
    let Ok(input) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) else { return };
    let Some(reason) = decide(&input, &crate::economizer::state_root()) else { return };
    print!("{}", json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": reason}}));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_looking_around_counts_as_exploring() {
        assert!(is_exploring("ls -R app"));
        assert!(is_exploring("cd app && cat billing/*.py"));
        assert!(is_exploring("git ls-files | head"));
        assert!(is_exploring("Get-ChildItem -Recurse"));
        assert!(!is_exploring("python -m pytest -q"));
        assert!(!is_exploring("cat > docs/design.md <<EOF"));
        assert!(!is_exploring("git commit -m x"));
    }

    #[test]
    fn a_pattern_gives_its_code_names() {
        assert_eq!(names_in(r"post\(|charge_customer"), vec!["post", "charge_customer"]);
        assert_eq!(names_in("def split_line"), vec!["split_line"]);
        assert_eq!(names_in(r"TODO|a.*b"), vec!["TODO"]);
        assert!(names_in(r"\d+ -> [a-z]").is_empty());
    }
}
