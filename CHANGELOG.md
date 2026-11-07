# Changelog

## 0.22.3

- **Beaver replies are shorter still.** A finished task is at most 3 lines of under 20 words (what changed, the check and its result, the skip or risk); the first word is the answer; no bullets, bold, recap, offer of more work or closing question under 10 lines; ten lines at most even for a review.

## 0.22.2

- **A notification for every saving.** Under Claude Code each condensed output, and each change in pruned tools, now shows a toast with the tokens before and after and the share saved (it was only written to the journal). Under Codex the hook sends the same line as a message to the person (`systemMessage`, not shown to the model).
- **Beaver answers are short.** Beaver's rules had no limit on the reply, only on the code. They now say: at most 5 lines for a finished task (what changed, how it was checked, what was skipped or risky), the answer first, and no opening, restating, narrating, recap of the diff or offer of more work; longer only when the user asks for detail or a review.

## 0.22.1

- **Long shell output is shortened, not only folded.** The lossless folding saves about 2% of all tool output in real sessions, because most output has few repeated lines. Shell tools printed 48% of the tokens in 11,000 results from real transcripts, so from about 1,000 tokens (4,000 characters) a command's output now keeps its first 15 lines, its last 30 and every line that mentions an error, a failure or a warning (with the lines around it, up to 80 such places); the lines between become `[... N lines omitted ...]`, and the footer says so and gives the path of the whole output. Measured on those results (8,392 calls, 3.0M tokens, each counted once): **13.9%** of all tool output saved, against 1.6% with the lossless folding alone. Results that repeat an earlier one exactly are only 2.6%, so they are left as they are. Only what a shell printed is shortened (never a file read or a search), and only when that saves at least 30%. `DAM_ELIDE=off` brings back "every line kept". `dam squeeze bench FILE.jsonl...` prints the figures for your own sessions.

## 0.21.0

The dam runs in Claude Code, OpenAI Codex CLI and GitHub Copilot CLI, as one Rust program.

- **One program.** `dam` replaces `dam-squeeze`, `dam-graph`, `dam-beaver` and `dam-economizer`: `dam squeeze`, `dam graph`, `dam beaver`, `dam te`. One build per system to ship and update. The old names still work as subcommands, so a session open across the update keeps its hooks.
- **Codex and Copilot.** Manifests, marketplaces and hooks for each (`.codex-plugin/`, `.github/plugin/`, `.agents/plugins/`, `hooks/codex.json`, `hooks/copilot.json`). There the dam condenses long outputs and fetched pages (Copilot through `modifiedResult`, Codex through the hook's feedback, which replaces the output), answers from the map after a search and, in Codex, for code a prompt names, runs the loop guard and learns facts from each host's session log (ported back from TokenEconomizer's Python), with its judge run through `codex exec` (read-only sandbox, its hooks off) or `copilot -p` (every tool denied). State is kept per host. Tool pruning too: unused capability packages switched off in the project's `.codex/config.toml` (only a trusted, uncommitted, plain file, checked with `codex features list` and put back if Codex refuses it), unused MCP servers in Copilot's `.github/copilot/settings.local.json` (only in a folder Copilot trusts); `toolprune restore` gives back exactly what was there.
- **One declaration.** `.claude-plugin/plugin.json` and `hooks/hooks.json` are written by hand; `dam hosts` writes every Codex and Copilot file from them, and the tests fail when one is out of date. The generator, the end-to-end test and the packaging tests are Rust now (`dam/src/hostgen.rs`, `dam/tests/`), so `cargo test` runs them all.
- **Compression on real sessions.** The lossless folding saved 24% on a corpus of repetitive outputs but about 1.3% of all tool output in long sessions (measured on 11,000 results from real transcripts), and the in-process floor of 6,000 characters kept outputs of 2,000 to 6,000 characters from being condensed although the hooks' floor was 2,000. They now agree on 2,000 (about 2% overall). `dam squeeze bench FILE.jsonl...` prints the saving by output size for your own transcripts.
- **Decisions in Rust.** What to condense, web pages, code names, a search's name, the map's exact entries and the notes are decided in `dam/src/answers.rs` for every host; the Claude Code module calls `dam squeeze condense` and `dam graph facts` and keeps only its interface and accounting.
- **The counter everywhere.** Each host's condensed outputs are counted per session; `/the-dam:journal` shows them in any host, and `/the-dam:statusline install` puts them in Copilot's status line (`~/.copilot/settings.json`, never over a status line of its own). Codex's status line takes built-in items only.
- **Copilot answers prompts from the map** through `userPromptTransformed`, as its prompt hook's output is dropped.
- **Every command is a skill**, so it works in the three hosts; in Claude Code the names stay (`/the-dam:status`, `config`, `toolprune`, …). `te` takes `--host claude|codex|copilot`.
- **Managed setups that block hooks.** `/the-dam:doctor` shows whether the hooks and the module ran on this machine and names a blocking policy it finds (Claude Code's managed settings, Codex's `allow_managed_hooks_only` or `[features] hooks = false`). `/the-dam:fallback on` adds one marked instruction to the user's instructions file (`~/.claude/CLAUDE.md`, `~/.codex/AGENTS.md` or `~/.copilot/copilot-instructions.md`), so the model runs `dam te context` and gets the learned facts, the map's command, how to condense outputs and Beaver's rules. Off by default; nothing changes where hooks run.
- The programs find the plugin's folder from `PLUGIN_ROOT` too: they looked for a `te_bootstrap.py` that is gone, so away from Claude Code Beaver fell back to a one-line rule.
- Reviewed for security: the Codex line on Windows, which Codex runs with `%COMSPEC% /C`, starts with built-ins only, so it never runs a `cmd.exe` or `dam.exe` a repository ships (cmd.exe searched the project first), and a `&` in the plugin's path no longer splits it; the fallback refuses to rewrite an instructions file it cannot read or whose markers do not pair up, and writes it atomically; the Codex judge gets none of the person's MCP servers and its reply is dropped if it used a tool; saved outputs are private to the person on Unix; every settings file the dam edits is written atomically, through a new file it alone creates; a `.dam-graph` the repository commits is never used as the map, and the facts added to a prompt stop at 4,000 characters; git runs with `core.fsmonitor` off, and where git cannot say whether a file is committed the dam treats it as committed; the exclude file is found with `git rev-parse --git-path`; a TOML section header counts only outside brackets; only the exact status line the dam wrote is ever replaced or removed.
- No proxy: hooks reach the outputs on all three hosts, and a proxy would sit on the API traffic and its credentials.

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
