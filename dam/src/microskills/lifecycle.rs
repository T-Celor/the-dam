// TokenEconomizer's micro-skills (Apache-2.0, Microsoft), rewritten from microskills/lifecycle.py.
use serde_json::{json, Value};
use std::collections::BTreeSet;

const K_PROMOTE: usize = 3;
const GAMMA: f64 = 0.8;
const LAMBDA: f64 = 5.0;
const FIRE_GATE: f64 = 0.40;
const FIRE_MIN_N: i64 = 4;

const EDIT_TOOLS: [&str; 5] = ["edit", "multiedit", "write", "create", "str_replace_editor"];

// os.path.basename of this platform.
fn basename(path: &str) -> &str {
    let path = if cfg!(windows) && path.as_bytes().get(1) == Some(&b':') { path.get(2..).unwrap_or("") } else { path };
    let seps: &[char] = if cfg!(windows) { &['/', '\\'] } else { &['/'] };

    path.rsplit(seps).next().unwrap_or("")
}

// Every file an edit tool wrote, by basename; `python_only` is the experiments' reading.
pub fn edited_files(calls: &[(String, Value)], python_only: bool) -> BTreeSet<String> {
    let mut files = BTreeSet::new();
    for (tool, args) in calls {
        if !EDIT_TOOLS.contains(&tool.as_str()) {
            continue;
        }
        let path = ["file_path", "path"].iter().filter_map(|key| args.get(*key)).find(|value| crate::economizer::python_truthy(value)).and_then(Value::as_str).unwrap_or("");
        if !path.is_empty() && (!python_only || path.ends_with(".py")) {
            files.insert(basename(path).to_string());
        }
    }

    files
}

pub struct Lifecycle {
    key: Value,
    pub high: bool,
    pub drift: bool,
    pub state: String,
    confirms: Vec<(i64, Vec<String>)>,
    contradicts: Vec<i64>,
    pub fire_num: i64,
    pub fire_den: i64,
    pub retired_at: Option<i64>,
    promoted_at: Option<i64>,
}

impl Lifecycle {
    pub fn new(key: &str, high: bool, drift: bool) -> Self {
        Lifecycle { key: json!(key), high, drift, state: "CANDIDATE".to_string(), confirms: Vec::new(), contradicts: Vec::new(), fire_num: 0, fire_den: 0, retired_at: None, promoted_at: None }
    }

    pub fn to_dict(&self) -> Value {
        json!({"key": self.key, "high": self.high, "drift": self.drift, "state": self.state, "confirms": self.confirms, "contradicts": self.contradicts,
            "fire_num": self.fire_num, "fire_den": self.fire_den, "promoted_at": self.promoted_at, "retired_at": self.retired_at})
    }

    // None where Python's `d["key"]` or `d["high"]` would raise.
    pub fn from_dict(d: &Value) -> Option<Self> {
        let (key, high) = (d.get("key")?, d.get("high")?);
        let mut life = Lifecycle::new("", crate::economizer::python_truthy(high), true);
        life.key = key.clone();
        let int = |name: &str| d.get(name).and_then(Value::as_i64);
        if let Some(drift) = d.get("drift") {
            life.drift = crate::economizer::python_truthy(drift);
        }
        if let Some(state) = d.get("state").and_then(Value::as_str) {
            life.state = state.to_string();
        }
        life.confirms = d.get("confirms").and_then(Value::as_array).map_or_else(Vec::new, |items| {
            items.iter().filter_map(|item| Some((item.get(0)?.as_i64()?, item.get(1)?.as_array()?.iter().filter_map(|file| file.as_str().map(str::to_string)).collect()))).collect()
        });
        life.contradicts = d.get("contradicts").and_then(Value::as_array).map_or_else(Vec::new, |items| items.iter().filter_map(Value::as_i64).collect());
        (life.fire_num, life.fire_den) = (int("fire_num").unwrap_or(0), int("fire_den").unwrap_or(0));
        (life.promoted_at, life.retired_at) = (int("promoted_at"), int("retired_at"));

        Some(life)
    }

    fn drifted_confirms(&self) -> usize {
        if !self.drift {
            return self.confirms.len();
        }
        let mut kept: Vec<BTreeSet<&String>> = Vec::new();
        for (_, files) in &self.confirms {
            let files: BTreeSet<&String> = files.iter().collect();
            if files.is_empty() || kept.iter().all(|other| files.is_disjoint(other)) {
                kept.push(files);
            }
        }

        kept.len()
    }

    fn score(&self, now: i64) -> f64 {
        let decayed = |at: &i64| GAMMA.powf((now - at) as f64);
        let mut score = 0.0;
        for (at, _) in &self.confirms {
            score += decayed(at);
        }
        let mut against = 0.0;
        for at in &self.contradicts {
            against += decayed(at);
        }

        score - LAMBDA * against
    }

    pub fn firing_rate(&self) -> f64 {
        if self.fire_den == 0 { 0.0 } else { self.fire_num as f64 / self.fire_den as f64 }
    }

    pub fn injectable(&self) -> bool {
        self.state == "ACTIVE" && self.fire_den >= FIRE_MIN_N && self.firing_rate() >= FIRE_GATE
    }

    pub fn update(&mut self, now: i64, verdict: Option<&str>, fired: bool, exercised: bool, files: &BTreeSet<String>) {
        if exercised {
            self.fire_den += 1;
            if fired {
                self.fire_num += 1;
            }
        }
        match verdict {
            Some("confirm") => self.confirms.push((now, files.iter().cloned().collect())),
            Some("contradict") => self.contradicts.push(now),
            _ => {}
        }
        if self.state == "CANDIDATE" {
            if self.drifted_confirms() >= K_PROMOTE {
                (self.state, self.promoted_at) = ("ACTIVE".to_string(), Some(now));
            }
        } else if self.state == "ACTIVE" {
            // A high-stakes fact is retired by a single contradiction.
            if (self.high && verdict == Some("contradict")) || self.score(now) < 0.0 {
                (self.state, self.retired_at) = ("RETIRED".to_string(), Some(now));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_confirmations_promote_and_a_high_stakes_contradiction_retires() {
        let mut life = Lifecycle::new("f", true, false);
        let none = BTreeSet::new();
        for now in 0..3 {
            life.update(now, Some("confirm"), true, true, &none);
        }
        assert_eq!(life.state, "ACTIVE");
        life.update(3, None, true, true, &none);
        assert!(life.injectable());
        life.update(4, Some("contradict"), false, true, &none);
        assert_eq!(life.state, "RETIRED");
    }

    #[test]
    fn a_state_file_round_trips() {
        let mut life = Lifecycle::new("k", false, true);
        life.update(0, Some("confirm"), true, true, &BTreeSet::from(["a.py".to_string()]));
        let again = Lifecycle::from_dict(&life.to_dict()).expect("complete");
        assert_eq!(again.to_dict(), life.to_dict());
    }
}
