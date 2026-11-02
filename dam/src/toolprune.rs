// Tool pruning for Claude Code (Apache-2.0, Microsoft), rewritten from toolprune/stage.py, toolprune/tools.py, plugin/adapter.py, plugin/fold.py, plugin/toolcheck.py and cmd_toolprune of plugin/cli.py.
use crate::config::{plain_text, Config};
use crate::context::Ctx;
use crate::denylist;
use crate::sidecall::{claude_cli, claude_flags};
use crate::economizer::{now, python_truthy};
use crate::paths::realpath;
use crate::trace::{claude_deferred_tools, claude_tool_usage};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

// The work surface and what a person needs in an interactive session: never proposed.
const PROTECTED: &[&str] = &[
    "bash", "view", "grep", "glob", "edit", "create", "str_replace", "read_bash", "stop_bash", "list_bash", "read_file", "shell", "apply_patch",
    "Bash", "Read", "Edit", "MultiEdit", "Write", "Grep", "Glob", "LS", "NotebookRead", "BashOutput", "KillShell", "KillBash",
    "AskUserQuestion", "EnterPlanMode", "ExitPlanMode", "TodoWrite", "TaskCreate", "TaskUpdate", "TaskGet", "TaskList", "TaskOutput", "TaskStop",
    "Agent", "Task", "Skill", "ToolSearch", "SlashCommand", "Monitor",
    // The web is never proposed: a run of self-contained tasks can leave it unused for long, and a person's next task may need it.
    "WebFetch", "WebSearch",
];
const HIGH_RISK: &str = "NotebookEdit";
const HIGH_RISK_FACTOR: i64 = 3;
const PENDING_MAX_S: f64 = 24.0 * 3600.0;
const WAITER_POLL_S: f64 = 600.0;

// Python's fnmatch.fnmatchcase.
fn matches(pattern: &[char], name: &[char]) -> bool {
    match pattern.first() {
        None => name.is_empty(),
        Some('*') => (0..=name.len()).any(|skip| matches(&pattern[1..], &name[skip..])),
        Some('?') => !name.is_empty() && matches(&pattern[1..], &name[1..]),
        Some('[') => {
            let mut end = 1;
            if pattern.get(end) == Some(&'!') {
                end += 1;
            }
            if pattern.get(end) == Some(&']') {
                end += 1;
            }
            while end < pattern.len() && pattern[end] != ']' {
                end += 1;
            }
            if end >= pattern.len() {
                return name.first() == Some(&'[') && matches(&pattern[1..], &name[1..]);
            }
            let (negated, set) = if pattern[1] == '!' { (true, &pattern[2..end]) } else { (false, &pattern[1..end]) };
            let Some(&first) = name.first() else { return false };
            let (mut found, mut at) = (false, 0);
            while at < set.len() {
                if at + 2 < set.len() && set[at + 1] == '-' {
                    found |= (set[at]..=set[at + 2]).contains(&first);
                    at += 3;
                } else {
                    found |= set[at] == first;
                    at += 1;
                }
            }
            found != negated && matches(&pattern[end + 1..], &name[1..])
        }
        Some(&c) => name.first() == Some(&c) && matches(&pattern[1..], &name[1..]),
    }
}

// The protected names, and the patterns (`mcp__headroom__*`) a name may match instead.
struct Protected(Vec<String>);

impl Protected {
    fn has(&self, name: &str) -> bool {
        let name: Vec<char> = name.chars().collect();
        PROTECTED.iter().any(|known| known.chars().eq(name.iter().copied())) || self.0.iter().any(|pattern| matches(&pattern.chars().collect::<Vec<_>>(), &name))
    }
}

// Has a never-used tool gone unused long enough to be left out? Over `toolprune_evidence` prompts spread across `toolprune_min_sessions` sessions, or over `toolprune_sessions` sessions however short.
fn enough_evidence(sessions: i64, prompts: i64, config: &Config, high_risk: bool) -> bool {
    let k = if high_risk { HIGH_RISK_FACTOR } else { 1 };

    (sessions >= config.number("toolprune_min_sessions") && prompts >= config.number("toolprune_evidence") * k) || sessions >= config.number("toolprune_sessions") * k
}

fn count(value: &Value) -> i64 {
    match value {
        Value::Bool(flag) => *flag as i64,
        Value::String(text) => text.trim().parse().unwrap_or(0),
        other => other.as_i64().or_else(|| other.as_f64().map(|n| n as i64)).unwrap_or(0),
    }
}

fn counts(value: Option<&Value>) -> BTreeMap<String, i64> {
    value.and_then(Value::as_object).into_iter().flatten().map(|(name, n)| (name.clone(), count(n))).collect()
}

fn names(value: Option<&Value>) -> BTreeSet<String> {
    value.and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect()
}

// The tool list `refresh-catalog` recorded: what a fresh session loads.
fn load_catalog(root: &Path) -> Vec<String> {
    let Some(doc) = fs::read(catalog_path(root)).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok()) else { return Vec::new() };

    doc.get("tools").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect()
}

fn catalog_path(root: &Path) -> PathBuf {
    root.join("hosts").join("claude").join("catalog.json")
}

// Tool pruning's evidence: the sessions a tool went unused and the prompts those sessions ran, weighed by `enough_evidence`. A list applies from a later session.
pub fn fold_session(ctx: &Ctx, run_dir: &Path, _session_id: &str, _transcript: Option<&str>, _workdir: &Path, prompts: Option<i64>) -> Value {
    let config = &ctx.config;
    if config.text("toolprune") == "off" {
        return Value::Null;
    }
    let st = ctx.store.read("untool.json").filter(python_truthy).unwrap_or_else(|| json!({}));
    let evidence = config.number("toolprune_min_sessions").max(1);
    let mut used = names(st.get("used_tools"));
    let mut streaks = counts(st.get("streaks"));
    // A session from before prompts were counted counts as one prompt.
    let mut prompt_streaks = if st.get("prompt_streaks").is_some_and(python_truthy) { counts(st.get("prompt_streaks")) } else { streaks.clone() };
    let mut catalog_tools = names(st.get("catalog_tools"));
    let mut catalog_desc: Map<String, Value> = st.get("catalog_desc").and_then(Value::as_object).cloned().unwrap_or_default();
    let mut applied = denylist::applied_for(&ctx.store, &ctx.repo.root);
    let kept: BTreeSet<String> = denylist::kept_for(&ctx.store, &ctx.repo.root).into_iter().collect();
    // Whether this session ran under a mask: a masked tool list is not the full one, so no catalog is read from it.
    let masked = !applied.is_empty();
    let protected = Protected(config.list("toolprune_protect"));
    // A tool that has since become protected comes back: the plugin's OWN deny rule for it is removed, never one the person wrote.
    let mut restored = Vec::new();
    if applied.iter().any(|tool| protected.has(tool)) && !config.shadow {
        let back: Vec<String> = applied.iter().filter(|tool| protected.has(tool)).cloned().collect();
        restored = denylist::restore(&ctx.repo.root, &ctx.store, &back).unwrap_or_default();
    }
    if !restored.is_empty() {
        applied = denylist::applied_for(&ctx.store, &ctx.repo.root);
    }

    let transcript = realpath(&run_dir.join("transcript.jsonl"));
    let task_used: BTreeSet<String> = claude_tool_usage(&transcript).into_keys().collect();
    used.extend(task_used.iter().cloned());
    let mut source = None;
    if !masked {
        let probed = load_catalog(&ctx.root);
        let mut seen: BTreeSet<String> = probed.iter().cloned().collect();
        seen.extend(task_used.iter().cloned());
        seen.extend(claude_deferred_tools(&transcript));
        if !seen.is_empty() {
            source = Some(if probed.is_empty() { "observed" } else { "probe+observed" });
            for tool in seen {
                catalog_desc.insert(tool.clone(), json!(""));
                catalog_tools.insert(tool);
            }
        }
    }
    let before = streaks.clone();
    for tool in &catalog_tools {
        let streak = if task_used.contains(tool) { 0 } else { streaks.get(tool).copied().unwrap_or(0) + 1 };
        streaks.insert(tool.clone(), streak);
    }
    for (tool, &streak) in &streaks {
        if streak == 0 {
            prompt_streaks.insert(tool.clone(), 0);
        } else if streak == before.get(tool).copied().unwrap_or(0) + 1 {
            *prompt_streaks.entry(tool.clone()).or_insert(0) += prompts.unwrap_or(1);
        }
    }

    let sessions = st.get("sessions").map_or(0, count) + 1;
    let raw: Vec<&String> = catalog_tools.iter()
        .filter(|tool| !protected.has(tool) && !used.contains(*tool) && streaks.get(*tool).copied().unwrap_or(0) >= evidence)
        .filter(|tool| enough_evidence(streaks.get(*tool).copied().unwrap_or(0), prompt_streaks.get(*tool).copied().unwrap_or(0), config, tool.as_str() == HIGH_RISK))
        .collect();
    let proposal: Vec<String> = raw.into_iter().filter(|tool| !applied.contains(tool) && !kept.contains(*tool)).cloned().collect();
    let earlier = names(st.get("proposal"));
    let new_in_proposal: Vec<&String> = proposal.iter().filter(|tool| !earlier.contains(*tool)).collect::<BTreeSet<_>>().into_iter().collect();
    let mut doc = json!({
        "used_tools": used, "streaks": streaks, "prompt_streaks": prompt_streaks,
        "catalog_tools": catalog_tools, "catalog_desc": catalog_desc,
        // The last source still describes the catalog when this session read none.
        "catalog_source": source.map(Value::from).or_else(|| st.get("catalog_source").cloned()).unwrap_or(Value::Null),
        "sessions": sessions, "prompts": st.get("prompts").map_or(0, count) + prompts.unwrap_or(0),
        "updated": now(), "proposal": proposal, "new_in_proposal": new_in_proposal,
    });
    if config.text("toolprune") == "auto" && !proposal.is_empty() && !config.shadow {
        // `auto`: the proposal waits for `apply_pending`, from when it first waited.
        let since = st.get("auto_pending").and_then(|pending| pending.get("since")).filter(|since| python_truthy(since)).cloned().unwrap_or_else(|| json!(now()));
        doc["auto_pending"] = json!({"items": proposal, "root": ctx.repo.root, "since": since});
        // A refusal to write the file stands until a write succeeds.
        if let Some(refused) = st.get("auto_apply_refused").filter(|refused| python_truthy(refused)) {
            doc["auto_apply_refused"] = refused.clone();
        }
    }
    if !restored.is_empty() {
        doc["restored_protected"] = json!(restored);
    }
    let _ = ctx.store.write("untool.json", &doc);

    json!({"proposal": doc["proposal"], "pending": doc.get("auto_pending").is_some(), "restored": restored})
}

fn mtime(path: &Path) -> Option<f64> {
    Some(fs::metadata(path).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs_f64())
}

// When a session of this repository last did anything: a hook call (its record's write) or a line in its own log.
fn last_activity(ctx: &Ctx) -> f64 {
    let dir = ctx.store.base.join("sessions");
    let Ok(entries) = fs::read_dir(&dir) else { return 0.0 };
    let mut latest = 0.0_f64;
    for entry in entries.flatten().filter(|entry| entry.file_name().to_string_lossy().ends_with(".json")) {
        let Some(wrote) = mtime(&entry.path()) else { continue };
        latest = latest.max(wrote);
        let transcript = ctx.store.read(&format!("sessions/{}", entry.file_name().to_string_lossy())).and_then(|record| record.get("transcript").and_then(Value::as_str).map(PathBuf::from));
        latest = latest.max(transcript.and_then(|path| mtime(&path)).unwrap_or(0.0));
    }

    latest
}

// Writes the pending `auto` deny list once the repository has been quiet long enough. Returns the seconds to wait before looking again, or None when there is nothing left to wait for.
pub fn apply_pending(ctx: &Ctx, now_s: Option<f64>) -> Option<f64> {
    let now_s = now_s.filter(|n| *n != 0.0).unwrap_or_else(now);
    let _lock = ctx.store.locked("mechanisms");
    let mut unt = ctx.store.read("untool.json").filter(Value::is_object).unwrap_or_else(|| json!({}));
    let pending = unt.get("auto_pending").filter(|pending| python_truthy(pending)).cloned()?;
    let root = pending.get("root").and_then(Value::as_str).filter(|root| !root.is_empty()).unwrap_or(&ctx.repo.root).to_string();
    let mut done = denylist::applied_for(&ctx.store, &root);
    done.extend(denylist::kept_for(&ctx.store, &root));
    let items: Vec<String> = pending.get("items").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).filter(|item| !done.iter().any(|known| known == item)).map(str::to_string).collect();
    if !items.is_empty() && ctx.config.text("toolprune") == "auto" && !ctx.config.shadow {
        let idle = now_s - last_activity(ctx);
        let quiet = ctx.config.number("toolprune_quiet_minutes") as f64 * 60.0;
        let waited = now_s - pending.get("since").and_then(Value::as_f64).filter(|since| *since != 0.0).unwrap_or(now_s);
        if idle < quiet && waited < PENDING_MAX_S {
            return Some((quiet - idle).min(PENDING_MAX_S - waited));
        }
        match denylist::apply(&root, &items, &ctx.store) {
            Ok(added) => {
                let left: Vec<Value> = unt.get("proposal").and_then(Value::as_array).into_iter().flatten().filter(|tool| !tool.as_str().is_some_and(|name| items.iter().any(|item| item == name))).cloned().collect();
                unt["auto_applied"] = json!(added);
                unt["proposal"] = json!(left);
                unt.as_object_mut()?.remove("auto_apply_refused");
            }
            Err(why) => unt["auto_apply_refused"] = json!(why),
        }
    }
    unt.as_object_mut()?.remove("auto_pending");
    let _ = ctx.store.write("untool.json", &unt);

    None
}

// Looks at the pending deny list now and, if it has to wait, leaves a detached `toolprune apply-pending` to write it later (only the look, when TE_FOLD_SYNC=1).
pub fn schedule_pending(ctx: &Ctx) -> Option<f64> {
    let wait = apply_pending(ctx, None);
    if wait.is_some() && std::env::var("TE_FOLD_SYNC").as_deref() != Ok("1") {
        spawn_waiter(ctx);
    }

    wait
}

// The waiter is this program again, detached, as the fold's other jobs are.
fn spawn_waiter(ctx: &Ctx) -> Option<()> {
    crate::session::spawn(ctx, &["toolprune", "apply-pending"], "fold.log").then_some(())
}

// ---- the commands --------------------------------------------------------------------------
fn rebuild(ctx: &Ctx) -> Ctx {
    Ctx::with_repo(ctx.root.clone(), ctx.repo.clone())
}

// The proposal, less what this checkout already applied or gave back: the clones of one repository share its evidence.
fn proposal_of(ctx: &Ctx, unt: &Value) -> Vec<String> {
    let mut mine = denylist::applied_for(&ctx.store, &ctx.repo.root);
    mine.extend(denylist::kept_for(&ctx.store, &ctx.repo.root));

    unt.get("proposal").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).filter(|tool| !mine.iter().any(|known| known == tool)).map(str::to_string).collect()
}

fn dash(items: &[String]) -> String {
    if items.is_empty() { "-".to_string() } else { items.join(", ") }
}

fn show(ctx: &Ctx, unt: &Value) -> i32 {
    let shown = |key: &str| unt.get(key).map_or_else(|| "0".to_string(), plain_text);
    let source = unt.get("catalog_source").filter(|source| python_truthy(source)).map_or_else(|| "none yet".to_string(), plain_text);
    println!("sessions observed: {} ({} prompts)  catalog: {source}", shown("sessions"), shown("prompts"));
    println!("applied (tools this plugin added to {}): {}", denylist::settings_path(&ctx.repo.root).display(), dash(&denylist::applied_for(&ctx.store, &ctx.repo.root)));
    let proposal = proposal_of(ctx, unt);
    println!("proposed: {}", dash(&proposal));
    let waiting = unt.get("auto_pending").and_then(|pending| pending.get("items")).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).any(|tool| proposal.iter().any(|known| known == tool));
    if waiting && ctx.config.text("toolprune") == "auto" {
        println!("pending: written once this repository's sessions have been idle {} minutes, so that no conversation you resume re-reads its cached prompt (`toolprune apply` writes it now)", ctx.config.number("toolprune_quiet_minutes"));
    }
    let kept = denylist::kept_for(&ctx.store, &ctx.repo.root);
    if !kept.is_empty() {
        println!("kept, as you gave them back (`toolprune apply NAME` removes one again): {}", kept.join(", "));
    }
    if let Some(streaks) = unt.get("streaks").and_then(Value::as_object).filter(|streaks| !streaks.is_empty()) {
        let prompts = counts(unt.get("prompt_streaks"));
        let used = names(unt.get("used_tools"));
        let config = &ctx.config;
        println!("how long each tool has gone unused (a tool never used is left out after {} prompts over {}+ sessions, or after {} sessions):", config.number("toolprune_evidence"), config.number("toolprune_min_sessions"), config.number("toolprune_sessions"));
        let mut rows: Vec<(&String, i64)> = streaks.iter().map(|(tool, n)| (tool, count(n))).collect();
        rows.sort_by_key(|(tool, n)| (-n, -prompts.get(*tool).copied().unwrap_or(0)));
        for (tool, n) in rows.into_iter().take(30) {
            println!("  {n:4} sessions {:5} prompts  {tool} {}", prompts.get(tool).copied().unwrap_or(n), if used.contains(tool) { "used" } else { "" });
        }
    }
    let source = unt.get("catalog_source").and_then(Value::as_str).unwrap_or("");
    if source.is_empty() || source == "observed" {
        if catalog_path(&ctx.root).exists() {
            // Recorded, but a repository reads the catalog when its next session is folded.
            println!("note: the catalog `toolprune refresh-catalog` recorded joins this repository's evidence from its next session.");
        } else {
            println!("note: without `te --host claude toolprune refresh-catalog` only deferred and observed tools are known, which leaves the always-loaded tools out of the evidence.");
        }
    }

    0
}

// A command that changes something checks first, so that a sandboxed agent gets no half a change: a deny list written, and no record that the plugin added it.
fn refuse_unwritable(base: &Path) -> bool {
    let probe = base.join(format!(".write-test-{}", std::process::id()));
    let why = match fs::create_dir_all(base).and_then(|_| fs::write(&probe, "")).and_then(|_| fs::remove_file(&probe)) {
        Ok(()) => return false,
        Err(e) => e.to_string().split(" (os error").next().unwrap_or("").to_string(),
    };
    let argv: Vec<String> = std::env::args().skip(1).collect();
    println!("not changed: TokenEconomizer cannot write its state in {} ({why}). Inside a sandboxed agent, run this command with permission to write outside the workspace (Codex asks you to approve it), or run it from a terminal as `~/.tokeneconomizer/bin/te {}`.", base.display(), if argv.is_empty() { "...".to_string() } else { argv.join(" ") });

    true
}

// `toolprune apply-pending`, which the fold leaves running, detached: wait until the pending `auto` deny list can be written without costing a warm prompt cache, then write it. One waiter per repository: a second finds the first one's heartbeat fresh and leaves.
fn wait_and_apply(ctx: &Ctx) -> i32 {
    let beat = ctx.store.base.join("toolprune-waiter");
    if mtime(&beat).is_some_and(|wrote| now() - wrote < 2.0 * WAITER_POLL_S) {
        return 0;
    }
    let _ = fs::create_dir_all(&ctx.store.base);
    let _ = fs::write(&beat, std::process::id().to_string());
    let mut current = rebuild(ctx);
    let code = loop {
        let Some(wait) = apply_pending(&current, None) else { break 0 };
        std::thread::sleep(Duration::from_secs_f64(wait.max(1.0).min(WAITER_POLL_S)));
        if let Ok(file) = fs::OpenOptions::new().write(true).open(&beat) {
            let _ = file.set_modified(SystemTime::now());
        }
        // The settings may have changed meanwhile.
        current = rebuild(&current);
    };
    let _ = fs::remove_file(&beat);

    code
}

fn scratch_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.subsec_nanos());
    let dir = std::env::temp_dir().join(format!("{prefix}{}-{nanos}", std::process::id()));
    let _ = fs::create_dir_all(&dir);

    dir
}

// The MCP servers a session launches outlive a plain kill, and on Windows keep the probe's directory in use.
fn kill_tree(child: &mut std::process::Child) {
    if cfg!(windows) {
        let _ = Command::new("taskkill").args(["/T", "/F", "/PID", &child.id().to_string()]).stdout(Stdio::null()).stderr(Stdio::null()).status();
    } else {
        let _ = Command::new("kill").args(["-9", &format!("-{}", child.id())]).status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

// The tool list from a `claude -p` session's init event; the process is killed right after it, before the model is asked anything.
fn init_tools(cli: &str) -> (Vec<String>, Value) {
    let mut command = Command::new(cli);
    command.args(["-p", "Reply with OK", "--output-format", "stream-json", "--verbose", "--model", "haiku", "--max-turns", "1"]).args(claude_flags(cli));
    let cwd = scratch_dir("te-probe-");
    command.env("TE_SIDECALL", "1").current_dir(&cwd).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);
    let (mut tools, mut version) = (Vec::new(), Value::Null);
    if let Ok(mut child) = command.spawn() {
        // On a thread, so that a claude that hangs without a word still meets the deadline.
        let deadline = Instant::now() + Duration::from_secs(60);
        let (sender, lines) = std::sync::mpsc::channel();
        if let Some(out) = child.stdout.take() {
            std::thread::spawn(move || {
                for line in BufReader::new(out).lines().map_while(Result::ok) {
                    if sender.send(line).is_err() {
                        break;
                    }
                }
            });
        }
        while let Ok(line) = lines.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            if let Ok(event) = serde_json::from_str::<Value>(&line) {
                if event["type"] == "system" && event["subtype"] == "init" {
                    tools = event.get("tools").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect();
                    version = event.get("claude_code_version").cloned().unwrap_or(Value::Null);
                    break;
                }
            }
        }
        kill_tree(&mut child);
    }
    let _ = fs::remove_dir_all(&cwd);

    (tools, version)
}

// Records the tool list a fresh session loads on this machine.
fn refresh_catalog(root: &Path) -> Result<(PathBuf, Vec<String>), String> {
    let (mut tools, version) = init_tools(&claude_cli());
    if tools.is_empty() {
        return Err("could not read the tool list from a `claude -p` init event".to_string());
    }
    tools.sort();
    let path = catalog_path(root);
    denylist::write_json(&path, &json!({"tools": tools, "claude_code_version": version, "probed_at": now()})).map_err(|e| e.to_string())?;

    Ok((path, tools))
}

fn prompt_tokens(cli: &str, flags: &[String], extra: &[String]) -> Result<(i64, Option<Value>), String> {
    let cwd = scratch_dir("te-measure-");
    let out = Command::new(cli).args(["-p", "Reply with exactly: OK", "--output-format", "json", "--model", "haiku", "--max-turns", "1"]).args(flags).args(extra).env("TE_SIDECALL", "1").current_dir(&cwd).stdin(Stdio::null()).output();
    let _ = fs::remove_dir_all(&cwd);
    let out = out.map_err(|e| e.to_string())?;
    let data: Value = if out.stdout.iter().all(u8::is_ascii_whitespace) { json!({}) } else { serde_json::from_slice(&out.stdout).map_err(|e| e.to_string())? };
    let usage = data.get("usage").cloned().unwrap_or(Value::Null);
    let tokens = ["input_tokens", "cache_read_input_tokens", "cache_creation_input_tokens"].iter().map(|key| usage.get(key).map_or(0, count)).sum();

    Ok((tokens, data.get("total_cost_usd").filter(|cost| python_truthy(cost)).cloned()))
}

// Prompt tokens of two small requests, with and without `tools` removed.
fn measure(tools: &[String]) -> Result<Value, String> {
    let cli = claude_cli();
    let flags = claude_flags(&cli);
    let (full, first) = prompt_tokens(&cli, &flags, &[])?;
    let (lean, second) = prompt_tokens(&cli, &flags, &["--disallowed-tools".to_string(), tools.join(",")])?;
    let costs: Vec<Value> = [first, second].into_iter().flatten().collect();
    let cost = if costs.iter().all(|cost| cost.is_i64()) { json!(costs.iter().map(count).sum::<i64>()) } else { json!((costs.iter().filter_map(Value::as_f64).sum::<f64>() * 1e4).round() / 1e4) };

    Ok(json!({"tools": tools, "prompt_tokens_full": full, "prompt_tokens_lean": lean, "saved_per_request": full - lean, "probe_cost_usd": cost}))
}

// `te toolprune [show|apply|restore|refresh-catalog|measure|apply-pending] [ITEM ...]`
pub fn command(ctx: &Ctx, args: &[String]) -> i32 {
    let action = args.first().map_or("show", String::as_str);
    let items = args.get(1..).unwrap_or(&[]);
    let unt = ctx.store.read("untool.json").filter(python_truthy).unwrap_or_else(|| json!({}));
    if matches!(action, "apply" | "restore") && refuse_unwritable(&ctx.store.base) {
        return 1;
    }
    match action {
        "show" => show(ctx, &unt),
        "apply" => {
            let tools = if items.is_empty() { proposal_of(ctx, &unt) } else { items.to_vec() };
            if tools.is_empty() {
                println!("nothing to apply");
                return 0;
            }
            let added = match denylist::apply(&ctx.repo.root, &tools, &ctx.store) {
                Ok(added) => added,
                Err(why) => {
                    println!("not applied: {why}");
                    return 1;
                }
            };
            // Named by the person: no longer one they gave back, whatever `restore` said before.
            if let Err(why) = denylist::set_kept(&ctx.store, &ctx.repo.root, &[], &tools) {
                println!("not applied: {why}");
                return 1;
            }
            println!("added: {}\nin {} -- takes effect from the next session.", if added.is_empty() { "(already present)".to_string() } else { added.join(", ") }, denylist::settings_path(&ctx.repo.root).display());
            0
        }
        "restore" => {
            let removed = match denylist::restore(&ctx.repo.root, &ctx.store, items) {
                Ok(removed) => removed,
                Err(why) => {
                    println!("not restored: {why}");
                    return 1;
                }
            };
            // Given back by the person: `toolprune: auto` must not take them away again at the next fold.
            if let Err(why) = denylist::set_kept(&ctx.store, &ctx.repo.root, &removed, &[]) {
                println!("not restored: {why}");
                return 1;
            }
            let note = removed.first().map_or_else(String::new, |first| format!("; kept from now on (`toolprune apply {first}` removes one again)"));
            println!("removed: {} (from the next session){note}", dash(&removed));
            0
        }
        "apply-pending" => wait_and_apply(ctx),
        "refresh-catalog" => match refresh_catalog(&ctx.root) {
            Ok((path, tools)) => {
                println!("{} tools recorded in {}:\n  {}", tools.len(), path.display(), tools.join(", "));
                0
            }
            Err(why) => {
                eprintln!("te: {why}");
                1
            }
        },
        "measure" => {
            let mut tools = if items.is_empty() { proposal_of(ctx, &unt) } else { items.to_vec() };
            if tools.is_empty() {
                tools = denylist::applied_for(&ctx.store, &ctx.repo.root);
            }
            if tools.is_empty() {
                println!("no proposal to measure");
                return 1;
            }
            match measure(&tools) {
                Ok(result) => {
                    println!("{}", denylist::pretty(&result));
                    0
                }
                Err(why) => {
                    eprintln!("te: {why}");
                    1
                }
            }
        }
        _ => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::RepoId;

    fn chars(text: &str) -> Vec<char> {
        text.chars().collect()
    }

    #[test]
    fn patterns_match_like_fnmatch() {
        let ok = |pattern: &str, name: &str| matches(&chars(pattern), &chars(name));
        assert!(ok("mcp__headroom__*", "mcp__headroom__search"));
        assert!(!ok("mcp__headroom__*", "mcp__other__search"));
        assert!(ok("Tool?", "Tool1") && !ok("Tool?", "Tool"));
        assert!(ok("[ab]x", "bx") && !ok("[!ab]x", "bx") && ok("[a-c]x", "cx"));
        assert!(ok("[x", "[x"));
        assert!(Protected(vec!["mcp__a__*".to_string()]).has("mcp__a__b") && Protected(Vec::new()).has("Bash") && !Protected(Vec::new()).has("NotebookEdit"));
    }

    #[test]
    fn evidence_needs_prompts_over_sessions_or_enough_sessions() {
        let config = Config::new();
        assert!(!enough_evidence(1, 99, &config, false));
        assert!(enough_evidence(2, 10, &config, false));
        assert!(!enough_evidence(4, 9, &config, false));
        assert!(enough_evidence(5, 0, &config, false));
        assert!(!enough_evidence(5, 0, &config, true) && !enough_evidence(14, 29, &config, true) && enough_evidence(15, 0, &config, true));
    }

    #[test]
    fn an_unused_tool_is_proposed_after_enough_sessions_and_held_for_auto() {
        let dir = std::env::temp_dir().join(format!("dam-toolprune-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let run = dir.join("run");
        fs::create_dir_all(&run).unwrap();
        fs::write(run.join("transcript.jsonl"), r#"{"type":"assistant","cwd":"/r","message":{"content":[{"type":"tool_use","id":"1","name":"Bash","input":{"command":"ls"}}]}}"#).unwrap();
        fs::create_dir_all(dir.join("hosts").join("claude")).unwrap();
        fs::write(catalog_path(&dir), r#"{"tools": ["Bash", "Foo", "Skill"]}"#).unwrap();
        let repo = RepoId { root: dir.join("repo").to_string_lossy().into_owned(), key: "k".into(), label: "l".into(), remote: String::new(), how: "path".into() };
        let ctx = Ctx::with_repo(dir.clone(), repo);
        let mut last = Value::Null;
        for _ in 0..5 {
            last = fold_session(&ctx, &run, "s", None, &dir, Some(1));
        }
        assert_eq!(last["proposal"], json!(["Foo"]));
        assert_eq!(last["pending"], json!(true));
        let doc = ctx.store.read("untool.json").unwrap();
        assert_eq!(doc["sessions"], 5);
        assert_eq!(doc["streaks"]["Foo"], 5);
        assert_eq!(doc["streaks"]["Bash"], 0);
        assert_eq!(doc["catalog_source"], "probe+observed");
        let _ = fs::remove_dir_all(&dir);
    }
}
