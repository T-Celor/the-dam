// `te` for Claude Code (Apache-2.0, Microsoft), rewritten from plugin/cli.py: status, microskills, toolprune, config, fold, judge, repos
// and doctor. What the repository map, the probe and moving state between repositories did is gone, and says so.
use crate::config::{canonical, plain_text, repr_text, unset_config, write_config, Config};
use crate::context::Ctx;
use crate::economizer::{dumps, env};
use crate::paths::{abspath, find_repo_root, realpath};
use crate::{fold, judge, sidecall, status};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const HOSTS: [&str; 5] = ["claude", "codex", "copilot", "opencode", "pi"];
const OBSOLETE_REPOMAP: &str = "the repository map was removed (it needed aider and was off by default)";

#[derive(Default)]
struct Args {
    cwd: Option<PathBuf>,
    host: Option<String>,
    words: Vec<String>,
    sessions: Vec<String>,
    job: Option<String>,
    flags: BTreeSet<String>,
}

impl Args {
    fn flag(&self, name: &str) -> bool {
        self.flags.contains(name)
    }

    fn cwd(&self) -> PathBuf {
        self.cwd.clone().or_else(|| std::env::current_dir().ok()).unwrap_or_else(|| PathBuf::from("."))
    }
}

fn parse(args: &[String]) -> Result<Args, String> {
    let mut out = Args::default();
    let mut words = args.iter();
    while let Some(word) = words.next() {
        let mut value = |name: &str| words.next().cloned().ok_or_else(|| format!("te: argument {name}: expected one argument"));
        match word.as_str() {
            "--cwd" => out.cwd = Some(PathBuf::from(value("--cwd")?)),
            "--host" => out.host = Some(value("--host")?),
            "--session" => out.sessions.push(value("--session")?),
            "--job" => out.job = Some(value("--job")?),
            "--json" | "--global" | "--quiet" | "--verbose" => {
                out.flags.insert(word.trim_start_matches('-').to_string());
            }
            _ => out.words.push(word.clone()),
        }
    }

    Ok(out)
}

// A command that changes something checks first that it can write: no half a change.
fn refuse_unwritable(dir: &Path) -> bool {
    let probe = dir.join(format!(".write-test-{}", std::process::id()));
    let why = fs::create_dir_all(dir).and_then(|()| fs::write(&probe, "")).and_then(|()| fs::remove_file(&probe));
    match why {
        Ok(()) => false,
        Err(error) => {
            let argv: Vec<String> = std::env::args().skip(1).collect();
            println!("not changed: TokenEconomizer cannot write its state in {} ({error}). Inside a sandboxed agent, run this command with permission to write outside the workspace, or run it from a terminal as `~/.tokeneconomizer/bin/te {}`.", dir.display(), if argv.is_empty() { "...".to_string() } else { argv.join(" ") });
            true
        }
    }
}

// The repository a recorded session is pinned to, when it holds the session's record, else the one `--cwd` names.
fn session_ctx(cwd: &Path, session: Option<&str>) -> Ctx {
    let ctx = Ctx::for_cwd(cwd);
    if let Some(repo) = session.and_then(|id| status::pinned(&ctx.root, id).map(|repo| (id, repo))) {
        let pinned = Ctx::with_repo(ctx.root.clone(), repo.1);
        if pinned.store.base.join("sessions").join(format!("{}.json", repo.0)).exists() {
            return pinned;
        }
    }

    ctx
}

fn config_view(config: &Config) -> Value {
    let mut other = config.as_dict().as_object().cloned().unwrap_or_default();
    let mut take = |key: &str| other.shift_remove(key).unwrap_or(Value::Null);
    let pinned = take("loopguard_thresholds");
    let mut thresholds = crate::loopguard::scaled_thresholds(&config.aggr);
    let mut names: Vec<String> = Vec::new();
    for (name, value) in pinned.as_object().into_iter().flatten() {
        thresholds.insert(name.clone(), value.clone());
        names.push(name.clone());
    }
    names.sort();
    let state = |mode: &str| if mode == "off" { "off" } else { "on" };
    let view = json!({
        "enabled": take("enabled"),
        "microskills": {"state": state(config.text("microskills")), "microskills": take("microskills"), "microskills_max_facts": take("microskills_max_facts"), "microskills_max_tokens": take("microskills_max_tokens"), "microskills_propose_every": take("microskills_propose_every")},
        "toolprune": {"state": state(config.text("toolprune")), "toolprune": take("toolprune"), "toolprune_evidence": take("toolprune_evidence"), "toolprune_min_sessions": take("toolprune_min_sessions"), "toolprune_sessions": take("toolprune_sessions"), "toolprune_quiet_minutes": take("toolprune_quiet_minutes"), "toolprune_protect": take("toolprune_protect")},
        "loopguard": {"state": state(&config.loopguard), "loopguard": take("loopguard"), "loopguard_aggr": take("loopguard_aggr"), "loopguard_nudges": take("loopguard_nudges"), "thresholds": thresholds, "pinned": names},
    });
    let Value::Object(mut view) = view else { unreachable!("an object above") };
    view.insert("other".to_string(), Value::Object(other));
    view.insert("warnings".to_string(), json!(config.warnings));

    Value::Object(view)
}

fn cmd_config(args: &Args, ctx: &Ctx) -> i32 {
    let action = args.words.get(1).map_or("show", String::as_str);
    let pairs = args.words.get(2..).unwrap_or(&[]);
    if action == "show" {
        println!("{}", status::pretty(&config_view(&ctx.config)));
        return 0;
    }
    if !["set", "unset"].contains(&action) {
        eprintln!("te config: invalid choice: {} (choose from 'show', 'set', 'unset')", repr_text(action));
        return 2;
    }
    let path = if args.flag("global") { ctx.root.join("config.json") } else { ctx.repo_dir().join("config.json") };
    if path.parent().is_some_and(refuse_unwritable) {
        return 1;
    }
    if action == "unset" {
        let keys: Vec<String> = pairs.iter().map(|key| key.trim().to_string()).collect();
        if keys.iter().any(|key| canonical(key).starts_with("repomap")) {
            println!("rejected: {OBSOLETE_REPOMAP}");
            return 2;
        }
        return match unset_config(&path, &keys) {
            Ok(current) => {
                println!("{}: {}", path.display(), dumps(&Value::Object(current), None));
                0
            }
            Err(why) => {
                println!("rejected: {why}");
                2
            }
        };
    }
    let mut updates: Vec<(String, String)> = Vec::new();
    for pair in pairs {
        let Some((key, value)) = pair.split_once('=') else {
            println!("expected KEY=VALUE, got {}", repr_text(pair));
            return 2;
        };
        match updates.iter_mut().find(|(known, _)| known == key.trim()) {
            Some(entry) => entry.1 = value.trim().to_string(),
            None => updates.push((key.trim().to_string(), value.trim().to_string())),
        }
    }
    if updates.is_empty() {
        println!("nothing to set: give KEY=VALUE (see `te config show` for the keys)");
        return 2;
    }
    if updates.iter().any(|(key, _)| canonical(key).starts_with("repomap")) {
        println!("rejected: {OBSOLETE_REPOMAP}");
        return 2;
    }
    let current = match write_config(&path, &updates) {
        Ok(current) => current,
        Err(why) => {
            println!("rejected: {why}");
            return 2;
        }
    };
    println!("{}: {}", path.display(), dumps(&Value::Object(current.clone()), None));
    println!("applies from the next hook call; a new fact-size cap applies once the next session has been folded");
    if updates.iter().any(|(key, _)| ["microskills", "loopguard"].contains(&canonical(key))) && sidecall::resume(&ctx.root) {
        println!("the plugin's model calls through `claude -p` will be tried again");
    }
    if current.get("microskills").map_or(String::new(), plain_text).contains("distiller") {
        let every = current.get("microskills_propose_every").filter(|every| crate::economizer::python_truthy(every)).map_or_else(|| ctx.config.number("microskills_propose_every").to_string(), plain_text);
        println!("note: micro-skills calls a model through this agent every {every} sessions to propose facts, charged to the session's cost; `microskills=envfacts` learns without model calls");
    }
    if current.get("loopguard").and_then(Value::as_str) == Some("stop") {
        println!("note: `stop` acts as `nudge`: the plugin never stops a turn; a turn still stuck after the redirect gets a notice saying how to stop it");
    }

    0
}

fn cmd_fold(args: &Args) -> i32 {
    // A session is folded into the repository it is pinned to, wherever `--cwd` points.
    let ctx = session_ctx(&args.cwd(), args.sessions.first().map(String::as_str));
    let sessions = if args.sessions.is_empty() { fold::pending_sessions(&ctx, &[]) } else { args.sessions.clone() };
    for session in &sessions {
        match fold::fold_session(&ctx, session, None) {
            Ok(Some(record)) if args.flag("verbose") => println!("{}", dumps(&record, None)),
            Ok(_) => {}
            Err(error) => eprintln!("te fold: {session}: {error}"),
        }
    }

    0
}

fn mtime(path: &Path) -> Option<f64> {
    Some(fs::metadata(path).ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs_f64())
}

fn read_json(path: &Path) -> Value {
    fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or(Value::Null)
}

// The repository name a state directory was learned under.
fn dir_label(dir: &Path) -> String {
    if let Some(label) = read_json(&dir.join("repo.json")).get("label").and_then(Value::as_str).filter(|label| !label.is_empty()) {
        return label.to_string();
    }
    let hosts: Vec<PathBuf> = HOSTS.iter().map(|host| dir.join(host)).filter(|path| path.is_dir()).collect();
    for host in &hosts {
        for sub in ["envfacts_state", "manual_state"] {
            let mut names: Vec<String> = fs::read_dir(host.join("mechanisms").join(sub)).into_iter().flatten().flatten().filter_map(|entry| entry.file_name().into_string().ok()).filter(|name| name.ends_with(".json")).collect();
            names.sort();
            if let Some(name) = names.first() {
                return name.trim_end_matches(".json").replace("__", "/");
            }
        }
        let ledger = crate::economizer::Store { base: host.clone() }.read_lines("ledger.jsonl");
        if let Some(label) = ledger.iter().rev().find_map(|record| record.get("repo").and_then(Value::as_str).filter(|label| !label.is_empty())) {
            return label.to_string();
        }
    }

    String::new()
}

fn home() -> Option<PathBuf> {
    let home = env("HOME").or_else(|| env("USERPROFILE"))?;

    Some(realpath(Path::new(&home)))
}

// One row per state directory: identity, agents and sessions, checkouts, notes.
fn listing(root: &Path, current: &str) -> (Vec<Value>, usize) {
    let index = read_json(&root.join("index.json"));
    let mut checkouts: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (path, entry) in index.get("checkouts").and_then(Value::as_object).into_iter().flatten() {
        checkouts.entry(entry.get("key").and_then(Value::as_str).unwrap_or("").to_string()).or_default().push(path.clone());
    }
    let home = home();
    let mut keys: Vec<String> = fs::read_dir(root.join("repos")).into_iter().flatten().flatten().filter_map(|entry| entry.file_name().into_string().ok()).collect();
    keys.sort();
    let mut rows: Vec<(bool, f64, Value)> = Vec::new();
    for key in keys {
        let dir = root.join("repos").join(&key);
        if !dir.is_dir() {
            continue;
        }
        let doc = read_json(&dir.join("repo.json"));
        let (mut agents, mut cwds, mut last) = (Map::new(), BTreeSet::new(), 0.0_f64);
        for host in HOSTS.iter().filter(|host| dir.join(host).is_dir()) {
            let base = dir.join(host);
            let mut sessions: BTreeMap<String, Value> = BTreeMap::new();
            let mut names: Vec<String> = fs::read_dir(base.join("sessions")).into_iter().flatten().flatten().filter_map(|entry| entry.file_name().into_string().ok()).collect();
            names.sort();
            for name in names.iter().filter(|name| name.ends_with(".json")) {
                let record = read_json(&base.join("sessions").join(name));
                sessions.insert(name.trim_end_matches(".json").to_string(), if record.is_object() { record } else { json!({}) });
            }
            for id in read_json(&base.join("folded.json")).as_object().into_iter().flatten().map(|(id, _)| id) {
                sessions.entry(id.clone()).or_insert_with(|| json!({}));
            }
            agents.insert((*host).to_string(), json!(sessions.len()));
            for name in ["ledger.jsonl", "sessions"] {
                last = last.max(mtime(&base.join(name)).unwrap_or(0.0));
            }
            cwds.extend(sessions.values().filter_map(|record| record.get("cwd").and_then(Value::as_str).filter(|cwd| !cwd.is_empty()).map(str::to_string)));
        }
        let mut places = checkouts.remove(&key).unwrap_or_default();
        places.sort();
        let mut notes = Vec::new();
        if home.as_ref().is_some_and(|home| places.iter().any(|place| realpath(Path::new(place)) == *home)) {
            notes.push("your home directory: holds only sessions started in ~ itself".to_string());
        }
        let label = doc.get("label").and_then(Value::as_str).filter(|label| !label.is_empty()).map(str::to_string).or_else(|| Some(dir_label(&dir)).filter(|label| !label.is_empty())).unwrap_or_else(|| key.clone());
        let text = |name: &str| doc.get(name).and_then(Value::as_str).unwrap_or("").to_string();
        let is_current = key == current;
        rows.push((is_current, last, json!({"key": key, "label": label, "how": text("how"), "remote": text("remote"), "agents": agents, "last": last, "state": dir.to_string_lossy(), "checkouts": places, "cwds": cwds, "current": is_current, "notes": notes})));
    }
    rows.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.total_cmp(&a.1)));
    let merged = fs::read_dir(root.join("merged")).map_or(0, |entries| entries.count());

    (rows.into_iter().map(|row| row.2).collect(), merged)
}

fn how_words(how: &str, remote: &str) -> String {
    match how {
        "remote" => format!("by its origin remote ({remote})"),
        "path" => "by its path (a git repository with no remote)".to_string(),
        _ => "by its directory (not a git repository)".to_string(),
    }
}

fn when(at: f64) -> String {
    if at == 0.0 { "-".to_string() } else { crate::session::minutes(at) }
}

fn cmd_repos(args: &Args, ctx: &Ctx) -> i32 {
    let action = args.words.get(1).map_or("list", String::as_str);
    if action == "migrate" || action == "merge" {
        println!("`te repos {action}` was removed: state is no longer moved between repositories, and what an earlier version filed under another key stays where it is");
        return 1;
    }
    if action != "list" {
        eprintln!("te repos: invalid choice: {} (choose from 'list', 'migrate', 'merge')", repr_text(action));
        return 2;
    }
    let (rows, merged) = listing(&ctx.root, &ctx.repo.key);
    if args.flag("json") {
        println!("{}", status::pretty(&json!({"repos": rows, "merged": merged})));
        return 0;
    }
    if rows.is_empty() {
        println!("no state yet in {}", ctx.root.join("repos").display());
    }
    let text = |row: &Value, name: &str| row[name].as_str().unwrap_or("").to_string();
    for row in &rows {
        println!("{}  [{}]{}", text(row, "label"), text(row, "key"), if row["current"] == true { "  <- this directory" } else { "" });
        println!("  identified   {}", if text(row, "how").is_empty() { "recorded by an earlier version".to_string() } else { how_words(&text(row, "how"), &text(row, "remote")) });
        let agents: Vec<String> = row["agents"].as_object().into_iter().flatten().map(|(host, count)| format!("{host} {count}")).collect();
        println!("  sessions     {}; last {}", if agents.is_empty() { "none".to_string() } else { agents.join(", ") }, when(row["last"].as_f64().unwrap_or(0.0)));
        let list = |name: &str| -> Vec<String> { row[name].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_string).collect() };
        let (places, cwds) = (list("checkouts"), list("cwds"));
        if !places.is_empty() {
            println!("  checkouts    {}", places.join(", "));
        } else if !cwds.is_empty() {
            println!("  ran in       {}{}", cwds.iter().take(3).cloned().collect::<Vec<_>>().join(", "), if cwds.len() > 3 { format!(" (+{})", cwds.len() - 3) } else { String::new() });
        }
        for note in list("notes") {
            println!("  note         {note}");
        }
    }
    if merged > 0 {
        println!("\n{merged} earlier state director{} merged into the above, kept under {}", if merged == 1 { "y" } else { "ies" }, ctx.root.join("merged").display());
    }

    0
}

// `.git` entries between `cwd` and its checkout that git would not take for a repository.
fn ignored_git_entries(cwd: &Path) -> Vec<PathBuf> {
    let root = find_repo_root(cwd);
    let mut out = Vec::new();
    for dir in abspath(cwd).ancestors() {
        if root.as_deref() == Some(dir) {
            break;
        }
        let dot = dir.join(".git");
        if fs::symlink_metadata(&dot).is_ok() {
            out.push(dot);
        }
    }

    out
}

fn cmd_doctor(args: &Args, ctx: &Ctx) -> i32 {
    let mut ok = true;
    println!("program     dam-economizer {}", env!("CARGO_PKG_VERSION"));
    let root = &ctx.root;
    let probe = root.join(".write-test");
    match fs::create_dir_all(root).and_then(|()| fs::write(&probe, "")).and_then(|()| fs::remove_file(&probe)) {
        Ok(()) => println!("state root  {} (writable)", root.display()),
        Err(error) => {
            println!("state root  {} NOT writable: {error}", root.display());
            ok = false;
        }
    }
    let cli = sidecall::claude_cli();
    let found = Path::new(&cli).is_file() || sidecall::which(&cli).is_some();
    println!("agents      {}", if found { "claude" } else { "none of claude on PATH" });
    if found {
        if let Some(pause) = sidecall::paused(root) {
            println!("claude      model calls paused since {}: `claude -p` said it is not signed in; `config set microskills=on` (or `loopguard=on`) tries again", when(pause["at"].as_f64().unwrap_or(0.0)));
        }
        println!("claude      catalog {}", if root.join("hosts").join("claude").join("catalog.json").exists() { "present" } else { "not probed" });
    }
    let repo = &ctx.repo;
    println!("repository  {}: identified {}; state {}", repo.root, how_words(&repo.how, &repo.remote), repo.key);
    let cwd = args.cwd();
    let home_git = home().map(|home| home.join(".git"));
    for path in ignored_git_entries(&cwd) {
        if Some(realpath(&path)) != home_git {
            println!("repository  ignored {}: not a git repository (no HEAD, or a dangling gitdir)", path.display());
        }
    }
    if let Some(home) = home().filter(|home| fs::symlink_metadata(home.join(".git")).is_ok()) {
        if find_repo_root(&home).as_deref() == Some(home.as_path()) {
            println!("home        {} is a git repository; it is the repository of sessions started there, not of the unversioned directories below it", home.display());
        } else {
            println!("home        {}/.git exists but is not a git repository (git ignores it, and so does TokenEconomizer)", home.display());
        }
    }
    let log = root.join("errors.log");
    match fs::read(&log) {
        Ok(bytes) => {
            let text = String::from_utf8_lossy(&bytes);
            let lines: Vec<&str> = text.lines().collect();
            println!("errors.log (tail):\n  {}", lines[lines.len().saturating_sub(6)..].join("\n  "));
        }
        Err(_) => println!("errors.log  none"),
    }

    i32::from(!ok)
}

// The `te` command line: 0 for done, 2 for a line that is not understood.
pub fn main(raw: &[String]) -> i32 {
    let args = match parse(raw) {
        Ok(args) => args,
        Err(why) => {
            eprintln!("{why}");
            return 2;
        }
    };
    if args.host.as_deref().is_some_and(|host| host != "claude") {
        eprintln!("te: only Claude Code is supported (--host claude)");
        return 2;
    }
    let command = args.words.first().map_or("", String::as_str);
    match command {
        "status" => {
            println!("{}", status::report(&Ctx::for_cwd(&args.cwd()), args.flag("json")));
            0
        }
        "microskills" | "facts" => crate::microskills::command(&Ctx::for_cwd(&args.cwd())),
        "toolprune" | "untool" => crate::toolprune::command(&Ctx::for_cwd(&args.cwd()), args.words.get(1..).unwrap_or(&[])),
        "config" => cmd_config(&args, &Ctx::for_cwd(&args.cwd())),
        "fold" => cmd_fold(&args),
        "judge" => match (args.sessions.first(), &args.job) {
            (Some(session), Some(job)) => {
                judge::run_job(&session_ctx(&args.cwd(), Some(session)), session, job);
                0
            }
            _ => {
                eprintln!("te judge: the following arguments are required: --session, --job");
                2
            }
        },
        "doctor" => cmd_doctor(&args, &Ctx::for_cwd(&args.cwd())),
        "repos" => cmd_repos(&args, &Ctx::for_cwd(&args.cwd())),
        "repomap" => {
            println!("{OBSOLETE_REPOMAP}");
            1
        }
        "probe" => {
            println!("the probe was removed: it ran real Copilot sessions, and Copilot is no longer supported");
            2
        }
        _ => {
            eprintln!("usage: te [--cwd DIR] {{status,microskills,toolprune,config,fold,judge,doctor,repos}} ...");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_go_anywhere_and_words_stay_in_order() {
        let args = parse(&["config".into(), "--cwd".into(), "x".into(), "set".into(), "a=b".into(), "--global".into()]).unwrap();
        assert_eq!(args.words, ["config", "set", "a=b"]);
        assert!(args.flag("global"));
        assert_eq!(args.cwd, Some(PathBuf::from("x")));
        assert!(parse(&["--cwd".into()]).is_err());
    }

    #[test]
    fn a_setting_is_written_under_its_current_name_and_unset_again() {
        let dir = crate::session::TempDir::new("te-cli-test-").unwrap();
        let path = dir.0.join("config.json");
        write_config(&path, &[("spiral".into(), "off".into()), ("loopguard.min_turns".into(), "7".into())]).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "{\n  \"loopguard\": \"off\",\n  \"loopguard_thresholds\": {\n    \"min_turns\": 7\n  }\n}");
        assert!(write_config(&path, &[("loopguard".into(), "banana".into())]).is_err());
        assert!(write_config(&path, &[("paid".into(), "1".into())]).unwrap_err().contains("no longer exists"));
        unset_config(&path, &["spiral".into(), "loopguard.min_turns".into()]).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "{}");
        assert_eq!(unset_config(&path, &["nope".into()]).unwrap_err(), "unknown key(s): nope");
    }

    #[test]
    fn the_state_root_is_listed_with_its_sessions() {
        let dir = crate::session::TempDir::new("te-cli-test-").unwrap();
        let state = dir.0.join("repos").join("a-1").join("claude").join("sessions");
        fs::create_dir_all(&state).unwrap();
        fs::write(state.join("s1.json"), r#"{"cwd": "/w"}"#).unwrap();
        fs::write(dir.0.join("repos").join("a-1").join("repo.json"), r#"{"label": "o/a", "how": "path"}"#).unwrap();
        let (rows, merged) = listing(&dir.0, "a-1");
        assert_eq!((rows.len(), merged), (1, 0));
        assert_eq!(rows[0]["agents"], json!({"claude": 1}));
        assert_eq!(rows[0]["cwds"], json!(["/w"]));
        assert_eq!(rows[0]["label"], "o/a");
    }
}
