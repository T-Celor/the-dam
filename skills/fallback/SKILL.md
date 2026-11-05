---
name: fallback
description: >
  Where plugin hooks are blocked, have the model fetch the dam's context itself: one marked instruction in the user's
  instructions file (CLAUDE.md, Codex's AGENTS.md or Copilot's copilot-instructions.md), or remove it.
  Use for /the-dam:fallback on|off|status.
argument-hint: "[on | off | status]"
disable-model-invocation: true
---

The user's arguments: "$ARGUMENTS" (where that is not filled in, take `on`, `off` or `status` from what the user asked; `status` when they said none).

Run the dam's `te` program with `fallback <argument> --host <agent>`, where `<agent>` is `claude` under Claude Code, `codex` under Codex, `copilot` under GitHub Copilot. The plugin's folder is two levels above this skill's folder:

- In a POSIX shell (Git Bash on Windows): `"<plugin folder>/scripts/te" fallback <argument> --host <agent>`
- In PowerShell: `& "<plugin folder>\bin\windows-x64\dam.exe" te fallback <argument> --host <agent>`

Show the output verbatim in a code block. It changes the user's instructions file only for `on` or `off`, and only when the user asked for that word; never choose it yourself. After `on`, say in one line that it applies from the next session, and to run it again after a plugin update.
