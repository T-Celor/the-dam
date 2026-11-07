// Which Claude model does what: Sonnet 5.5 runs the session, Opus 5.5 advises, Haiku 5.5 does the workers' tasks.
// A plugin cannot set the session's model or the advisor (only the person's own settings can), so `dam te models install`
// writes them there, remembering what was in place, and `uninstall` puts it back. The workers are pinned by the module.
use serde_json::{json, Map, Value};
use std::path::PathBuf;

pub const MAIN: &str = "claude-sonnet-5-5";
pub const ADVISOR: &str = "claude-opus-5-5";
pub const WORKER: &str = "claude-haiku-5-5";
const KEYS: [(&str, &str); 2] = [("model", MAIN), ("advisorModel", ADVISOR)];

fn settings_path() -> PathBuf {
    crate::channels::claude_dir().join("settings.json")
}

fn backup_path() -> PathBuf {
    crate::economizer::state_root().join("models-backup.json")
}

// `settings` with the two models set (`backup` gets what they were), or put back from `backup`. Other keys are left as they are.
pub fn with_models(settings: &str, on: bool, backup: &mut Map<String, Value>) -> Result<String, String> {
    let mut doc: Map<String, Value> = if settings.trim().is_empty() { Map::new() } else { serde_json::from_str(settings).map_err(|error| format!("not JSON the dam can edit ({error}): left as it is"))? };
    for (key, model) in KEYS {
        if on {
            // Kept once: a second install must not replace the person's own choice with ours.
            backup.entry(key.to_string()).or_insert_with(|| doc.get(key).cloned().unwrap_or(Value::Null));
            doc.insert(key.to_string(), json!(model));
        } else if let Some(was) = backup.remove(key) {
            // Only what is still ours: a model the person changed since is theirs.
            if doc.get(key).and_then(Value::as_str) == Some(model) {
                if was.is_null() {
                    doc.remove(key);
                } else {
                    doc.insert(key.to_string(), was);
                }
            }
        }
    }

    Ok(format!("{}\n", serde_json::to_string_pretty(&Value::Object(doc)).unwrap_or_default()))
}

pub fn show() -> String {
    let doc: Value = std::fs::read_to_string(settings_path()).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or(Value::Null);
    let now = |key: &str| doc[key].as_str().unwrap_or("not set").to_string();

    format!("session model   {} (wanted: {MAIN})\nadvisor         {} (wanted: {ADVISOR})\nworkers         {WORKER}, pinned by the plugin's module for Claude Code's built-in subagents; the plugin's `worker` agent uses it too\nsettings        {}", now("model"), now("advisorModel"), settings_path().display())
}

// `dam te models install|uninstall`.
pub fn set(on: bool) -> Result<String, String> {
    let (path, backup_file) = (settings_path(), backup_path());
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("{}: {error}; left as it is", path.display())),
    };
    let mut backup: Map<String, Value> = std::fs::read_to_string(&backup_file).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
    let next = with_models(&text, on, &mut backup).map_err(|why| format!("{}: {why}", path.display()))?;
    crate::paths::write_atomic(&backup_file, serde_json::to_string_pretty(&Value::Object(backup)).unwrap_or_default().as_bytes()).map_err(|error| format!("{}: {error}", backup_file.display()))?;
    crate::paths::write_atomic(&path, next.as_bytes()).map_err(|error| format!("{}: {error}", path.display()))?;

    Ok(if on { format!("session model {MAIN}, advisor {ADVISOR} written to {}; they apply from the next Claude Code start (a --model on the command line still wins)", path.display()) } else { format!("the models the dam set were put back in {}", path.display()) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_models_are_set_and_put_back_and_the_rest_is_kept() {
        let mut backup = Map::new();
        let on = with_models("{\"theme\": \"dark\", \"model\": \"opus\"}", true, &mut backup).unwrap();
        let doc: Value = serde_json::from_str(&on).unwrap();
        assert_eq!((doc["model"].as_str(), doc["advisorModel"].as_str(), doc["theme"].as_str()), (Some(MAIN), Some(ADVISOR), Some("dark")));
        // A second install keeps the first backup.
        let again = with_models(&on, true, &mut backup).unwrap();
        assert_eq!(backup["model"], "opus");
        let off: Value = serde_json::from_str(&with_models(&again, false, &mut backup).unwrap()).unwrap();
        assert_eq!((off["model"].as_str(), off.get("advisorModel"), off["theme"].as_str()), (Some("opus"), None, Some("dark")));
    }

    #[test]
    fn a_model_the_person_changed_since_is_left_alone() {
        let mut backup = Map::new();
        with_models("{}", true, &mut backup).unwrap();
        let off: Value = serde_json::from_str(&with_models("{\"model\": \"haiku\", \"advisorModel\": \"claude-opus-5-5\"}", false, &mut backup).unwrap()).unwrap();
        assert_eq!(off["model"], "haiku");
        assert!(off.get("advisorModel").is_none());
        assert!(with_models("{ nope", true, &mut Map::new()).is_err());
    }
}
