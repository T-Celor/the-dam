// The output compression and map answers of hooks/register.tsx, for hosts without the mod: Codex CLI and Copilot CLI.
// The decisions are ports of hooks/tools.ts; `hook` is the entry of `dam squeeze hook`. Never runs under Claude Code.
use crate::beaver::{host, Host};
use crate::{graph, squeeze, web};
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::Path;

const WORTH_SQUEEZING_CHARS: usize = 2000;
// Beyond this the work would not fit the hook's time; the host keeps its own handling.
const MOST_SQUEEZED_CHARS: usize = 1_500_000;
const WORTH_KEEPING_TOKENS: usize = 100;
const MOST_NAMES: usize = 3;
const GRAPH: &str = ".dam-graph/graph.tsv";
const KEPT_ALL_OUTPUTS: usize = 200;

// Rough, as no tokenizer is at hand: about four characters per token.
fn tokens_in(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

// The condensed text with its token figures, when it saves enough to be worth the model's reading the note.
pub fn squeezed(original: &str, text: String) -> Option<(String, usize, usize)> {
    let (before, after) = (tokens_in(original), tokens_in(&text));

    (before >= after + WORTH_KEEPING_TOKENS).then_some((text, before, after))
}

pub fn squeeze_note(before: usize, after: usize, path: &str) -> String {
    format!("\n\n[The dam condensed this output from about {before} to {after} tokens without dropping any line. Original: {path}.]")
}

pub fn web_note(before: usize, after: usize, path: &str) -> String {
    format!("\n\n[The dam turned this web page into Markdown, from about {before} to {after} tokens: its main content, without menus, scripts or styles. The full HTML: {path}.]")
}

pub fn worth_squeezing(text: &str) -> bool {
    let length = text.chars().count();

    (WORTH_SQUEEZING_CHARS..MOST_SQUEEZED_CHARS).contains(&length)
}

// Whether an output is a web page (with the headers `curl -i` prints, or none).
pub fn looks_like_web_page(text: &str) -> bool {
    let head: String = text.chars().take(8192).collect();
    let head = head.strip_prefix('\u{feff}').unwrap_or(&head);
    let body = if head.starts_with("HTTP/") {
        let blank = ["\r\n\r\n", "\n\n", "\r\n\n", "\n\r\n"].iter().filter_map(|gap| head.find(gap)).min();
        // The original slices one character past the blank line's start; leading space is skipped anyway.
        blank.map_or(head, |at| &head[at + 1..])
    } else {
        head
    };
    let body = body.trim_start().to_lowercase();
    let tag = |name: &str| body.strip_prefix(name).is_some_and(|rest| rest.starts_with(|c: char| c.is_whitespace() || c == '>'));
    if body.starts_with("<!doctype html") || tag("<html") || tag("<head") {
        return true;
    }
    // An XML prolog before the page.
    body.strip_prefix("<?xml").and_then(|rest| rest.split_once('>')).is_some_and(|(_, rest)| {
        let rest = rest.trim_start();
        rest.starts_with("<!doctype html") || rest.starts_with("<html")
    })
}

// The first web address a command names, for the links of the page it fetched.
pub fn page_url(command: &str) -> Option<String> {
    let (at, scheme) = ["https://", "http://"].iter().filter_map(|scheme| Some((command.find(scheme)?, scheme.len()))).min()?;
    let url: String = command[at..].chars().take_while(|c| !c.is_whitespace() && !"'\"`<>|;)".contains(*c)).collect();

    (url.len() > scheme).then_some(url)
}

// The names in a prompt that look like code rather than words: snake_case, camelCase, PascalCase with two capitals, or a call.
pub fn code_names(text: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() && names.len() < MOST_NAMES {
        if !(bytes[i].is_ascii_alphabetic() || bytes[i] == b'_') {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
        let name = &text[start..i];
        let is_call = text[i..].starts_with("()");
        if is_call {
            i += 2;
        }
        let b = name.as_bytes();
        let is_code = is_call
            || b.windows(3).any(|w| w[1] == b'_' && w[0].is_ascii_alphanumeric() && w[2].is_ascii_alphanumeric())
            || b.windows(2).any(|w| w[0].is_ascii_lowercase() && w[1].is_ascii_uppercase())
            || is_pascal(name);
        if is_code && name.len() >= 4 && !names.iter().any(|n| n == name) {
            names.push(name.to_string());
        }
    }

    names
}

// /^[A-Z][a-z0-9]+[A-Z]/
fn is_pascal(name: &str) -> bool {
    let b = name.as_bytes();
    if b.first().is_none_or(|c| !c.is_ascii_uppercase()) {
        return false;
    }
    let run = b[1..].iter().take_while(|c| c.is_ascii_lowercase() || c.is_ascii_digit()).count();

    run >= 1 && b.get(1 + run).is_some_and(|c| c.is_ascii_uppercase())
}

// The name a search looks for, when its pattern is one identifier (`\bname\b`, `name(`, `def name`), else None.
pub fn grep_name(pattern: &str) -> Option<String> {
    let cleaned = pattern.replace("\\b", "").replace("\\(", "").replace('(', "").replace(['^', '$'], "");
    let mut name = cleaned.as_str();
    for word in ["def", "fn", "function", "class", "struct", "interface", "type", "const", "let", "var"] {
        if let Some(rest) = name.strip_prefix(word).filter(|rest| rest.starts_with(char::is_whitespace)) {
            name = rest;
            break;
        }
    }
    let name = name.trim();
    let b = name.as_bytes();
    let is_name = b.len() >= 4 && (b[0].is_ascii_alphabetic() || b[0] == b'_') && b.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_');

    is_name.then(|| name.to_string())
}

// The entries of a graph answer about exactly `name`: where it is defined, what it calls and what calls it.
pub fn exact_entries(answer: &str, name: &str) -> Vec<String> {
    let normalized = answer.replace("\r\n", "\n");
    let mut found = Vec::new();
    // Entries are separated by blank (or whitespace-only) lines.
    let mut entry: Vec<&str> = Vec::new();
    let mut flush = |entry: &mut Vec<&str>| {
        let text = entry.join("\n");
        let head = text.trim_start();
        if head.starts_with(&format!("{name}(")) || head.starts_with(&format!("{name} ")) {
            found.push(text);
        }
        entry.clear();
    };
    for line in normalized.split('\n') {
        if line.trim().is_empty() {
            flush(&mut entry);
        } else {
            entry.push(line);
        }
    }
    flush(&mut entry);

    found
}

// What the model reads about the names a prompt or a search named, in place of finding them itself.
pub fn graph_facts(entries: &[String], program: &str) -> String {
    format!("The project's map on these names (`\"{program}\" graph query \"<question>\"` asks it more):\n\n{}", entries.join("\n\n"))
}

// The graph's exact entries for `names`, as the text to add; None without a graph or when it knows none of them.
pub fn ask_graph(cwd: &Path, names: &[String]) -> Option<String> {
    let path = cwd.join(GRAPH);
    // A map the repository commits is its authors' text, not the dam's: it must not reach the model as the map's word.
    if !is_dams_own(cwd) {
        return None;
    }
    let path = path.to_str()?;
    let mut entries: Vec<String> = names.iter().filter_map(|name| Some(exact_entries(&graph::answer_from(path, name)?, name))).flatten().collect();
    let mut used = 0;
    entries.retain(|entry| {
        used += entry.len();
        used <= MOST_FACTS_CHARS
    });

    (!entries.is_empty()).then(|| graph_facts(&entries, &crate::channels::program()))
}

// Enough for a few definitions and their callers, little enough to stay a footnote.
const MOST_FACTS_CHARS: usize = 4000;

// The map is the dam's when git does not track it (the dam keeps it out of git's sight); unknown counts as not.
fn is_dams_own(cwd: &Path) -> bool {
    let run = std::process::Command::new("git").args(["-c", "core.fsmonitor=false", "ls-files", "--", GRAPH]).current_dir(cwd).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output();

    run.is_ok_and(|run| run.status.success() && run.stdout.is_empty())
}

// The pattern of a search a shell command runs (`rg`, `grep`, `git grep`).
fn searched_pattern(command: &str) -> Option<String> {
    let words: Vec<&str> = command.split_whitespace().collect();
    let base = |word: &str| word.rsplit(['/', '\\']).next().unwrap_or(word).trim_end_matches(".exe").to_string();
    let first = words.iter().position(|w| matches!(base(w).as_str(), "rg" | "grep" | "egrep" | "fgrep"))?;
    let mut rest = words[first + 1..].iter();
    while let Some(word) = rest.next() {
        if matches!(*word, "-e" | "--regexp") {
            return rest.next().map(|w| w.trim_matches(['\'', '"']).to_string());
        }
        if !word.starts_with('-') {
            return Some(word.trim_matches(['\'', '"']).to_string());
        }
    }

    None
}

// ---- the host's payload ----

enum Kind {
    Shell,
    Search,
    Mcp,
}

struct Call {
    kind: Kind,
    command: Option<String>,
    pattern: Option<String>,
    text: String,
    cwd: String,
    session: String,
}

fn text_blocks(value: &Value) -> Option<String> {
    let blocks = value.as_array().filter(|blocks| !blocks.is_empty())?;
    let texts: Option<Vec<&str>> = blocks.iter().map(|b| if b["type"] == "text" { b["text"].as_str() } else { None }).collect();

    Some(texts?.join("\n"))
}

// A Codex tool response's text, or None for a failed call or a shape this does not know.
fn codex_text(response: &Value) -> Option<String> {
    let parsed;
    let object = match response {
        Value::String(text) if text.trim_start().starts_with('{') => {
            parsed = serde_json::from_str::<Value>(text).ok().filter(Value::is_object);
            match &parsed {
                Some(object) => object,
                None => return Some(text.clone()),
            }
        }
        Value::String(text) => return Some(text.clone()),
        other => other,
    };
    let failed = object["is_error"] == true || object["isError"] == true || object["exit_code"].as_i64().is_some_and(|code| code != 0) || object["metadata"]["exit_code"].as_i64().is_some_and(|code| code != 0);
    if failed {
        return None;
    }
    if let Some(blocks) = text_blocks(&object["content"]) {
        return Some(blocks);
    }
    ["stdout", "output", "aggregated_output", "text"].iter().find_map(|key| object[*key].as_str()).map(String::from)
}

fn codex_call(input: &Value) -> Option<Call> {
    let tool = input["tool_name"].as_str()?;
    let kind = match tool {
        "Bash" => Kind::Shell,
        name if name.starts_with("mcp__") => Kind::Mcp,
        _ => return None,
    };
    let command = input["tool_input"]["command"].as_str().map(String::from);
    let pattern = if matches!(kind, Kind::Shell) { command.as_deref().and_then(searched_pattern) } else { None };

    Some(Call { kind, command, pattern, text: codex_text(&input["tool_response"])?, cwd: input["cwd"].as_str().unwrap_or(".").into(), session: input["session_id"].as_str().unwrap_or("").into() })
}

// Copilot's tool arguments come as a JSON string or as an object.
fn copilot_args(input: &Value) -> Value {
    match &input["toolArgs"] {
        Value::String(text) => serde_json::from_str(text).unwrap_or(Value::Null),
        other => other.clone(),
    }
}

fn copilot_call(input: &Value) -> Option<Call> {
    let tool = input["toolName"].as_str()?.to_lowercase();
    // Only a successful result may be replaced.
    if input["toolResult"]["resultType"] != "success" {
        return None;
    }
    let args = copilot_args(input);
    let kind = match tool.as_str() {
        "bash" | "powershell" => Kind::Shell,
        "grep" | "rg" => Kind::Search,
        // MCP tools are named server-tool or server__tool; the built-in ones (view, glob, web_fetch, edit...) never are.
        name if name.contains("__") || name.contains('-') || name.contains('/') => Kind::Mcp,
        _ => return None,
    };
    let command = args["command"].as_str().map(String::from);
    let pattern = match kind {
        Kind::Search => args["pattern"].as_str().map(String::from),
        Kind::Shell => command.as_deref().and_then(searched_pattern),
        Kind::Mcp => None,
    };

    Some(Call { kind, command, pattern, text: input["toolResult"]["textResultForLlm"].as_str()?.to_string(), cwd: input["cwd"].as_str().unwrap_or(".").into(), session: input["sessionId"].as_str().unwrap_or("").into() })
}

// ---- compression ----

// Where originals are kept, the mod's folder; the system's temporary folder where no home is set.
fn outputs_dir() -> Option<std::path::PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).filter(|home| !home.is_empty());
    let dir = home.map_or_else(|| std::env::temp_dir().join("the-dam-outputs"), |home| Path::new(&home).join(".claude").join("the-dam-outputs"));
    std::fs::create_dir_all(&dir).ok()?;
    // Raw outputs can hold secrets: only the person reads them.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700));
    }

    Some(dir)
}

// What an output becomes, without its footer: a fetched page as Markdown, anything else condensed losslessly.
pub struct Condensed {
    pub text: String,
    pub before: usize,
    pub after: usize,
    pub url: Option<String>,
    pub is_web: bool,
    // Lines were left out (long shell output only): the footer says so.
    pub elided: bool,
}

// Whether any step of a shell command prints content for reading (a file, a diff, a search) rather than a run's log.
pub fn reads_content(command: &str) -> bool {
    const READERS: [&str; 20] = ["cat", "sed", "head", "tail", "bat", "type", "nl", "less", "more", "awk", "gc", "get-content", "select-string", "sls", "rg", "grep", "egrep", "fgrep", "ag", "findstr"];
    command.split(['\n', ';', '|', '&']).any(|step| {
        let mut words = step.split_whitespace().skip_while(|word| word.contains('=') && !word.starts_with('-') || matches!(*word, "cd" | "sudo" | "time"));
        let Some(first) = words.next() else { return false };
        let base = first.trim_matches(['"', '\'']).rsplit(['/', '\\']).next().unwrap_or(first).trim_end_matches(".exe").to_lowercase();
        // `git diff`, `git show`, `git log -p`, `git blame`, `git grep` print content too.
        READERS.contains(&base.as_str()) || (base == "git" && words.any(|word| matches!(word, "diff" | "show" | "log" | "blame" | "grep" | "cat-file")))
    })
}

// The one decision for every host, the mod included: None where it would not pay.
pub fn condensed(original: &str, command: Option<&str>) -> Option<Condensed> {
    if !worth_squeezing(original) {
        return None;
    }
    let url = command.and_then(page_url);
    let page = if looks_like_web_page(original) { web::convert(original, url.as_deref()).map(|page| web::render(&page, url.as_deref())) } else { None };
    let is_web = page.is_some();
    let lossless = squeezed(original, page.unwrap_or_else(|| squeeze::squeeze(original)));
    // Only what a shell printed, and only when it is long: the model can open the original for the middle.
    // Never what a command printed to be read: file contents, a diff, search hits, where each line may be the one needed.
    let shortened = if command.is_some_and(|command| !reads_content(command)) && !is_web && squeeze::elide_enabled() && original.chars().count() >= squeeze::ELIDE_FROM_CHARS { squeeze::elide(original).and_then(|(short, _)| squeezed(original, short)) } else { None };
    let (text, before, after, elided) = match (lossless, shortened) {
        (Some(kept), Some(short)) if short.2 < kept.2 => (short.0, short.1, short.2, true),
        (Some(kept), _) => (kept.0, kept.1, kept.2, false),
        (None, Some(short)) => (short.0, short.1, short.2, true),
        (None, None) => return None,
    };

    Some(Condensed { text, before, after, url, is_web, elided })
}

pub fn elide_note(before: usize, after: usize, path: &str) -> String {
    format!("\n\n[The dam shortened this output from about {before} to {after} tokens: the first and last lines and every line mentioning an error or warning are kept, the lines between are left out (the gaps say how many). The whole output: {path}.]")
}

fn note_of(done: &Condensed, path: &str) -> String {
    if done.is_web {
        web_note(done.before, done.after, path)
    } else if done.elided {
        elide_note(done.before, done.after, path)
    } else {
        squeeze_note(done.before, done.after, path)
    }
}

// `dam squeeze condense [COMMAND]`, the mod's call: the output on stdin, its condensed form as JSON, or nothing.
pub fn condense_command(command: Option<&str>) -> bool {
    let mut original = String::new();
    if std::io::stdin().read_to_string(&mut original).is_err() {
        return false;
    }
    let Some(done) = condensed(&original, command) else { return false };
    // The footer's words stay here: the mod only puts in the path where it keeps the original.
    let note = note_of(&done, "{path}");
    println!("{}", json!({"text": done.text, "note": note, "before": done.before, "after": done.after, "url": done.url, "web": done.is_web}));

    true
}

// `dam graph facts --prompt TEXT | --grep PATTERN`, the mod's call: what the map says of the code it names, or nothing.
pub fn facts_command(args: &[String]) -> bool {
    let facts = match (args.first().map(String::as_str), args.get(1)) {
        (Some("--prompt"), Some(text)) => prompt_facts(text, "."),
        (Some("--grep"), Some(pattern)) => grep_name(pattern).and_then(|name| ask_graph(Path::new("."), &[name])),
        _ => None,
    };
    facts.map(|facts| print!("{facts}")).is_some()
}

// The condensed output with its footer (the original is saved under `dir`), or None where it would not pay.
pub fn condense(original: &str, command: Option<&str>, dir: &Path) -> Option<(String, usize, usize)> {
    let done = condensed(original, command)?;
    std::fs::create_dir_all(dir).ok()?;
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    let path = dir.join(format!("hook-{}-{stamp}.txt", std::process::id()));
    std::fs::write(&path, original).ok()?;
    let path = path.to_string_lossy().replace('\\', "/");
    let footer = note_of(&done, &path);

    Some((format!("{}{footer}", done.text), done.before, done.after))
}

// ---- the hook ----

// The JSON to print for one Codex or Copilot payload, or None for "nothing to say" (`{}`).
pub fn respond(host: Host, input: &str, dir: &Path) -> Option<String> {
    let input: Value = serde_json::from_str(input).ok()?;
    match host {
        Host::Claude => None,
        Host::Codex => match input["hook_event_name"].as_str()? {
            "PostToolUse" => post_tool(host, &codex_call(&input)?, dir),
            "UserPromptSubmit" => prompt_facts(input["prompt"].as_str()?, input["cwd"].as_str().unwrap_or(".")).map(|facts| json!({"hookSpecificOutput": {"hookEventName": "UserPromptSubmit", "additionalContext": facts}}).to_string()),
            _ => None,
        },
        // Copilot ignores what userPromptSubmitted prints; userPromptTransformed may rewrite what the model receives.
        Host::Copilot if input.get("transformedPrompt").is_some() => {
            let transformed = input["transformedPrompt"].as_str().filter(|text| !text.is_empty())?;
            let facts = prompt_facts(input["prompt"].as_str().unwrap_or(transformed), input["cwd"].as_str().unwrap_or("."))?;
            Some(json!({"modifiedTransformedPrompt": format!("{transformed}

{facts}")}).to_string())
        }
        Host::Copilot => post_tool(host, &copilot_call(&input)?, dir),
    }
}

fn prompt_facts(prompt: &str, cwd: &str) -> Option<String> {
    // A command is not a question about code.
    if prompt.trim_start().starts_with('/') {
        return None;
    }

    ask_graph(Path::new(cwd), &code_names(prompt))
}

fn post_tool(host: Host, call: &Call, dir: &Path) -> Option<String> {
    let command = if matches!(call.kind, Kind::Shell) { call.command.as_deref() } else { None };
    let mut saving = String::new();
    let condensed = condense(&call.text, command, dir).map(|(text, before, after)| {
        // What the status line and the journal count, as the mod counts it under Claude Code.
        crate::journal::add_saving(host, &call.session, before, after);
        saving = format!("💧 The dam saved {} tokens on this output ({before} → {after}, -{}%)", before - after, (before - after) * 100 / before.max(1));
        text
    });
    // A search for one name also gets where the map says it is defined and who calls it.
    let facts = match (&call.kind, &call.pattern) {
        (Kind::Mcp, _) | (_, None) => None,
        (_, Some(pattern)) => grep_name(pattern).and_then(|name| ask_graph(Path::new(&call.cwd), &[name])),
    };
    if condensed.is_none() && facts.is_none() {
        return None;
    }
    Some(match host {
        // Codex cannot rewrite the output: its block feedback replaces the tool result, so the condensed text is the reason.
        Host::Codex => match (condensed, facts) {
            // `systemMessage` is shown to the person, not the model.
            (Some(text), facts) => json!({"decision": "block", "reason": facts.map_or(text.clone(), |facts| format!("{text}\n\n{facts}")), "systemMessage": saving}).to_string(),
            (None, facts) => json!({"hookSpecificOutput": {"hookEventName": "PostToolUse", "additionalContext": facts}}).to_string(),
        },
        _ => {
            let mut out = serde_json::Map::new();
            if let Some(text) = condensed {
                out.insert("modifiedResult".into(), json!({"resultType": "success", "textResultForLlm": text}));
            }
            if let Some(facts) = facts {
                out.insert("additionalContext".into(), json!(facts));
            }
            Value::Object(out).to_string()
        }
    })
}

// `dam squeeze hook`: one payload on stdin, one answer on stdout; `{}` on anything unexpected, and always exit 0.
pub fn hook() {
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    // Release builds abort on panic, so catch_unwind cannot help: the panic hook answers `{}` and exits 0 instead.
    std::panic::set_hook(Box::new(|_| {
        // Not println!: on a closed stdout it would panic again, and abort with a failure.
        let _ = writeln!(std::io::stdout(), "{{}}");
        std::process::exit(0);
    }));
    // The map the answers read is brought up to date first, in the folder the payload names.
    if let Some(cwd) = serde_json::from_str::<Value>(&input).ok().and_then(|p| p["cwd"].as_str().map(String::from)) {
        if std::env::set_current_dir(&cwd).is_ok() {
            graph::refresh();
        }
    }
    let answer = outputs_dir().and_then(|dir| {
        let answer = respond(host(), &input, &dir);
        if answer.is_some() {
            squeeze::prune(&dir, KEPT_ALL_OUTPUTS);
        }
        answer
    });
    println!("{}", answer.unwrap_or_else(|| "{}".into()));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_log(n: usize) -> String {
        (0..n).map(|i| format!("compiling crate{i} v{}.{}.{} ({:x})\n", i % 7, i % 11, i % 13, i * 2654435761usize % 0xfffff)).collect()
    }

    #[test]
    fn what_a_command_prints_to_be_read_is_never_shortened() {
        let log = build_log(900);
        for command in ["cat src/lib.rs", "sed -n 1,200p src/lib.rs", "cd dam && head -500 x.rs", "git diff HEAD~3", "git -C repo show abc", "rg -n foo", "cargo build 2>&1 | tail -300", "Get-Content a.txt", "FOO=1 grep -r bar ."] {
            assert!(reads_content(command), "{command}");
            assert!(!condensed(&log, Some(command)).is_some_and(|done| done.elided), "{command}");
        }
        for command in ["cargo build", "cargo test --release", "npm install", "ls -la", "git status", "git push origin main"] {
            assert!(!reads_content(command), "{command}");
        }
        assert!(condensed(&log, Some("cargo build")).is_some_and(|done| done.elided));
    }

    #[test]
    fn only_a_long_shell_output_is_shortened_and_the_footer_says_so() {
        let mut log = build_log(900);
        log = log.replacen("crate450 ", "FAILED crate450 ", 1);
        let shell = condensed(&log, Some("cargo build")).unwrap();
        assert!(shell.elided && shell.text.contains("FAILED crate450"));
        assert!(elide_note(1, 1, "/p").contains("lines between are left out"));
        // A file read or a search keeps every line.
        assert!(!condensed(&log, None).is_some_and(|done| done.elided));
    }

    fn temp(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("dam-answers-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn log() -> String {
        (0..2000).map(|i| format!("INFO worker handled request {i} status 200 in {}ms\n", 10 + i % 9)).collect()
    }

    // A map the dam made: in a git repository, out of git's sight.
    fn with_graph(dir: &Path) {
        std::fs::create_dir_all(dir.join(".dam-graph")).unwrap();
        let tsv = "P\tsrc/deny.py\nN\tF\tdeny.py\t0\t1\nN\tf\tapply_deny\t0\t7\nN\tf\trun\t0\t3\nE\t2\t1\tc\nE\t0\t1\td\n";
        std::fs::write(dir.join(GRAPH), tsv).unwrap();
        std::process::Command::new("git").args(["init", "-q"]).current_dir(dir).status().unwrap();
    }

    #[test]
    fn a_map_the_repository_commits_is_never_given_as_the_map() {
        let dir = crate::session::TempDir::new("dam-answers-").unwrap();
        with_graph(&dir.0);
        assert!(ask_graph(&dir.0, &["apply_deny".to_string()]).is_some());
        let git = |args: &[&str]| std::process::Command::new("git").args(["-c", "user.email=t@t", "-c", "user.name=t"]).args(args).current_dir(&dir.0).output().unwrap();
        git(&["add", "-f", GRAPH]);
        assert!(ask_graph(&dir.0, &["apply_deny".to_string()]).is_none());
    }

    #[test]
    fn code_names_ported() {
        assert_eq!(code_names("why does SmartCrusherPlanner drop rows in crush_rows() and parseConfig?"), ["SmartCrusherPlanner", "crush_rows", "parseConfig"]);
        assert!(code_names("fix the typo in README and merci beaucoup").is_empty());
        assert!(code_names("call run() then Foo").is_empty());
    }

    #[test]
    fn grep_names_ported() {
        assert_eq!(grep_name("\\bSmartCrusher\\b").as_deref(), Some("SmartCrusher"));
        assert_eq!(grep_name("def compress_tool_results").as_deref(), Some("compress_tool_results"));
        assert_eq!(grep_name("parse\\(").as_deref(), Some("parse"));
        assert_eq!(grep_name("TODO|FIXME"), None);
        assert_eq!(grep_name("foo.*bar"), None);
    }

    #[test]
    fn exact_entries_ported() {
        let answer = "3 nodes. Best matches of 3:\n\nSmartCrusherPlanner a.rs:49\n\nSmartCrusher b.py:269\n  called by: main()\n\ncrusher() c.py:1";
        assert_eq!(exact_entries(answer, "SmartCrusher"), ["SmartCrusher b.py:269\n  called by: main()"]);
        assert_eq!(exact_entries(answer, "crusher"), ["crusher() c.py:1"]);
    }

    #[test]
    fn web_pages_and_urls_ported() {
        assert!(looks_like_web_page("<!DOCTYPE html><html><body>x</body></html>"));
        assert!(looks_like_web_page("HTTP/2 200\r\ncontent-type: text/html\r\n\r\n<html lang=\"en\">"));
        assert!(!looks_like_web_page("error: expected <html> at line 3"));
        assert!(!looks_like_web_page("{\"body\": \"<html>\"}"));
        assert_eq!(page_url("curl -sL \"https://example.com/a?b=1\" | head").as_deref(), Some("https://example.com/a?b=1"));
        assert_eq!(page_url("(Invoke-WebRequest https://x.org/p).Content").as_deref(), Some("https://x.org/p"));
        assert_eq!(page_url("ls"), None);
        assert_eq!(page_url("curl http://x").as_deref(), Some("http://x"));
    }

    #[test]
    fn a_short_output_is_not_worth_squeezing() {
        assert!(!worth_squeezing(&"x".repeat(1999)));
        assert!(worth_squeezing(&"x".repeat(2000)));
        assert!(squeezed("a", "a".into()).is_none());
        let big = "x".repeat(4000);
        assert_eq!(squeezed(&big, "short".into()), Some(("short".into(), 1000, 2)));
        assert!(squeezed(&big, big[100..].to_string()).is_none());
    }

    #[test]
    fn mcp_text_blocks_ported() {
        assert_eq!(text_blocks(&json!([{"type": "text", "text": "a"}, {"type": "text", "text": "b"}])).as_deref(), Some("a\nb"));
        assert_eq!(text_blocks(&json!([{"type": "image", "data": ""}])), None);
        assert_eq!(text_blocks(&json!({"rows": []})), None);
    }

    #[test]
    fn the_texts_keep_their_wording() {
        assert_eq!(squeeze_note(10, 2, "/p"), "\n\n[The dam condensed this output from about 10 to 2 tokens without dropping any line. Original: /p.]");
        assert!(graph_facts(&["a".into()], "dam").starts_with("The project's map on these names (`\"dam\" graph query \"<question>\"` asks it more):\n\na"));
    }

    #[test]
    fn codex_post_tool_use_gets_a_block_with_the_condensed_text() {
        let dir = temp("codex");
        let payload = json!({"session_id": "s", "cwd": dir.to_str().unwrap(), "hook_event_name": "PostToolUse", "tool_name": "Bash", "tool_input": {"command": "run-tests"}, "tool_response": log()}).to_string();
        let out: Value = serde_json::from_str(&respond(Host::Codex, &payload, &dir.join("out")).unwrap()).unwrap();
        assert_eq!(out["decision"], "block");
        let reason = out["reason"].as_str().unwrap();
        assert!(reason.len() < log().len() / 2 && reason.contains("The dam condensed this output"));
        assert_eq!(std::fs::read_dir(dir.join("out")).unwrap().count(), 1);
        // A failed command and a short output are left alone.
        let failed = json!({"cwd": ".", "hook_event_name": "PostToolUse", "tool_name": "Bash", "tool_input": {}, "tool_response": {"output": log(), "exit_code": 1}}).to_string();
        assert!(respond(Host::Codex, &failed, &dir).is_none());
        let short = json!({"cwd": ".", "hook_event_name": "PostToolUse", "tool_name": "Bash", "tool_input": {}, "tool_response": "ok"}).to_string();
        assert!(respond(Host::Codex, &short, &dir).is_none());
        let mcp = json!({"cwd": ".", "hook_event_name": "PostToolUse", "tool_name": "mcp__s__t", "tool_input": {}, "tool_response": {"content": [{"type": "text", "text": log()}]}}).to_string();
        assert!(respond(Host::Codex, &mcp, &dir.join("out")).unwrap().contains("block"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn copilot_post_tool_use_gets_a_modified_result() {
        let dir = temp("copilot");
        let payload = json!({"sessionId": "s", "cwd": dir.to_str().unwrap(), "toolName": "bash", "toolArgs": "{\"command\":\"run-tests\"}", "toolResult": {"resultType": "success", "textResultForLlm": log()}}).to_string();
        let out: Value = serde_json::from_str(&respond(Host::Copilot, &payload, &dir.join("out")).unwrap()).unwrap();
        assert_eq!(out["modifiedResult"]["resultType"], "success");
        assert!(out["modifiedResult"]["textResultForLlm"].as_str().unwrap().contains("The dam condensed this output"));
        let failed = payload.replace("\"success\"", "\"failure\"");
        assert!(respond(Host::Copilot, &failed, &dir).is_none());
        let viewed = payload.replace("\"bash\"", "\"view\"");
        assert!(respond(Host::Copilot, &viewed, &dir).is_none());
        assert!(respond(Host::Claude, &payload, &dir).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_page_a_command_fetched_becomes_markdown() {
        let dir = temp("web");
        let page = format!("<!DOCTYPE html><html><head><title>T</title><script>{}</script></head><body><article><h1>Hello</h1><p>{}</p></article></body></html>", "var a=1;".repeat(400), "Some words here. ".repeat(60));
        let payload = json!({"cwd": ".", "hook_event_name": "PostToolUse", "tool_name": "Bash", "tool_input": {"command": "curl -s https://example.com/p"}, "tool_response": page}).to_string();
        let out: Value = serde_json::from_str(&respond(Host::Codex, &payload, &dir).unwrap()).unwrap();
        let reason = out["reason"].as_str().unwrap();
        assert!(reason.contains("Hello") && reason.contains("turned this web page into Markdown") && !reason.contains("var a=1"), "{reason}");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_search_gets_the_maps_answer_on_both_hosts() {
        let dir = temp("search");
        with_graph(&dir);
        let cwd = dir.to_str().unwrap();
        let codex = json!({"cwd": cwd, "hook_event_name": "PostToolUse", "tool_name": "Bash", "tool_input": {"command": "rg -n apply_deny src"}, "tool_response": "src/a.py:1: apply_deny()"}).to_string();
        let out: Value = serde_json::from_str(&respond(Host::Codex, &codex, &dir).unwrap()).unwrap();
        assert_eq!(out["hookSpecificOutput"]["hookEventName"], "PostToolUse");
        assert!(out["hookSpecificOutput"]["additionalContext"].as_str().unwrap().contains("called by: run()"));
        let copilot = json!({"cwd": cwd, "toolName": "grep", "toolArgs": {"pattern": "\\bapply_deny\\b"}, "toolResult": {"resultType": "success", "textResultForLlm": "src/a.py:1"}}).to_string();
        let out: Value = serde_json::from_str(&respond(Host::Copilot, &copilot, &dir).unwrap()).unwrap();
        assert!(out["additionalContext"].as_str().unwrap().contains("apply_deny() src/deny.py:7"));
        assert!(out.get("modifiedResult").is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_codex_prompt_naming_a_function_gets_its_map_entry() {
        let dir = temp("prompt");
        with_graph(&dir);
        let payload = |prompt: &str| json!({"cwd": dir.to_str().unwrap(), "hook_event_name": "UserPromptSubmit", "prompt": prompt}).to_string();
        let out: Value = serde_json::from_str(&respond(Host::Codex, &payload("who calls apply_deny() ?"), &dir).unwrap()).unwrap();
        assert_eq!(out["hookSpecificOutput"]["hookEventName"], "UserPromptSubmit");
        assert!(out["hookSpecificOutput"]["additionalContext"].as_str().unwrap().contains("called by: run()"));
        assert!(respond(Host::Codex, &payload("fix the typo please"), &dir).is_none());
        assert!(respond(Host::Codex, &payload("/review apply_deny()"), &dir).is_none());
        assert!(respond(Host::Codex, "not json", &dir).is_none());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
