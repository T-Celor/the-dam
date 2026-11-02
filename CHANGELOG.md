# Changelog

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
