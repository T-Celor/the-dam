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
// Looks and searches the main agent makes itself in one prompt before broad exploration goes to a worker.
const MOST_DIRECT: i64 = 6;
// Times a prompt's exploration is sent to a worker instead; after that the call goes through, so a host that cannot start one is never stuck.
const MOST_SENT_TO_WORKER: i64 = 5;

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
fn map_decision(input: &Value, state_dir: &Path) -> Option<String> {
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

// Whether this tool call is the agent looking around: a listing, a search, a read of code, or `ls` / `cat` / `rg` in a shell.
fn is_exploration(tool: &str, input: &Value) -> bool {
    match tool {
        "grep" | "glob" => true,
        "read" => input["tool_input"]["file_path"].as_str().is_some_and(|path| crate::scan::lang_of(path).is_some()),
        "bash" | "powershell" => is_exploring(input["tool_input"]["command"].as_str().unwrap_or("")),
        _ => false,
    }
}

// Broad exploration is a worker's job: past MOST_DIRECT looks in one prompt the main agent is sent to a worker (the smallest
// model, read-only, returns facts and paths). Handing work to a worker starts the count again. Sub-agents are not counted.
fn worker_decision(input: &Value, state_dir: &Path, tool: &str) -> Option<String> {
    if input["agent_id"].as_str().is_some_and(|id| !id.is_empty()) {
        return None;
    }
    let session: String = input["session_id"].as_str().or_else(|| input["sessionId"].as_str()).unwrap_or("session").chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    let prompt = input["prompt_id"].as_str().unwrap_or("");
    let file = state_dir.join("goldfish").join(format!("direct-{session}.json"));
    let mut state: Value = fs::read_to_string(&file).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_else(|| json!({}));
    if state["prompt"].as_str() != Some(prompt) {
        state = json!({"prompt": prompt, "n": 0, "sent": 0});
    }
    let save = |state: &Value| {
        if let Some(folder) = file.parent() {
            let _ = fs::create_dir_all(folder);
        }
        let _ = fs::write(&file, state.to_string());
    };
    // Handing work to a worker (or an Explore agent) settles it for this prompt.
    if matches!(tool, "agent" | "task") {
        let kind = input["tool_input"]["subagent_type"].as_str().unwrap_or("").to_lowercase();
        if kind.contains("worker") || kind.contains("explore") {
            state["n"] = json!(0);
            state["sent"] = json!(0);
            save(&state);
        }
        return None;
    }
    if !is_exploration(tool, input) {
        return None;
    }
    let looks = state["n"].as_i64().unwrap_or(0);
    state["n"] = json!(looks + 1);
    let sent = state["sent"].as_i64().unwrap_or(0);
    let refuse = looks >= MOST_DIRECT && sent < MOST_SENT_TO_WORKER;
    if refuse {
        state["sent"] = json!(sent + 1);
    }
    save(&state);

    refuse.then(|| format!("Not by yourself: that is {looks} looks and searches in this request. Broad exploration is a worker's job and is required here. Call Agent(subagent_type \"the-dam:worker\", prompt = one precise question and the paths to look at) now. It is read-only, runs on the smallest model, and returns facts and file:line, not file contents. Keep Read for the files you are about to edit. Then continue from what it reports."))
}

// The refusal for a tool call, or None to let it run: the map first, then a worker for broad exploration.
pub fn decide(input: &Value, state_dir: &Path) -> Option<String> {
    let tool = input["tool_name"].as_str()?.to_lowercase();
    map_decision(input, state_dir).or_else(|| worker_decision(input, state_dir, &tool))
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
    // A debugging aid: DAM_HOOK_DUMP=<file> keeps each payload this hook receives.
    if let Some(dump) = std::env::var_os("DAM_HOOK_DUMP") {
        use std::io::Write;
        if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(dump) {
            let _ = writeln!(file, "{}", text.trim());
        }
    }
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
    fn broad_exploration_is_sent_to_a_worker_after_six_looks() {
        let dir = std::env::temp_dir().join(format!("worker-first-{}", std::process::id()));
        let look = |prompt: &str| json!({"tool_name": "Read", "session_id": "s", "prompt_id": prompt, "tool_input": {"file_path": "src/a.rs"}});
        let looks = |n: usize, prompt: &str| (0..n).map(|_| worker_decision(&look(prompt), &dir, "read")).collect::<Vec<_>>();
        assert!(looks(6, "p1").iter().all(Option::is_none), "six looks are free");
        assert!(worker_decision(&look("p1"), &dir, "read").unwrap().contains("the-dam:worker"));
        assert!((0..4).all(|_| worker_decision(&look("p1"), &dir, "read").is_some()), "asked five times in all");
        assert!(worker_decision(&look("p1"), &dir, "read").is_none(), "then it goes through");
        // A new request starts over; a sub-agent is not counted; a worker starts the count again.
        assert!(looks(6, "p2").iter().all(Option::is_none));
        let inside = json!({"tool_name": "Read", "session_id": "s", "prompt_id": "p2", "agent_id": "a1", "tool_input": {"file_path": "src/a.rs"}});
        assert!(worker_decision(&inside, &dir, "read").is_none());
        let hand_over = json!({"tool_name": "Agent", "session_id": "s", "prompt_id": "p2", "tool_input": {"subagent_type": "the-dam:worker"}});
        assert!(worker_decision(&hand_over, &dir, "agent").is_none());
        // Handing over starts the count again: six more looks are free, then it is the worker's turn again.
        assert!(looks(6, "p2").iter().all(Option::is_none));
        assert!(worker_decision(&look("p2"), &dir, "read").is_some());
        // Documents are not code looks.
        let doc = json!({"tool_name": "Read", "session_id": "t", "prompt_id": "q", "tool_input": {"file_path": "notes.md"}});
        assert!((0..9).all(|_| worker_decision(&doc, &dir, "read").is_none()));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_pattern_gives_its_code_names() {
        assert_eq!(names_in(r"post\(|charge_customer"), vec!["post", "charge_customer"]);
        assert_eq!(names_in("def split_line"), vec!["split_line"]);
        assert_eq!(names_in(r"TODO|a.*b"), vec!["TODO"]);
        assert!(names_in(r"\d+ -> [a-z]").is_empty());
    }
}
