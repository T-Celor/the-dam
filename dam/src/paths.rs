// Which repository a directory belongs to (Apache-2.0, Microsoft), rewritten from plugin/paths.py:
// one key however it is checked out, read from `.git` directly and with no call to git.
use fancy_regex::Regex;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

static REMOTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"^\s*\[remote\s+"([^"]+)"\]\s*$"#).expect("fixed"));
static URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*url\s*=\s*(\S+)\s*$").expect("fixed"));
static SCHEME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-z+]+://").expect("fixed"));
static USER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[^@/]+@").expect("fixed"));
static SCP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[^/]+:[^/\d]").expect("fixed"));
static DRIVE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[a-zA-Z]:/").expect("fixed"));
static SLASHES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"/+").expect("fixed"));
static UNSAFE_NAME: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"[<>:"/\\|?*]+"#).expect("fixed"));

// One file or folder name for a repository's state (names.py): `owner/repo` is `owner__repo`, and a label holding a
// Windows path stays one name. Only trailing dots and spaces go, which Windows refuses; a leading dot stays.
pub fn repo_filename(repo: &str) -> String {
    let name = UNSAFE_NAME.replace_all(repo, "__");
    let name = name.trim_end_matches(['.', ' ']);

    if name.is_empty() { "repo".to_string() } else { name.to_string() }
}
static NOT_SLUG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^a-z0-9]+").expect("fixed"));

#[derive(Debug, Clone, PartialEq)]
pub struct RepoId {
    pub root: String,
    pub key: String,
    pub label: String,
    pub remote: String,
    pub how: String,
}

fn normpath(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir if matches!(out.components().next_back(), Some(Component::Normal(_))) => {
                out.pop();
            }
            Component::ParentDir if out.has_root() => {}
            other => out.push(other),
        }
    }

    out
}

pub fn abspath(path: &Path) -> PathBuf {
    normpath(&std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()))
}

fn plain_verbatim(path: PathBuf) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(share) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{share}"));
    }
    text.strip_prefix(r"\\?\").map_or_else(|| path.clone(), PathBuf::from)
}

// As os.path.realpath: what exists is resolved, what does not is kept as written.
// Replaces `path` whole or not at all. The temporary file beside it is created new (O_EXCL, which follows no link):
// a repository could have planted a link at that name, to send the write to the person's own settings.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        fs::create_dir_all(dir)?;
    }
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |at| at.subsec_nanos());
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".dam-{}-{nanos:x}.tmp", std::process::id()));
    let temp = path.with_file_name(name);
    // Only the link goes, never what it points to.
    let _ = fs::remove_file(&temp);
    let written = fs::OpenOptions::new().write(true).create_new(true).open(&temp).and_then(|mut file| file.write_all(bytes).and_then(|()| file.sync_all()));
    match written.and_then(|()| fs::rename(&temp, path)) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&temp);
            Err(error)
        }
    }
}

// Runs `work` while holding the lock file `path` (created new, so only one process has it, and holding a token only this call
// wrote). Hooks of parallel tool calls share files of state; each read-modify-write is kept apart this way. A lock older than
// thirty seconds (more than a hook's own 25 s watchdog) is a dead process's and is taken over; after three seconds of waiting
// `work` runs anyway, because an agent is never held up for bookkeeping. Only the lock this call made is given back.
pub fn with_lock<T>(path: &Path, work: impl FnOnce() -> T) -> T {
    use std::io::Write;
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        let _ = fs::create_dir_all(dir);
    }
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |at| at.as_nanos());
    let token = format!("{}-{nanos:x}", std::process::id());
    let started = std::time::Instant::now();
    let mut held = false;
    while started.elapsed() < std::time::Duration::from_secs(3) {
        if fs::OpenOptions::new().write(true).create_new(true).open(path).and_then(|mut file| file.write_all(token.as_bytes())).is_ok() {
            held = true;
            break;
        }
        let age = fs::metadata(path).and_then(|m| m.modified()).ok().and_then(|at| at.elapsed().ok());
        if age.is_some_and(|age| age > std::time::Duration::from_secs(30)) {
            let _ = fs::remove_file(path);
        } else {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
    let result = work();
    if held && fs::read_to_string(path).is_ok_and(|now| now == token) {
        let _ = fs::remove_file(path);
    }

    result
}

pub fn realpath(path: &Path) -> PathBuf {
    let whole = abspath(path);
    let mut existing = whole.as_path();
    loop {
        if let Ok(resolved) = fs::canonicalize(existing) {
            let rest = whole.strip_prefix(existing).unwrap_or(Path::new(""));
            let resolved = plain_verbatim(resolved);
            return if rest.as_os_str().is_empty() { resolved } else { resolved.join(rest) };
        }
        match existing.parent() {
            Some(parent) => existing = parent,
            None => return whole,
        }
    }
}

fn home_dir() -> Option<PathBuf> {
    let home = ["HOME", "USERPROFILE"].iter().find_map(|name| std::env::var_os(name).filter(|value| !value.is_empty()))?;

    Some(realpath(Path::new(&home)))
}

fn gitdir_of_file(dot: &Path) -> Option<PathBuf> {
    let text = fs::read_to_string(dot).ok()?;
    let line: String = text.chars().take(4096).collect();
    let gitdir = line.trim().strip_prefix("gitdir:")?.trim();
    let gitdir = Path::new(gitdir);

    Some(if gitdir.is_absolute() { gitdir.to_path_buf() } else { normpath(&dot.parent()?.join(gitdir)) })
}

fn is_git_entry(dot: &Path) -> bool {
    if dot.is_dir() {
        return dot.join("HEAD").is_file();
    }

    gitdir_of_file(dot).is_some_and(|gitdir| gitdir.is_dir())
}

pub fn find_repo_root(start: &Path) -> Option<PathBuf> {
    let ceilings: Vec<PathBuf> = std::env::var_os("GIT_CEILING_DIRECTORIES").map_or_else(Vec::new, |list| std::env::split_paths(&list).filter(|dir| !dir.as_os_str().is_empty()).map(|dir| abspath(&dir)).collect());
    let home = home_dir();
    let mut current = abspath(start);
    loop {
        if is_git_entry(&current.join(".git")) {
            return Some(current);
        }
        let parent = current.parent()?.to_path_buf();
        // A versioned home directory is the repository of `~` itself, not of every project below it.
        if ceilings.contains(&parent) || home.as_ref().is_some_and(|home| realpath(&parent) == *home) {
            return None;
        }
        current = parent;
    }
}

fn git_common_dir(root: &Path) -> Option<PathBuf> {
    let dot = root.join(".git");
    if dot.is_dir() {
        return Some(dot);
    }
    let gitdir = gitdir_of_file(&dot)?;

    Some(match fs::read_to_string(gitdir.join("commondir")) {
        Ok(common) => normpath(&gitdir.join(common.trim())),
        Err(_) => gitdir,
    })
}

pub fn origin_url(root: &Path) -> Option<String> {
    let config = fs::read(git_common_dir(root)?.join("config")).ok()?;
    let config = String::from_utf8_lossy(&config);
    let (mut remotes, mut current): (Vec<(String, String)>, Option<String>) = (Vec::new(), None);
    for line in config.lines() {
        if let Ok(Some(found)) = REMOTE.captures(line) {
            current = Some(found[1].to_string());
            continue;
        }
        if line.trim_start().starts_with('[') {
            current = None;
            continue;
        }
        if let (Some(name), Ok(Some(found))) = (&current, URL.captures(line)) {
            if !remotes.iter().any(|(known, _)| known == name) {
                remotes.push((name.clone(), found[1].to_string()));
            }
        }
    }
    let origin = remotes.iter().find(|(name, _)| name == "origin").filter(|(_, url)| !url.is_empty());

    origin.or(remotes.first()).map(|(_, url)| url.clone())
}

pub fn normalize_remote(url: &str) -> String {
    let text = url.trim();
    let text = SCHEME.replace(text, "");
    let text = USER.replace(&text, "");
    let text = SLASHES.replace_all(&text.replace('\\', "/"), "/").to_string();
    let text = if !DRIVE.is_match(&text).unwrap_or(false) && SCP.is_match(&text).unwrap_or(false) { text.replacen(':', "/", 1) } else { text };
    let text = text.trim_end_matches('/');

    text.strip_suffix(".git").unwrap_or(text).to_lowercase()
}

pub fn slug(text: &str) -> String {
    let lower = if text.is_empty() { "repo".to_string() } else { text.to_lowercase() };
    let dashed = NOT_SLUG.replace_all(&lower, "-");
    let cut: String = dashed.trim_matches('-').chars().take(40).collect();

    if cut.is_empty() { "repo".to_string() } else { cut }
}

fn digest(canon: &str) -> String {
    sha1_smol::Sha1::from(canon).digest().to_string()[..10].to_string()
}

fn main_checkout(root: &Path) -> Option<PathBuf> {
    let common = normpath(&git_common_dir(root)?);

    (common.file_name()? == ".git").then(|| common.parent().map(Path::to_path_buf))?
}

fn base_name(path: &Path) -> String {
    path.file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned())
}

pub fn repo_identity(cwd: &Path) -> RepoId {
    let found = find_repo_root(cwd);
    let root = found.clone().unwrap_or_else(|| abspath(cwd));
    let remote = if found.is_some() { origin_url(&root).map_or_else(String::new, |url| normalize_remote(&url)) } else { String::new() };
    let (how, canon, name, label) = if !remote.is_empty() {
        let parts: Vec<&str> = remote.split('/').collect();
        let name = remote.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string();
        ("remote", remote.clone(), name, parts[parts.len().saturating_sub(2)..].join("/"))
    } else {
        let main = if found.is_some() { main_checkout(&root).or_else(|| git_common_dir(&root)).unwrap_or_else(|| root.clone()) } else { root.clone() };
        let name = base_name(&main);
        (if found.is_some() { "path" } else { "dir" }, realpath(&main).to_string_lossy().into_owned(), name.clone(), name)
    };

    RepoId { root: root.to_string_lossy().into_owned(), key: format!("{}-{}", slug(&name), digest(&canon)), label: if label.is_empty() { "repo".to_string() } else { label }, remote, how: how.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_remote_has_one_spelling() {
        for url in ["git@github.com:Owner/Repo.git", "https://github.com/owner/repo", "ssh://git@github.com/owner/repo.git/", "https://user@github.com/Owner/Repo"] {
            assert_eq!(normalize_remote(url), "github.com/owner/repo", "{url}");
        }
        assert_eq!(normalize_remote("host:2222/repo"), "host:2222/repo");
    }

    #[test]
    fn a_slug_is_short_and_never_empty() {
        assert_eq!(slug("The Dam!"), "the-dam");
        assert_eq!(slug("***"), "repo");
        assert_eq!(slug(""), "repo");
        assert_eq!(slug(&"a".repeat(50)).len(), 40);
    }

    #[test]
    fn the_key_digest_is_sha1() {
        assert_eq!(digest("abc"), "a9993e3647");
    }
}

#[cfg(test)]
mod tests_upstream {
    use super::*;

    #[test]
    fn an_atomic_write_replaces_the_whole_file_and_leaves_no_temporary_one() {
        let dir = crate::session::TempDir::new("dam-paths-").unwrap();
        let path = dir.0.join("sub").join("settings.json");
        write_atomic(&path, b"one").unwrap();
        write_atomic(&path, b"two").unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"two");
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }

    // The values upstream's paths.normalize_remote and names.repo_filename give.
    #[test]
    fn windows_remotes_and_state_names_match_upstream() {
        for (remote, want) in [(r"C:\Work\Sample\Repo", "c:/work/sample/repo"), (r"C:\Work\Sample\Repo", "c:/work/sample/repo"), ("C:/Work/Sample/Repo.git", "c:/work/sample/repo"), ("git@example.com:Owner/Repo.git", "example.com/owner/repo"), ("https://example.com/Owner/Repo", "example.com/owner/repo"), ("ssh://git@host:22/a/b.git", "host:22/a/b"), ("host:path/x", "host/path/x")] {
            assert_eq!(normalize_remote(remote), want, "{remote}");
        }
        for (label, want) in [("owner/repo", "owner__repo"), (".dotfiles", ".dotfiles"), (r"C:\Work\Repo", "C__Work__Repo"), ("a:b", "a__b"), ("trailing. ", "trailing"), ("...", "repo"), ("x<y>z|w?*", "x__y__z__w__")] {
            assert_eq!(repo_filename(label), want, "{label}");
        }
    }
}

#[cfg(test)]
mod tests_lock {
    use super::*;

    #[test]
    fn parallel_workers_take_turns_on_one_counter() {
        let dir = std::env::temp_dir().join(format!("dam-lock-{}", std::process::id()));
        let counter = dir.join("n.txt");
        let lock = dir.join("n.lock");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&counter, "0").unwrap();
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| {
                    for _ in 0..10 {
                        with_lock(&lock, || {
                            let n: u32 = fs::read_to_string(&counter).unwrap().trim().parse().unwrap();
                            fs::write(&counter, (n + 1).to_string()).unwrap();
                        });
                    }
                });
            }
        });
        assert_eq!(fs::read_to_string(&counter).unwrap(), "80");
        assert!(!lock.exists(), "the lock is given back");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_dead_process_lock_is_taken_over_at_once() {
        let dir = std::env::temp_dir().join(format!("dam-lock-stale-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let lock = dir.join("held.lock");
        fs::write(&lock, "dead-1").unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
        fs::OpenOptions::new().write(true).open(&lock).unwrap().set_modified(old).unwrap();
        let started = std::time::Instant::now();
        assert!(with_lock(&lock, || true));
        assert!(started.elapsed() < std::time::Duration::from_secs(1), "taken over, not waited out");
        assert!(!lock.exists(), "and given back");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_lock_someone_else_took_is_not_removed() {
        let dir = std::env::temp_dir().join(format!("dam-lock-other-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let lock = dir.join("busy.lock");
        // Held for longer than the wait: the work runs anyway, and the other process's lock is left alone.
        fs::write(&lock, "someone-else").unwrap();
        assert!(with_lock(&lock, || true));
        assert_eq!(fs::read_to_string(&lock).unwrap(), "someone-else");
        fs::remove_dir_all(&dir).ok();
    }
}
