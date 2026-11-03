# Changelog

## 0.20.3

- A hook's program ends by itself once Claude Code has given up on it: `dam-beaver` after 25 s, `dam-economizer hook` after 70 s. One `dam-beaver track` had stayed alive for hours, which kept its `.exe` locked and the plugin from updating.

## 0.20.2

- The README opens on a short animation of the plugin, and gives each part its own section: a loop showing how it works and what it saves, then a few lines of text. The measured gains draw themselves in at the top of the Gains section.
- The pictures gain a little nature: water, reeds and a beaver on its log dam. The colour band is gone. Every picture is 3840 pixels wide, in a light and a dark theme, with a still for readers who turn motion off.
- The README is shorter: the overview diagram, the concept table and the two charts it repeated are gone.

## 0.20.1

- The README's pictures are redrawn as flat infographics, in a light and a dark theme that follow the reader's: the measured gains as one still at the top, the two charts as stills, and one short loop per part of the dam (compression, web pages, map, tool pruning, loop guard, learned facts, Beaver). `assets/draw.py` draws them all; the long animation is gone.

## 0.20.0

- 💧 counts what each request did not carry: a token taken out of a tool output stays out of every later request, so it counts once per request until a compaction empties the context. A pruned MCP tool's schema counts once per request, with its size measured while it was loaded. 0.19.2 counted a saved token once against a total that counts the context at every request, which read as 0.1%.
- MCP tools' text outputs are condensed too, back in their own shape; images and structured results are left alone.
- Outputs are condensed from 2,000 characters instead of 6,000, when that saves 100 tokens or more.
- The README's animation comes in a light and a dark theme, following the reader's, drawn in Apple's system colours and type at 1680 × 900, with the logo drawn from its own shapes.
- Subagents' reports and WebFetch's answers stay as they are: short prose with nothing repeated. Read stays exact, as Edit needs the file's bytes.

## 0.19.2

- 💧 reads `saved/total (%)`: what compression saved against every token this session's model requests counted (input, cache and output, subagents included), so it moves from the first answer even when nothing could be condensed.
- Token counts go up to G and T.

## 0.19.1

- The scripts are executable again on Linux and macOS: 0.19.0 lost their executable bit, so its hooks could not start there.
- The README opens with an animation of what each part does and what it saves (`assets/teaser.py` draws it).

## 0.19.0

- Ponytail is renamed Beaver: `/the-dam:beaver`, `beaver-review`, `beaver-audit`, `beaver-debt`, `beaver-gain`, `beaver-help`, `/beaver lite|full|ultra`, `stop beaver`. Ponytail's commands and settings still work.
- Beaver's rules are shorter and tell the model to ask the map before searching, follow the learned facts and open a condensed output's original only when needed.
- `UPSTREAM` records the release or commit the dam follows from Ponytail, TokenEconomizer, Headroom and graphify, and `scripts/upstream.sh` shows what changed in each since.
- The code graph is now the map, and it reads documents and settings: Markdown and reStructuredText headings, links and quoted code names, and the top-level keys of YAML, TOML and JSON.
- Fact learning is on by default. As in Microsoft's version, it collects from the first session, and its proposer first calls a model after the second.
- A pending list of tools to leave out no longer waits for good when its waiting process was killed: each session start starts another.
- The status line always shows every tool, with `0`, `off` or `–`.
- `/the-dam`'s journal shows the newest entries first, leaves out what does not fit with a count, and keeps this session's totals on its last rows.
- The tokens saved in this session are counted per session id, so a reload of the plugin no longer starts them again at 0.
- A web page a `Bash` or `PowerShell` command fetched reaches the model as Markdown, as markdowner gives it: its main content, without menus, scripts or styles, with the full HTML kept in a file. Written in Rust, with no network call and no parser library.
- The Markdown follows html-to-markdown's rules: a backslash only where text would read as Markdown, hard line breaks, tables with spans, captions and empty headers, and layout tables read as text.
- The status line and `/the-dam` show the tokens compression saved in this session and in this project, each with its share of what the tools printed.
- `PowerShell` output is condensed like `Bash` output.
- TokenEconomizer brought in line with Microsoft's main of 2026-11-02 (d58755d), for what concerns Claude Code:
  - the loop guard sees a test run started through a launcher (`uv run pytest`, `npx jest`, `bundle exec rspec`) or a Python (`python tests/runtests.py`), and counts a run as a check only where `status_of_tests` does;
  - a repository whose label holds a Windows path keeps one state file name, and local Windows remotes are recognised;
  - a loop-guard count threshold with a fraction is refused;
  - `claude` installed by npm as a `.cmd` shim is started without cmd.exe;
  - `toolprune measure` passes the deny list as a settings file and reports a failed probe instead of counting it as saved;
  - `/the-dam:status` explains each mechanism's state (`te status --explain`).

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
