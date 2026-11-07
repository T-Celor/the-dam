---
name: models
description: "Show or set which Claude model does what: Sonnet 5.5 for the session, Opus 5.5 as the advisor, Haiku 5.5 for the workers. Use for /the-dam:models show|install|uninstall."
argument-hint: "[show | install | uninstall]"
disable-model-invocation: true
---

The user's arguments: "$ARGUMENTS" (where that is not filled in, take `show`, `install` or `uninstall` from what the user asked; with none, `show`).

Run the dam's `te` program with `models <argument>`. The plugin's folder is two levels above this skill's folder:

- In a POSIX shell (Git Bash on Windows): `"<plugin folder>/scripts/te" models <argument>`
- In PowerShell: `& "<plugin folder>\bin\windows-x64\dam.exe" te models <argument>`

Show the output verbatim in a code block. `install` and `uninstall` edit the user's own `settings.json` (`model` and `advisorModel`, nothing else; `uninstall` puts back what was there); run them only when the user typed them, never choose it yourself. The workers need no install: the plugin's module pins Claude Code's built-in subagents to Haiku 5.5, and the `worker` and `advisor` agents carry their model.
