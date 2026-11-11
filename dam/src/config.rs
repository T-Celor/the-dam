// TokenEconomizer's settings (Apache-2.0, Microsoft), rewritten from plugin/config.py: defaults, then
// the global file, the repository's file and the environment. Invalid values are ignored with a
// warning, as a typo in a config file must not break the agent's hooks.
use crate::loopguard::Thresholds;
use serde_json::{json, Map, Value};
use std::fs;
use std::path::Path;

const CHOICES: [(&str, &[&str]); 7] = [
    ("mode", &["auto", "interactive", "headless"]),
    ("microskills", &["off", "envfacts", "envfacts+distiller", "distiller"]),
    ("toolprune", &["off", "propose", "auto"]),
    ("loopguard", &["off", "nudge", "stop"]),
    ("repomap", &["off", "on", "start"]),
    ("loopguard_aggr", &["conservative", "default", "aggressive"]),
    ("vocabulary", &["auto", "generic", "python-git", "node-web"]),
];
const THRESHOLDS: [&str; 12] = ["min_turns", "edit_churn", "distinct_edit_files", "error_outputs", "stagnation", "envelope_pct", "envelope_margin", "envelope_min_samples", "backstop_mult", "judge_confidence", "gate_min_turns", "gate_error_outputs"];
// Old names first, so that a new name set alongside its old one wins.
const ENV: [(&str, &str); 30] = [
    ("TE_MODE", "mode"),
    ("TE_SHADOW", "shadow"),
    ("TE_PAID_MODEL", "paid_model"),
    ("TE_VOCABULARY", "vocabulary"),
    ("TE_INJECT", "microskills"),
    ("TE_INJECT_MAX_FACTS", "microskills_max_facts"),
    ("TE_INJECT_MAX_TOKENS", "microskills_max_tokens"),
    ("TE_PROPOSE_EVERY", "microskills_propose_every"),
    ("TE_UNTOOL", "toolprune"),
    ("TE_UNTOOL_EVIDENCE", "toolprune_evidence"),
    ("TE_SPIRAL", "loopguard"),
    ("TE_SPIRAL_AGGR", "loopguard_aggr"),
    ("TE_SPIRAL_NUDGES", "loopguard_nudges"),
    ("TE_MICROSKILLS", "microskills"),
    ("TE_MICROSKILLS_MAX_FACTS", "microskills_max_facts"),
    ("TE_MICROSKILLS_MAX_TOKENS", "microskills_max_tokens"),
    ("TE_MICROSKILLS_PROPOSE_EVERY", "microskills_propose_every"),
    ("TE_TOOLPRUNE", "toolprune"),
    ("TE_TOOLPRUNE_EVIDENCE", "toolprune_evidence"),
    ("TE_TOOLPRUNE_QUIET_MINUTES", "toolprune_quiet_minutes"),
    ("TE_TOOLPRUNE_MIN_SESSIONS", "toolprune_min_sessions"),
    ("TE_TOOLPRUNE_SESSIONS", "toolprune_sessions"),
    ("TE_TOOLPRUNE_PROTECT", "toolprune_protect"),
    ("TE_LOOPGUARD", "loopguard"),
    ("TE_LOOPGUARD_AGGR", "loopguard_aggr"),
    ("TE_LOOPGUARD_NUDGES", "loopguard_nudges"),
    ("TE_REPOMAP", "repomap"),
    ("TE_REPOMAP_TOKENS", "repomap_tokens"),
    ("TE_REPOMAP_PYTHON", "repomap_python"),
    ("TE_REPOMAP_TIMEOUT", "repomap_timeout"),
];

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

fn truthy(word: &str) -> bool {
    matches!(word.trim().to_lowercase().as_str(), "1" | "true" | "yes" | "on")
}

pub fn plain_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => repr(other),
    }
}

// The way Python writes a value in its messages.
fn repr(value: &Value) -> String {
    match value {
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => repr_text(text),
        Value::Array(items) => format!("[{}]", items.iter().map(repr).collect::<Vec<_>>().join(", ")),
        Value::Object(map) => format!("{{{}}}", map.iter().map(|(key, item)| format!("{}: {}", repr_text(key), repr(item))).collect::<Vec<_>>().join(", ")),
    }
}

pub fn repr_text(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') { '"' } else { '\'' };
    let body = text.replace('\\', "\\\\").replace('\n', "\\n").replace('\r', "\\r").replace('\t', "\\t");
    let body = if quote == '\'' { body.replace('\'', "\\'") } else { body };

    format!("{quote}{body}{quote}")
}

fn repr_float(number: f64) -> String {
    if number.fract() == 0.0 {
        format!("{number:.1}")
    } else {
        number.to_string()
    }
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Null => "NoneType",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "str",
    }
}

const ALIASES: [(&str, &str); 10] = [
    ("inject", "microskills"),
    ("inject_max_facts", "microskills_max_facts"),
    ("inject_max_tokens", "microskills_max_tokens"),
    ("propose_every", "microskills_propose_every"),
    ("untool", "toolprune"),
    ("untool_evidence", "toolprune_evidence"),
    ("spiral", "loopguard"),
    ("spiral_aggr", "loopguard_aggr"),
    ("spiral_nudges", "loopguard_nudges"),
    ("spiral_thresholds", "loopguard_thresholds"),
];

pub fn canonical(key: &str) -> &str {
    ALIASES.iter().find(|(old, _)| *old == key).map_or(key, |(_, new)| *new)
}

fn is_old_name(key: &str) -> bool {
    key.starts_with("spiral.") || canonical(key) != key
}

fn threshold_range(name: &str) -> Option<(bool, f64, Option<f64>)> {
    Some(match name {
        "min_turns" | "edit_churn" | "distinct_edit_files" | "error_outputs" | "envelope_min_samples" | "gate_min_turns" | "gate_error_outputs" => (true, 1.0, None),
        "stagnation" | "judge_confidence" => (false, 0.0, Some(1.0)),
        "envelope_pct" => (false, 1.0, Some(100.0)),
        "envelope_margin" | "backstop_mult" => (false, 0.1, None),
        _ => return None,
    })
}

fn int_setting(key: &str) -> Option<i64> {
    match key {
        "toolprune_quiet_minutes" => Some(0),
        "microskills_max_facts" | "microskills_max_tokens" | "microskills_propose_every" | "toolprune_evidence" | "toolprune_min_sessions" | "toolprune_sessions" | "loopguard_nudges" | "repomap_tokens" | "repomap_timeout" => Some(1),
        _ => None,
    }
}

fn to_int(value: &Value) -> Result<i64, String> {
    match value {
        Value::Bool(flag) => Ok(i64::from(*flag)),
        Value::Number(number) => number.as_i64().or_else(|| number.as_f64().map(|n| n as i64)).ok_or_else(|| "too large".to_string()),
        Value::String(text) => text.trim().replace('_', "").parse().map_err(|_| format!("invalid literal for int() with base 10: {}", repr_text(text))),
        other => Err(format!("int() argument must be a string, a bytes-like object or a real number, not '{}'", type_name(other))),
    }
}

#[derive(Debug, PartialEq)]
pub struct Config {
    pub enabled: bool,
    pub mode: String,
    pub loopguard: String,
    pub aggr: String,
    pub nudges: i64,
    pub thresholds: Thresholds,
    pub repomap: String,
    pub shadow: bool,
    // What the hooks do not read: reported by `status` and kept in the shape the file gives.
    rest: Map<String, Value>,
    pub warnings: Vec<String>,
}

impl Config {
    pub fn new() -> Self {
        let rest = json!({"microskills": "envfacts+distiller", "microskills_max_facts": 4, "microskills_max_tokens": 150, "microskills_propose_every": 3, "toolprune": "propose", "toolprune_evidence": 10, "toolprune_min_sessions": 2, "toolprune_sessions": 5, "toolprune_quiet_minutes": 60, "toolprune_protect": [], "repomap_tokens": 2048, "repomap_python": "", "repomap_timeout": 8, "paid_model": "", "vocabulary": "auto"});
        let Value::Object(rest) = rest else { unreachable!("an object above") };

        Config { enabled: true, mode: "auto".to_string(), loopguard: "nudge".to_string(), aggr: "default".to_string(), nudges: 1, thresholds: Map::new(), repomap: "off".to_string(), shadow: false, rest, warnings: Vec::new() }
    }

    pub fn text(&self, key: &str) -> &str {
        self.rest.get(key).and_then(Value::as_str).unwrap_or("")
    }

    pub fn number(&self, key: &str) -> i64 {
        self.rest.get(key).and_then(Value::as_i64).unwrap_or(0)
    }

    pub fn list(&self, key: &str) -> Vec<String> {
        self.rest.get(key).and_then(Value::as_array).map_or_else(Vec::new, |items| items.iter().filter_map(Value::as_str).map(str::to_string).collect())
    }

    fn set_threshold(&mut self, name: &str, value: &Value, source: &str) {
        let Some((is_int, low, high)) = threshold_range(name) else {
            self.warnings.push(format!("{source}: unknown loopguard threshold {} (one of {})", repr_text(name), THRESHOLDS.join(", ")));
            return;
        };
        let mut reject = |why: String| self.warnings.push(format!("{source}: loopguard.{name}={} rejected ({why})", repr(value)));
        let number = match value {
            Value::Bool(_) => return reject("not a number".to_string()),
            Value::Number(number) => number.as_f64().unwrap_or(f64::NAN),
            Value::String(text) => match text.trim().replace('_', "").parse::<f64>() {
                Ok(number) => number,
                Err(_) => return reject(format!("could not convert string to float: {}", repr_text(text))),
            },
            other => return reject(format!("float() argument must be a string or a real number, not '{}'", type_name(other))),
        };
        if !number.is_finite() {
            return reject("must be finite".to_string());
        }
        if is_int && number.fract() != 0.0 {
            return reject("must be a whole number".to_string());
        }
        let number = if is_int { number.trunc() } else { number };
        if number < low || high.is_some_and(|top| number > top) {
            let shown = if is_int { (number as i64).to_string() } else { repr_float(number) };
            let low = if is_int { (low as i64).to_string() } else { repr_float(low) };
            let range = high.map_or_else(|| format!(">= {low}"), |top| format!("between {low} and {}", repr_float(top)));
            self.warnings.push(format!("{source}: loopguard.{name}={shown} rejected (must be {range})"));
            return;
        }
        let stored = if is_int { json!(number as i64) } else { json!(number) };
        self.thresholds.insert(name.to_string(), stored);
    }

    pub fn set(&mut self, key: &str, value: &Value) {
        self.set_from(key, value, "?");
    }

    pub(crate) fn set_from(&mut self, key: &str, value: &Value, source: &str) {
        if let Some(name) = key.strip_prefix("loopguard.").or(key.strip_prefix("spiral.")) {
            self.set_threshold(name, value, source);
            return;
        }
        let key = canonical(key);
        if key == "paid" {
            return;
        }
        if key == "loopguard_thresholds" {
            let Some(map) = value.as_object() else {
                self.warnings.push(format!("{source}: loopguard_thresholds must be an object"));
                return;
            };
            for (name, item) in map {
                self.set_threshold(name, item, source);
            }
            return;
        }
        let on_value = match key {
            "microskills" => Some("envfacts+distiller"),
            "toolprune" => Some("auto"),
            "loopguard" => Some("nudge"),
            "repomap" => Some("on"),
            _ => None,
        };
        let is_switch_input = value.is_string() || value.is_boolean() || value.is_i64() || value.is_u64();
        let switched = on_value.filter(|_| is_switch_input).map(|on_value| match plain_text(value).trim().to_lowercase().as_str() {
            "on" | "true" | "yes" | "1" | "enable" | "enabled" => json!(on_value),
            "off" | "false" | "no" | "0" | "disable" | "disabled" => json!("off"),
            "guard" if key == "loopguard" => json!("stop"),
            _ => value.clone(),
        });
        let value = switched.as_ref().unwrap_or(value);

        match key {
            "enabled" => self.enabled = value.as_bool().unwrap_or_else(|| truthy(&plain_text(value))),
            "shadow" => self.shadow = value.as_bool().unwrap_or_else(|| truthy(&plain_text(value))),
            "toolprune_protect" => {
                let items: Vec<String> = match value {
                    Value::Array(items) => items.iter().map(plain_text).collect(),
                    other => plain_text(other).split(',').map(str::to_string).collect(),
                };
                let items: Vec<Value> = items.iter().map(|item| item.trim()).filter(|item| !item.is_empty()).map(|item| json!(item)).collect();
                self.rest.insert(key.to_string(), Value::Array(items));
            }
            key if int_setting(key).is_some() => {
                let low = int_setting(key).unwrap_or(1);
                match to_int(value).and_then(|number| if number < low { Err(format!("must be >= {low}")) } else { Ok(number) }) {
                    Ok(number) if key == "loopguard_nudges" => self.nudges = number,
                    Ok(number) => {
                        self.rest.insert(key.to_string(), json!(number));
                    }
                    Err(why) => self.warnings.push(format!("{source}: {key}={} rejected ({why})", repr(value))),
                }
            }
            "mode" | "microskills" | "toolprune" | "loopguard" | "repomap" | "loopguard_aggr" | "vocabulary" | "repomap_python" | "paid_model" => {
                let chosen = plain_text(value).trim().to_string();
                if let Some((_, choices)) = CHOICES.iter().find(|(name, _)| *name == key).filter(|(_, choices)| !choices.contains(&chosen.as_str())) {
                    let listed = choices.iter().map(|choice| repr_text(choice)).collect::<Vec<_>>().join(", ");
                    self.warnings.push(format!("{source}: {key}={} not one of ({listed})", repr_text(&chosen)));
                    return;
                }
                match key {
                    "mode" => self.mode = chosen,
                    "loopguard" => self.loopguard = chosen,
                    "repomap" => self.repomap = chosen,
                    "loopguard_aggr" => self.aggr = chosen,
                    other => {
                        self.rest.insert(other.to_string(), json!(chosen));
                    }
                }
            }
            other => self.warnings.push(format!("{source}: unknown key {}", repr_text(other))),
        }
    }

    fn apply_file(&mut self, path: &Path, label: &str) {
        let Some(Value::Object(map)) = fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()) else { return };
        let source = format!("{label} {}", path.display());
        // Within a file a new name wins over its old one.
        for (key, value) in map.iter().filter(|(key, _)| is_old_name(key)) {
            self.set_from(key, value, &source);
        }
        for (key, value) in map.iter().filter(|(key, _)| !is_old_name(key)) {
            self.set_from(key, value, &source);
        }
    }

    pub fn load(root: &Path, repo_dir: &Path) -> Self {
        let mut config = Config::new();
        config.apply_file(&root.join("config.json"), "global");
        config.apply_file(&repo_dir.join("config.json"), "repo");
        for (variable, key) in ENV {
            if let Some(value) = env(variable) {
                config.set_from(key, &json!(value), variable);
            }
        }
        for variable in ["TE_SPIRAL_THRESHOLDS", "TE_LOOPGUARD_THRESHOLDS"] {
            if let Some(text) = env(variable) {
                match serde_json::from_str::<Value>(&text) {
                    Ok(value) => config.set_from("loopguard_thresholds", &value, variable),
                    Err(_) => config.warnings.push(format!("{variable}: not valid JSON")),
                }
            }
        }
        config.enabled &= !truthy(&env("TE_DISABLE").unwrap_or_default());
        if config.loopguard == "stop" {
            config.warnings.push("loopguard=stop acts as nudge: the plugin no longer stops a turn; a turn still stuck after the redirect gets a notice saying how to stop it (set loopguard=nudge to silence this)".to_string());
        }

        config
    }

    pub fn as_dict(&self) -> Value {
        let rest = |key: &str| self.rest.get(key).cloned().unwrap_or(Value::Null);

        let mut out = Map::new();
        out.insert("enabled".to_string(), json!(self.enabled));
        out.insert("mode".to_string(), json!(self.mode));
        for key in ["microskills", "microskills_max_facts", "microskills_max_tokens", "microskills_propose_every", "toolprune", "toolprune_evidence", "toolprune_min_sessions", "toolprune_sessions", "toolprune_quiet_minutes", "toolprune_protect"] {
            out.insert(key.to_string(), rest(key));
        }
        out.insert("loopguard".to_string(), json!(self.loopguard));
        out.insert("loopguard_aggr".to_string(), json!(self.aggr));
        out.insert("loopguard_nudges".to_string(), json!(self.nudges));
        out.insert("loopguard_thresholds".to_string(), Value::Object(self.thresholds.clone()));
        out.insert("repomap".to_string(), json!(self.repomap));
        for key in ["repomap_tokens", "repomap_python", "repomap_timeout", "paid_model"] {
            out.insert(key.to_string(), rest(key));
        }
        out.insert("shadow".to_string(), json!(self.shadow));
        out.insert("vocabulary".to_string(), rest("vocabulary"));

        Value::Object(out)
    }
}

const PAID_GONE: &str = "the loop guard always asks its judge (`loopguard=off` turns it off), and micro-skills' proposer runs whenever micro-skills is on (`microskills=envfacts` keeps only the facts that need no model call)";

fn threshold_name(key: &str) -> Option<&str> {
    key.strip_prefix("loopguard.").or(key.strip_prefix("spiral."))
}

fn read_object(path: &Path) -> Map<String, Value> {
    match fs::read(path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()) {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

fn write_object(path: &Path, doc: Map<String, Value>) -> Result<Map<String, Value>, String> {
    let write = || -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        crate::paths::write_atomic(path, crate::economizer::dumps(&Value::Object(doc.clone()), Some(2)).as_bytes())
    };
    write().map_err(|error| error.to_string())?;

    Ok(doc)
}

fn drop_old_names(doc: &mut Map<String, Value>, key: &str) {
    for (old, new) in ALIASES {
        if new == key {
            doc.remove(old);
        }
    }
}

// Merge `updates` into the config file at `path`, each checked first, under the settings' current names.
pub fn write_config(path: &Path, updates: &[(String, String)]) -> Result<Map<String, Value>, String> {
    let gone: Vec<String> = updates.iter().filter(|(key, _)| canonical(key) == "paid").map(|(key, _)| format!("`{key}` no longer exists: {PAID_GONE}")).collect();
    if !gone.is_empty() {
        return Err(gone.join("; "));
    }
    let mut probe = Config::new();
    let mut bad = false;
    for (key, value) in updates {
        let before = probe.warnings.len();
        probe.set_from(key, &json!(value), "te config");
        bad |= probe.warnings.len() > before;
    }
    if bad {
        return Err(probe.warnings.join("; "));
    }
    let mut current = read_object(path);
    // An old file's thresholds move to the new name, so nothing pinned is lost.
    if let Some(old) = current.remove("spiral_thresholds") {
        if let Value::Object(mut old) = old {
            if let Some(Value::Object(newer)) = current.get("loopguard_thresholds") {
                old.extend(newer.clone());
            }
            current.insert("loopguard_thresholds".to_string(), Value::Object(old));
        }
    }
    let view = probe.as_dict();
    for (key, _) in updates {
        let table = |current: &mut Map<String, Value>| -> Map<String, Value> {
            match current.remove("loopguard_thresholds") {
                Some(Value::Object(table)) => table,
                _ => Map::new(),
            }
        };
        if let Some(name) = threshold_name(key) {
            let mut pinned = table(&mut current);
            pinned.insert(name.to_string(), probe.thresholds[name].clone());
            current.insert("loopguard_thresholds".to_string(), Value::Object(pinned));
        } else if canonical(key) == "loopguard_thresholds" {
            let mut pinned = table(&mut current);
            pinned.extend(probe.thresholds.clone());
            current.insert("loopguard_thresholds".to_string(), Value::Object(pinned));
        } else {
            let key = canonical(key);
            drop_old_names(&mut current, key);
            current.insert(key.to_string(), view[key].clone());
        }
    }

    write_object(path, current)
}

// Remove keys, by either name, so that the next source down applies again.
pub fn unset_config(path: &Path, keys: &[String]) -> Result<Map<String, Value>, String> {
    let known = Config::new().as_dict();
    let bad: Vec<&str> = keys.iter().map(String::as_str).filter(|key| known.get(canonical(key)).is_none() && !threshold_name(key).is_some_and(|name| THRESHOLDS.contains(&name))).collect();
    if !bad.is_empty() {
        return Err(format!("unknown key(s): {}", bad.join(", ")));
    }
    let mut current = read_object(path);
    for key in keys {
        if let Some(name) = threshold_name(key) {
            for table in ["loopguard_thresholds", "spiral_thresholds"] {
                if let Some(Value::Object(pinned)) = current.get_mut(table) {
                    pinned.remove(name);
                }
                if current.get(table).is_some_and(|pinned| !crate::economizer::python_truthy(pinned)) {
                    current.remove(table);
                }
            }
        } else {
            let key = canonical(key);
            current.remove(key);
            drop_old_names(&mut current, key);
        }
    }

    write_object(path, current)
}
