---
name: statusline
description: "Put the dam's counter in GitHub Copilot CLI's status line, or take it out (Claude Code has it already; Codex's status line takes built-in items only). Use for /the-dam:statusline install|uninstall."
argument-hint: "[install | uninstall]"
disable-model-invocation: true
---

The user's arguments: "$ARGUMENTS" (where that is not filled in, take `install` or `uninstall` from what the user asked).

Run the dam's `te` program with `statusline <argument>`, adding `--host <agent>`: `claude` under Claude Code, `codex` under Codex, `copilot` under GitHub Copilot. The plugin's folder is two levels above this skill's folder:

- In a POSIX shell (Git Bash on Windows): `"<plugin folder>/scripts/te" statusline <argument> --host <agent>`
- In PowerShell: `& "<plugin folder>\bin\windows-x64\dam.exe" te statusline <argument> --host <agent>`

Show the output verbatim in a code block. It changes Copilot's `settings.json` only when the user typed `install` or `uninstall`; never choose it yourself.
