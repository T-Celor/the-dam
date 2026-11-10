// Tool pruning for Claude Code, Codex and Copilot (Apache-2.0, Microsoft), rewritten from toolprune/stage.py, toolprune/tools.py, plugin/adapter.py, plugin/fold.py (_fold_toolprune, _fold_toolprune_codex), toolprune/codex.py, plugin/toolcheck.py and cmd_toolprune of plugin/cli.py.
use crate::channels::Host;
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

// adapter.COPILOT_PROTECTED: the Copilot CLI's own work surface and a person's tools there (1.0.83 names).
const COPILOT_PROTECTED: &[&str] = &[
    "bash", "read_bash", "write_bash", "stop_bash", "list_bash", "view", "edit", "create", "str_replace", "apply_patch", "custom", "rg", "grep", "glob",
    "task", "skill", "read_agent", "list_agents", "write_agent", "ask_user", "web_fetch", "web_search", "report_intent", "update_todo", "sql",
];

// trace.copilot_mcp_servers: the built-in GitHub server and every server in the user's `mcp-config.json`.
fn copilot_mcp_servers(home: &Path) -> BTreeSet<String> {
    let mut servers = BTreeSet::from(["github-mcp-server".to_string()]);
    if let Some(doc) = fs::read(home.join("mcp-config.json")).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok()) {
        servers.extend(doc.get("mcpServers").and_then(Value::as_object).into_iter().flatten().map(|(name, _)| name.clone()));
    }

    servers
}

// trace.mcp_server_of: the longest `<server>-` prefix of a Copilot tool name.
fn mcp_server_of<'a>(tool: &str, servers: &'a BTreeSet<String>) -> Option<&'a String> {
    servers.iter().filter(|server| tool.strip_prefix(server.as_str()).is_some_and(|rest| rest.starts_with('-'))).max_by_key(|server| server.len())
}

// trace.parse_tool_names_copilot: the tool list a request was sent, in `session.usage_checkpoint` events.
fn copilot_loaded_tools(events: &Path) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for line in crate::trace::lines(events) {
        let Ok(event) = serde_json::from_str::<Value>(&line) else { continue };
        for state in event["data"].get("promptCacheBreakState").and_then(Value::as_array).into_iter().flatten() {
            for model in state.get("models").and_then(Value::as_object).into_iter().flatten().map(|(_, model)| model) {
                names.extend(model.get("tools").and_then(Value::as_array).into_iter().flatten().filter_map(|tool| tool.get("name").and_then(Value::as_str)).map(str::to_string));
            }
        }
    }

    names
}

// denylist.copilot_server_proposal: the servers every one of whose catalog tools is in `raw`, and none used.
fn copilot_server_proposal(raw: &BTreeSet<&String>, catalog: &BTreeSet<String>, used: &BTreeSet<String>, servers: &BTreeSet<String>) -> Vec<String> {
    let mut by_server: BTreeMap<&String, Vec<&String>> = BTreeMap::new();
    for tool in catalog {
        if let Some(server) = mcp_server_of(tool, servers) {
            by_server.entry(server).or_default().push(tool);
        }
    }

    by_server.into_iter().filter(|(_, tools)| tools.iter().all(|tool| raw.contains(tool) && !used.contains(*tool))).map(|(server, _)| server.clone()).collect()
}

// fold._server_summary: the Copilot evidence at the unit it acts on, a server's least-evidenced tool.
fn server_summary(catalog: &BTreeSet<String>, used: &BTreeSet<String>, streaks: &BTreeMap<String, i64>, prompt_streaks: &BTreeMap<String, i64>, servers: &BTreeSet<String>) -> Value {
    let mut out = Map::new();
    for tool in catalog {
        let Some(server) = mcp_server_of(tool, servers) else { continue };
        let entry = out.entry(server.clone()).or_insert_with(|| json!({"tools": 0, "used": false, "unused_sessions": null, "unused_prompts": null}));
        entry["tools"] = json!(entry["tools"].as_i64().unwrap_or(0) + 1);
        entry["used"] = json!(entry["used"] == true || used.contains(tool));
        for (key, n) in [("unused_sessions", streaks.get(tool)), ("unused_prompts", prompt_streaks.get(tool))] {
            let n = n.copied().unwrap_or(0);
            entry[key] = json!(entry[key].as_i64().map_or(n, |known| known.min(n)));
        }
    }

    Value::Object(out)
}

// ---- Codex: capability packages (fold._fold_toolprune_codex, toolprune/codex.py, trace.codex_capability_trace) ----
// toolprune/codex.py MIN_LOADED_EXPOSURES for the packages the live lever reaches: the profile demotes one only after this many loaded sessions.
const CODEX_MIN_LOADED: &[(&str, i64)] = &[("collaboration", 1), ("goals", 1), ("image_media", 3)];
// fold.CODEX_HIGH_RISK: rarely used, expensive to lose.
const CODEX_HIGH_RISK: &str = "image_media";
static NESTED_TOOL: std::sync::LazyLock<fancy_regex::Regex> = std::sync::LazyLock::new(|| fancy_regex::Regex::new(r"\btools\.([A-Za-z_][A-Za-z0-9_]*)\s*\(").expect("fixed"));

// _ITEM_TYPES_BY_CAPABILITY, by name; `planning`, `web` and `request_user_input` are no package the lever removes.
fn codex_capability_of(name: &str) -> Option<&'static str> {
    match name {
        "spawn_agent" | "send_input" | "send_message" | "followup_task" | "resume_agent" | "wait_agent" | "close_agent" | "interrupt_agent" | "list_agents" | "collab_tool_call" => Some("collaboration"),
        "create_goal" | "get_goal" | "update_goal" => Some("goals"),
        "view_image" | "image" | "audio" | "generated_image" | "image_generation" => Some("image_media"),
        _ => None,
    }
}

// _item_capabilities: an item's type and names, and the `tools.<name>(` calls inside a code-mode `exec` call.
fn codex_item_capabilities(item: &Value) -> BTreeSet<&'static str> {
    let text = |key: &str| item.get(key).map(plain_text).unwrap_or_default();
    let mut found: BTreeSet<&'static str> = ["type", "name", "tool_name"].iter().filter_map(|key| codex_capability_of(&text(key))).collect();
    if matches!(text("type").as_str(), "custom_tool_call" | "function_call" | "tool_call") || text("name") == "exec" {
        for raw in ["input", "arguments"].iter().filter_map(|key| item.get(*key)) {
            let raw = if raw.is_string() { plain_text(raw) } else if raw.is_object() { raw.to_string() } else { continue };
            found.extend(NESTED_TOOL.captures_iter(&raw).flatten().filter_map(|caps| codex_capability_of(caps.get(1)?.as_str())));
        }
    }

    found
}

// inspect_usage over codex_capability_trace's items: the packages a rollout attempted, and those a call completed.
fn codex_usage(rollout: &Path) -> (BTreeSet<&'static str>, BTreeSet<&'static str>) {
    let (mut attempted, mut completed) = (BTreeSet::new(), BTreeSet::new());
    for line in crate::trace::lines(rollout) {
        let Ok(record) = serde_json::from_str::<Value>(&line) else { continue };
        let payload = &record["payload"];
        let item = match (record.get("type").and_then(Value::as_str), payload.get("type").and_then(Value::as_str)) {
            // rollout_item_to_exec: the rollout's item types, as the exec stream names them.
            (Some("event_msg"), Some("item_completed")) => {
                let mut item = payload["item"].clone();
                let kind = match item.get("type").and_then(Value::as_str) {
                    Some("ImageView") => "view_image",
                    Some("CollabToolCall") => "collab_tool_call",
                    _ => continue,
                };
                item["type"] = json!(kind);
                item
            }
            (Some("response_item"), Some(kind @ ("custom_tool_call" | "function_call"))) => json!({"type": kind, "name": payload.get("name"), "input": payload.get("input"), "arguments": payload.get("arguments"), "status": payload.get("status")}),
            _ => continue,
        };
        let found = codex_item_capabilities(&item);
        let status = item.get("status").map(plain_text).unwrap_or_default().to_lowercase();
        attempted.extend(found.iter().copied());
        if !matches!(status.as_str(), "failed" | "error" | "cancelled") {
            completed.extend(found);
        }
    }

    (attempted, completed)
}

// _fold_toolprune_codex, with CodexUntoolProfile reduced to what decides a live proposal: a package loaded (not applied)
// in a session counts that session and its prompts; one completed use keeps it for good; it is proposed once the profile
// would have demoted it (MIN_LOADED_EXPOSURES loaded sessions) and `enough_evidence` holds. The profile's probes and
// task-text hints steer a harness's next mask, which a plugin does not launch, so they are not kept.
fn fold_codex(ctx: &Ctx, rollout: &Path, prompts: Option<i64>) -> Value {
    let config = &ctx.config;
    let host = Host::Codex;
    if !rollout.exists() || rollout.extension().is_some_and(|ext| ext == "zst") {
        return Value::Null;
    }
    let prior = ctx.store.read("untool.json").filter(python_truthy).unwrap_or_else(|| json!({}));
    let applied = denylist::applied_for(&ctx.store, &ctx.repo.root, host);
    let kept = denylist::kept_for(&ctx.store, &ctx.repo.root, host);
    let (attempted, completed) = codex_usage(rollout);
    let mut capabilities = Map::new();
    let mut proposal = Vec::new();
    for capability in denylist::codex_capabilities() {
        let before = prior["capabilities"].get(&capability).cloned().unwrap_or_else(|| json!({}));
        let number = |key: &str| before.get(key).map_or(0, count);
        let c = capability.as_str();
        let loaded = !applied.contains(&capability);
        let used = before.get("used").is_some_and(python_truthy) || completed.contains(c);
        let (sessions, prompts_seen) = if loaded { (number("loaded_sessions") + 1, number("loaded_prompts") + prompts.unwrap_or(1)) } else { (number("loaded_sessions"), number("loaded_prompts")) };
        let unused = number("unused_sessions") + i64::from(loaded && !attempted.contains(c));
        let needed = CODEX_MIN_LOADED.iter().find(|(name, _)| *name == c).map_or(1, |(_, n)| *n);
        if !used && !applied.contains(&capability) && !kept.contains(&capability) && sessions >= needed && enough_evidence(sessions, prompts_seen, config, c == CODEX_HIGH_RISK) {
            proposal.push(capability.clone());
        }
        capabilities.insert(capability, json!({"loaded_sessions": sessions, "loaded_prompts": prompts_seen, "unused_sessions": unused, "used": used}));
    }
    let earlier = names(prior.get("proposal"));
    let new_in_proposal: Vec<&String> = proposal.iter().filter(|c| !earlier.contains(*c)).collect();
    let mut doc = json!({
        "sessions": prior.get("sessions").map_or(0, count) + 1, "catalog_source": "capabilities",
        "prompts": prior.get("prompts").map_or(0, count) + prompts.unwrap_or(0),
        "capabilities": capabilities, "proposal": proposal, "updated": now(), "new_in_proposal": new_in_proposal,
    });
    hold(ctx, &mut doc, &prior, &proposal);
    let _ = ctx.store.write("untool.json", &doc);

    json!({"proposal": doc["proposal"], "pending": doc.get("auto_pending").is_some(), "restored": []})
}

// fold._hold: under `auto`, the proposal waits for `apply_pending`, from when it first waited; a refusal to write the file stands until a write succeeds.
fn hold(ctx: &Ctx, doc: &mut Value, prior: &Value, proposal: &[String]) {
    if ctx.config.text("toolprune") != "auto" || proposal.is_empty() || ctx.config.shadow {
        return;
    }
    let since = prior.get("auto_pending").and_then(|pending| pending.get("since")).filter(|since| python_truthy(since)).cloned().unwrap_or_else(|| json!(now()));
    doc["auto_pending"] = json!({"items": proposal, "root": ctx.repo.root, "since": since});
    if let Some(refused) = prior.get("auto_apply_refused").filter(|refused| python_truthy(refused)) {
        doc["auto_apply_refused"] = refused.clone();
    }
}

// Tool pruning's evidence: the sessions a tool went unused and the prompts those sessions ran, weighed by `enough_evidence`. A list applies from a later session.
// Codex learns capability packages from its rollout (`transcript`); Copilot learns tools as Claude Code does, and proposes whole MCP servers.
pub fn fold_session(ctx: &Ctx, run_dir: &Path, _session_id: &str, transcript: Option<&str>, _workdir: &Path, prompts: Option<i64>) -> Value {
    fold_for(ctx, crate::hosts::current(), run_dir, transcript, prompts)
}

fn fold_for(ctx: &Ctx, host: Host, run_dir: &Path, transcript: Option<&str>, prompts: Option<i64>) -> Value {
    let config = &ctx.config;
    if config.text("toolprune") == "off" {
        return Value::Null;
    }
    if host == Host::Codex {
        return transcript.map_or(Value::Null, |rollout| fold_codex(ctx, Path::new(rollout), prompts));
    }
    let st = ctx.store.read("untool.json").filter(python_truthy).unwrap_or_else(|| json!({}));
    let evidence = config.number("toolprune_min_sessions").max(1);
    let mut used = names(st.get("used_tools"));
    let mut streaks = counts(st.get("streaks"));
    // A session from before prompts were counted counts as one prompt.
    let mut prompt_streaks = if st.get("prompt_streaks").is_some_and(python_truthy) { counts(st.get("prompt_streaks")) } else { streaks.clone() };
    let mut catalog_tools = names(st.get("catalog_tools"));
    let mut catalog_desc: Map<String, Value> = st.get("catalog_desc").and_then(Value::as_object).cloned().unwrap_or_default();
    let mut applied = denylist::applied_for(&ctx.store, &ctx.repo.root, host);
    let kept: BTreeSet<String> = denylist::kept_for(&ctx.store, &ctx.repo.root, host).into_iter().collect();
    // Whether this session ran under a mask: a masked tool list is not the full one, so no catalog is read from it.
    let masked = !applied.is_empty();
    let mut protect = config.list("toolprune_protect");
    if host == Host::Copilot {
        protect.extend(COPILOT_PROTECTED.iter().map(|name| name.to_string()));
    }
    let protected = Protected(protect);
    // A tool that has since become protected comes back: the plugin's OWN deny rule for it is removed, never one the person wrote.
    let mut restored = Vec::new();
    if applied.iter().any(|tool| protected.has(tool)) && !config.shadow {
        let back: Vec<String> = applied.iter().filter(|tool| protected.has(tool)).cloned().collect();
        restored = denylist::restore(&ctx.repo.root, &ctx.store, &back, host).unwrap_or_default();
    }
    if !restored.is_empty() {
        applied = denylist::applied_for(&ctx.store, &ctx.repo.root, host);
    }

    let raw_log = transcript.map(Path::new);
    let transcript = realpath(&run_dir.join("transcript.jsonl"));
    let task_used: BTreeSet<String> = claude_tool_usage(&transcript).into_keys().collect();
    used.extend(task_used.iter().cloned());
    let mut source = None;
    if !masked {
        let probed = if host == Host::Claude { load_catalog(&ctx.root) } else { Vec::new() };
        let mut seen: BTreeSet<String> = probed.iter().cloned().collect();
        seen.extend(task_used.iter().cloned());
        match host {
            Host::Claude => seen.extend(claude_deferred_tools(&transcript)),
            // Some builds record the request's tool list in `session.usage_checkpoint` (adapter.parse_catalog_with_source).
            _ => seen.extend(raw_log.map(copilot_loaded_tools).unwrap_or_default()),
        }
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
    // Copilot's only persistent lever is a whole MCP server.
    let servers = (host == Host::Copilot).then(|| copilot_mcp_servers(&crate::hosts::copilot_home()));
    let raw: Vec<String> = match &servers {
        Some(servers) => copilot_server_proposal(&raw.into_iter().collect(), &catalog_tools, &used, servers),
        None => raw.into_iter().cloned().collect(),
    };
    let proposal: Vec<String> = raw.into_iter().filter(|item| !applied.contains(item) && !kept.contains(item)).collect();
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
    hold(ctx, &mut doc, &st, &proposal);
    if let Some(servers) = &servers {
        doc["servers"] = server_summary(&catalog_tools, &used, &streaks, &prompt_streaks, servers);
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
    let host = crate::hosts::current();
    let mut done = denylist::applied_for(&ctx.store, &root, host);
    done.extend(denylist::kept_for(&ctx.store, &root, host));
    let items: Vec<String> = pending.get("items").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).filter(|item| !done.iter().any(|known| known == item)).map(str::to_string).collect();
    if !items.is_empty() && ctx.config.text("toolprune") == "auto" && !ctx.config.shadow {
        let idle = now_s - last_activity(ctx);
        let quiet = ctx.config.number("toolprune_quiet_minutes") as f64 * 60.0;
        let waited = now_s - pending.get("since").and_then(Value::as_f64).filter(|since| *since != 0.0).unwrap_or(now_s);
        if idle < quiet && waited < PENDING_MAX_S {
            return Some((quiet - idle).min(PENDING_MAX_S - waited));
        }
        match denylist::apply(&root, &items, &ctx.store, host) {
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

// A waiter killed with its session (a closed terminal, a reboot) would leave the pending list waiting for good: the
// fold only starts one when a proposal is new. Each session start starts another while a list waits and no heartbeat is fresh.
pub fn revive_waiter(ctx: &Ctx) {
    let is_waiting = ctx.store.read("untool.json").and_then(|unt| unt.get("auto_pending").cloned()).is_some_and(|pending| python_truthy(&pending));
    let is_alive = mtime(&ctx.store.base.join("toolprune-waiter")).is_some_and(|wrote| now() - wrote < 2.0 * WAITER_POLL_S);
    if is_waiting && !is_alive && std::env::var("TE_FOLD_SYNC").as_deref() != Ok("1") {
        spawn_waiter(ctx);
    }
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
    let host = crate::hosts::current();
    let mut mine = denylist::applied_for(&ctx.store, &ctx.repo.root, host);
    mine.extend(denylist::kept_for(&ctx.store, &ctx.repo.root, host));

    unt.get("proposal").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).filter(|tool| !mine.iter().any(|known| known == tool)).map(str::to_string).collect()
}

fn dash(items: &[String]) -> String {
    if items.is_empty() { "-".to_string() } else { items.join(", ") }
}

fn show(ctx: &Ctx, unt: &Value) -> i32 {
    let host = crate::hosts::current();
    let shown = |key: &str| unt.get(key).map_or_else(|| "0".to_string(), plain_text);
    let source = unt.get("catalog_source").filter(|source| python_truthy(source)).map_or_else(|| "none yet".to_string(), plain_text);
    println!("sessions observed: {} ({} prompts)  catalog: {source}", shown("sessions"), shown("prompts"));
    let unit = denylist::unit(host);
    println!("applied ({} this plugin added to {}): {}", unit.strip_suffix('y').map_or_else(|| format!("{unit}s"), |stem| format!("{stem}ies")), denylist::settings_path(&ctx.repo.root, host).display(), dash(&denylist::applied_for(&ctx.store, &ctx.repo.root, host)));
    let proposal = proposal_of(ctx, unt);
    println!("proposed: {}", dash(&proposal));
    let waiting = unt.get("auto_pending").and_then(|pending| pending.get("items")).and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).any(|tool| proposal.iter().any(|known| known == tool));
    if waiting && ctx.config.text("toolprune") == "auto" {
        println!("pending: written once this repository's sessions have been idle {} minutes, so that no conversation you resume re-reads its cached prompt (`toolprune apply` writes it now)", ctx.config.number("toolprune_quiet_minutes"));
    }
    let kept = denylist::kept_for(&ctx.store, &ctx.repo.root, host);
    if !kept.is_empty() {
        println!("kept, as you gave them back (`toolprune apply NAME` removes one again): {}", kept.join(", "));
    }
    let untrusted = denylist::trusted(host, &ctx.repo.root) == Some(false);
    if host == Host::Codex {
        if let Some(capabilities) = unt.get("capabilities").and_then(Value::as_object).filter(|caps| !caps.is_empty()) {
            println!("capabilities (removable from a project's .codex/config.toml):");
            for (name, entry) in capabilities {
                let number = |key: &str| entry.get(key).map_or(0, count);
                println!("  {name:14} loaded in {:3} sessions ({:3} prompts), unused in {:3}{}", number("loaded_sessions"), number("loaded_prompts"), number("unused_sessions"), if entry.get("used").is_some_and(python_truthy) { "  USED (kept)" } else { "" });
            }
        }
        if untrusted {
            println!("note: Codex reads a project's .codex/config.toml only in a TRUSTED project; trust this one in Codex first, or `toolprune apply` refuses.");
        }
        return 0;
    }
    if host == Host::Copilot {
        if let Some(servers) = unt.get("servers").and_then(Value::as_object).filter(|servers| !servers.is_empty()) {
            println!("MCP servers (Copilot can only remove a whole server):");
            for (name, entry) in servers {
                let state = if entry.get("used").is_some_and(python_truthy) { "USED (kept)".to_string() } else { format!("unused for {} sessions ({} prompts)", entry.get("unused_sessions").map_or(0, count), entry.get("unused_prompts").map_or(0, count)) };
                println!("  {name:24} {:3} tools, {state}", entry.get("tools").map_or(0, count));
            }
        }
        if untrusted {
            println!("note: Copilot reads a repository's .github/copilot settings only in a trusted folder; trust this one in Copilot first, or `toolprune apply` refuses.");
        }
    }
    if let Some(streaks) = unt.get("streaks").and_then(Value::as_object).filter(|streaks| !streaks.is_empty() && host == Host::Claude) {
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
    if host == Host::Copilot {
        // shortcut: no `refresh-catalog` on Copilot (upstream captures a `copilot -p` request on localhost); the catalog is what sessions loaded or used.
        if source.is_empty() || source == "observed" {
            println!("note: only the tools Copilot sessions recorded loading or used are known.");
        }
    } else if source.is_empty() || source == "observed" {
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
    let Ok(mut command) = crate::sidecall::command(cli) else { return (Vec::new(), Value::Null) };
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
    let out = crate::sidecall::command(cli)?.args(["-p", "Reply with exactly: OK", "--output-format", "json", "--model", "haiku", "--max-turns", "1"]).args(flags).args(extra).env("TE_SIDECALL", "1").current_dir(&cwd).stdin(Stdio::null()).output();
    let _ = fs::remove_dir_all(&cwd);
    let out = out.map_err(|e| e.to_string())?;
    let data: Value = serde_json::from_slice(&out.stdout).ok().filter(Value::is_object).unwrap_or_else(|| json!({}));
    let usage = data.get("usage").cloned().unwrap_or(Value::Null);
    let tokens: i64 = ["input_tokens", "cache_read_input_tokens", "cache_creation_input_tokens"].iter().map(|key| usage.get(key).map_or(0, count)).sum();
    // A failed probe would otherwise count as 0 tokens, and the whole request as saved.
    if !out.status.success() || python_truthy(data.get("is_error").unwrap_or(&Value::Null)) || tokens == 0 {
        let what = if out.status.success() { "reported no usage".to_string() } else { format!("exited {}", out.status.code().unwrap_or(-1)) };
        let said = String::from_utf8_lossy(if out.stderr.is_empty() { &out.stdout } else { &out.stderr }).trim().chars().take(300).collect::<String>();
        return Err(format!("`claude -p` {what}: {}", if said.is_empty() { "no output" } else { &said }));
    }

    Ok((tokens, data.get("total_cost_usd").filter(|cost| python_truthy(cost)).cloned()))
}

// Prompt tokens of two small requests, with and without `tools` removed.
fn measure(tools: &[String]) -> Result<Value, String> {
    let cli = claude_cli();
    let flags = claude_flags(&cli);
    let (full, first) = prompt_tokens(&cli, &flags, &[])?;
    // The deny list as a settings file rather than `--disallowed-tools`: hundreds of names overflow cmd.exe's 8191-character
    // line, and bare-name deny rules remove the same tools, as `toolprune apply` writes them.
    let dir = scratch_dir("te-deny-");
    let settings = dir.join("settings.json");
    fs::write(&settings, serde_json::to_string(&json!({"permissions": {"deny": tools}})).unwrap_or_default()).map_err(|e| e.to_string())?;
    let lean = prompt_tokens(&cli, &flags, &["--settings".to_string(), settings.to_string_lossy().into_owned()]);
    let _ = fs::remove_dir_all(&dir);
    let (lean, second) = lean?;
    let costs: Vec<Value> = [first, second].into_iter().flatten().collect();
    let cost = if costs.iter().all(|cost| cost.is_i64()) { json!(costs.iter().map(count).sum::<i64>()) } else { json!((costs.iter().filter_map(Value::as_f64).sum::<f64>() * 1e4).round() / 1e4) };

    Ok(json!({"tools": tools, "prompt_tokens_full": full, "prompt_tokens_lean": lean, "saved_per_request": full - lean, "probe_cost_usd": cost}))
}

// `te toolprune [show|apply|restore|refresh-catalog|measure|apply-pending] [ITEM ...]`
pub fn command(ctx: &Ctx, args: &[String]) -> i32 {
    let action = args.first().map_or("show", String::as_str);
    let items = args.get(1..).unwrap_or(&[]);
    let unt = ctx.store.read("untool.json").filter(python_truthy).unwrap_or_else(|| json!({}));
    let host = crate::hosts::current();
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
            let codex = denylist::codex_capabilities();
            let bad: Vec<&String> = tools.iter().filter(|tool| host == Host::Codex && !codex.contains(tool)).collect();
            if !bad.is_empty() {
                println!("not a removable Codex capability: {} (choose from {})", bad.iter().map(|tool| tool.as_str()).collect::<Vec<_>>().join(", "), codex.join(", "));
                return 2;
            }
            let added = match denylist::apply(&ctx.repo.root, &tools, &ctx.store, host) {
                Ok(added) => added,
                Err(why) => {
                    println!("not applied: {why}");
                    return 1;
                }
            };
            // Named by the person: no longer one they gave back, whatever `restore` said before.
            if let Err(why) = denylist::set_kept(&ctx.store, &ctx.repo.root, host, &[], &tools) {
                println!("not applied: {why}");
                return 1;
            }
            println!("added: {}\nin {} -- takes effect from the next session.", if added.is_empty() { "(already present)".to_string() } else { added.join(", ") }, denylist::settings_path(&ctx.repo.root, host).display());
            0
        }
        "restore" => {
            let removed = match denylist::restore(&ctx.repo.root, &ctx.store, items, host) {
                Ok(removed) => removed,
                Err(why) => {
                    println!("not restored: {why}");
                    return 1;
                }
            };
            // Given back by the person: `toolprune: auto` must not take them away again at the next fold.
            if let Err(why) = denylist::set_kept(&ctx.store, &ctx.repo.root, host, &removed, &[]) {
                println!("not restored: {why}");
                return 1;
            }
            let note = removed.first().map_or_else(String::new, |first| format!("; kept from now on (`toolprune apply {first}` removes one again)"));
            println!("removed: {} (from the next session){note}", dash(&removed));
            0
        }
        "apply-pending" => wait_and_apply(ctx),
        "refresh-catalog" if host == Host::Codex => {
            println!("Codex needs no catalog: its removable capabilities are a fixed set ({}), read from each session.", denylist::codex_capabilities().join(", "));
            0
        }
        // shortcut: upstream's Copilot probe and measurement capture a `copilot -p` request through a localhost proxy; not ported, so both stay Claude Code's.
        "refresh-catalog" | "measure" if host != Host::Claude => {
            println!("toolprune {action}: Claude Code only in the dam; under {} the evidence comes from each session's own log", crate::channels::host_name(host));
            1
        }
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
                tools = denylist::applied_for(&ctx.store, &ctx.repo.root, host);
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
    fn an_unused_tool_is_proposed_after_enough_sessions_and_not_held_under_propose() {
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
        // The default is `propose`: nothing waits to be written without the person's yes.
        assert_eq!(last["pending"], json!(false));
        let doc = ctx.store.read("untool.json").unwrap();
        assert_eq!(doc["sessions"], 5);
        assert_eq!(doc["streaks"]["Foo"], 5);
        assert_eq!(doc["streaks"]["Bash"], 0);
        assert_eq!(doc["catalog_source"], "probe+observed");
        let _ = fs::remove_dir_all(&dir);
    }

    fn lines_file(path: &Path, lines: &[Value]) {
        fs::write(path, lines.iter().map(Value::to_string).collect::<Vec<_>>().join("\n")).unwrap();
    }

    fn live_ctx(dir: &Path) -> Ctx {
        let repo = RepoId { root: dir.join("repo").to_string_lossy().into_owned(), key: "k".into(), label: "l".into(), remote: String::new(), how: "path".into() };
        Ctx::with_repo(dir.to_path_buf(), repo)
    }

    #[test]
    fn codex_capabilities_are_learned_from_a_rollout() {
        let dir = std::env::temp_dir().join(format!("dam-toolprune-codex-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let rollout = dir.join("rollout-1.jsonl");
        lines_file(&rollout, &[
            json!({"type": "session_meta", "payload": {"id": "s", "cwd": "/r"}}),
            json!({"type": "event_msg", "payload": {"type": "task_started"}}),
            // Code mode: the image is viewed through `exec`, and a sub-agent spawn failed.
            json!({"type": "response_item", "payload": {"type": "custom_tool_call", "name": "exec", "call_id": "c1", "input": "const shot = await tools.view_image({path: 'a.png'});", "status": "completed"}}),
            json!({"type": "response_item", "payload": {"type": "function_call", "name": "spawn_agent", "call_id": "c2", "arguments": "{}", "status": "failed"}}),
            json!({"type": "event_msg", "payload": {"type": "item_completed", "item": {"type": "CommandExecution", "id": "c3", "command": ["ls"], "exit_code": 0}}}),
        ]);
        let ctx = live_ctx(&dir);
        let log = crate::hosts::claude_log(Host::Codex, &rollout, &dir).unwrap();
        let prompts = crate::trace::claude_prompt_count(&log);
        assert_eq!(prompts, 1);
        let mut last = Value::Null;
        for session in 1..=5 {
            last = fold_for(&ctx, Host::Codex, &dir, rollout.to_str(), Some(prompts));
            if session < 5 {
                assert_eq!(last["proposal"], json!([]), "session {session}");
            }
        }
        assert_eq!(last["proposal"], json!(["collaboration", "goals"]));
        assert_eq!(last["pending"], json!(false));
        let doc = ctx.store.read("untool.json").unwrap();
        assert_eq!((doc["sessions"].as_i64(), doc["catalog_source"].as_str()), (Some(5), Some("capabilities")));
        assert_eq!(doc["capabilities"]["image_media"]["used"], true);
        assert_eq!(doc["capabilities"]["collaboration"], json!({"loaded_sessions": 5, "loaded_prompts": 5, "unused_sessions": 0, "used": false}));
        assert_eq!(doc["capabilities"]["goals"]["unused_sessions"], 5);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copilot_proposes_whole_unused_mcp_servers_from_its_events_log() {
        let dir = std::env::temp_dir().join(format!("dam-toolprune-copilot-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let events = dir.join("events.jsonl");
        let tools = json!([{"name": "bash"}, {"name": "report_intent"}, {"name": "github-mcp-server-list_issues"}, {"name": "github-mcp-server-get_me"}]);
        lines_file(&events, &[
            json!({"type": "session.start", "data": {"context": {"cwd": "/r"}}}),
            json!({"type": "user.message", "data": {"content": "fix it"}}),
            json!({"type": "session.usage_checkpoint", "data": {"promptCacheBreakState": [{"models": {"gpt-5": {"tools": tools}}}]}}),
            json!({"type": "tool.execution_start", "data": {"toolCallId": "t1", "toolName": "bash", "arguments": {"command": "npm test"}}}),
            json!({"type": "tool.execution_complete", "data": {"toolCallId": "t1", "success": true, "result": {"content": "ok"}}}),
        ]);
        let log = crate::hosts::claude_log(Host::Copilot, &events, &dir).unwrap();
        let run = crate::fold::run_dir(&dir, &log).unwrap();
        let ctx = live_ctx(&dir);
        let mut last = Value::Null;
        for _ in 0..5 {
            last = fold_for(&ctx, Host::Copilot, &run, events.to_str(), Some(1));
        }
        assert_eq!(last["proposal"], json!(["github-mcp-server"]));
        let doc = ctx.store.read("untool.json").unwrap();
        assert_eq!(doc["servers"]["github-mcp-server"], json!({"tools": 2, "used": false, "unused_sessions": 5, "unused_prompts": 5}));
        assert_eq!(doc["streaks"]["Bash"], 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn copilot_tools_belong_to_their_longest_server() {
        let servers: BTreeSet<String> = ["github", "github-mcp-server", "db"].iter().map(|name| name.to_string()).collect();
        assert_eq!(mcp_server_of("github-mcp-server-get_me", &servers).map(String::as_str), Some("github-mcp-server"));
        assert_eq!(mcp_server_of("dbx-query", &servers), None);
        let catalog: BTreeSet<String> = ["db-query", "db-write", "github-x"].iter().map(|name| name.to_string()).collect();
        let (query, write, x) = ("db-query".to_string(), "db-write".to_string(), "github-x".to_string());
        // A server goes only when every one of its tools is in the evidence and none was used.
        assert_eq!(copilot_server_proposal(&[&query, &x].into_iter().collect(), &catalog, &BTreeSet::new(), &servers), vec!["github".to_string()]);
        assert_eq!(copilot_server_proposal(&[&query, &write].into_iter().collect(), &catalog, &BTreeSet::from([write.clone()]), &servers), Vec::<String>::new());
    }
}
