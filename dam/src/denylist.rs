// The deny list in .claude/settings.local.json (Apache-2.0, Microsoft), rewritten from plugin/denylist.py.
use crate::economizer::{now, python_truthy, Store};
use crate::paths::realpath;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const APPLIED: &str = "untool_applied.json";

// Per settings file, in the order the record holds them.
type ByPath = Vec<(String, Vec<String>)>;

pub fn settings_path(repo_root: &str) -> PathBuf {
    Path::new(repo_root).join(".claude").join("settings.local.json")
}

fn canon(path: &str) -> String {
    if path.is_empty() { String::new() } else { realpath(Path::new(path)).to_string_lossy().into_owned() }
}

fn strings(value: Option<&Value>) -> Vec<String> {
    value.and_then(Value::as_array).map_or_else(Vec::new, |items| items.iter().filter_map(Value::as_str).map(str::to_string).collect())
}

fn put(by: &mut ByPath, path: String, items: Vec<String>) {
    match by.iter_mut().find(|(known, _)| *known == path) {
        Some(entry) => entry.1 = items,
        None => by.push((path, items)),
    }
}

fn paths_of(map: Option<&Value>) -> ByPath {
    let mut by = Vec::new();
    for (path, items) in map.and_then(Value::as_object).into_iter().flatten() {
        put(&mut by, canon(path), strings(Some(items)));
    }

    by
}

// The record holds one file's entries as {"tools", "path"} up to 0.1.0; that shape is still read.
fn applied_by_path(doc: &Value) -> ByPath {
    if doc.get("by_path").is_some_and(Value::is_object) {
        return paths_of(doc.get("by_path"));
    }
    let mut by = Vec::new();
    if doc.get("tools").is_some_and(python_truthy) {
        put(&mut by, canon(doc.get("path").and_then(Value::as_str).unwrap_or("")), strings(doc.get("tools")));
    }

    by
}

fn listing(by: &ByPath) -> Value {
    Value::Object(by.iter().filter(|(_, items)| !items.is_empty()).map(|(path, items)| {
        let mut sorted = items.clone();
        sorted.sort();
        (path.clone(), json!(sorted))
    }).collect())
}

fn write_applied(store: &Store, by: &ByPath, kept: &ByPath) -> Result<(), String> {
    store.write(APPLIED, &json!({"by_path": listing(by), "kept": listing(kept), "updated": now()})).map_err(|e| e.to_string())
}

fn applied_at(store: &Store, path: &Path) -> Vec<String> {
    let by = applied_by_path(&store.read(APPLIED).unwrap_or(Value::Null));
    let find = |key: &str| by.iter().find(|(known, _)| known == key).map(|(_, items)| items.clone()).filter(|items| !items.is_empty());

    find(&canon(&path.to_string_lossy())).or_else(|| find("")).unwrap_or_default()
}

// What the plugin added to this checkout's settings file.
pub fn applied_for(store: &Store, repo_root: &str) -> Vec<String> {
    applied_at(store, &settings_path(repo_root))
}

// What the person gave back in this checkout, which `auto` must leave alone.
pub fn kept_for(store: &Store, repo_root: &str) -> Vec<String> {
    let kept = paths_of(store.read(APPLIED).unwrap_or(Value::Null).get("kept"));
    let wanted = canon(&settings_path(repo_root).to_string_lossy());

    kept.into_iter().find(|(path, _)| *path == wanted).map(|(_, items)| items).unwrap_or_default()
}

// Marks entries as given back (`restore`), or no longer (`apply` by name).
pub fn set_kept(store: &Store, repo_root: &str, add: &[String], remove: &[String]) -> Result<(), String> {
    let doc = store.read(APPLIED).unwrap_or(Value::Null);
    let mut kept = paths_of(doc.get("kept"));
    let path = canon(&settings_path(repo_root).to_string_lossy());
    let mut now_kept: BTreeSet<String> = kept.iter().find(|(known, _)| *known == path).map(|(_, items)| items.iter().cloned().collect()).unwrap_or_default();
    now_kept.extend(add.iter().cloned());
    now_kept.retain(|item| !remove.contains(item));
    put(&mut kept, path, now_kept.into_iter().collect());

    write_applied(store, &applied_by_path(&doc), &kept)
}

fn record(store: &Store, items: &BTreeSet<String>, path: &Path) -> Result<(), String> {
    let doc = store.read(APPLIED).unwrap_or(Value::Null);
    let mut by = applied_by_path(&doc);
    by.retain(|(known, _)| !known.is_empty());
    put(&mut by, canon(&path.to_string_lossy()), items.iter().cloned().collect());

    write_applied(store, &by, &paths_of(doc.get("kept")))
}

// The settings object at `path`: empty when there is no file. One that is there but is not a JSON
// object is refused rather than written over -- it is the person's.
fn read_settings(path: &Path) -> Result<Map<String, Value>, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Map::new()),
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    };
    let text = String::from_utf8(bytes).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str::<Value>(&text) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(format!("refusing to edit {}: it is not a JSON object", path.display())),
        Err(e) => Err(format!("refusing to edit {}: it is not valid JSON ({e}); fix it first", path.display())),
    }
}

// A write must not leave the repository through a symlink: a checkout decides what these paths are.
fn guard_target(repo_root: &str, path: &Path) -> Result<(), String> {
    let low = |p: PathBuf| if cfg!(windows) { PathBuf::from(p.to_string_lossy().to_lowercase()) } else { p };
    let root = low(realpath(Path::new(repo_root)));
    let parent = low(realpath(path.parent().unwrap_or(Path::new(""))));
    if fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink()) || !parent.starts_with(&root) {
        return Err(format!("refusing to write {}: it is, or goes through, a symlink that leads out of the repository", path.display()));
    }

    Ok(())
}

// Python's json.dump(indent=2): the same text, with non-ASCII escaped and the keys in their order.
pub(crate) fn pretty(value: &Value) -> String {
    let mut out = String::new();
    for c in serde_json::to_string_pretty(value).unwrap_or_default().chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            for unit in c.encode_utf16(&mut [0u16; 2]) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }

    out
}

pub(crate) fn write_json(path: &Path, value: &Value) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let temp = PathBuf::from(format!("{}.te.tmp", path.display()));
    fs::write(&temp, pretty(value) + "\n")?;

    fs::rename(&temp, path)
}

// `.git/info/exclude` is the repository's local-only ignore: a file meant to be per-user is never committed by accident.
fn exclude_from_git(repo_root: &str) -> Option<()> {
    let dot = Path::new(repo_root).join(".git");
    let gitdir = if dot.is_dir() {
        dot
    } else {
        // A `.git` FILE names its git directory, so a checkout could point it anywhere: follow it only to a git directory.
        let line: String = fs::read_to_string(&dot).ok()?.chars().take(4096).collect();
        let named = Path::new(line.trim().strip_prefix("gitdir:")?.trim()).to_path_buf();
        let gitdir = if named.is_absolute() { named } else { dot.parent()?.join(named) };
        if !gitdir.join("HEAD").is_file() {
            return None;
        }
        match fs::read_to_string(gitdir.join("commondir")) {
            Ok(common) => gitdir.join(common.trim()),
            Err(_) => gitdir,
        }
    };
    let info = gitdir.join("info");
    let path = info.join("exclude");
    let linked = |p: &Path| fs::symlink_metadata(p).is_ok_and(|meta| meta.file_type().is_symlink());
    if linked(&info) || linked(&path) {
        return None;
    }
    let entry = "/.claude/settings.local.json";
    let existing = if path.exists() { fs::read_to_string(&path).ok()? } else { String::new() };
    if existing.split_whitespace().any(|word| word == entry) {
        return Some(());
    }
    fs::create_dir_all(&info).ok()?;
    let mut file = OpenOptions::new().create(true).append(true).open(&path).ok()?;
    let gap = if existing.is_empty() || existing.ends_with('\n') { "" } else { "\n" };

    write!(file, "{gap}# added by TokenEconomizer (per-user tool deny list)\n{entry}\n").ok()
}

// Adds `items` to `permissions.deny`; returns those newly added.
pub fn apply(repo_root: &str, items: &[String], store: &Store) -> Result<Vec<String>, String> {
    let path = settings_path(repo_root);
    guard_target(repo_root, &path)?;
    let created = !path.exists();
    let mut data = read_settings(&path)?;
    let refuse = |what: &str| format!("refusing to edit {}: {what} is not what a settings file holds there", path.display());
    let permissions = data.entry("permissions").or_insert_with(|| json!({})).as_object_mut().ok_or_else(|| refuse("permissions"))?;
    let deny = permissions.entry("deny").or_insert_with(|| json!([])).as_array_mut().ok_or_else(|| refuse("permissions.deny"))?;
    let added: Vec<String> = items.iter().filter(|item| !deny.iter().any(|rule| rule.as_str() == Some(item.as_str()))).cloned().collect();
    if added.is_empty() {
        return Ok(added);
    }
    deny.extend(added.iter().map(|item| json!(item)));
    // Recorded before written, as `restore` writes before it forgets: whichever write fails, the record still covers every entry of the plugin's in the file.
    let mut all: BTreeSet<String> = applied_at(store, &path).into_iter().collect();
    all.extend(added.iter().cloned());
    record(store, &all, &path)?;
    write_json(&path, &Value::Object(data)).map_err(|e| e.to_string())?;
    if created {
        exclude_from_git(repo_root);
    }

    Ok(added)
}

// Removes entries this plugin added (all of them when `items` is empty); returns those removed.
pub fn restore(repo_root: &str, store: &Store, items: &[String]) -> Result<Vec<String>, String> {
    let path = settings_path(repo_root);
    let mine: BTreeSet<String> = applied_at(store, &path).into_iter().collect();
    let target: BTreeSet<String> = if items.is_empty() { mine.clone() } else { mine.iter().filter(|item| items.contains(item)).cloned().collect() };
    if target.is_empty() {
        return Ok(Vec::new());
    }
    guard_target(repo_root, &path)?;
    let mut data = read_settings(&path)?;
    let refuse = |what: &str| format!("refusing to edit {}: {what} is not what a settings file holds there", path.display());
    let (mut removed, mut changed) = (Vec::new(), false);
    let mut drop_permissions = false;
    match data.get_mut("permissions") {
        Some(Value::Object(holder)) => {
            if holder.contains_key("deny") {
                let listed = match holder.get("deny") {
                    Some(Value::Array(rules)) => rules.clone(),
                    Some(other) if !python_truthy(other) => Vec::new(),
                    _ => return Err(refuse("permissions.deny")),
                };
                let (gone, kept): (Vec<Value>, Vec<Value>) = listed.into_iter().partition(|rule| rule.as_str().is_some_and(|name| target.contains(name)));
                removed = gone.iter().filter_map(Value::as_str).map(str::to_string).collect();
                if kept.is_empty() {
                    holder.shift_remove("deny");
                } else {
                    holder.insert("deny".to_string(), Value::Array(kept));
                }
                drop_permissions = holder.is_empty();
                changed = true;
            }
        }
        Some(other) if python_truthy(other) => return Err(refuse("permissions")),
        _ => {}
    }
    if drop_permissions {
        data.shift_remove("permissions");
    }
    if changed {
        write_json(&path, &Value::Object(data)).map_err(|e| e.to_string())?;
    }
    record(store, &mine.difference(&target).cloned().collect(), &path)?;

    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dam-denylist-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn apply_and_restore_touch_only_their_own_rules() {
        let dir = scratch("own");
        let (repo, state) = (dir.join("repo"), Store { base: dir.join("state") });
        let root = repo.to_string_lossy().into_owned();
        let path = settings_path(&root);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, r#"{"theme": "dark", "permissions": {"deny": ["Mine"], "allow": ["x"]}}"#).unwrap();

        let words = |list: &[&str]| list.iter().map(|word| word.to_string()).collect::<Vec<_>>();
        assert_eq!(apply(&root, &words(&["Mine", "A", "B"]), &state).unwrap(), words(&["A", "B"]));
        assert_eq!(applied_for(&state, &root), words(&["A", "B"]));
        assert_eq!(restore(&root, &state, &words(&["A"])).unwrap(), words(&["A"]));
        assert_eq!(restore(&root, &state, &[]).unwrap(), words(&["B"]));
        let after: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(after, json!({"theme": "dark", "permissions": {"deny": ["Mine"], "allow": ["x"]}}));
        assert!(applied_for(&state, &root).is_empty());
        set_kept(&state, &root, &words(&["A"]), &[]).unwrap();
        assert_eq!(kept_for(&state, &root), words(&["A"]));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_settings_file_that_is_not_an_object_is_left_alone() {
        let dir = scratch("refuse");
        let (repo, state) = (dir.join("repo"), Store { base: dir.join("state") });
        let root = repo.to_string_lossy().into_owned();
        let path = settings_path(&root);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        for text in ["[1]", "{not json"] {
            fs::write(&path, text).unwrap();
            let error = apply(&root, &["A".to_string()], &state).unwrap_err();
            assert!(error.starts_with("refusing to edit"), "{error}");
            assert_eq!(fs::read_to_string(&path).unwrap(), text);
        }
        let _ = fs::remove_dir_all(&dir);
    }
}
