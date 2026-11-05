---
name: repos
description: "List every repository TokenEconomizer keeps state for, or merge one repository's state into another. Use for /the-dam:repos."
argument-hint: "[list | migrate | merge KEY [INTO]]"
disable-model-invocation: true
---

The user's arguments: "$ARGUMENTS" (where that is not filled in, take them from what the user asked; it lists the repositories when there are none).

Run the dam's `te` program with `repos <arguments>`, adding `--host <agent>`: `claude` under Claude Code, `codex` under Codex, `copilot` under GitHub Copilot. The plugin's folder is two levels above this skill's folder:

- In a POSIX shell (Git Bash on Windows): `"<plugin folder>/scripts/te" --cwd "$(pwd)" repos <arguments> --host <agent>`
- In PowerShell: `& "<plugin folder>\bin\windows-x64\dam.exe" te --cwd "$PWD" repos <arguments> --host <agent>`

Show the output verbatim in a code block. It moves state only when the user typed `migrate` or `merge`; never add them yourself. If the list shows state recorded by an earlier version, say in one sentence that it is sorted out at the next session start, or now with `/the-dam:repos migrate`.
