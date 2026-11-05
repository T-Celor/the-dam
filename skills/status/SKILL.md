---
name: status
description: "Show what TokenEconomizer has learned for this repository and what it has done. Use for /the-dam:status."
disable-model-invocation: true
---

Run the dam's `te` program with `status --explain`, adding `--host <agent>`: `claude` under Claude Code, `codex` under Codex, `copilot` under GitHub Copilot. The plugin's folder is two levels above this skill's folder:

- In a POSIX shell (Git Bash on Windows): `"<plugin folder>/scripts/te" --cwd "$(pwd)" status --explain --host <agent>`
- In PowerShell: `& "<plugin folder>\bin\windows-x64\dam.exe" te --cwd "$PWD" status --explain --host <agent>`

Show the output verbatim in a code block. Add nothing unless it contains a warning; then explain the warning in one sentence.
