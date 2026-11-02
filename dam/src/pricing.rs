// Cache-aware effective tokens (Apache-2.0, Microsoft), rewritten from pricing.py and plugin/accounting.py.
use serde_json::Value;

// input, cache read, cache write, output: fresh-input-token equivalents.
type Weights = [f64; 4];

const CLAUDE: Weights = [1.0, 0.10, 1.25, 5.0];
const FALLBACK: Weights = [1.0, 0.10, 1.25, 6.0];
// The providers' published prices, snapshot 2026-07-27.
const MODELS: &[(&str, Weights)] = &[
    ("claude-haiku-4-5", CLAUDE),
    ("claude-opus-4-6", CLAUDE),
    ("claude-opus-4-7", CLAUDE),
    ("claude-opus-4-8", CLAUDE),
    ("claude-opus-5", CLAUDE),
    ("claude-sonnet-4-5", CLAUDE),
    ("claude-sonnet-4-6", CLAUDE),
    ("claude-sonnet-5", CLAUDE),
    ("gemini-2.5-pro", [1.0, 0.096, 0.0, 8.0]),
    ("gemini-3-flash-preview", [1.0, 0.10, 0.0, 6.0]),
    ("gemini-3.1-pro-preview", [1.0, 0.10, 0.0, 6.0]),
    ("gemini-3.5-flash", [1.0, 0.10, 0.0, 6.0]),
    ("gemini-3.6-flash", [1.0, 0.10, 0.0, 5.0]),
    ("gpt-5-mini", [1.0, 0.08, 0.0, 8.0]),
    ("gpt-5.3-codex", [1.0, 0.0971, 0.0, 8.0]),
    ("gpt-5.4", [1.0, 0.10, 0.0, 6.0]),
    ("gpt-5.4-mini", [1.0, 0.0933, 0.0, 6.0]),
    ("gpt-5.5", [1.0, 0.10, 0.0, 6.0]),
    ("gpt-5.6-luna", [1.0, 0.10, 1.25, 6.0]),
    ("gpt-5.6-sol", [1.0, 0.10, 1.25, 6.0]),
    ("gpt-5.6-terra", [1.0, 0.10, 1.248, 6.0]),
    ("mai-code-1-flash-picker", [1.0, 0.0933, 0.0, 6.0]),
    ("gpt-5", [1.0, 0.10, 1.0, 8.0]),
    ("gpt-5.1", [1.0, 0.10, 1.0, 8.0]),
    ("gpt-5-codex", [1.0, 0.10, 1.0, 8.0]),
    ("gpt-5.1-codex", [1.0, 0.10, 1.0, 8.0]),
    ("gpt-5.1-codex-max", [1.0, 0.10, 1.0, 8.0]),
    ("gpt-5.2-codex", [1.0, 0.10, 1.0, 8.0]),
    ("codex-mini-latest", [1.0, 0.25, 1.0, 4.0]),
];

// None for a billed id that is not priced: a router's `provider/model` is priced by its tail.
fn weights_for(model: Option<&str>) -> Option<Weights> {
    let id = model.map_or("", |model| model.rsplit('/').next().unwrap_or(model));
    if id.is_empty() {
        return Some(CLAUDE);
    }
    let key = id.to_lowercase();
    match MODELS.iter().find(|(name, _)| *name == key) {
        Some((_, weights)) => Some(*weights),
        None => key.contains("claude").then_some(CLAUDE),
    }
}

fn count(usage: &Value, key: &str) -> f64 {
    match usage.get(key) {
        Some(Value::Number(number)) => number.as_f64().unwrap_or(0.0),
        Some(Value::Bool(true)) => 1.0,
        _ => 0.0,
    }
}

// (effective tokens, how they were priced); nothing without a usage breakdown.
pub fn effective(usage: &Value) -> (Option<f64>, &'static str) {
    if usage.get("input_tokens").is_none_or(Value::is_null) {
        return (None, "no_usage");
    }
    let model = usage.get("model").and_then(Value::as_str);
    let (weights, priced_by) = match weights_for(model) {
        Some(weights) => (weights, "model"),
        None => (FALLBACK, "fallback"),
    };

    (Some(count(usage, "input_tokens") * weights[0] + count(usage, "cache_read_input_tokens") * weights[1] + count(usage, "cache_creation_input_tokens") * weights[2] + count(usage, "output_tokens") * weights[3]), priced_by)
}

// About four characters a token.
pub fn approx_tokens(text: &str) -> i64 {
    ((text.chars().count() / 4) as i64).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_model_is_priced_by_its_own_id() {
        let usage = |model: &str| json!({"input_tokens": 10, "cache_read_input_tokens": 100, "cache_creation_input_tokens": 4, "output_tokens": 2, "model": model});
        assert_eq!(effective(&usage("claude-opus-5")), (Some(10.0 + 10.0 + 5.0 + 10.0), "model"));
        assert_eq!(effective(&usage("anthropic/claude-haiku-4.5")).1, "model");
        assert_eq!(effective(&usage("gpt-5.3-codex")), (Some(10.0 + 100.0 * 0.0971 + 16.0), "model"));
        assert_eq!(effective(&usage("mystery-1")), (Some(10.0 + 10.0 + 5.0 + 12.0), "fallback"));
        assert_eq!(effective(&json!({"input_tokens": null})), (None, "no_usage"));
    }
}
