// The hook lines exactly as Codex and Copilot run them, not `dam` called directly: each handler of hooks/codex.json
// and hooks/copilot.json goes through the shell its host uses (Codex: `sh -c`, on Windows `%COMSPEC% /C` with
// `commandWindows`; Copilot: `bash`, and `powershell` on Windows), from an installed copy of the plugin whose path has a
// space and an `&`. That is where quoting, PATH and launcher mistakes show. No model, no network.
use dam::session::TempDir;
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn source() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("the plugin's folder")
}

fn folder() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", _) => "windows-x64",
        ("linux", "aarch64") => "linux-arm64",
        ("linux", _) => "linux-x64",
        (_, "aarch64") => "macos-arm64",
        _ => "macos-x64",
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

// What a user's machine has after installing the plugin: scripts, skills, hooks and this build's program, in a folder
// whose name would break a careless command line.
struct Installed {
    home: TempDir,
    root: PathBuf,
    repo: PathBuf,
}

fn install() -> Installed {
    let home = TempDir::new("dam-launch-").unwrap();
    let root = home.0.join("plug in & more");
    for part in ["scripts", "skills", "hooks", ".claude-plugin"] {
        copy_dir(&source().join(part), &root.join(part));
    }
    let ext = if cfg!(windows) { ".exe" } else { "" };
    let bin = root.join("bin").join(folder());
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::copy(env!("CARGO_BIN_EXE_dam"), bin.join(format!("dam{ext}"))).unwrap();
    let repo = home.0.join("repo");
    std::fs::create_dir_all(repo.join("src")).unwrap();
    std::fs::write(repo.join("src").join("a.py"), "def a_thing():\n    return 1\n").unwrap();
    for args in [&["init", "-q"][..], &["add", "."]] {
        assert!(Command::new("git").args(args).current_dir(&repo).stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap().success());
    }

    Installed { home, root, repo }
}

// The payload a host sends for `event`, in that host's shape.
fn payload(host: &str, event: &str, repo: &Path) -> Value {
    let cwd = repo.to_string_lossy();
    if host == "codex" {
        json!({"session_id": "s1", "hook_event_name": event, "cwd": cwd, "source": "startup", "prompt": "why does a_thing return 1?", "prompt_id": "p1", "tool_name": "Bash", "tool_input": {"command": "ls"}, "tool_response": "ok", "model": "gpt", "transcript_path": null})
    } else {
        json!({"sessionId": "s1", "timestamp": 1, "cwd": cwd, "source": "new", "prompt": "why does a_thing return 1?", "transformedPrompt": "<user>why does a_thing return 1?</user>", "toolName": "bash", "toolArgs": {"command": "ls"}, "toolResult": {"resultType": "success", "textResultForLlm": "ok"}})
    }
}

// Runs `command` as `shell` with the host's environment and `input` on stdin; (success, stdout, stderr).
fn execute(installed: &Installed, host: &str, shell: &str, command: &str, input: &Value) -> (bool, String, String) {
    let mut run = match shell {
        "sh" => {
            let mut run = Command::new("sh");
            run.arg("-c").arg(command);
            run
        }
        "cmd" => {
            // Codex's `%COMSPEC% /C <line>`, the line passed untouched.
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                let mut run = Command::new(std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into()));
                run.arg("/C").raw_arg(command);
                run
            }
            #[cfg(not(windows))]
            unreachable!()
        }
        _ => {
            let mut run = Command::new("powershell");
            run.args(["-NoProfile", "-NonInteractive", "-Command", command]);
            run
        }
    };
    run.current_dir(&installed.repo).env_clear().stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    for key in ["PATH", "SYSTEMROOT", "COMSPEC", "PATHEXT", "TEMP", "TMP"] {
        if let Some(value) = std::env::var_os(key) {
            run.env(key, value);
        }
    }
    let home = &installed.home.0;
    run.env("HOME", home).env("USERPROFILE", home).env("APPDATA", home.join("appdata")).env("TOKENECONOMIZER_HOME", home.join("state")).env("TE_JUDGE_NO_SPAWN", "1").env("PLUGIN_ROOT", &installed.root);
    // Codex also sets the data folder; Copilot sets its own.
    run.env(if host == "codex" { "PLUGIN_DATA" } else { "COPILOT_PLUGIN_DATA" }, home.join("data"));
    let mut child = run.spawn().expect("a shell");
    let mut stdin = child.stdin.take().unwrap();
    let body = input.to_string();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(body.as_bytes());
    });
    let out = child.wait_with_output().expect("the hook ran");
    let _ = writer.join();

    (out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned())
}

// What a hook may print: nothing, JSON, or (Beaver's start under Claude and Codex) plain text.
fn check(label: &str, line: &str, result: (bool, String, String)) {
    let (ok, out, err) = result;
    assert!(ok, "{label}: `{line}` failed: {err}");
    assert!(!err.contains("usage:") && !err.contains("not recognized") && !err.contains("cannot find") && !err.contains("No such file"), "{label}: `{line}`: {err}");
    let text = out.trim();
    if text.starts_with('{') || text.starts_with('[') {
        serde_json::from_str::<Value>(text).unwrap_or_else(|e| panic!("{label}: `{line}` printed broken JSON ({e}): {text}"));
    }
}

fn handlers(file: &str, key: &str) -> Vec<(String, Value)> {
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(source().join(file)).unwrap()).unwrap();
    let mut found = Vec::new();
    for (event, entries) in doc["hooks"].as_object().unwrap() {
        for entry in entries.as_array().unwrap() {
            // Codex nests handlers in a group; Copilot's entries are the handlers.
            match entry["hooks"].as_array() {
                Some(list) if key == "command" => found.extend(list.iter().map(|h| (event.clone(), h.clone()))),
                _ => found.push((event.clone(), entry.clone())),
            }
        }
    }

    found
}

#[test]
fn every_codex_hook_line_runs_the_way_codex_runs_it() {
    let installed = install();
    let mut ran = 0;
    for (event, handler) in handlers("hooks/codex.json", "command") {
        let (shell, line) = if cfg!(windows) { ("cmd", handler["commandWindows"].as_str().unwrap()) } else { ("sh", handler["command"].as_str().unwrap()) };
        let result = execute(&installed, "codex", shell, line, &payload("codex", &event, &installed.repo));
        if line.contains("beaver activate") {
            assert!(result.1.contains("BEAVER MODE ACTIVE"), "`{line}` printed {:?} {:?}", result.1, result.2);
        }
        check(&format!("codex {event}"), line, result);
        ran += 1;
    }
    assert!(ran >= 4, "{ran} Codex hooks");
}

#[test]
fn every_codex_unix_line_runs_in_sh_too() {
    // On Windows Codex uses `commandWindows`, but the `command` line is what macOS and Linux run: it needs no Windows.
    if cfg!(windows) && Command::new("sh").arg("-c").arg("true").status().is_err() {
        return;
    }
    let installed = install();
    for (event, handler) in handlers("hooks/codex.json", "command") {
        let line = handler["command"].as_str().unwrap();
        check(&format!("codex sh {event}"), line, execute(&installed, "codex", "sh", line, &payload("codex", &event, &installed.repo)));
    }
}

#[test]
fn every_copilot_bash_line_runs_the_way_copilot_runs_it() {
    if Command::new("sh").arg("-c").arg("true").status().is_err() {
        return;
    }
    let installed = install();
    let mut ran = 0;
    for (event, handler) in handlers("hooks/copilot.json", "bash") {
        let line = handler["bash"].as_str().unwrap();
        let result = execute(&installed, "copilot", "sh", line, &payload("copilot", &event, &installed.repo));
        if line.contains("beaver activate") {
            assert!(result.1.contains("additionalContext"), "`{line}` printed {:?} {:?}", result.1, result.2);
        }
        check(&format!("copilot bash {event}"), line, result);
        ran += 1;
    }
    assert!(ran >= 4, "{ran} Copilot hooks");
}

#[test]
fn every_copilot_powershell_line_runs_on_windows() {
    if !cfg!(windows) {
        return;
    }
    let installed = install();
    for (event, handler) in handlers("hooks/copilot.json", "powershell") {
        let line = handler["powershell"].as_str().unwrap();
        check(&format!("copilot powershell {event}"), line, execute(&installed, "copilot", "powershell", line, &payload("copilot", &event, &installed.repo)));
    }
}

// A repository could plant a `dam.exe` or `cmd.exe`: Codex's Windows line must never run them.
#[test]
fn a_program_planted_in_the_project_is_never_run() {
    if !cfg!(windows) {
        return;
    }
    let installed = install();
    // Copies of cmd.exe named like what the line starts: if either ran, Beaver's rules would not be printed.
    for name in ["dam.exe", "cmd.exe"] {
        std::fs::copy(std::env::var("COMSPEC").unwrap(), installed.repo.join(name)).unwrap();
    }
    let start = payload("codex", "SessionStart", &installed.repo);
    let (_, handler) = handlers("hooks/codex.json", "command").into_iter().find(|(_, h)| h["command"].as_str().is_some_and(|c| c.contains("beaver activate"))).unwrap();
    let line = handler["commandWindows"].as_str().unwrap();
    let (ok, out, err) = execute(&installed, "codex", "cmd", line, &start);
    assert!(ok && out.contains("BEAVER MODE ACTIVE"), "the plugin's own program did not run: {out} {err}");
    // The check can fail: the same line without the leading built-ins does run the planted program.
    let naive = line.rsplit_once("&& ").unwrap().1;
    let (_, out, _) = execute(&installed, "codex", "cmd", naive, &start);
    assert!(!out.contains("BEAVER MODE ACTIVE"), "the naive line should have run the planted dam.exe");
}
