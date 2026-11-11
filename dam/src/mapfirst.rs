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
    let explores = is_exploration(&tool, input);
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
                let _ = crate::paths::write_atomic(&file, format!("{seen}overview\n").as_bytes());
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
        let _ = crate::paths::write_atomic(&file, format!("{seen}{key}\n").as_bytes());
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
        let _ = crate::paths::write_atomic(&file, state.to_string().as_bytes());
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
    // A read is counted but never refused: an edit needs the file read first.
    let refuse = tool != "read" && looks >= MOST_DIRECT && sent < MOST_SENT_TO_WORKER;
    if refuse {
        state["sent"] = json!(sent + 1);
    }
    save(&state);

    refuse.then(|| format!("Not by yourself: that is {looks} looks and searches in this request. Broad exploration is a worker's job and is required here. Call Agent(subagent_type \"the-dam:worker\", prompt = one precise question and the paths to look at) now. It is read-only, runs on the smallest model, and returns facts and file:line, not file contents. Reads of the files you are about to edit stay yours. Then continue from what it reports."))
}

// The refusal for a tool call, or None to let it run: the map first, then a worker for broad exploration.
pub fn decide(input: &Value, state_dir: &Path) -> Option<String> {
    decide_for(input, state_dir, true)
}

// `workers` false: the map's answers only. Codex and Copilot name their sub-agents differently, so a worker is not required there.
pub fn decide_for(input: &Value, state_dir: &Path, workers: bool) -> Option<String> {
    // The state this reads and writes is shared by the hooks of parallel tool calls: one at a time per session.
    let session: String = input["session_id"].as_str().or_else(|| input["sessionId"].as_str()).unwrap_or("session").chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    // The map is built (or brought up to date) before the lock is taken: a rebuild can last longer than a lock is worth.
    if let Some(cwd) = input["cwd"].as_str().filter(|cwd| !cwd.is_empty()) {
        let tool = input["tool_name"].as_str().unwrap_or("").to_lowercase();
        if is_exploration(&tool, input) && std::env::set_current_dir(cwd).is_ok() {
            crate::graph::ensure_built();
        }
    }
    crate::paths::with_lock(&state_dir.join("goldfish").join(format!("lock-{session}")), || decide_locked(input, state_dir, workers))
}

fn decide_locked(input: &Value, state_dir: &Path, workers: bool) -> Option<String> {
    let tool = input["tool_name"].as_str()?.to_lowercase();
    // The worker is read-only: its shell may look, not change.
    let is_worker = input["agent_type"].as_str().is_some_and(|kind| kind.to_lowercase().contains("worker"));
    if is_worker && matches!(tool.as_str(), "bash" | "powershell") && !only_looks(input["tool_input"]["command"].as_str().unwrap_or("")) {
        return Some("A worker only looks: list, read, search, or query the map. It does not run builds or change files. Report what you found and let the main agent act.".to_string());
    }
    map_decision(input, state_dir).or_else(|| if workers { worker_decision(input, state_dir, &tool) } else { None })
}

// Whether every part of a shell command only looks (a listing, a read, a search, a map query, `git status`), nothing that builds or changes.
fn only_looks(command: &str) -> bool {
    // A substitution hides a second command, even inside double quotes.
    if command.contains("$(") || command.contains('`') {
        return false;
    }
    // What a quoted word holds (a search pattern) is data: only the command's own words are checked.
    let Some(flat) = flatten(command) else { return false };
    // Redirections, comments, grouping and process substitution are not looking.
    if flat.contains(['>', '<', '#', '(', ')']) {
        return false;
    }
    // Options that make a looking command write or run something.
    const WRITES: [&str; 11] = ["-delete", "-exec", "-execdir", "-ok", "-okdir", "-fprint", "-fprint0", "-fprintf", "-fls", "--output", "--pre"];
    flat.split(['&', ';', '|', '\n']).map(str::trim).filter(|part| !part.is_empty()).all(|part| {
        let words: Vec<&str> = part.split_whitespace().collect();
        let name = |word: &str| word.trim_matches(['"', '\'']).replace('\\', "/").to_lowercase().rsplit('/').next().unwrap_or("").to_string();
        let first = name(words[0]);
        // An option with quotes inside it (`-de''lete`) is not what it looks like.
        if words.iter().any(|word| word.starts_with('-') && word.contains('Q')) {
            return false;
        }
        if words.iter().any(|word| WRITES.iter().any(|w| *word == *w || word.starts_with(&format!("{w}="))) || (first == "tree" && *word == "-o") || (first == "git" && (word.starts_with("-O") || word.starts_with("--open-files-in-pager")))) {
            return false;
        }
        if first == "cd" || (matches!(first.as_str(), "dam" | "dam.exe" | "dam-run") && words.get(1) == Some(&"graph") && matches!(words.get(2), Some(&"query") | Some(&"facts"))) {
            return true;
        }
        if first == "git" {
            // Past git's harmless options (`--no-pager`, `-C dir`) to the subcommand; `-c` and `--exec-path` can run programs.
            let mut rest = words[1..].iter();
            while let Some(word) = rest.next() {
                if matches!(*word, "-c" | "--exec-path") || word.starts_with("--config-env") || word.starts_with("--exec-path=") {
                    return false;
                }
                if *word == "-C" {
                    rest.next();
                } else if !word.starts_with('-') {
                    return matches!(*word, "ls-files" | "grep" | "status" | "log" | "diff" | "show" | "blame");
                }
            }
            return false;
        }
        LOOK_COMMANDS.contains(&first.as_str())
    })
}

// The command with each quoted span replaced by `Q`, except one that starts a part (a quoted program path, its spaces
// made `_`). None when it cannot be read with certainty: a backslash outside quotes (it can hide a quote), or a quoted
// word that starts with `-` (a quoted option).
fn flatten(command: &str) -> Option<String> {
    let mut out = String::new();
    let mut chars = command.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            return None;
        }
        if c != '"' && c != '\'' {
            out.push(c);
            continue;
        }
        let mut inner = String::new();
        while let Some(d) = chars.next() {
            if d == c {
                break;
            }
            // Inside double quotes a backslash escapes the next character.
            if d == '\\' && c == '"' {
                inner.push(d);
                inner.extend(chars.next());
                continue;
            }
            inner.push(d);
        }
        let before = out.trim_end_matches([' ', '\t']);
        if before.chars().last().is_none_or(|last| matches!(last, '&' | ';' | '|' | '\n')) {
            out.push(c);
            out.push_str(&inner.replace(' ', "_"));
            out.push(c);
        } else {
            if inner.starts_with('-') && out.ends_with([' ', '\t']) {
                return None;
            }
            out.push('Q');
        }
    }

    Some(out)
}

const LOOK_COMMANDS: [&str; 17] = ["ls", "dir", "find", "tree", "cat", "type", "head", "tail", "wc", "grep", "rg", "egrep", "get-childitem", "gci", "get-content", "select-string", "sls"];

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
    let host = crate::beaver::host();
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
    let Ok(mut input) = serde_json::from_str::<Value>(text.trim_start_matches('\u{feff}')) else { return };
    let claude = host == crate::beaver::Host::Claude;
    if !claude {
        input = crate::hosts::normalize(host, Some("PreToolUse"), &input);
        // Copilot's editors name the file `path`.
        if let Some(path) = input["tool_input"]["path"].as_str().map(String::from).filter(|_| input["tool_input"]["file_path"].is_null()) {
            input["tool_input"]["file_path"] = json!(path);
        }
    }
    let Some(reason) = decide_for(&input, &crate::economizer::state_root(), claude) else { return };
    let what = if reason.starts_with("The map first") { "map overview given before the first look" } else if reason.starts_with("Answered from the map") { "search answered from the map" } else { "code held back until the design is reviewed" };
    crate::journal::event(host, input["session_id"].as_str().unwrap_or(""), "🗺️", what);
    print!("{}", deny_output(host, &reason));
}

// A refusal in the host's own shape: Copilot's preToolUse takes it at the top level.
fn deny_output(host: crate::beaver::Host, reason: &str) -> Value {
    match host {
        crate::beaver::Host::Copilot => json!({"permissionDecision": "deny", "permissionDecisionReason": reason}),
        _ => json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": reason}}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_comes_in_the_hosts_own_shape() {
        use crate::beaver::Host;
        assert_eq!(deny_output(Host::Copilot, "why")["permissionDecision"], "deny");
        assert_eq!(deny_output(Host::Codex, "why")["hookSpecificOutput"]["permissionDecisionReason"], "why");
        let copilot = crate::hosts::normalize(Host::Copilot, Some("PreToolUse"), &json!({"sessionId": "s", "cwd": "/r", "toolName": "grep", "toolArgs": "{\"pattern\":\"charge_customer\"}"}));
        assert_eq!((copilot["tool_name"].as_str(), copilot["tool_input"]["pattern"].as_str(), copilot["session_id"].as_str()), (Some("Grep"), Some("charge_customer"), Some("s")));
    }

    #[test]
    fn only_looking_around_counts_as_exploring() {
        // (a read of a design is not a write of it: the goldfish hook ignores it on Copilot)
        let read = crate::hosts::normalize(crate::beaver::Host::Copilot, Some("PostToolUse"), &json!({"toolName": "view", "toolArgs": "{\"path\":\"a-design.md\"}"}));
        assert!(!crate::autoreview::writes(&read));
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
        let read = |prompt: &str| json!({"tool_name": "Read", "session_id": "s", "prompt_id": prompt, "tool_input": {"file_path": "src/a.rs"}});
        let grep = |prompt: &str| json!({"tool_name": "Grep", "session_id": "s", "prompt_id": prompt, "tool_input": {"pattern": "x"}});
        let reads = |n: usize, prompt: &str| (0..n).map(|_| worker_decision(&read(prompt), &dir, "read")).collect::<Vec<_>>();
        assert!(reads(6, "p1").iter().all(Option::is_none), "six looks are free");
        // A read is counted and never refused (an edit needs it); a search past the sixth look is.
        assert!(reads(3, "p1").iter().all(Option::is_none));
        assert!(worker_decision(&grep("p1"), &dir, "grep").unwrap().contains("the-dam:worker"));
        assert!((0..4).all(|_| worker_decision(&grep("p1"), &dir, "grep").is_some()), "asked five times in all");
        assert!(worker_decision(&grep("p1"), &dir, "grep").is_none(), "then it goes through");
        // A new request starts over; a sub-agent is not counted; handing over (a worker or Explore) starts the count again.
        assert!(reads(6, "p2").iter().all(Option::is_none));
        let inside = json!({"tool_name": "Grep", "session_id": "s", "prompt_id": "p2", "agent_id": "a1", "tool_input": {"pattern": "x"}});
        assert!(worker_decision(&inside, &dir, "grep").is_none());
        assert!(worker_decision(&grep("p2"), &dir, "grep").is_some());
        for kind in ["the-dam:worker", "Explore"] {
            let hand_over = json!({"tool_name": "Agent", "session_id": "s", "prompt_id": "p2", "tool_input": {"subagent_type": kind}});
            assert!(worker_decision(&hand_over, &dir, "agent").is_none());
            assert!(reads(6, "p2").iter().all(Option::is_none), "six more looks after {kind}");
            assert!(worker_decision(&grep("p2"), &dir, "grep").is_some());
        }
        // Documents are not code looks.
        let doc = json!({"tool_name": "Read", "session_id": "t", "prompt_id": "q", "tool_input": {"file_path": "notes.md"}});
        assert!((0..9).all(|_| worker_decision(&doc, &dir, "read").is_none()));
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_worker_may_only_look() {
        let dir = std::env::temp_dir().join(format!("worker-looks-{}", std::process::id()));
        let run = |agent: &str, command: &str| json!({"tool_name": "Bash", "session_id": "s", "agent_type": agent, "tool_input": {"command": command}});
        for allowed in ["ls -R app", "cd app && cat a.py | head", "git status", "rg -n post app", "cat a; ls", "\"/p/scripts/dam-run\" graph query \"post\"", "rg -n \"fn (a|b)|#\\[test\\]\" src", "rg -e \"fn x\" src", "\"C:/Users/A B/p/scripts/dam-run\" graph query x", "git --no-pager log --pretty=oneline", "git -C d status", "dam graph query \"C# parser\""] {
            assert!(only_looks(allowed), "{allowed}");
        }
        for refused in ["sed -i s/a/b/ a.py", "cat a; rm b", "git commit -m x", "python -m pytest", "echo hi > a.txt", "find . -delete", "ls $(rm -rf app)", "rm -rf app # graph query", "ls `rm a`", "find . -fprint x", "git diff --output=x", "rg --pre sh x", "rm graph query -rf src", "git grep -Osh x", "ls \"$(rm a)\"", "rg \\' ; rm -rf app ; echo \\'", "find . \"-delete\"", "find . -de''lete", "rg \"--pre\" sh x", "git grep \"-Osh\" x", "git -c core.fsmonitor=\"rm -rf app\" status", "cat <(rm -rf app)", "Get-Content (Remove-Item app)"] {
            assert!(!only_looks(refused), "{refused}");
        }
        assert!(decide(&run("the-dam:worker", "rm -rf app"), &dir).unwrap().contains("only looks"));
        assert!(decide(&run("the-dam:worker", "ls app"), &dir).is_none());
        assert!(decide(&run("", "rm -rf app"), &dir).is_none(), "the main agent is not a worker");
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
