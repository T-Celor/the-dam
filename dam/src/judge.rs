// The loop guard's judge, off the hook's critical path (Apache-2.0, Microsoft), rewritten from plugin/judge.py and
// loopguard/judge.py: the hook only starts a job (`economizer::AsyncJudge`); this is the detached half.
use crate::context::Ctx;
use crate::economizer::{env, now, python_truthy};
use crate::pricing::approx_tokens;
use crate::sidecall::{self, Runner};
use fancy_regex::Regex;
use serde_json::{json, Map, Value};
use std::sync::LazyLock;

static OBJECT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)\{.*\}").expect("fixed"));

const STUCK_JUDGE_SYSTEM: &str = "You monitor an AI coding agent that is partway through ONE task in a software repo. A cheap heuristic flagged its recent process as a possible UNPRODUCTIVE LOOP. Read the leakage-safe digest of its RECENT actions and judge whether it is genuinely STUCK \u{2014} repeating the same kind of edit/test/failure with no progress toward green tests, unlikely to recover if it keeps going the same way \u{2014} versus still making real progress (changing failures, narrowing the problem, exploring new hypotheses).\n\nJudge ONLY the workflow from the digest; do NOT infer the task's solution or which lines to change. Favor SPARING (stuck=false) when there is any sign of progress; only return stuck=true with high confidence for a clear, repetitive dead-end.\nReturn ONLY a JSON object: {\"stuck\": bool, \"confidence\": <0.0-1.0>, \"reason\": \"<short leakage-safe justification>\"}.";

const NUDGE_WRITER_SYSTEM: &str = "An AI coding agent is stuck in a REPETITIVE, UNPRODUCTIVE loop partway through one task in a software repo. You are given a leakage-safe digest of its RECENT ACTIONS only (tool names, shell commands, searched patterns, edited file BASENAMES, and error SIGNATURES \u{2014} never file contents or the task description). Write a SHORT redirect (2-4 sentences) addressed to the agent that: (1) names the specific repetitive pattern you actually see in the digest (e.g. re-editing the same file, re-running the same test to the same failure); (2) proposes a MATERIALLY DIFFERENT next step (a different file/root-cause to consider, re-reading the failing test's expectation, verifying a new hypothesis before editing again).\nHARD CONSTRAINTS: Do NOT tell it to stop, give up, or reduce effort \u{2014} only redirect. Do NOT infer the task's solution, name specific line numbers to change, or reference any code/identifier you cannot see in the digest. Output ONLY the redirect as plain text \u{2014} no preamble, no JSON, no headings.";

fn take(text: &str, count: usize) -> String {
    text.chars().take(count).collect()
}

// The call's real cost when the runner recovered it, else an uncached estimate: the prompt at x1, the reply at x5.
fn tokens(prompt: &str, reply: &str, real: Option<f64>) -> Value {
    real.map_or_else(|| json!(approx_tokens(prompt) + 5 * approx_tokens(reply)), |real| json!(real))
}

// Python's float() of a JSON value.
fn as_float(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::Bool(flag) => Some(f64::from(u8::from(*flag))),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

pub fn llm_stuck_judge(digest: &str, runner: Option<&Runner>, model: &str) -> Value {
    let Some(runner) = runner.filter(|_| !digest.is_empty()) else { return json!({"stuck": null, "confidence": 0.0, "reason": "no-runner", "_tokens": 0}) };
    let prompt = format!("{STUCK_JUDGE_SYSTEM}\n\nRecent process digest:\n{digest}\n\nReturn the JSON verdict:");
    let (reply, real) = match runner(&prompt, model) {
        Ok(answer) => answer,
        Err(error) => return json!({"stuck": null, "confidence": 0.0, "reason": take(&format!("runner-error:{error}"), 60), "_tokens": 0}),
    };
    let mut verdict = Map::new();
    verdict.insert("stuck".to_string(), Value::Null);
    verdict.insert("confidence".to_string(), json!(0.0));
    verdict.insert("reason".to_string(), json!(""));
    verdict.insert("_tokens".to_string(), tokens(&prompt, &reply, real));
    if let Some(Value::Object(found)) = OBJECT.find(&reply).ok().flatten().and_then(|found| serde_json::from_str::<Value>(found.as_str()).ok()) {
        verdict.insert("stuck".to_string(), found.get("stuck").filter(|stuck| !stuck.is_null()).map_or(Value::Null, |stuck| json!(python_truthy(stuck))));
        let confidence = found.get("confidence").filter(|value| python_truthy(value)).map_or(Some(0.0), as_float);
        if let Some(confidence) = confidence {
            verdict.insert("confidence".to_string(), json!(confidence.min(1.0).max(0.0)));
            verdict.insert("reason".to_string(), json!(take(&found.get("reason").map_or_else(String::new, crate::config::plain_text), 160)));
        }
    }

    Value::Object(verdict)
}

pub fn llm_nudge_writer(digest: &str, runner: Option<&Runner>, model: &str) -> Value {
    let none = json!({"text": "", "_tokens": 0});
    let Some(runner) = runner.filter(|_| !digest.is_empty()) else { return none };
    let prompt = format!("{NUDGE_WRITER_SYSTEM}\n\nRecent process digest:\n{digest}\n\nWrite the redirect:");
    let Ok((reply, real)) = runner(&prompt, model) else { return none };
    let reply = reply.trim();
    if reply.is_empty() {
        return none;
    }

    json!({"text": take(reply, 1200), "_tokens": tokens(&prompt, reply, real)})
}

fn number(value: Option<&Value>) -> f64 {
    value.filter(|value| python_truthy(value)).and_then(as_float).unwrap_or(0.0)
}

// The detached half: judge the digest, write the redirect if stuck, charge the ledger.
pub fn run_job(ctx: &Ctx, session: &str, job: &str) -> Option<Value> {
    let request = ctx.store.read(&format!("judge/{session}.request.json")).unwrap_or(Value::Null);
    if request.get("id").and_then(Value::as_str) != Some(job) {
        return None;
    }
    let runner = sidecall::paused(&ctx.root).is_none().then(|| sidecall::runner_for(&ctx.config));
    let digest = request.get("digest").and_then(Value::as_str).unwrap_or("");
    let (mut verdict, mut written) = (None::<Value>, None::<Value>);
    if let Some(runner) = &runner {
        let mut judged = llm_stuck_judge(digest, Some(&**runner), "agent");
        if python_truthy(&judged["stuck"]) && number(judged.get("confidence")) >= number(request.get("threshold")) {
            let nudge = llm_nudge_writer(digest, Some(&**runner), "agent");
            let text = nudge["text"].as_str().unwrap_or("").trim().to_string();
            if !text.is_empty() {
                judged["_nudge"] = json!(text);
            }
            written = Some(nudge);
        }
        verdict = Some(judged);
    }
    let spend = json!({"spiral_judge_tokens": number(verdict.as_ref().and_then(|v| v.get("_tokens"))), "spiral_nudge_writer_tokens": number(written.as_ref().and_then(|v| v.get("_tokens")))});
    if spend.as_object().is_some_and(|row| row.values().any(|amount| amount.as_f64() != Some(0.0))) {
        // Charged before the verdict is published: a fold that runs the instant it lands has the record.
        let _ = ctx.store.append("ledger.jsonl", &json!({"kind": "overhead", "session_id": session, "host": crate::hosts::name(), "job": job, "at": now(), "overhead": spend}));
    }
    let _ = ctx.store.write(&format!("judge/{session}.result.json"), &json!({"id": job, "verdict": verdict, "at": now()}));

    verdict
}

pub fn spawn_job(ctx: &Ctx, session: &str, job: &str) {
    if env("TE_JUDGE_SYNC").as_deref() == Some("1") {
        run_job(ctx, session, job);
    } else {
        crate::session::spawn(ctx, &["judge", "--session", session, "--job", job], "judge.log");
    }
}
