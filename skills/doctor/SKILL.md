---
name: doctor
description: >
  Check that the dam's context reaches the model here (hooks, managed policies) and whether the fallback is needed.
  Use for /the-dam:doctor, "dam doctor", "is the dam working", "the map is not added".
disable-model-invocation: true
---

Run the dam's `te` program with `doctor --host <agent>`, where `<agent>` is `claude` under Claude Code, `codex` under Codex, `copilot` under GitHub Copilot. The plugin's folder is two levels above this skill's folder:

- In a POSIX shell (Git Bash on Windows): `"<plugin folder>/scripts/te" --cwd "$(pwd)" doctor --host <agent>`
- In PowerShell: `& "<plugin folder>\bin\windows-x64\dam.exe" te --cwd "$PWD" doctor --host <agent>`

Show the output verbatim in a code block. If a `policy` line shows, or the `fallback` line says hooks look blocked, end with one sentence: the organization's settings keep the dam from adding its context, and the dam's `fallback` skill with `on` makes the model fetch it instead. Never turn the fallback on yourself.
