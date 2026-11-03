# Changelog

## 0.18.0

- The status line shows one icon and figure per tool, and leaves out a tool with nothing to show.
- `/the-dam` opens the session's journal beside the chat, with the totals.
- The code graph answers on its own for the code names in a prompt and for a `Grep` on one name.

## 0.17.0

- Outputs Claude Code saves to a file because they are too long are condensed too, with errors and warnings put in front.
- `Grep` results are condensed.
- The code graph is announced from the first prompt, and a query builds it when it is not there yet. Prompts no longer wait up to 3 seconds each.
- Ponytail's start hook can no longer run past Claude Code's limit on a large project: its codebase map stops at 3 seconds.
- Several sessions at once: each has its own Ponytail level, saved originals and count; two graph builds no longer share a temporary file.
- The session-end hook no longer starts PowerShell, which made Claude Code cancel it.

## 0.16.0

- Licence changed to PolyForm Strict 1.0.0 from this version on. Earlier versions stay under Apache 2.0.
- Licence texts of the Rust libraries added in `licenses/THIRD-PARTY-RUST.txt`.

## 0.15.0

- TokenEconomizer is rewritten in Rust for Claude Code: no more Python, and its bundled copy is gone.
- Dropped: other agents, the repository map, `probe`, `te repos merge` and `migrate`.

## 0.14.0

- `dam-economizer status` and the end-of-session hook run in Rust; the status line no longer needs Python.
- Closing a headless session no longer reports `Hook cancelled` (two runs of `claude -p` checked).
- CI runs every program end to end on each system (`scripts/smoke.sh`).
- Compression comparison with Headroom measured again.

## 0.13.0

- Hooks run on Windows, Linux and macOS through `scripts/dam-run`, with builds from CI.
- TokenEconomizer's prompt and tool hooks run in Rust (`dam-economizer`); the rest stays in its bundled Python.
- First public release: icon, licence, documentation and CI.

## 0.12.0

- One Rust application with a small program per job.
- `dam-graph` replaces graphify, and `dam-ponytail` replaces Ponytail's Node hooks.

## 0.9.0 to 0.11.0

- Lossless compression with table folds, Headroom's reversible folds, and a built-in compressor instead of its proxy.
- TokenEconomizer and Ponytail bundled in the plugin.

## 0.1.0 to 0.8.0

- A status line and a text report in place of the first animated scene.
