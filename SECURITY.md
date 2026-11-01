# Security

Please report a vulnerability privately through [GitHub's security advisories](https://github.com/T-Celor/the-dam/security/advisories/new), not in a public issue.

## What the plugin touches

- It reads the output of your `Bash` tool calls and rewrites what the model sees of them.
- It writes full outputs to `~/.claude/the-dam-outputs/` (the last 20), code graphs to `.dam-graph/` in your projects, and TokenEconomizer's state to `~/.tokeneconomizer/`. These can contain anything your commands printed, including secrets.
- At session start it can install what is missing with `uv` or `cargo`.
- TokenEconomizer's judge can send a digest of recent commands and error signatures to a model through your own Claude Code. `/the-dam:config` turns it off.

The Rust programs make no network calls. The Windows builds are not signed.
