// Folding a finished session into the ledger (Apache-2.0, Microsoft), rewritten from plugin/fold.py (fold_session, _run_dir,
// pending_sessions, spawn_fold). What the micro-skills and tool pruning learn from it is theirs (`microskills`, `toolprune`).
use crate::context::Ctx;
use crate::economizer::{env, now, python_truthy};
use crate::session::TempDir;
use crate::sidecall;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

// An unfolded session whose transcript has been quiet this long is over.
const STALE_S: f64 = 600.0;
// A fold must not run without its lock: two folds of one session would count it twice. It can wait for a paid side call.
const LOCK_WAIT: Duration = Duration::from_secs(sidecall::TIMEOUT_S + 60);

pub fn run_dir(workdir: &Path, trace: &Path) -> std::io::Result<PathBuf> {
    let run = workdir.join("run");
    fs::create_dir_all(&run)?;
    let link = run.join("transcript.jsonl");
    #[cfg(windows)]
    let linked = std::os::windows::fs::symlink_file(trace, &link);
    #[cfg(unix)]
    let linked = std::os::unix::fs::symlink(trace, &link);
    if linked.is_err() {
        fs::copy(trace, &link)?;
    }

    Ok(run)
}

fn whole(value: Option<&Value>) -> i64 {
    value.and_then(|item| item.as_i64().or_else(|| item.as_f64().map(|n| n as i64))).unwrap_or(0)
}

// The ledger record, or None for a session already folded (or one whose fold another process holds).
pub fn fold_session(ctx: &Ctx, session: &str, transcript_path: Option<&str>) -> std::io::Result<Option<Value>> {
    let Some(_fold) = ctx.store.locked_within("fold", LOCK_WAIT) else { return Ok(None) };
    let mut folded = ctx.store.read("folded.json").filter(Value::is_object).unwrap_or_else(|| json!({}));
    if folded.get(session).is_some() {
        return Ok(None);
    }
    let sess_name = format!("sessions/{session}.json");
    let mut sess = ctx.store.read(&sess_name).filter(Value::is_object).unwrap_or_else(|| json!({}));
    let transcript = transcript_path.map(str::to_string).filter(|path| !path.is_empty()).or_else(|| sess.get("transcript").and_then(Value::as_str).filter(|path| !path.is_empty()).map(str::to_string));
    let mut record = json!({"kind": "session", "session_id": session, "host": "claude", "repo": ctx.repo.label, "folded_at": now(), "started": sess.get("started"), "ended": sess.get("ended"), "shadow": ctx.config.shadow});
    let workdir = TempDir::new("te-fold-")?;
    let trace_path = transcript.as_deref().map(Path::new).filter(|path| path.exists());
    let prompts = transcript.as_deref().map(|path| crate::trace::claude_prompt_count(Path::new(path)));
    record["prompts"] = json!(prompts);
    match trace_path {
        None => record["error"] = json!("transcript unreadable"),
        Some(trace_path) => {
            let run = run_dir(&workdir.0, trace_path)?;
            let _mechanisms = ctx.store.locked("mechanisms");
            let profile = ctx.config.text("microskills");
            // The proposer's paid calls, unless this agent's model calls are paused: then none answers.
            let runner = (profile.contains("distiller") && sidecall::paused(&ctx.root).is_none()).then(|| sidecall::runner_for(&ctx.config));
            let ask = move |prompt: &str, model: &str| runner.as_ref()?(prompt, model).ok().map(|(text, effective)| (text, json!(effective)));
            let injection = crate::microskills::fold_session(ctx, &run, session, sess.get("injected_ids"), &ask);
            // A session that ran no prompt is no evidence about the tools.
            let untool = if prompts == Some(0) { Value::Null } else { crate::toolprune::fold_session(ctx, &run, session, transcript.as_deref(), &workdir.0, prompts) };
            drop(_mechanisms);
            record["injection"] = injection;
            if untool.get("pending").is_some_and(python_truthy) {
                crate::toolprune::schedule_pending(ctx);
            }
            record["untool"] = untool;
            let usage = crate::trace::claude_session_usage(Path::new(transcript.as_deref().unwrap_or("")));
            let (effective, priced_by) = crate::pricing::effective(&usage);
            for key in ["model", "requests", "input_tokens", "cache_read_input_tokens", "cache_creation_input_tokens", "output_tokens"] {
                record[key] = usage.get(key).cloned().unwrap_or(Value::Null);
            }
            record["effective_tokens"] = json!(effective);
            record["priced_by"] = json!(priced_by);
        }
    }
    let optimizer = record.get("injection").and_then(|injection| injection.get("optimizer_tokens")).filter(|tokens| python_truthy(tokens)).cloned().unwrap_or_else(|| json!(0));
    let float = |key: &str| sess.get(key).filter(|value| python_truthy(value)).and_then(Value::as_f64).unwrap_or(0.0);
    record["turns"] = json!(sess.get("turns").and_then(Value::as_array).map_or(0, Vec::len));
    record["nudges"] = json!(whole(sess.get("nudges")));
    record["aborts"] = json!(whole(sess.get("aborts")));
    record["injected_chars"] = json!(whole(sess.get("injected_chars")));
    record["untool_applied"] = json!(crate::denylist::applied_for(&ctx.store, &ctx.repo.root));
    record["overhead"] = json!({"optimizer_tokens": optimizer, "spiral_judge_tokens": float("judge_tokens"), "spiral_nudge_writer_tokens": float("writer_tokens")});
    ctx.store.append("ledger.jsonl", &record)?;
    folded[session] = json!({"at": now(), "transcript": transcript});
    ctx.store.write("folded.json", &folded)?;
    if sess.as_object().is_some_and(|doc| !doc.is_empty()) {
        sess["folded"] = json!(true);
        ctx.store.write(&sess_name, &sess)?;
    }

    Ok(Some(record))
}

// Recorded sessions that ended, or went quiet, without being folded; the oldest first.
pub fn pending_sessions(ctx: &Ctx, exclude: &[&str]) -> Vec<String> {
    let folded = ctx.store.read("folded.json").unwrap_or(Value::Null);
    let Ok(names) = fs::read_dir(ctx.store.base.join("sessions")) else { return Vec::new() };
    let mut names: Vec<String> = names.flatten().filter_map(|entry| entry.file_name().into_string().ok()).collect();
    names.sort();
    let mut out: Vec<(f64, String)> = Vec::new();
    for name in names {
        let Some(id) = name.strip_suffix(".json") else { continue };
        if folded.get(id).is_some() || exclude.contains(&id) {
            continue;
        }
        let sess = ctx.store.read(&format!("sessions/{name}")).unwrap_or(Value::Null);
        let transcript = sess.get("transcript").and_then(Value::as_str).unwrap_or("");
        let quiet = fs::metadata(transcript).and_then(|meta| meta.modified()).ok().and_then(|at| std::time::SystemTime::now().duration_since(at).ok()).is_some_and(|age| age.as_secs_f64() >= STALE_S);
        if sess.get("ended").is_some_and(python_truthy) || quiet {
            out.push((sess.get("started").filter(|value| python_truthy(value)).and_then(Value::as_f64).unwrap_or(0.0), id.to_string()));
        }
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    out.into_iter().map(|(_, id)| id).collect()
}

// Fold in a detached process (inline under TE_FOLD_SYNC=1, for tests).
pub fn spawn_fold(ctx: &Ctx, sessions: &[String]) {
    if sessions.is_empty() {
        return;
    }
    if env("TE_FOLD_SYNC").as_deref() == Some("1") {
        for session in sessions {
            let _ = fold_session(ctx, session, None);
        }
        return;
    }
    let mut words = vec!["fold"];
    for session in sessions {
        words.extend(["--session", session]);
    }
    crate::session::spawn(ctx, &words, "fold.log");
}
