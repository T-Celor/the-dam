// TokenEconomizer's micro-skills (Apache-2.0, Microsoft), rewritten from microskills/observe.py and the vocabularies of microskills/__init__.py.
use super::lifecycle::edited_files;
use super::{first, hit, rx, split, strip};
use crate::trace::{strip_rtk_prefix, Call};
use fancy_regex::Regex;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;
use std::sync::LazyLock;

#[derive(Clone, Copy)]
pub struct Vocabulary {
    pub name: &'static str,
    interpreters: &'static [&'static str],
    canonical_interpreter: &'static str,
    runner_heads: &'static [&'static str],
    runner_scripts: &'static [&'static str],
    runner_modules: &'static [&'static str],
    package_tools: &'static [&'static str],
    task_heads: &'static [&'static str],
    build_heads: &'static [&'static str],
    pub generic: bool,
}

const RUNNER_SCRIPTS: [&str; 4] = ["runtests.py", "bin/test", "bin/doctest", "manage.py"];
const RUNNER_MODULES: [&str; 6] = ["pytest", "unittest", "nose", "django", "tox", "nox"];
const FETCH_HEADS: [&str; 2] = ["curl", "wget"];
const ENV_PROBE_HEADS: [&str; 4] = ["which", "command", "type", "whereis"];
// `<tool> build`, `npm run lint`, ...: what a generic reading counts as building or checking.
const BUILD_SUBS: [&str; 12] = ["build", "check", "compile", "lint", "vet", "typecheck", "type-check", "clippy", "package", "verify", "fmt", "format"];

pub const PYTHON_GIT: Vocabulary = Vocabulary {
    name: "python-git",
    interpreters: &["python", "python2", "python3", "pypy", "pypy3"],
    canonical_interpreter: "python",
    runner_heads: &["pytest", "py.test", "tox", "nox", "nosetests", "trial"],
    runner_scripts: &RUNNER_SCRIPTS,
    runner_modules: &RUNNER_MODULES,
    package_tools: &["pip", "pip3", "conda", "mamba", "poetry", "pipenv", "uv", "apt", "apt-get"],
    task_heads: &["make", "npm", "yarn", "pnpm", "cargo", "go"],
    build_heads: &[],
    generic: false,
};

pub const NODE_WEB: Vocabulary = Vocabulary {
    name: "node-web",
    interpreters: &["node", "nodejs", "deno", "bun", "tsx", "ts-node"],
    canonical_interpreter: "node",
    runner_heads: &["jest", "vitest", "mocha", "playwright", "cypress", "ava", "karma"],
    runner_scripts: &[],
    runner_modules: &[],
    package_tools: &["npm", "yarn", "pnpm", "corepack", "apt", "apt-get"],
    task_heads: &["make", "npm", "yarn", "pnpm", "cargo", "go"],
    build_heads: &[],
    generic: false,
};

// The plugin's: every ecosystem at once.
pub const GENERIC: Vocabulary = Vocabulary {
    name: "generic",
    interpreters: &["python", "python2", "python3", "pypy", "pypy3", "node", "nodejs", "deno", "bun", "tsx", "ts-node", "ruby", "perl", "php", "Rscript", "julia", "lua", "elixir"],
    canonical_interpreter: "python",
    runner_heads: &["pytest", "py.test", "tox", "nox", "nosetests", "trial", "jest", "vitest", "mocha", "playwright", "cypress", "ava", "karma", "rspec", "phpunit", "ctest", "bats"],
    runner_scripts: &RUNNER_SCRIPTS,
    runner_modules: &RUNNER_MODULES,
    package_tools: &[
        "pip", "pip3", "conda", "mamba", "poetry", "pipenv", "uv", "apt", "apt-get", "npm", "yarn", "pnpm", "corepack", "gem", "bundle", "composer", "cargo", "go", "mvn", "gradle", "dotnet", "brew", "rustup", "pyenv", "nvm", "asdf",
    ],
    task_heads: &["make", "npm", "yarn", "pnpm", "cargo", "go", "mvn", "gradle", "gradlew", "dotnet", "mix", "swift", "rake"],
    build_heads: &["make", "cmake", "ninja", "bazel", "tsc", "eslint", "prettier", "ruff", "flake8", "pylint", "mypy", "pyright", "black", "isort", "golangci-lint", "gcc", "g++", "clang", "javac", "webpack", "vite"],
    generic: true,
};

// What `plugin/adapter.py` calls detect_vocabulary.
pub fn detect_vocabulary(repo_root: &str, preference: &str) -> Vocabulary {
    match preference {
        "node-web" => return NODE_WEB,
        "python-git" => return PYTHON_GIT,
        _ => {}
    }
    let has = |names: &[&str]| names.iter().any(|name| Path::new(repo_root).join(name).exists());
    let python = has(&["pyproject.toml", "setup.py", "setup.cfg", "requirements.txt", "Pipfile", "tox.ini", "environment.yml"]);
    if has(&["package.json"]) && !python {
        return Vocabulary { canonical_interpreter: "node", ..GENERIC };
    }

    GENERIC
}

const GIT_FETCH_SUBS: [&str; 4] = ["fetch", "clone", "pull", "ls-remote"];
const GIT_PROBE_SUBS: [&str; 7] = ["remote", "branch", "tag", "fsck", "stash", "show-ref", "for-each-ref"];
const PREFIX_HEADS: [&str; 7] = ["sudo", "env", "time", "exec", "nice", "nohup", "timeout"];
const PASSTHROUGH: [&str; 12] = ["cd", "echo", "head", "tail", "cat", "sort", "uniq", "tee", "true", "printf", "less", "more"];
const WORK_INTENTS: [&str; 5] = ["run_interpreter", "run_tests", "run_build", "package_tool", "fetch_remote"];

static ACTIVATE: LazyLock<Regex> = LazyLock::new(|| rx(r"^(?:source|\.)\s+\S*activate\s+([\w.-]+)\s*$"));
static CD: LazyLock<Regex> = LazyLock::new(|| rx(r#"^cd\s+(['"]?)([^\s;&|'"]+)\1\s*$"#));
static CD_FAILED: LazyLock<Regex> = LazyLock::new(|| rx(r"\bcd: [^\n]*: (?:No such file or directory|Not a directory)"));
static CONDA_ACTIVATE: LazyLock<Regex> = LazyLock::new(|| rx(r"^(?:conda|mamba)\s+activate\s+([\w.-]+)\s*$"));
static CONDA_RUN: LazyLock<Regex> = LazyLock::new(|| rx(r"^(?:conda|mamba)\s+run\s+(?:--no-capture-output\s+|--live-stream\s+)*(?:-n|--name)\s+([\w.-]+)\s+"));
static TOOL_RUN: LazyLock<Regex> = LazyLock::new(|| rx(r"^(poetry|pipenv|uv|npx|bundle|yarn|pnpm)\s+(?:run|exec)\s+"));
static NPX: LazyLock<Regex> = LazyLock::new(|| rx(r"^npx\s+(?:(?:--yes|-y|--no-install|--no)\s+)*"));
static INTERP_VERSIONED: LazyLock<Regex> = LazyLock::new(|| rx(r"^(?:python|node)\d(?:\.\d+)?$"));
static ENV_ASSIGN: LazyLock<Regex> = LazyLock::new(|| rx(r#"^(?:[A-Za-z_][A-Za-z0-9_]*=(?:"[^"]*"|'[^']*'|\S+)\s+)+"#));
static ASSIGN_TOKEN: LazyLock<Regex> = LazyLock::new(|| rx(r"^[A-Za-z_][A-Za-z0-9_]*="));
static WRAP: LazyLock<Regex> = LazyLock::new(|| rx(r"(?s)^/bin/(?:ba)?sh\s+-l?c\s+(.*)$"));
static HEREDOC: LazyLock<Regex> = LazyLock::new(|| rx(r#"(?s)<<-?\s*['"]?(\w+)['"]?[^\n]*\n.*?\n\s*\1\b"#));
static URL_HOST: LazyLock<Regex> = LazyLock::new(|| rx(r#"https?://([^/\s'"`]+)"#));
// Python's `$` also matches before a final newline.
static DRIVE: LazyLock<Regex> = LazyLock::new(|| rx(r"^/([A-Za-z])(?=/|\n?\z)|^([A-Za-z]):(?=/|\n?\z)"));
static SHELL_ABS_DRIVE: LazyLock<Regex> = LazyLock::new(|| rx(r"^[a-z]:(?:/|\n?\z)"));

pub const FORM_ERRORS: [&str; 5] = ["module_missing", "command_missing", "script_missing", "usage_error", "permission_denied"];
pub const NETWORK_ERRORS: [&str; 3] = ["connection_refused", "name_resolution", "unreachable"];

// Error classes an environment produces; the first match names the class.
static ERROR_CLASSES: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    vec![
        ("module_missing", rx(r"(?i)ModuleNotFoundError|No module named|Cannot find module|ERR_MODULE_NOT_FOUND")),
        ("command_missing", rx(r"(?i)command not found|: not found\b|is not recognized")),
        ("script_missing", rx(r"(?i)can't open file .*No such file or directory")),
        ("permission_denied", rx(r"(?i)Permission denied|EACCES")),
        ("usage_error", rx(r"(?mi)^usage:|unrecognized arguments|no such option|error: argument|unknown option")),
        ("connection_refused", rx(r"(?i)Connection refused|Failed to connect to|couldn't connect to|Could not connect|ECONNREFUSED")),
        ("name_resolution", rx(r"(?i)Could not resolve host|Temporary failure in name resolution|Name or service not known|getaddrinfo|nodename nor servname|ENOTFOUND")),
        ("unreachable", rx(r"(?i)Network is unreachable|No route to host|Connection timed out|Operation timed out|timed out after|ETIMEDOUT")),
    ]
});

static PROBE_HIT: LazyLock<Vec<(&'static str, Regex)>> = LazyLock::new(|| {
    vec![
        ("remote", rx(r"(?m)^\S+\s+\S+\s+\((?:fetch|push)\)\s*$")),
        ("branch", rx(r"(?m)^\s*remotes/\S+")),
        ("tag", rx(r"(?m)^v?\d+(?:\.\d+)+(?:[a-z]+\d*|\.post\d+|-\S+)?\s*$")),
        ("fsck", rx(r"(?m)^(?:unreachable|dangling)\s+(?:commit|tree|blob|tag)\s+[0-9a-f]{7,}")),
        ("stash", rx(r"(?m)^stash@\{\d+\}")),
        ("refs", rx(r"(?m)^[0-9a-f]{7,}\s+refs/")),
    ]
});

static EXIT_HINT: LazyLock<Regex> = LazyLock::new(|| rx(r"(?i)\bexit code[:=\s]+(\d+)"));
static LABEL_ERR: LazyLock<Regex> = LazyLock::new(|| rx(r"(?i)Failed to import test module|unittest\.loader\._FailedTest|ERROR: (?:file or directory )?not found|no tests ran|module '[\w.]+' has no attribute"));
static NO_MODULE: LazyLock<Regex> = LazyLock::new(|| rx(r#"No module named ['"]?([\w.]+)"#));
static LOADED: LazyLock<Regex> = LazyLock::new(|| rx(r"\bRan [1-9]\d* tests?\b|\b[1-9]\d* (?:passed|failed|passing|failing)\b|\bcollected [1-9]\d* items?\b"));
// Other runners' summaries, for the generic reading.
static LOADED_GENERIC: LazyLock<Regex> = LazyLock::new(|| {
    rx(r"(?m)^ok\s+\S+\s+[\d.]+s\b|^--- (?:PASS|FAIL)\b|\btest result: (?:ok|FAILED)\b|\b[1-9]\d* examples?, \d+ failures?\b|\b(?:Passed|Failed)!\s+-|\bTests run: [1-9]\d*")
});
static ENV_NEED: LazyLock<Regex> = LazyLock::new(|| rx(r#"environment variable\s+['"`]?([A-Z][A-Z0-9_]{2,})"#));
static ENV_PREFIX: LazyLock<Regex> = LazyLock::new(|| rx(r#"(?:^|[\s;&(])(?:export\s+)?([A-Z][A-Z0-9_]{2,})=(['"]?)([\w.\-/:]+)\2"#));
static ENV_INCODE: LazyLock<Regex> = LazyLock::new(|| rx(r#"os\.environ(?:\.setdefault\(\s*|\[\s*)['"]([A-Z][A-Z0-9_]{2,})['"]\s*(?:,|\]\s*=)\s*['"]([\w.\-/:]+)['"]"#));
static FAILED: LazyLock<Regex> = LazyLock::new(|| rx(r"(?m)Traceback \(most recent call last\)|^\w*Error\b|\b[Ee]xit [Cc]ode[:=\s]+[1-9]"));
static LABEL_TOKEN: LazyLock<Regex> = LazyLock::new(|| rx(r"^[A-Za-z_][\w.\-/]*(?:::[\w.\-\[\]]+)*$"));

pub struct Observation {
    pub call: usize,
    pub intent: &'static str,
    pub form: String,
    pub head: String,
    segment: String,
    pub hosts: Vec<String>,
    pub probe: Option<&'static str>,
    pub outcome: Option<&'static str>,
    pub labels: Vec<String>,
    pub label_err: bool,
    pub loaded: bool,
    pub tests_ran: bool,
    // Typed with `VAR=value` before it, which its form leaves out.
    pub env: bool,
}

impl Observation {
    fn new(call: usize, intent: &'static str, form: String, head: &str, segment: &str) -> Self {
        Observation { call, intent, form, head: head.to_string(), segment: segment.to_string(), hosts: Vec::new(), probe: None, outcome: None, labels: Vec::new(), label_err: false, loaded: false, tests_ran: false, env: false }
    }

    fn with_hosts(mut self, body: &str) -> Self {
        self.hosts = URL_HOST.captures_iter(body).filter_map(|caps| Some(caps.ok()?.get(1)?.as_str().to_string())).collect();

        self
    }
}

pub fn unwrap(cmd: &str) -> String {
    let Ok(Some(caps)) = WRAP.captures(strip(cmd)) else { return cmd.to_string() };
    let inner: Vec<char> = strip(&caps[1]).chars().collect();
    if inner.len() >= 2 && inner[0] == inner[inner.len() - 1] && "\"'".contains(inner[0]) {
        let body: String = inner[1..inner.len() - 1].iter().collect();

        return if inner[0] == '"' { body.replace("\\\"", "\"").replace("\\$", "$").replace("\\\\", "\\") } else { body };
    }

    inner.iter().collect()
}

// Heredoc bodies are elided first so their contents are never mistaken for commands.
pub fn segments(cmd: &str) -> Vec<String> {
    let cmd = HEREDOC.replace_all(cmd, |caps: &fancy_regex::Captures| format!("{} <heredoc>", caps[0].split('\n').next().unwrap_or(""))).into_owned();
    let chars: Vec<char> = cmd.chars().collect();
    let (mut out, mut buf, mut quote, mut i) = (Vec::new(), String::new(), None, 0);
    while i < chars.len() {
        let c = chars[i];
        let two: String = chars[i..(i + 2).min(chars.len())].iter().collect();
        if let Some(open) = quote {
            buf.push(c);
            if c == open {
                quote = None;
            } else if c == '\\' && i + 1 < chars.len() {
                buf.push(chars[i + 1]);
                i += 1;
            }
        } else if c == '"' || c == '\'' {
            quote = Some(c);
            buf.push(c);
        } else if c == '\n' || c == ';' || c == '|' || two == "&&" {
            out.push(std::mem::take(&mut buf));
            if two == "&&" || two == "||" {
                i += 1;
            }
        } else {
            buf.push(c);
        }
        i += 1;
    }
    out.push(buf);

    out.iter().map(|segment| strip(strip(segment).trim_matches(['(', ')'])).to_string()).filter(|segment| !segment.is_empty()).collect()
}

// ---- paths: the command is shell text, compared in one POSIX spelling --------------------------

// os.path.isabs of Python 3.12.
fn is_abs(path: &str) -> bool {
    if !cfg!(windows) {
        return path.starts_with('/');
    }
    let head: String = path.chars().take(3).map(|c| if c == '/' { '\\' } else { c }).collect();

    head.starts_with('\\') || head.chars().skip(1).collect::<String>().starts_with(":\\")
}

fn px_normpath(path: &str) -> String {
    let slashes = if path.starts_with("//") && !path.starts_with("///") { 2 } else { usize::from(path.starts_with('/')) };
    let mut kept: Vec<&str> = Vec::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part != ".." || (slashes == 0 && kept.is_empty()) || kept.last() == Some(&"..") {
            kept.push(part);
        } else {
            kept.pop();
        }
    }
    let joined = format!("{}{}", "/".repeat(slashes), kept.join("/"));

    if joined.is_empty() { ".".to_string() } else { joined }
}

fn px_join(base: &str, tail: &str) -> String {
    if tail.starts_with('/') {
        tail.to_string()
    } else if base.is_empty() || base.ends_with('/') {
        format!("{base}{tail}")
    } else {
        format!("{base}/{tail}")
    }
}

// posixpath.relpath: a relative path hangs off the working directory, as it does there.
fn px_relpath(path: &str, start: &str) -> String {
    let cwd = std::env::current_dir().map(|dir| dir.to_string_lossy().into_owned()).unwrap_or_default();
    let parts = |path: &str| px_normpath(&px_join(&cwd, path)).split('/').filter(|part| !part.is_empty()).map(str::to_string).collect::<Vec<_>>();
    let (start, path) = (parts(start), parts(path));
    let common = start.iter().zip(&path).take_while(|(a, b)| a == b).count();
    let rel: Vec<&str> = std::iter::repeat("..").take(start.len() - common).chain(path[common..].iter().map(String::as_str)).collect();

    if rel.is_empty() { ".".to_string() } else { rel.join("/") }
}

// `p` with forward slashes and a Windows drive spelled `c:`; a POSIX path is returned as it is.
fn shell_path(path: &str) -> String {
    if !cfg!(windows) {
        return path.to_string();
    }
    let path = path.replace('\\', "/");
    match DRIVE.captures(&path) {
        Ok(Some(caps)) => {
            let letter = caps.get(1).or_else(|| caps.get(2)).map_or("", |m| m.as_str()).to_lowercase();
            format!("{letter}:{}", &path[caps.get(0).map_or(0, |m| m.end())..])
        }
        _ => path,
    }
}

fn shell_isabs(path: &str) -> bool {
    path.starts_with('/') || (cfg!(windows) && hit(&SHELL_ABS_DRIVE, path))
}

// The nearest directory at or above `start` that holds a `.git`; None when there is none on disk.
// shortcut: a UNC path is walked as plain components, where ntpath keeps \\server\share as its root.
pub fn repository_top(start: &str) -> Option<String> {
    if start.is_empty() || !is_abs(start) || !Path::new(start).is_dir() {
        return None;
    }
    let sep = if cfg!(windows) { '\\' } else { '/' };
    let path = if cfg!(windows) { start.replace('/', "\\") } else { start.to_string() };
    let drive = if cfg!(windows) && path.as_bytes().get(1) == Some(&b':') { &path[..2] } else { "" };
    let rest = &path[drive.len()..];
    let rooted = rest.starts_with(sep);
    let prefix = format!("{drive}{}", if rooted { sep.to_string() } else { String::new() });
    let mut kept: Vec<&str> = Vec::new();
    for part in rest.split(sep).filter(|part| !part.is_empty() && *part != ".") {
        if part != ".." || (!rooted && kept.is_empty()) || kept.last() == Some(&"..") {
            kept.push(part);
        } else {
            kept.pop();
        }
    }
    for n in (0..=kept.len()).rev() {
        let dir = format!("{prefix}{}", kept[..n].join(&sep.to_string()));
        let marker = if dir.ends_with(sep) { format!("{dir}.git") } else { format!("{dir}{sep}.git") };
        if Path::new(&marker).exists() {
            return Some(dir);
        }
    }

    None
}

// Where `cd target` from `cur` lands, relative to the directory the session started in ("" for
// that directory itself); None when it leaves it or cannot be told.
fn cd_target(cur: &str, target: &str, root: Option<&str>, top: Option<&str>) -> Option<String> {
    if target.is_empty() || target.starts_with(['~', '$', '-']) {
        return None;
    }
    let (target, cur) = (shell_path(target), shell_path(if cur.is_empty() { "." } else { cur }));
    let rel = if shell_isabs(&target) {
        let root = root.filter(|root| !root.is_empty())?;
        px_relpath(&px_normpath(&target), &px_normpath(&shell_path(root)))
    } else {
        px_normpath(&px_join(&cur, &target))
    };
    if rel == "." {
        return Some(String::new());
    }
    if rel == ".." || rel.starts_with("../") {
        let (top, root) = (top.filter(|top| !top.is_empty())?, root.filter(|root| !root.is_empty())?);
        let at = px_normpath(&px_join(&shell_path(root), &rel));

        return if at == px_normpath(&shell_path(top)) { Some(String::new()) } else { None };
    }

    Some(rel)
}

// ---- classification ---------------------------------------------------------------------------

// Observations for one shell command (possibly compound). Activation prefixes carry across its segments.
pub fn classify_command(cmd: &str, call: usize, vocab: &Vocabulary, cwd: &str, root: Option<&str>, top: Option<&str>) -> Vec<Observation> {
    let mut observations = Vec::new();
    let mut carried = String::new();
    let mut here: Option<String> = Some(cwd.to_string());
    let mut cd = if cwd.is_empty() { String::new() } else { format!("cd {cwd} &&") };
    for segment in segments(&unwrap(cmd)) {
        let typed = strip(&segment).to_string();
        let seg = strip(&ENV_ASSIGN.replace(&typed, "")).to_string();
        let mut assigned = seg != typed;
        let unwrapped = strip(&strip_rtk_prefix(&seg)).to_string();
        let seg = strip(&ENV_ASSIGN.replace(&unwrapped, "")).to_string();
        assigned = assigned || seg != unwrapped;
        if seg.is_empty() {
            continue;
        }
        // A directory change is part of how the rest of the command runs, so it is carried into the forms that follow.
        if let Ok(Some(caps)) = CD.captures(&seg) {
            let wanted = &caps[2];
            match here.as_deref().and_then(|current| cd_target(current, wanted, root, top)) {
                None => (here, cd) = (None, format!("cd {wanted} &&")),
                Some(there) => {
                    cd = if there.is_empty() { String::new() } else { format!("cd {there} &&") };
                    here = Some(there);
                }
            }
            continue;
        }
        let activation = ACTIVATE.captures(&seg).ok().flatten().or_else(|| CONDA_ACTIVATE.captures(&seg).ok().flatten());
        if let Some(caps) = activation {
            let tool = if seg.starts_with("conda") || seg.starts_with("mamba") { "conda" } else { "source" };
            carried = format!("{tool} activate {} &&", &caps[1]);
            continue;
        }
        let mut prefix = [cd.as_str(), carried.as_str()].iter().filter(|part| !part.is_empty()).copied().collect::<Vec<_>>().join(" ");
        let mut body = seg.as_str();
        let push = |prefix: &mut String, text: String| *prefix = if prefix.is_empty() { text } else { format!("{prefix} {text}") };
        if let Ok(Some(caps)) = CONDA_RUN.captures(body) {
            push(&mut prefix, format!("conda run -n {}", &caps[1]));
            body = &body[caps.get(0).map_or(0, |m| m.end())..];
        }
        if let Ok(Some(caps)) = TOOL_RUN.captures(body) {
            push(&mut prefix, format!("{} run", &caps[1]));
            body = &body[caps.get(0).map_or(0, |m| m.end())..];
        } else if let Ok(Some(caps)) = NPX.captures(body) {
            push(&mut prefix, "npx".to_string());
            body = &body[caps.get(0).map_or(0, |m| m.end())..];
        }
        let mut tokens = split(body);
        while let Some(&token) = tokens.first() {
            let assignment = hit(&ASSIGN_TOKEN, token);
            if !assignment && !PREFIX_HEADS.contains(&token) {
                break;
            }
            let skip = if !assignment && token == "timeout" { 2 } else { 1 };
            assigned |= assignment;
            tokens = tokens.split_off(skip.min(tokens.len()));
        }
        let Some((&head, rest)) = tokens.split_first() else { continue };
        let base = head.rsplit('/').next().unwrap_or(head);
        if let Some(mut found) = classify(head, base, rest, body, &prefix, &seg, call, vocab) {
            found.env = assigned;
            observations.push(found);
        }
    }

    observations
}

fn runner_script<'a>(rest: &[&'a str], vocab: &Vocabulary) -> Option<&'a str> {
    let token = *rest.iter().find(|token| !token.starts_with('-'))?;
    if vocab.runner_scripts.iter().any(|script| token.ends_with(script)) {
        return Some(if token.starts_with("./") { token.trim_start_matches(['.', '/']) } else { token });
    }

    None
}

#[allow(clippy::too_many_arguments)]
fn classify(head: &str, base: &str, rest: &[&str], body: &str, prefix: &str, seg: &str, call: usize, vocab: &Vocabulary) -> Option<Observation> {
    let form = |expr: &str| if prefix.is_empty() { expr.to_string() } else { strip(&format!("{prefix} {expr}")).to_string() };
    let made = |intent: &'static str, expr: &str| Some(Observation::new(call, intent, form(expr), head, seg));
    if vocab.interpreters.contains(&base) || hit(&INTERP_VERSIONED, base) {
        if !rest.is_empty() && rest.iter().all(|token| ["--version", "-V", "-VV", "-v"].contains(token)) {
            return made("probe_env", head);
        }
        if let Some(at) = rest.iter().position(|token| *token == "-m") {
            let module = rest.get(at + 1).copied().unwrap_or("");
            if vocab.runner_modules.contains(&module) {
                return made("run_tests", &format!("{head} -m {module}"));
            }
        }
        if let Some(script) = runner_script(rest, vocab).filter(|script| *script != "manage.py" || rest.contains(&"test")) {
            return made("run_tests", &format!("{head} {script}"));
        }
        return made("run_interpreter", head);
    }
    if vocab.runner_heads.contains(&base) {
        return made("run_tests", head);
    }
    for script in vocab.runner_scripts {
        if head == *script || head.ends_with(&format!("/{script}")) || head == format!("./{script}") {
            if *script == "manage.py" && !rest.contains(&"test") {
                break;
            }
            // A script run through its shebang is the interpreter running it, so `./tests/runtests.py` and `python tests/runtests.py` pool.
            return made("run_tests", &format!("{} {}", vocab.canonical_interpreter, head.trim_start_matches(['.', '/'])));
        }
    }
    if vocab.task_heads.contains(&base) && rest.first() == Some(&"test") {
        return made("run_tests", &format!("{base} test"));
    }
    if vocab.generic {
        if ["npm", "yarn", "pnpm", "bun"].contains(&base) && rest.len() > 1 && rest[0] == "run" {
            let script = rest[1];
            let kind = script.split(':').next().unwrap_or("");
            if kind == "test" {
                return made("run_tests", &format!("{base} run {script}"));
            }
            if BUILD_SUBS.contains(&kind) {
                return made("run_build", &format!("{base} run {script}"));
            }
        }
        if vocab.task_heads.contains(&base) && rest.first().is_some_and(|sub| BUILD_SUBS.contains(sub)) {
            return made("run_build", &format!("{base} {}", rest[0]));
        }
        if vocab.build_heads.contains(&base) {
            return made("run_build", base);
        }
    }
    if ENV_PROBE_HEADS.contains(&base) {
        return made("probe_env", base);
    }
    if FETCH_HEADS.contains(&base) {
        return made("fetch_remote", base).map(|found| found.with_hosts(body));
    }
    if base == "git" && !rest.is_empty() {
        let sub = rest[0];
        if GIT_FETCH_SUBS.contains(&sub) {
            return made("fetch_remote", &format!("git {sub}")).map(|found| found.with_hosts(body));
        }
        if GIT_PROBE_SUBS.contains(&sub) {
            if let Some(kind) = probe_kind(sub, &rest[1..]) {
                return made("probe_vcs_refs", &format!("git {sub}")).map(|mut found| {
                    found.probe = Some(kind);
                    found
                });
            }
        }
        return None;
    }
    if vocab.package_tools.contains(&base) {
        if ["pip", "npm", "yarn", "pnpm"].iter().any(|tool| base.starts_with(tool)) && rest.first().is_some_and(|sub| ["install", "download", "add", "ci"].contains(sub)) {
            return made("fetch_remote", &format!("{base} {}", rest[0])).map(|found| found.with_hosts(body));
        }
        return made("package_tool", base);
    }

    None
}

fn probe_kind(sub: &str, args: &[&str]) -> Option<&'static str> {
    match sub {
        "remote" => Some("remote"),
        "branch" if args.iter().any(|arg| ["-a", "-r", "--all", "--remotes", "-ar", "-ra"].contains(arg)) => Some("branch"),
        // Plain `git tag` lists past tags too.
        "tag" if args.contains(&"--contains") => Some("tag"),
        "fsck" => Some("fsck"),
        "stash" if args.first() == Some(&"list") => Some("stash"),
        "show-ref" | "for-each-ref" => Some("refs"),
        _ => None,
    }
}

fn error_class(output: &str) -> Option<&'static str> {
    ERROR_CLASSES.iter().find(|(_, class)| hit(class, output)).map(|(name, _)| *name)
}

// None where the output says no exit code; otherwise whether it was zero.
fn exit_zero(output: &str) -> Option<bool> {
    let caps = EXIT_HINT.captures(output).ok().flatten()?;

    Some(caps[1].chars().all(|digit| digit == '0'))
}

fn label_failed(labels: &[String], out: &str) -> bool {
    if hit(&LABEL_ERR, out) {
        return true;
    }
    let heads: Vec<&str> = labels.iter().map(|label| label.split(['.', '/', ':']).next().unwrap_or("")).collect();

    NO_MODULE.captures_iter(out).filter_map(|caps| caps.ok()).any(|caps| heads.contains(&caps[1].split('.').next().unwrap_or("")))
}

// The positional arguments after the runner's own token that look like a module, a path or a test id.
fn test_labels(o: &Observation) -> Vec<String> {
    let tokens = split(&o.segment);
    let last = split(&o.form).last().copied().unwrap_or("");
    let start = tokens.iter().position(|token| *token == last || token.trim_start_matches(['.', '/']) == last || token.ends_with(&format!("/{last}"))).map_or(tokens.len(), |at| at + 1);
    let (mut labels, mut skip, mut prev) = (Vec::new(), false, "");
    for &token in &tokens[start..] {
        if skip {
            (skip, prev) = (false, token);
            continue;
        }
        if [">", ">>", "2>", "<", "|"].contains(&token) || ["2>", ">", "<", "|"].iter().any(|p| token.starts_with(p)) {
            skip = [">", ">>", "2>", "<"].contains(&token);
            prev = token;
            continue;
        }
        let structured = token.contains('.') || token.contains("::") || token.contains('/');
        let after_option = prev.starts_with('-') && !prev.contains('=');
        if !token.starts_with('-') && hit(&LABEL_TOKEN, token) && (structured || !after_option) {
            labels.push(token.to_string());
        }
        prev = token;
    }

    labels
}

// ("sep", "::") or ("strip", prefix) when label `b` is label `a` rewritten into the shape the runner loads.
pub fn label_rule(a: &str, b: &str) -> Option<(&'static str, String)> {
    if a.contains("::") && !b.contains("::") {
        let dotted = a.replace("::", ".");
        if b == dotted || dotted.starts_with(&format!("{b}.")) || b.starts_with(&format!("{dotted}.")) {
            return Some(("sep", "::".to_string()));
        }
    }
    for sep in [".", "/"] {
        if !a.contains(sep) {
            continue;
        }
        let prefix = format!("{}{sep}", a.split(sep).next().unwrap_or(""));
        let rest = &a[prefix.len()..];
        if !rest.is_empty() && !b.starts_with(&prefix) && (b == rest || rest.starts_with(&format!("{b}{sep}")) || b.starts_with(&format!("{rest}{sep}"))) {
            return Some(("strip", prefix));
        }
    }

    None
}

// Did this run show that the way the command was CALLED fails here, and not the arguments?
pub fn form_failed(o: &Observation) -> bool {
    let Some(outcome) = o.outcome.filter(|outcome| FORM_ERRORS.contains(outcome)) else { return false };

    !o.label_err && !(o.intent == "run_tests" && outcome == "usage_error")
}

// A test run must show tests running: one whose output was filtered away proves nothing.
pub fn form_worked(o: &Observation) -> bool {
    o.outcome == Some("ran") && (o.intent != "run_tests" || o.tests_ran)
}

pub fn is_bare(form: &str) -> bool {
    let head = split(form).first().copied().unwrap_or("");

    !head.contains('/') && !form.contains(" activate ") && !form.contains(" run ") && !["conda run", "poetry run", "pipenv run", "uv run", "npx "].iter().any(|start| form.starts_with(start))
}

pub struct EnvCall {
    pub need: Vec<String>,
    pub sets: Vec<(String, String)>,
    pub ok: bool,
}

pub struct Digest {
    pub obs: Vec<Observation>,
    pub edited: Vec<String>,
    pub n_calls: usize,
    pub env: Vec<EnvCall>,
}

// What one call says about environment variables: those its output asks for, and those its command sets.
fn env_call(cmd: &str, out: &str, ok: bool) -> EnvCall {
    let mut sets: Vec<(String, String)> = Vec::new();
    let mut set = |name: String, value: String| match sets.iter_mut().find(|(known, _)| *known == name) {
        Some(slot) => slot.1 = value,
        None => sets.push((name, value)),
    };
    for caps in ENV_PREFIX.captures_iter(cmd).flatten() {
        set(caps[1].to_string(), caps[3].to_string());
    }
    for caps in ENV_INCODE.captures_iter(cmd).flatten() {
        set(caps[1].to_string(), caps[2].to_string());
    }
    let need: BTreeSet<String> = ENV_NEED.captures_iter(out).flatten().map(|caps| caps[1].to_string()).collect();

    EnvCall { need: need.into_iter().collect(), sets, ok: ok && !hit(&FAILED, out) }
}

fn error_line<'a>(out: &'a str, err: &str) -> &'a str {
    let Some((from, to)) = ERROR_CLASSES.iter().find(|(name, _)| *name == err).and_then(|(_, class)| first(class, out)) else { return "" };
    let start = out[..from].rfind('\n').map_or(0, |at| at + 1);
    let end = out[to..].find('\n').map_or(out.len(), |at| to + at);

    &out[start..end]
}

// Per-session digest: observations with outcomes, the edited files, the call count. Outcome attribution inside
// a compound command is conservative: an infrastructure error is attributed to the segment whose executable
// the error names, else to the last relevant segment.
pub fn observe_task(calls: &[Call], vocab: &Vocabulary) -> Digest {
    let root = calls.iter().filter_map(|call| call.cwd.as_deref()).find(|cwd| !cwd.is_empty());
    let top = root.and_then(repository_top);
    let (mut all, mut triples, mut env_calls) = (Vec::new(), Vec::new(), Vec::new());
    for (idx, call) in calls.iter().enumerate() {
        let tool = call.tool.as_deref().unwrap_or("").to_lowercase();
        let args = if call.args.is_object() { call.args.clone() } else { Value::Object(Default::default()) };
        triples.push((tool.clone(), args.clone()));
        if !["shell", "bash", "stop_bash"].contains(&tool.as_str()) {
            continue;
        }
        let command = ["command", "cmd"].iter().filter_map(|key| args.get(*key)).find(|value| crate::economizer::python_truthy(value)).and_then(Value::as_str).unwrap_or("");
        let out = call.result.as_str();
        if hit(&CD_FAILED, out) {
            continue;
        }
        let here = match (root, call.cwd.as_deref().filter(|cwd| !cwd.is_empty())) {
            (Some(_), Some(cwd)) => cd_target("", cwd, root, top.as_deref()),
            _ => Some(String::new()),
        };
        let mut obs = match here {
            None => classify_command(command, idx, vocab, "", None, None),
            Some(here) => classify_command(command, idx, vocab, &here, root, top.as_deref()),
        };
        let hint = exit_zero(out);
        if vocab.generic || command.contains("python") || command.contains('=') {
            let found = env_call(command, out, hint.unwrap_or(call.success));
            if !found.need.is_empty() || !found.sets.is_empty() {
                env_calls.push(found);
            }
        }
        if obs.is_empty() {
            continue;
        }
        for o in &mut obs {
            if o.intent == "run_tests" {
                o.labels = test_labels(o);
                o.label_err = !o.labels.is_empty() && label_failed(&o.labels, out);
                o.loaded = !o.labels.is_empty() && !o.label_err && hit(&LOADED, out);
                o.tests_ran = hit(&LOADED, out) || (vocab.generic && hit(&LOADED_GENERIC, out));
            }
        }
        // Pass-through segments neither add evidence nor pollute the output; filters that rewrite content do.
        let n_seg = segments(&unwrap(command)).iter().filter(|seg| !PASSTHROUGH.contains(&split(seg).first().copied().unwrap_or("").rsplit('/').next().unwrap_or(""))).count();
        let intents: BTreeSet<&str> = obs.iter().map(|o| o.intent).collect();
        let homogeneous = n_seg == obs.len() && intents.len() == 1;
        let success = hint.unwrap_or(call.success);
        let err = error_class(out);
        if let Some(err) = err {
            let targets: Vec<usize> = (0..obs.len()).filter(|&n| WORK_INTENTS.contains(&obs[n].intent)).collect();
            let line = error_line(out, err);
            let mut named: Vec<usize> = targets.iter().copied().filter(|&n| !obs[n].head.is_empty() && (out.contains(&obs[n].head) || line.contains(obs[n].head.rsplit('/').next().unwrap_or("")))).collect();
            if NETWORK_ERRORS.contains(&err) {
                let fetches: Vec<usize> = targets.iter().copied().filter(|&n| obs[n].intent == "fetch_remote").collect();
                named = fetches.iter().copied().filter(|&n| obs[n].hosts.is_empty() || obs[n].hosts.iter().any(|host| out.contains(host.as_str()))).collect();
                if named.is_empty() {
                    named = fetches;
                }
            }
            let chosen: Vec<usize> = if !named.is_empty() { named } else if !success { targets.last().copied().into_iter().collect() } else { Vec::new() };
            for n in chosen {
                obs[n].outcome = Some(err);
            }
            if success {
                for o in &mut obs {
                    o.outcome = o.outcome.or(Some("ran"));
                }
            }
        } else if success {
            for o in &mut obs {
                o.outcome = Some("ran");
            }
        } else {
            let mut first_run = true;
            for o in &mut obs {
                if WORK_INTENTS.contains(&o.intent) {
                    o.outcome = if first_run { Some("ran") } else { None };
                    first_run = false;
                } else {
                    o.outcome = if o.intent != "probe_vcs_refs" { Some("ran") } else { None };
                }
            }
        }
        for o in &mut obs {
            if o.intent == "probe_vcs_refs" {
                // Judged only when every segment of the call is a probe.
                let probed = PROBE_HIT.iter().find(|(kind, _)| Some(*kind) == o.probe).is_some_and(|(_, class)| hit(class, out));
                o.outcome = if !homogeneous { None } else if probed { Some("hit") } else if success { Some("empty") } else { None };
            }
            if o.intent == "fetch_remote" && o.outcome == Some("ran") && (!homogeneous || !success || strip(out).is_empty() || err.is_some_and(|err| NETWORK_ERRORS.contains(&err))) {
                // A compound command's exit code and output belong to its last segment, not to the fetch.
                o.outcome = None;
            }
        }
        all.extend(obs);
    }
    let edited = edited_files(&triples, !vocab.generic).into_iter().collect();

    Digest { obs: all, edited, n_calls: calls.len(), env: env_calls }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forms(cmd: &str) -> Vec<(String, &'static str)> {
        classify_command(cmd, 0, &GENERIC, "", None, None).into_iter().map(|o| (o.form, o.intent)).collect()
    }

    #[test]
    fn a_directory_change_and_an_activation_carry_into_the_form() {
        assert_eq!(forms("cd tests && python runtests.py x"), [("cd tests && python runtests.py".to_string(), "run_tests")]);
        assert_eq!(forms("source activate py39 && pytest -x"), [("source activate py39 && pytest".to_string(), "run_tests")]);
        assert_eq!(forms("K=v rtk proxy npm run lint"), [("npm run lint".to_string(), "run_build")]);
    }

    #[test]
    fn heredocs_and_quotes_do_not_split_a_command() {
        assert_eq!(segments("cat <<'E'\na && b\nE\npython t.py"), ["cat <<'E' <heredoc>", "python t.py"]);
        assert_eq!(segments("echo 'a; b' | wc"), ["echo 'a; b'", "wc"]);
    }

    #[test]
    fn labels_are_rewritten_into_the_shape_the_runner_loads() {
        assert_eq!(label_rule("a::B::c", "a.B.c"), Some(("sep", "::".to_string())));
        assert_eq!(label_rule("tests.foo.Bar", "foo.Bar"), Some(("strip", "tests.".to_string())));
        assert_eq!(label_rule("tests.foo", "other.name"), None);
    }

    #[test]
    fn a_cd_up_to_the_repository_top_is_the_plain_form() {
        assert_eq!(cd_target("", "/r/sub", Some("/r/sub"), Some("/r")), Some(String::new()));
        assert_eq!(cd_target("", "/r", Some("/r/sub"), Some("/r")), Some(String::new()));
        assert_eq!(cd_target("", "..", Some("/r/sub"), None), None);
        assert_eq!(cd_target("", "~/x", Some("/r"), None), None);
    }
}
