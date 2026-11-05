---
name: microskills
description: "Show the repository facts (micro-skills) TokenEconomizer learned and gives at session start. Use for /the-dam:microskills."
disable-model-invocation: true
---

Run the dam's `te` program with `microskills`, adding `--host <agent>`: `claude` under Claude Code, `codex` under Codex, `copilot` under GitHub Copilot. The plugin's folder is two levels above this skill's folder:

- In a POSIX shell (Git Bash on Windows): `"<plugin folder>/scripts/te" --cwd "$(pwd)" microskills --host <agent>`
- In PowerShell: `& "<plugin folder>\bin\windows-x64\dam.exe" te --cwd "$PWD" microskills --host <agent>`

Show the output verbatim in a code block, with no commentary.
