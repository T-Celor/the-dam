---
name: models
description: "Show or set which model does what, on the same ladder: balanced for the session, the most capable as advisor, the smallest for workers (Claude Code: Sonnet 5.5, Opus 5.5, Haiku 5.5; Codex: gpt-6.1-sol, gpt-6-astra, gpt-6-luna). Use for /the-dam:models show|install|uninstall."
argument-hint: "[show | install | uninstall]"
disable-model-invocation: true
---

The user's arguments: "$ARGUMENTS" (where that is not filled in, take `show`, `install` or `uninstall` from what the user asked; with none, `show`).

Run the dam's `te` program with `models <argument>`, adding `--host <agent>`: `claude` under Claude Code, `codex` under Codex. The plugin's folder is two levels above this skill's folder:

- In a POSIX shell (Git Bash on Windows): `"<plugin folder>/scripts/te" models <argument> --host <agent>`
- In PowerShell: `& "<plugin folder>\bin\windows-x64\dam.exe" te models <argument> --host <agent>`

Show the output verbatim in a code block. `install` and `uninstall` edit the user's own settings (Claude Code: `settings.json`, keys `model` and `advisorModel`; Codex: `config.toml`, keys `model` and `agents.default_subagent_model`, plus the `advisor` and `worker` agent files in `~/.codex/agents/`), nothing else; `uninstall` puts back what was there and leaves anything the user changed since. Run them only when the user typed them, never choose it yourself. The plugin also does `install` once by itself, in the first session after it is installed.

GitHub Copilot CLI is not covered: its settings key for the model is not documented well enough to edit safely.
