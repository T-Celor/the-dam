// What a hook or command does around a session (Apache-2.0, Microsoft), rewritten from plugin/repos.py (pins, note_repo,
// prune_pins), paths.py (ensure_launcher), context.py (spawn) and winspawn.py.
use crate::context::Ctx;
use crate::economizer::{detach, env, now, pin_name, state_root, Store};
use crate::paths::{abspath, repo_identity};
use serde_json::{json, Value};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const PIN_MAX_AGE: Duration = Duration::from_secs(30 * 86400);

pub struct TempDir(pub PathBuf);

impl TempDir {
    pub fn new(prefix: &str) -> std::io::Result<Self> {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_nanos());
        let dir = std::env::temp_dir().join(format!("{prefix}{:x}{nanos:x}", std::process::id()));
        fs::create_dir_all(&dir)?;

        Ok(TempDir(dir))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// The repository a session is pinned to, else the one its directory names (and true: not pinned yet).
pub fn ctx_for(session: &str, cwd: &Path) -> (Ctx, bool) {
    let root = abspath(&state_root());
    match crate::status::pinned(&root, session) {
        Some(repo) => (Ctx::with_repo(root, repo), false),
        None => (Ctx::with_repo(root, repo_identity(cwd)), true),
    }
}

pub fn pin(ctx: &Ctx, session: &str, cwd: &str) -> std::io::Result<()> {
    let repo = &ctx.repo;
    let doc = json!({"root": repo.root, "key": repo.key, "label": repo.label, "remote": repo.remote, "how": repo.how, "cwd": cwd, "at": now()});

    Store { base: ctx.root.join("sessions").join("claude") }.write(&pin_name(session), &doc)
}

fn age(path: &Path) -> Option<Duration> {
    SystemTime::now().duration_since(fs::metadata(path).ok()?.modified().ok()?).ok()
}

// A pin older than thirty days is a finished session; at most once a day.
pub fn prune_pins(ctx: &Ctx) {
    let dir = ctx.root.join("sessions").join("claude");
    let marker = dir.join(".pruned");
    if age(&marker).is_some_and(|age| age.as_secs() < 86400) {
        return;
    }
    let Ok(names) = fs::read_dir(&dir) else { return };
    for entry in names.flatten() {
        let path = entry.path();
        if age(&path).is_some_and(|age| age > PIN_MAX_AGE) && path.extension().is_some_and(|ext| ext == "json") {
            let _ = fs::remove_file(&path);
        }
    }
    let _ = fs::File::create(&marker);
}

// The repository's identity beside its state, for `te repos`.
pub fn note_repo(ctx: &Ctx) -> std::io::Result<()> {
    let store = Store { base: ctx.repo_dir() };
    if !store.base.is_dir() {
        return Ok(());
    }
    let mut doc = store.read("repo.json").filter(Value::is_object).unwrap_or_else(|| json!({}));
    let repo = &ctx.repo;
    let want = [("key", &repo.key), ("label", &repo.label), ("remote", &repo.remote), ("how", &repo.how)];
    if want.iter().any(|(name, value)| doc.get(*name) != Some(&json!(value))) {
        for (name, value) in want {
            doc[name] = json!(value);
        }
        doc["updated"] = json!(now());
        store.write("repo.json", &doc)?;
    }

    Ok(())
}

fn quoted(text: &str) -> String {
    if !text.is_empty() && text.chars().all(|c| c.is_ascii_alphanumeric() || "@%+=:,./-_".contains(c)) {
        return text.to_string();
    }

    format!("'{}'", text.replace('\'', "'\"'\"'"))
}

// `<state root>/bin/te`, the one-line launcher a slash command falls back to.
pub fn ensure_launcher() -> Option<PathBuf> {
    let target = PathBuf::from(env("TE_PLUGIN_ROOT")?).join("scripts").join("te");
    #[cfg(unix)]
    let runnable = {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(&target).is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
    };
    #[cfg(not(unix))]
    let runnable = target.is_file();
    if !runnable {
        return None;
    }
    let path = abspath(&state_root()).join("bin").join("te");
    let body = format!("#!/bin/sh\n# Written by the TokenEconomizer plugin at session start: its `te` command line.\nexec {} \"$@\"\n", quoted(&target.to_string_lossy()));
    if fs::read_to_string(&path).is_ok_and(|text| text.replace("\r\n", "\n") == body) {
        return Some(path);
    }
    fs::create_dir_all(path.parent()?).ok()?;
    let temp = PathBuf::from(format!("{}.{}.tmp", path.display(), std::process::id()));
    // As Python's text mode writes it.
    fs::write(&temp, if cfg!(windows) { body.replace('\n', "\r\n") } else { body }).ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&temp, fs::Permissions::from_mode(0o700));
    }
    fs::rename(&temp, &path).ok()?;

    Some(path)
}

// This program again, detached from the hook that starts it, its output appended to `<store>/<log_name>`.
pub fn spawn(ctx: &Ctx, words: &[&str], log_name: &str) -> bool {
    if env("TE_JUDGE_NO_SPAWN").is_some() {
        return true;
    }
    let Ok(program) = std::env::current_exe() else { return false };
    let mut args: Vec<String> = words.iter().map(|word| word.to_string()).collect();
    args.extend(["--host".to_string(), "claude".to_string(), "--cwd".to_string(), ctx.repo.root.clone()]);
    let log_path = ctx.store.base.join(log_name);
    let log = fs::create_dir_all(&ctx.store.base).and_then(|()| OpenOptions::new().create(true).append(true).open(&log_path));
    let Ok(log) = log else { return false };
    let (Ok(out), Ok(err)) = (log.try_clone(), log.try_clone()) else { return false };
    let mut command = Command::new(&program);
    command.args(&args).current_dir(&ctx.repo.root).stdin(Stdio::null()).stdout(out).stderr(err);
    detach(&mut command, true);
    if command.spawn().is_ok() {
        return true;
    }

    // The agent's Windows job forbids breakaway: a process the WMI service creates is outside it.
    #[cfg(windows)]
    return wmi_spawn(&program, &args, &ctx.repo.root, &log_path);
    #[cfg(not(windows))]
    false
}

#[cfg(windows)]
fn wmi_spawn(program: &Path, args: &[String], cwd: &str, log: &Path) -> bool {
    // WMI passes neither the environment nor redirections: they travel in a spec file `run_spec` reads back.
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |elapsed| elapsed.as_nanos());
    let spec = log.with_file_name(format!("te-spawn-{}-{nanos:x}.json", std::process::id()));
    let environment: serde_json::Map<String, Value> = std::env::vars().map(|(name, value)| (name, json!(value))).collect();
    let doc = json!({"program": program, "args": args, "cwd": cwd, "env": environment, "log": log});
    if fs::write(&spec, doc.to_string()).is_err() {
        return false;
    }
    let line = format!("\"{}\" run-spec \"{}\"", program.display(), spec.display());
    let script = format!("$r = Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{{ CommandLine = '{}'; CurrentDirectory = '{}' }}; exit $r.ReturnValue", line.replace('\'', "''"), cwd.replace('\'', "''"));
    let mut powershell = Command::new("powershell");
    powershell.args(["-NoProfile", "-NonInteractive", "-Command", &script]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    use std::os::windows::process::CommandExt;
    powershell.creation_flags(0x0800_0000);
    let started = powershell.status().is_ok_and(|status| status.success());
    if !started {
        let _ = fs::remove_file(&spec);
    }

    started
}

// The WMI-created half: runs the command a spec file holds, with its environment and log.
pub fn run_spec(path: &str) -> i32 {
    let Some(doc) = fs::read(path).ok().and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok()) else { return 1 };
    let _ = fs::remove_file(path);
    let text = |key: &str| doc.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    let Ok(log) = OpenOptions::new().create(true).append(true).open(text("log")) else { return 1 };
    let (Ok(out), Ok(err)) = (log.try_clone(), log.try_clone()) else { return 1 };
    let args: Vec<String> = doc.get("args").and_then(Value::as_array).map_or_else(Vec::new, |items| items.iter().filter_map(Value::as_str).map(str::to_string).collect());
    let mut command = Command::new(text("program"));
    command.args(args).current_dir(text("cwd")).env_clear().stdin(Stdio::null()).stdout(out).stderr(err);
    for (name, value) in doc.get("env").and_then(Value::as_object).into_iter().flatten() {
        command.env(name, value.as_str().unwrap_or(""));
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }

    command.status().ok().and_then(|status| status.code()).unwrap_or(1)
}

// Local time as `strftime` shows it: year, month, day, hour, minute, second.
#[cfg(windows)]
pub fn local_time(at: f64) -> [i32; 6] {
    #[repr(C)]
    #[derive(Default)]
    struct SystemTimeParts {
        year: u16,
        month: u16,
        weekday: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        millis: u16,
    }
    #[repr(C)]
    struct FileTime {
        low: u32,
        high: u32,
    }
    extern "system" {
        fn FileTimeToSystemTime(file: *const FileTime, out: *mut SystemTimeParts) -> i32;
        fn SystemTimeToTzSpecificLocalTime(zone: *const u8, utc: *const SystemTimeParts, local: *mut SystemTimeParts) -> i32;
    }
    let ticks = ((at + 11_644_473_600.0) * 1e7) as u64;
    let file = FileTime { low: ticks as u32, high: (ticks >> 32) as u32 };
    let (mut utc, mut local) = (SystemTimeParts::default(), SystemTimeParts::default());
    // SAFETY: both structures are plain data the calls fill in.
    let shown = unsafe { FileTimeToSystemTime(&file, &mut utc) != 0 && SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) != 0 };
    let parts = if shown { local } else { utc };

    [parts.year as i32, parts.month as i32, parts.day as i32, parts.hour as i32, parts.minute as i32, parts.second as i32]
}

#[cfg(unix)]
pub fn local_time(at: f64) -> [i32; 6] {
    #[repr(C)]
    struct Tm {
        sec: i32,
        min: i32,
        hour: i32,
        mday: i32,
        mon: i32,
        year: i32,
        wday: i32,
        yday: i32,
        isdst: i32,
        gmtoff: i64,
        zone: *const u8,
    }
    extern "C" {
        fn localtime_r(time: *const i64, out: *mut Tm) -> *mut Tm;
    }
    let time = at as i64;
    let mut tm = Tm { sec: 0, min: 0, hour: 0, mday: 1, mon: 0, year: 70, wday: 0, yday: 0, isdst: 0, gmtoff: 0, zone: std::ptr::null() };
    // SAFETY: `tm` is a writable struct of the C library's layout.
    unsafe { localtime_r(&time, &mut tm) };

    [tm.year + 1900, tm.mon + 1, tm.mday, tm.hour, tm.min, tm.sec]
}

pub fn iso(at: f64) -> String {
    let t = local_time(at);

    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}", t[0], t[1], t[2], t[3], t[4], t[5])
}

pub fn minutes(at: f64) -> String {
    let t = local_time(at);

    format!("{:04}-{:02}-{:02} {:02}:{:02}", t[0], t[1], t[2], t[3], t[4])
}

pub fn append_error(line: &str) {
    let root = abspath(&state_root());
    let _ = fs::create_dir_all(&root);
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(root.join("errors.log")) {
        let _ = writeln!(file, "{line}");
    }
}
