// Each program as Claude Code would run it, on a throwaway repository and state. No model and no network, so it says
// nothing about a real session, only that the programs work on this system.
use dam::session::TempDir;
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

struct Env {
    home: TempDir,
}

impl Env {
    // The program's stdout (and whether it succeeded) for `args`, with `input` on stdin.
    fn dam(&self, repo: Option<&Path>, args: &[&str], input: &[u8]) -> (bool, Vec<u8>) {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dam"));
        command.args(args).env_clear().stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        for key in ["PATH", "SYSTEMROOT"] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        let plugin = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        command.env("HOME", &self.home.0).env("USERPROFILE", &self.home.0).env("APPDATA", self.home.0.join("appdata")).env("TOKENECONOMIZER_HOME", self.home.0.join("state")).env("CLAUDECODE", "1").env("CLAUDE_PLUGIN_ROOT", plugin).env("TE_JUDGE_NO_SPAWN", "1");
        if let Some(repo) = repo {
            command.current_dir(repo);
        }
        let mut child = command.spawn().expect("dam");
        let mut stdin = child.stdin.take().unwrap();
        let body = input.to_vec();
        let writer = std::thread::spawn(move || {
            let _ = stdin.write_all(&body);
        });
        let out = child.wait_with_output().expect("dam ran");
        let _ = writer.join();

        (out.status.success(), out.stdout)
    }

    fn text(&self, repo: Option<&Path>, args: &[&str], input: &str) -> String {
        let (ok, out) = self.dam(repo, args, input.as_bytes());
        assert!(ok, "dam {args:?} failed");

        String::from_utf8_lossy(&out).into_owned()
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn git(repo: &Path, args: &[&str]) {
    let status = Command::new("git").args(args).current_dir(repo).stdout(Stdio::null()).stderr(Stdio::null()).status().expect("git");
    assert!(status.success(), "git {args:?}");
}

#[test]
fn compression_round_trips_without_loss() {
    let env = Env { home: TempDir::new("dam-smoke-").unwrap() };
    let log: String = (0..400).map(|i| format!("2026-01-01 12:00:{i} INFO worker {i} finished job {i} in {i} ms\n")).collect();
    let small = env.text(None, &["squeeze"], &log);
    assert!(small.len() < log.len());
    assert_eq!(env.text(None, &["squeeze", "--decode"], &small), log);
}

#[test]
fn the_graph_status_hooks_and_beaver_work_on_a_repository() {
    let env = Env { home: TempDir::new("dam-smoke-").unwrap() };
    let repo = env.home.0.join("repo");
    copy_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &repo.join("src"));
    git(&repo, &["init", "-q"]);
    git(&repo, &["add", "-A"]);

    env.text(Some(&repo), &["graph", "update"], "");
    assert!(env.text(Some(&repo), &["graph", "query", "where is the lossless decode"], "").contains("squeeze"));

    let cwd = repo.to_str().unwrap();
    assert!(env.text(None, &["te", "--cwd", cwd, "status"], "").contains("TokenEconomizer -- repo"));
    let status: Value = serde_json::from_str(&env.text(None, &["te", "--cwd", cwd, "status", "--json"], "")).unwrap();
    assert_eq!(status["identified_by"], "path");

    // A whole turn of hooks on it.
    let key = status["repo_key"].as_str().unwrap();
    let state = env.home.0.join("state");
    std::fs::create_dir_all(state.join("sessions").join("claude")).unwrap();
    std::fs::create_dir_all(state.join("repos").join(key).join("claude")).unwrap();
    std::fs::write(state.join("sessions").join("claude").join("smoke.json"), json!({"key": key, "root": cwd, "label": "repo", "remote": "", "how": "path", "at": 1}).to_string()).unwrap();
    for event in [
        json!({"hook_event_name": "UserPromptSubmit", "prompt_id": "p1", "prompt": "go"}),
        json!({"hook_event_name": "PostToolUse", "prompt_id": "p1", "tool_name": "Bash", "tool_input": {"command": "ls"}, "tool_response": {"stdout": "a"}}),
        json!({"hook_event_name": "Stop", "prompt_id": "p1"}),
        json!({"hook_event_name": "SessionEnd", "reason": "exit"}),
    ] {
        let mut payload = json!({"session_id": "smoke", "cwd": cwd});
        payload.as_object_mut().unwrap().extend(event.as_object().unwrap().clone());
        env.text(Some(&repo), &["te", "hook"], &payload.to_string());
    }
    let session = std::fs::read_to_string(state.join("repos").join(key).join("claude").join("sessions").join("smoke.json")).unwrap();
    assert!(session.contains("\"end_reason\": \"exit\""), "{session}");

    let start = json!({"session_id": "smoke", "cwd": cwd}).to_string();
    assert!(env.text(Some(&repo), &["beaver", "activate"], &start).contains("BEAVER"));
    // A hook loaded before 0.21 still names the old program.
    assert!(env.text(Some(&repo), &["dam-beaver", "activate"], &start).contains("BEAVER"));
    assert!(env.text(None, &["te", "--cwd", cwd, "config", "show"], "").contains("loopguard"));
}

#[test]
fn a_query_after_an_edit_is_answered_from_a_rebuilt_map() {
    let env = Env { home: TempDir::new("dam-smoke-").unwrap() };
    let repo = env.home.0.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::write(repo.join("a.py"), "def first_thing():\n    return 1\n").unwrap();
    git(&repo, &["init", "-q"]);
    git(&repo, &["add", "-A"]);
    env.text(Some(&repo), &["graph", "update"], "");
    assert!(!env.text(Some(&repo), &["graph", "query", "second_thing"], "").contains("b.py"));

    // Some filesystems stamp whole seconds.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(repo.join("b.py"), "def second_thing():\n    return 2\n").unwrap();
    git(&repo, &["add", "-A"]);
    let answer = env.text(Some(&repo), &["graph", "query", "second_thing"], "");
    assert!(answer.contains("b.py"), "{answer}");
}
