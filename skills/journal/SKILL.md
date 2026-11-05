---
name: journal
description: "What the dam saved in this session and since it was installed: condensed outputs, Beaver's level, learned facts. Use for /the-dam:journal, \"what did the dam save\", the dam's counter under Codex or Copilot."
disable-model-invocation: true
---

Run the dam's `te` program with `journal`, adding `--host <agent>`: `claude` under Claude Code, `codex` under Codex, `copilot` under GitHub Copilot. The plugin's folder is two levels above this skill's folder:

- In a POSIX shell (Git Bash on Windows): `"<plugin folder>/scripts/te" --cwd "$(pwd)" journal --host <agent>`
- In PowerShell: `& "<plugin folder>\bin\windows-x64\dam.exe" te --cwd "$PWD" journal --host <agent>`

Show the output verbatim in a code block, with no commentary.
