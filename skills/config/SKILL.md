---
name: config
description: "Show or change TokenEconomizer's settings: turn each mechanism on or off, or tune it. Use for /the-dam:config."
argument-hint: "[show | set KEY=VALUE ... [--global] | unset KEY ... [--global]]"
disable-model-invocation: true
---

The user's arguments: "$ARGUMENTS" (where that is not filled in, take them from what the user asked; it shows every setting when there are none).

Run the dam's `te` program with `config <arguments>`, adding `--host <agent>`: `claude` under Claude Code, `codex` under Codex, `copilot` under GitHub Copilot. The plugin's folder is two levels above this skill's folder:

- In a POSIX shell (Git Bash on Windows): `"<plugin folder>/scripts/te" --cwd "$(pwd)" config <arguments> --host <agent>`
- In PowerShell: `& "<plugin folder>\bin\windows-x64\dam.exe" te --cwd "$PWD" config <arguments> --host <agent>`

Show the output verbatim in a code block. It changes a setting only when the user typed `set` or `unset`; never add them yourself. If a setting was rejected, say why in one sentence.
