// TokenEconomizer's micro-skills (Apache-2.0, Microsoft), rewritten from microskills/envfacts.py.
use super::lifecycle::Lifecycle;
use super::observe::{form_failed, form_worked, is_bare, label_rule, observe_task, Observation, Vocabulary, NETWORK_ERRORS};
use super::{hit, rx, split, take};
use crate::trace::Call;
use fancy_regex::Regex;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::sync::LazyLock;

const MAX_FACTS: usize = 4;
const MAX_TOKENS: usize = 150;
const HEADER: &str = "# environment facts learned from earlier tasks in this checkout";
const GENERIC_HEADER: &str = "# environment facts learned from earlier sessions in this repository";
// A FormRepair contradiction only counts after this many consecutive exercised sessions said so.
const CONTRADICT_STREAK: i64 = 2;

// Words that would turn a fact into completion or verification advice.
static LINT: LazyLock<Regex> = LazyLock::new(|| rx(r"(?i)\b(skip|no need|don'?t|do not|stop|done|completed?|finish(?:ed)?|verify|verification|instead of (?:running|testing))\b"));

fn error_phrase(class: &str) -> Option<&'static str> {
    match class {
        "module_missing" => Some("ModuleNotFoundError"),
        "command_missing" => Some("command not found"),
        "script_missing" => Some("a file-not-found error"),
        "usage_error" => Some("a usage error"),
        "permission_denied" => Some("permission denied"),
        _ => None,
    }
}

pub struct EnvFact {
    id: String,
    family: String,
    params: Map<String, Value>,
    life: Lifecycle,
}

// The counts under `key`, in the order they were first seen.
fn counts<'a>(params: &'a Map<String, Value>, key: &str) -> Vec<(&'a str, i64)> {
    params.get(key).and_then(Value::as_object).map_or_else(Vec::new, |map| map.iter().map(|(name, n)| (name.as_str(), n.as_i64().unwrap_or(0))).collect())
}

fn bump(params: &mut Map<String, Value>, key: &str, name: &str) {
    let entry = params.entry(key.to_string()).or_insert_with(|| json!({}));
    if let Some(map) = entry.as_object_mut() {
        let n = map.get(name).and_then(Value::as_i64).unwrap_or(0);
        map.insert(name.to_string(), json!(n + 1));
    }
}

fn text<'a>(params: &'a Map<String, Value>, key: &str) -> &'a str {
    params.get(key).and_then(Value::as_str).unwrap_or("")
}

fn moves_shell(form: &str) -> bool {
    form.starts_with("cd ") && form.contains(" && ")
}

// A form as a claim shows it. One that starts with `cd DIR &&` is shown as a subshell: an agent whose shell keeps its directory between calls would otherwise run every later command in DIR.
fn usage(form: &str, args: &str) -> String {
    if moves_shell(form) { format!("`({form}{args})`") } else { format!("`{form}{args}`") }
}

const MORE: &str = " \u{2026}";

// The part of a working form that carries the environment: its activation prefix, or the directory of its explicit path.
fn root_of(form: &str) -> String {
    if let Some((before, _)) = form.split_once(" && ") {
        return before.to_string();
    }
    if ["conda run", "poetry run", "pipenv run", "uv run", "npx"].iter().any(|start| form.starts_with(start)) {
        return split(form).into_iter().take(3).collect::<Vec<_>>().join(" ");
    }
    let head = split(form).first().copied().unwrap_or("");

    head.rsplit_once('/').map_or(head, |(dir, _)| dir).to_string()
}

fn join(items: &[String]) -> String {
    match items {
        [] => "the hosts it tried".to_string(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

fn typed_ok(o: &Observation) -> bool {
    form_worked(o) && !o.env
}

// Another form, and, outside test runs and builds, not a bare one: only a fuller spelling repairs a bare interpreter or package tool.
fn repairs(form: &str, a_form: &str, intent: &str) -> bool {
    form != a_form && (intent == "run_tests" || intent == "run_build" || !is_bare(form))
}

impl EnvFact {
    fn new(id: &str, family: &str, high: bool, params: Value) -> Self {
        let Value::Object(params) = params else { unreachable!("an object") };

        // An environment fact holds or not whatever files a session edits.
        EnvFact { id: id.to_string(), family: family.to_string(), params, life: Lifecycle::new(id, high, false) }
    }

    fn from_dict(d: &Value) -> Option<Self> {
        let life = Lifecycle::from_dict(d.get("life")?)?;
        let params = d.get("params").and_then(Value::as_object).cloned().unwrap_or_default();
        let mut life = life;
        life.drift = false;

        Some(EnvFact { id: d.get("id")?.as_str()?.to_string(), family: d.get("family")?.as_str()?.to_string(), params, life })
    }

    fn to_dict(&self) -> Value {
        json!({"id": self.id, "family": self.family, "params": self.params, "life": self.life.to_dict()})
    }

    // The first of the highest counts, as Python's max keeps.
    fn best(&self, key: &str) -> Option<String> {
        let mut best: Option<(&str, i64)> = None;
        for (name, n) in counts(&self.params, key) {
            if best.is_none_or(|(_, top)| n > top) {
                best = Some((name, n));
            }
        }

        best.map(|(name, _)| name.to_string())
    }

    // The form with the best record (sessions it worked in as typed less those it failed in), among those that leave the shell where it is whenever one has a good record at all.
    fn working_form(&self) -> Option<String> {
        let (worked, failed) = (counts(&self.params, "b_counts"), counts(&self.params, "b_fails"));
        let failures = |form: &str| failed.iter().find(|(name, _)| *name == form).map_or(0, |(_, n)| *n);
        let net: Vec<(&str, i64, i64)> = worked.iter().filter(|(form, n)| *n > failures(form)).map(|(form, n)| (*form, n - failures(form), *n)).collect();
        let loose: Vec<_> = net.iter().filter(|(form, _, _)| !moves_shell(form)).copied().collect();
        let pool = if loose.is_empty() { net } else { loose };
        let mut best: Option<(&str, i64, i64)> = None;
        for entry in pool {
            if best.is_none_or(|top| (entry.1, entry.2) > (top.1, top.2)) {
                best = Some(entry);
            }
        }

        best.map(|(form, _, _)| form.to_string())
    }

    fn claim(&self) -> String {
        let p = &self.params;
        match self.family.as_str() {
            "form_repair" => {
                let Some(working) = self.working_form() else { return String::new() };
                let class = self.best("error_counts").unwrap_or_else(|| "module_missing".to_string());

                format!("{} fails here with {}; {} works.", usage(text(p, "a"), MORE), error_phrase(&class).unwrap_or(&class), usage(&working, MORE))
            }
            "boundary" => {
                let mut hosts = counts(p, "hosts");
                hosts.sort_by(|a, b| b.1.cmp(&a.1));
                let hosts: Vec<String> = hosts.into_iter().take(3).map(|(host, _)| host.to_string()).collect();

                format!("Outbound connections to {} are refused here; nothing can be fetched from them.", join(&hosts))
            }
            "arg_repair" => {
                if text(p, "kind") == "sep" {
                    return format!("{} takes dotted test labels (`module.Class.test`, not `module::Class::test`).", usage(text(p, "form"), ""));
                }
                let examples = counts(p, "examples");
                let mut pairs: Vec<(&str, &str)> = examples.iter().map(|(key, _)| key.split_once(' ').unwrap_or((key, ""))).collect();
                if pairs.is_empty() {
                    pairs.push(("x", "x"));
                }
                let mut shortest = pairs[0];
                for pair in &pairs {
                    if pair.0.chars().count() < shortest.0.chars().count() {
                        shortest = *pair;
                    }
                }

                format!("{} takes test labels without the `{}` prefix (`{}`, not `{}`).", usage(text(p, "form"), ""), text(p, "param"), shortest.1, shortest.0)
            }
            "env_value" => {
                let good = self.best("good").unwrap_or_default();
                let mut bad: Vec<&str> = counts(p, "bad").into_iter().map(|(value, _)| value).collect();
                bad.sort_unstable();
                let var = text(p, "var");
                let tail = match bad.into_iter().find(|value| *value != good) {
                    Some(value) => format!(", `{value}` does not."),
                    None => ".".to_string(),
                };

                format!("Code that imports the project needs `{var}` set here: `{var}={good}` works{tail}")
            }
            "empty_probe" => "This checkout has no git remote and no branches, tags or unreachable objects beyond HEAD; its history ends at the checked-out commit.".to_string(),
            _ => String::new(),
        }
    }

    fn injectable(&self) -> bool {
        self.life.injectable()
    }
}

// Fold one session's verdict into a fact's lifecycle. A high-stakes CANDIDATE that is contradicted retires at once, and a FormRepair contradiction only counts after CONTRADICT_STREAK consecutive exercised sessions said so.
fn fold(fact: &mut EnvFact, now: i64, mut verdict: Option<&'static str>, fired: bool, exercised: bool, files: &BTreeSet<String>) {
    if fact.family == "form_repair" && exercised {
        let before = fact.params.get("contra_streak").and_then(Value::as_i64).unwrap_or(0);
        let streak = if verdict == Some("contradict") { before + 1 } else { 0 };
        fact.params.insert("contra_streak".to_string(), json!(streak));
        if verdict == Some("contradict") && streak < CONTRADICT_STREAK {
            verdict = None;
        }
    }
    fact.life.update(now, verdict, fired, exercised, files);
    if verdict == Some("contradict") && fact.life.high && fact.life.state == "CANDIDATE" {
        (fact.life.state, fact.life.retired_at) = ("RETIRED".to_string(), Some(now));
    }
}

pub struct EnvFacts {
    repo: String,
    vocab: Vocabulary,
    max_facts: usize,
    max_tokens: usize,
    pub now: i64,
    facts: Vec<EnvFact>,
    last_injected: BTreeSet<String>,
    last_digest: Option<Value>,
}

impl EnvFacts {
    fn new(repo: &str, vocab: Vocabulary, max_facts: i64, max_tokens: i64) -> Self {
        let cap = |given: i64, default: usize| if given > 0 { given as usize } else { default };

        EnvFacts { repo: repo.to_string(), vocab, max_facts: cap(max_facts, MAX_FACTS), max_tokens: cap(max_tokens, MAX_TOKENS), now: 0, facts: Vec::new(), last_injected: BTreeSet::new(), last_digest: None }
    }

    // None where Python's load would raise.
    pub fn load(repo: &str, mech: &std::path::Path, vocab: Vocabulary, max_facts: i64, max_tokens: i64) -> Option<Self> {
        let mut this = Self::new(repo, vocab, max_facts, max_tokens);
        let Some(state) = super::load_state(&super::state_path(mech, "envfacts_state", repo))? else { return Some(this) };
        this.now = state.get("now").and_then(Value::as_i64).unwrap_or(0);
        for (_, fact) in state.get("facts").and_then(Value::as_object).into_iter().flatten() {
            this.facts.push(EnvFact::from_dict(fact)?);
        }

        Some(this)
    }

    pub fn save(&self, mech: &std::path::Path) -> bool {
        let facts: Map<String, Value> = self.facts.iter().map(|fact| (fact.id.clone(), fact.to_dict())).collect();
        let state = json!({"repo": self.repo, "now": self.now, "opt_tokens": 0.0, "vocab": self.vocab.name, "facts": facts});

        super::save_state(&super::state_path(mech, "envfacts_state", &self.repo), &state)
    }

    fn header(&self) -> &'static str {
        if self.vocab.generic { GENERIC_HEADER } else { HEADER }
    }

    pub fn render(&mut self) -> String {
        let chosen = self.select();
        self.last_injected = chosen.iter().map(|n| self.facts[*n].id.clone()).collect();
        if chosen.is_empty() {
            return String::new();
        }
        let lines: Vec<String> = self.merged_lines(&chosen).iter().map(|line| format!("- {line}")).collect();

        format!("{}\n{}\n", self.header(), lines.join("\n"))
    }

    pub fn injected_ids(&self) -> Vec<String> {
        self.last_injected.iter().cloned().collect()
    }

    // Say which facts the session about to be folded was actually given.
    pub fn seed_injected(&mut self, ids: &[String]) {
        self.last_injected = ids.iter().filter(|id| self.facts.iter().any(|fact| &fact.id == *id)).cloned().collect();
    }

    // The claims that would be injected now, one string each (no header).
    pub fn injectable_claims(&self) -> Vec<String> {
        self.select().iter().map(|n| self.facts[*n].claim()).collect()
    }

    fn select(&self) -> Vec<usize> {
        let priority = |family: &str| match family {
            "form_repair" => 0,
            "arg_repair" => 1,
            "env_value" => 2,
            "empty_probe" => 3,
            "boundary" => 4,
            _ => 9,
        };
        let mut candidates: Vec<usize> = (0..self.facts.len()).filter(|&n| self.facts[n].injectable() && !self.facts[n].claim().is_empty() && !hit(&LINT, &self.facts[n].claim())).collect();
        let key = |n: &usize| {
            let fact = &self.facts[*n];
            (priority(&fact.family), usize::from(text(&fact.params, "intent") != "run_interpreter"), -fact.life.firing_rate())
        };
        candidates.sort_by(|a, b| {
            let (x, y) = (key(a), key(b));
            x.0.cmp(&y.0).then(x.1.cmp(&y.1)).then(x.2.partial_cmp(&y.2).unwrap_or(std::cmp::Ordering::Equal))
        });
        let mut chosen: Vec<usize> = Vec::new();
        for n in candidates {
            let mut trial = chosen.clone();
            trial.push(n);
            if trial.len() > self.max_facts {
                break;
            }
            let lines: Vec<String> = self.merged_lines(&trial).iter().map(|line| format!("- {line}")).collect();
            if format!("{}\n{}", self.header(), lines.join("\n")).chars().count() > self.max_tokens * 4 {
                continue;
            }
            chosen = trial;
        }

        chosen
    }

    // FormRepair facts whose working forms share a root (the same activation prefix or the same directory) collapse into one line: they are one environment fact.
    fn merged_lines(&self, chosen: &[usize]) -> Vec<String> {
        let (mut lines, mut groups): (Vec<String>, Vec<(String, Vec<&EnvFact>)>) = (Vec::new(), Vec::new());
        for &n in chosen {
            let fact = &self.facts[n];
            if fact.family != "form_repair" {
                lines.push(fact.claim());
                continue;
            }
            let root = root_of(&fact.working_form().unwrap_or_default());
            match groups.iter_mut().find(|(known, _)| *known == root) {
                Some((_, members)) => members.push(fact),
                None => groups.push((root, vec![fact])),
            }
        }
        // Python inserts each group at the front, so the last group comes first.
        for (_, members) in groups {
            let line = if let [only] = members.as_slice() {
                only.claim()
            } else {
                let failing: Vec<String> = members.iter().map(|fact| usage(text(&fact.params, "a"), MORE)).collect();
                let phrases: BTreeSet<&str> = members.iter().filter_map(|fact| error_phrase(&fact.best("error_counts").unwrap_or_default())).collect();
                let working: BTreeSet<String> = members.iter().map(|fact| usage(&fact.working_form().unwrap_or_default(), MORE)).collect();
                let phrases = if phrases.is_empty() { "environment error".to_string() } else { phrases.into_iter().collect::<Vec<_>>().join(", ") };
                let working: Vec<String> = working.into_iter().collect();

                format!("{} fail here ({phrases}); {} {}.", join(&failing), join(&working), if working.len() == 1 { "works" } else { "work" })
            };
            lines.insert(0, line);
        }

        lines
    }

    fn find(&mut self, id: &str) -> Option<&mut EnvFact> {
        self.facts.iter_mut().find(|fact| fact.id == id)
    }

    fn ensure(&mut self, id: &str, family: &str, high: bool, params: Value) -> &mut EnvFact {
        let at = match self.facts.iter().position(|fact| fact.id == id) {
            Some(at) => at,
            None => {
                self.facts.push(EnvFact::new(id, family, high, params));
                self.facts.len() - 1
            }
        };

        &mut self.facts[at]
    }

    fn family_ids(&self, family: &str) -> Vec<String> {
        self.facts.iter().filter(|fact| fact.family == family).map(|fact| fact.id.clone()).collect()
    }

    // Fold one finished session in, then advance the clock.
    pub fn update(&mut self, calls: &[Call], iid: &str) {
        let now = self.now;
        let digest = observe_task(calls, &self.vocab);
        let files: BTreeSet<String> = digest.edited.iter().cloned().collect();
        let injected = self.last_injected.clone();
        let mut verdicts: Vec<(String, Option<&'static str>, bool, bool)> = Vec::new();
        let of = |intent: &str| -> Vec<&Observation> { digest.obs.iter().filter(|o| o.intent == intent).collect() };

        // FormRepair: discover candidates, then fold every fact.
        for intent in ["run_interpreter", "run_tests", "run_build", "package_tool"] {
            let seq = of(intent);
            // A test run that failed because its LABEL did not load says nothing about the way the runner was called: those are ArgRepair's.
            let mut fails: Vec<&Observation> = seq.iter().copied().filter(|o| form_failed(o)).collect();
            if intent != "run_tests" && intent != "run_build" {
                // An explicit path or activation that fails is a missing dependency, not a resolution problem.
                fails.retain(|o| is_bare(&o.form));
            }
            for a in fails {
                if !seq.iter().any(|o| typed_ok(o) && o.call > a.call && repairs(&o.form, &a.form, intent)) {
                    continue;
                }
                let params = json!({"intent": intent, "a": a.form, "b_counts": {}, "error_counts": {}});
                let fact = self.ensure(&format!("form_repair:{intent}:{}", a.form), "form_repair", false, params);
                bump(&mut fact.params, "error_counts", a.outcome.unwrap_or(""));
            }
            // A session is one piece of evidence per form: every form that worked as typed adds one, and a known working form that failed as typed (and never worked) takes one back.
            // Python walks sets here, whose order changes from run to run; the order the forms were typed in is used instead.
            let mut ok: Vec<&str> = Vec::new();
            for o in seq.iter().filter(|o| typed_ok(o)) {
                if !ok.contains(&o.form.as_str()) {
                    ok.push(&o.form);
                }
            }
            let mut failed: Vec<&str> = Vec::new();
            for o in seq.iter().filter(|o| form_failed(o) && !o.env) {
                if !ok.contains(&o.form.as_str()) && !failed.contains(&o.form.as_str()) {
                    failed.push(&o.form);
                }
            }
            for fact in self.facts.iter_mut().filter(|fact| fact.family == "form_repair" && text(&fact.params, "intent") == intent) {
                let a_form = text(&fact.params, "a").to_string();
                for form in &ok {
                    if repairs(form, &a_form, intent) {
                        bump(&mut fact.params, "b_counts", form);
                    }
                }
                fact.params.entry("b_fails".to_string()).or_insert_with(|| json!({}));
                let known: Vec<String> = counts(&fact.params, "b_counts").into_iter().map(|(form, _)| form.to_string()).collect();
                for form in known.iter().filter(|form| failed.contains(&form.as_str())) {
                    bump(&mut fact.params, "b_fails", form);
                }
            }
        }
        for id in self.family_ids("form_repair") {
            let fact = self.find(&id).expect("listed above");
            let (intent, a_form) = (text(&fact.params, "intent").to_string(), text(&fact.params, "a").to_string());
            let b_forms: Vec<String> = counts(&fact.params, "b_counts").into_iter().map(|(form, _)| form.to_string()).collect();
            let seq = of(&intent);
            let exercised = !seq.is_empty();
            let a_failed = seq.iter().any(|o| o.form == a_form && form_failed(o));
            let b_ran = seq.iter().any(|o| b_forms.contains(&o.form) && form_worked(o)) || seq.iter().any(|o| form_worked(o) && !is_bare(&o.form));
            // Contradiction: the bare form ran the TEST RUNNER without an environment error. A bare interpreter one-liner succeeding is not evidence.
            let a_runner_ok = of("run_tests").iter().any(|o| o.form == a_form && form_worked(o));
            let fired = a_failed || (injected.contains(&id) && b_ran && !a_failed);
            let verdict = if a_failed || (b_ran && !a_runner_ok) { Some("confirm") } else if a_runner_ok { Some("contradict") } else { None };
            fold(fact, now, verdict, fired, exercised, &files);
            verdicts.push((id, verdict, fired, exercised));
        }

        // ArgRepair: labels the runner loads only in another shape.
        let runs: Vec<&Observation> = of("run_tests").into_iter().filter(|o| !o.labels.is_empty()).collect();
        for (i, a_obs) in runs.iter().enumerate() {
            if !a_obs.label_err {
                continue;
            }
            for a in &a_obs.labels {
                for b_obs in &runs[i + 1..] {
                    if b_obs.form != a_obs.form || !b_obs.loaded {
                        continue;
                    }
                    let Some(rule) = b_obs.labels.iter().find_map(|b| label_rule(a, b)) else { continue };
                    let b = b_obs.labels.iter().find(|b| label_rule(a, b).as_ref() == Some(&rule)).expect("the rule came from a label");
                    let params = json!({"form": a_obs.form, "kind": rule.0, "param": rule.1, "examples": {}});
                    let fact = self.ensure(&format!("arg_repair:{}:{}:{}", a_obs.form, rule.0, rule.1), "arg_repair", false, params);
                    bump(&mut fact.params, "examples", &format!("{a} {b}"));
                    break;
                }
            }
        }
        for id in self.family_ids("arg_repair") {
            let fact = self.find(&id).expect("listed above");
            let (form, kind, param) = (text(&fact.params, "form").to_string(), text(&fact.params, "kind").to_string(), text(&fact.params, "param").to_string());
            let bad = |label: &String| if kind == "sep" { label.contains("::") } else { label.starts_with(&param) };
            let mine: Vec<&&Observation> = runs.iter().filter(|o| o.form == form).collect();
            let bad_failed = mine.iter().any(|o| o.label_err && o.labels.iter().any(bad));
            let bad_loaded = mine.iter().any(|o| o.loaded && o.labels.iter().any(bad));
            let good_loaded = mine.iter().any(|o| o.loaded && o.labels.iter().any(|label| !bad(label)));
            let exercised = !mine.is_empty();
            let fired = bad_failed || (injected.contains(&id) && good_loaded && !bad_failed);
            let verdict = if bad_loaded { Some("contradict") } else if bad_failed || good_loaded { Some("confirm") } else { None };
            fold(fact, now, verdict, fired, exercised, &files);
            verdicts.push((id, verdict, fired, exercised));
        }

        // EnvValue: a variable the code asks for, and the value that satisfied it.
        let value_of = |sets: &[(String, String)], var: &str| sets.iter().find(|(name, _)| name == var).map(|(_, value)| value.clone());
        for (i, call) in digest.env.iter().enumerate() {
            for var in &call.need {
                let later: Vec<&super::observe::EnvCall> = digest.env[i + 1..].iter().filter(|d| value_of(&d.sets, var).is_some()).collect();
                let good: Vec<String> = later.iter().filter(|d| d.ok && !d.need.contains(var)).filter_map(|d| value_of(&d.sets, var)).collect();
                let Some(first_good) = good.first() else { continue };
                let fact = self.ensure(&format!("env_value:{var}"), "env_value", false, json!({"var": var, "good": {}, "bad": {}}));
                bump(&mut fact.params, "good", first_good);
                for d in later.iter().filter(|d| !d.ok) {
                    let value = value_of(&d.sets, var).unwrap_or_default();
                    if &value != first_good {
                        bump(&mut fact.params, "bad", &value);
                    }
                }
            }
        }
        for id in self.family_ids("env_value") {
            let fact = self.find(&id).expect("listed above");
            let (var, good) = (text(&fact.params, "var").to_string(), fact.best("good"));
            let asked = digest.env.iter().any(|c| c.need.contains(&var));
            let good_ok = digest.env.iter().any(|c| value_of(&c.sets, &var) == good && c.ok);
            let good_failed = digest.env.iter().any(|c| value_of(&c.sets, &var) == good && c.need.contains(&var));
            let exercised = asked || digest.env.iter().any(|c| value_of(&c.sets, &var).is_some());
            let fired = asked || (injected.contains(&id) && good_ok);
            // Only the value working confirms the fact: code asking for the variable again shows the waste recurs, not that the learned value is right.
            let verdict = if good_failed { Some("contradict") } else if good_ok { Some("confirm") } else { None };
            fold(fact, now, verdict, fired, exercised, &files);
            verdicts.push((id, verdict, fired, exercised));
        }

        // Boundary: hosts refused.
        let fetches = of("fetch_remote");
        let mut failed_hosts: Vec<(&str, i64)> = Vec::new();
        for host in fetches.iter().filter(|o| o.outcome.is_some_and(|outcome| NETWORK_ERRORS.contains(&outcome))).flat_map(|o| &o.hosts) {
            match failed_hosts.iter_mut().find(|(known, _)| known == host) {
                Some(entry) => entry.1 += 1,
                None => failed_hosts.push((host, 1)),
            }
        }
        let ok_hosts: BTreeSet<&str> = fetches.iter().filter(|o| o.outcome == Some("ran")).flat_map(|o| o.hosts.iter().map(String::as_str)).collect();
        let id = "boundary:fetch_remote";
        if !failed_hosts.is_empty() || self.facts.iter().any(|fact| fact.id == id) {
            let fact = self.ensure(id, "boundary", true, json!({"hosts": {}}));
            for (host, n) in &failed_hosts {
                for _ in 0..*n {
                    bump(&mut fact.params, "hosts", host);
                }
            }
            let known: Vec<String> = counts(&fact.params, "hosts").into_iter().map(|(host, _)| host.to_string()).collect();
            let exercised = fetches.iter().any(|o| !o.hosts.is_empty());
            let fired = !failed_hosts.is_empty();
            let verdict = if known.iter().any(|host| ok_hosts.contains(host.as_str())) { Some("contradict") } else if !failed_hosts.is_empty() { Some("confirm") } else { None };
            fold(fact, now, verdict, fired, exercised, &files);
            verdicts.push((id.to_string(), verdict, fired, exercised));
        }

        // EmptyProbe: refs beyond the checkout.
        let probes: Vec<&Observation> = of("probe_vcs_refs").into_iter().filter(|o| matches!(o.outcome, Some("hit" | "empty"))).collect();
        let id = "empty_probe:vcs_refs";
        if !probes.is_empty() || self.facts.iter().any(|fact| fact.id == id) {
            let fact = self.ensure(id, "empty_probe", true, json!({"kinds": {}}));
            for o in &probes {
                bump(&mut fact.params, "kinds", o.probe.unwrap_or(""));
            }
            let exercised = !probes.is_empty();
            let verdict = if probes.iter().any(|o| o.outcome == Some("hit")) { Some("contradict") } else if exercised { Some("confirm") } else { None };
            fold(fact, now, verdict, exercised, exercised, &files);
            verdicts.push((id.to_string(), verdict, exercised, exercised));
        }

        let verdicts: Map<String, Value> = verdicts.into_iter().map(|(id, verdict, fired, exercised)| (id, json!([verdict, fired, exercised]))).collect();
        self.last_digest = Some(json!({"iid": iid, "resolved": null, "injected": injected.iter().collect::<Vec<_>>(), "verdicts": verdicts, "n_calls": digest.n_calls}));
        self.last_injected.clear();
        self.now += 1;
    }

    pub fn debug_row(&self) -> Map<String, Value> {
        let mut row = Map::new();
        for fact in &self.facts {
            let rate = super::round2(fact.life.firing_rate());
            row.insert(fact.id.clone(), json!({"state": fact.life.state, "fire": format!("{}/{}", fact.life.fire_num, fact.life.fire_den), "rate": rate, "inject": fact.injectable(), "claim": take(&fact.claim(), 120), "family": fact.family}));
        }
        row.insert("_profile".to_string(), json!("envfacts"));
        row.insert("_vocab".to_string(), json!(self.vocab.name));
        if let Some(task) = self.last_digest.as_ref().filter(|task| crate::economizer::python_truthy(task)) {
            row.insert("_task".to_string(), task.clone());
        }

        row
    }
}
