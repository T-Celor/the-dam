---
name: toolprune
description: "Review the tools TokenEconomizer left out of this repository's sessions, give them back, or apply a waiting proposal. Use for /the-dam:toolprune."
argument-hint: "[show | apply [TOOL ...] | restore [TOOL ...] | refresh-catalog | measure]"
disable-model-invocation: true
---

The user's arguments: "$ARGUMENTS" (where that is not filled in, take them from what the user asked; it shows the proposal when there are none).

Run the dam's `te` program with `toolprune <arguments>`, adding `--host <agent>`: `claude` under Claude Code, `codex` under Codex, `copilot` under GitHub Copilot. The plugin's folder is two levels above this skill's folder:

- In a POSIX shell (Git Bash on Windows): `"<plugin folder>/scripts/te" --cwd "$(pwd)" toolprune <arguments> --host <agent>`
- In PowerShell: `& "<plugin folder>\bin\windows-x64\dam.exe" te --cwd "$PWD" toolprune <arguments> --host <agent>`

Show the output verbatim in a code block. It applies or restores the deny list only when the user typed `apply` or `restore`; never add them yourself. If it shows a proposal waiting for `toolprune apply`, end with one line: `/the-dam:toolprune apply` applies it, from the next session. If it shows a pending list, say it is written by itself once this repository's sessions have been idle, or now with `/the-dam:toolprune apply`.
