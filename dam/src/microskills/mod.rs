// TokenEconomizer's micro-skills (Apache-2.0, Microsoft), rewritten from microskills/stage.py and __init__.py, plugin/fold.py (_fold_microskills), plugin/cli.py (cmd_microskills) and pricing.py (approx_tokens).
mod distiller;
mod envfacts;
mod lifecycle;
mod observe;

use crate::context::Ctx;
use crate::economizer::{dumps_ordered, now, python_truthy};
use fancy_regex::{Regex, RegexBuilder};
use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

// Takes the prompt and the model; gives the model's text and the effective tokens the call really used (a number, or null where the host did not say), or None when it gave no answer.
pub type Runner = dyn Fn(&str, &str) -> Option<(String, Value)>;

// A pattern that is fixed in the code, with room for the backtracking its back-references need.
pub(crate) fn rx(pattern: &str) -> Regex {
    RegexBuilder::new(pattern).backtrack_limit(50_000_000).build().expect("fixed")
}

pub(crate) fn hit(re: &Regex, text: &str) -> bool {
    re.is_match(text).unwrap_or(false)
}

// The byte range of the first match.
pub(crate) fn first(re: &Regex, text: &str) -> Option<(usize, usize)> {
    re.find(text).ok().flatten().map(|found| (found.start(), found.end()))
}

// Python's str.isspace also counts the four separators \x1c to \x1f.
fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

pub(crate) fn strip(text: &str) -> &str {
    text.trim_matches(is_space)
}

pub(crate) fn split(text: &str) -> Vec<&str> {
    text.split(is_space).filter(|part| !part.is_empty()).collect()
}

// text[:count] in code points.
pub(crate) fn take(text: &str, count: usize) -> String {
    text.chars().take(count).collect()
}

// text[-count:] in code points.
pub(crate) fn tail(text: &str, count: usize) -> String {
    let skip = text.chars().count().saturating_sub(count);

    text.chars().skip(skip).collect()
}

// round(x, 2): the nearest hundredth of the exact value, a tie going to the even one.
pub(crate) fn round2(x: f64) -> f64 {
    format!("{x:.2}").parse().unwrap_or(x)
}

pub(crate) fn state_path(mech: &Path, sub: &str, repo: &str) -> PathBuf {
    let dir = mech.join(sub);
    let _ = fs::create_dir_all(&dir);

    dir.join(format!("{}.json", crate::paths::repo_filename(repo)))
}

// Outer None where the file exists and cannot be read, which makes Python's json.load raise.
pub(crate) fn load_state(path: &Path) -> Option<Option<Value>> {
    if !path.exists() {
        return Some(None);
    }

    serde_json::from_slice(&fs::read(path).ok()?).ok().map(Some)
}

// json.dump as the Python writes it: the keys in the order they were added.
pub(crate) fn save_state(path: &Path, state: &Value) -> bool {
    fs::write(path, dumps_ordered(state, Some(2))).is_ok()
}

// Learn the facts of one finished session and render the block for the next. None where the Python would have raised, with nothing written.
fn learn(ctx: &Ctx, run_dir: &Path, session_id: &str, injected: Option<&Value>, runner: &Runner, profile: &str) -> Option<Value> {
    let mech = ctx.store.base.join("mechanisms");
    fs::create_dir_all(&mech).ok()?;
    let repo = ctx.repo.label.as_str();
    let vocab = observe::detect_vocabulary(&ctx.repo.root, ctx.config.text("vocabulary"));
    let mut env = if profile.contains("envfacts") { Some(envfacts::EnvFacts::load(repo, &mech, vocab, ctx.config.number("microskills_max_facts"), ctx.config.number("microskills_max_tokens"))?) } else { None };
    let mut dist = if profile.contains("distiller") { Some(distiller::Distiller::load(repo, &mech, vocab, ctx.config.number("microskills_propose_every"))?) } else { None };
    let transcript = run_dir.join("transcript.jsonl");
    if let (Some(env), Some(Value::Array(ids))) = (env.as_mut(), injected) {
        env.seed_injected(&ids.iter().filter_map(|id| id.as_str().map(str::to_string)).collect::<Vec<_>>());
    }
    let before = dist.as_ref().map_or(0.0, |dist| dist.opt_tokens);
    if let Some(env) = env.as_mut() {
        env.update(&crate::trace::load_trajectory(&transcript), session_id);
    }
    if let Some(dist) = dist.as_mut() {
        // What the free layer already injects, which the proposer is told not to restate.
        let stated = env.as_ref().map(|env| env.injectable_claims()).unwrap_or_default();
        dist.update(&transcript, session_id, runner, &stated)?;
    }
    if !env.as_ref().is_none_or(|env| env.save(&mech)) || !dist.as_ref().is_none_or(|dist| dist.save(&mech)) {
        return None;
    }
    let opt_tokens = dist.as_ref().map_or(0.0, |dist| dist.opt_tokens) - before;
    let mut facts = dist.as_ref().map(|dist| dist.debug_row());
    if let Some(env) = &env {
        let row = facts.get_or_insert_with(Map::new);
        row.extend(env.debug_row());
        if dist.is_some() {
            row.insert("_profile".to_string(), json!("envfacts+distiller"));
        }
    }
    let (first_block, second_block) = (env.as_mut().map(|env| env.render()).unwrap_or_default(), dist.as_ref().map(|dist| dist.render()).unwrap_or_default());
    let separator = if !first_block.is_empty() && !second_block.is_empty() { "\n" } else { "" };
    let rendered = format!("{first_block}{separator}{second_block}");
    let reason = match (env.is_some(), dist.is_some()) {
        (true, true) => "envfacts+manual-distill",
        (true, false) => "envfacts",
        _ => "manual-distill",
    };
    let prior = ctx.store.read("injection.json").unwrap_or(Value::Null);
    let sessions = prior.get("sessions").and_then(Value::as_i64).unwrap_or(0) + 1;
    let ids = env.as_ref().map(|env| env.injected_ids()).unwrap_or_default();
    let record = json!({"profile": profile, "reason": reason, "rendered": rendered, "rendered_ids": ids, "facts": facts, "updated": now(), "sessions": sessions});
    ctx.store.write("injection.json", &record).ok()?;

    Some(json!({"optimizer_tokens": opt_tokens, "rendered_chars": rendered.chars().count()}))
}

// The fold of a finished session into the micro-skills: null when they are off here or the fold failed.
pub fn fold_session(ctx: &Ctx, run_dir: &Path, session_id: &str, injected: Option<&Value>, runner: &Runner) -> Value {
    let profile = ctx.config.text("microskills");
    if !["envfacts", "distiller", "envfacts+distiller"].contains(&profile) {
        return Value::Null;
    }

    learn(ctx, run_dir, session_id, injected, runner, profile).unwrap_or(Value::Null)
}

// What `te microskills` prints.
pub fn command(ctx: &Ctx) -> i32 {
    let injection = ctx.store.read("injection.json").filter(python_truthy).unwrap_or(Value::Null);
    if ctx.config.text("microskills") == "off" {
        println!(
            "micro-skills is off here (the default). `config set microskills=on` turns it on: facts are then learned from finished sessions, and a model proposes more every {} sessions, through this agent",
            ctx.config.number("microskills_propose_every")
        );
        if injection.is_null() {
            return 0;
        }
    }
    let rendered = injection.get("rendered").and_then(Value::as_str).unwrap_or("");
    let rendered = strip_end(rendered);
    println!("{}", if rendered.is_empty() { "(nothing injected yet: no fact has cleared its firing gate)" } else { rendered });
    let mut rows: Vec<(&String, &Value)> = injection.get("facts").and_then(Value::as_object).into_iter().flatten().filter(|(_, row)| row.get("state").is_some() && row.is_object()).collect();
    rows.sort_by(|a, b| a.0.cmp(b.0));
    if !rows.is_empty() {
        println!("\nfacts tracked:");
        for (key, row) in rows {
            let field = |name: &str| row.get(name).and_then(Value::as_str).unwrap_or("?").to_string();
            let marker = if row.get("inject").is_some_and(python_truthy) { "INJECTED" } else { "        " };
            println!("  {:9} fire={:>5} {marker} {key}", field("state"), field("fire"));
        }
    }

    0
}

fn strip_end(text: &str) -> &str {
    text.trim_end_matches(is_space)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_round_as_python_does() {
        assert_eq!([0.125, 0.375, 0.625, 2.0 / 3.0].map(round2), [0.12, 0.38, 0.62, 0.67]);
    }

    #[test]
    fn python_whitespace_and_slices() {
        assert_eq!(split("a\u{1c}b  c\u{a0}d"), ["a", "b", "c", "d"]);
        assert_eq!(tail("h\u{e9}llo", 3), "llo");
    }
}
