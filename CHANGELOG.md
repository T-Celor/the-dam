# Changelog

## 0.30.0

- **A use case in, the right pieces called.** Each prompt is read (`dam usecase`): new feature, bug fix, refactor, review, documents or question, in English or French. The agent gets the playbook for it, once per change of case: which of the dam's pieces to use and in what order. `dam usecase show` lists them, `DAM_USECASE=off` stops it.
- **The map comes first, enforced.** The map is built where it is missing (3 to 3,000 code files). The first exploration of a session, whatever the tool (Glob, Grep, Read of code, `ls`, `cat`, `rg`), is met once by the map's overview and the command to query it. A Grep for code names the map knows is answered by the map (at most three times a session); asked again, it runs. `DAM_MAPFIRST=off` stops it.
- **The design review comes before the code, enforced.** A design flagged in the session blocks Write, Edit and shell writes of code until `dam goldfish prepare` has run, twice at most so a host without sub-agents is never stuck. A design written with `cat >` or `Set-Content` is now seen too. Found by running the use case for real: the model had written the design through the shell and implemented first.
- Fixed: a path such as `docs/x.md` in a prompt no longer makes it a documentation request, and a sub-agent's report is not taken for a request.

## 0.29.1

- **The design review comes before the implementation, and is asked once per session.** The hook that flags a finished design document now says "before you implement anything from it", and flags a document once per session instead of after every edit (fixes made after a review are not a new draft). A real run of a billing change shows the whole chain: design, hook, four reviewers with the files the map found, implementation, Stop gate, advisor. README has the walk-through.

## 0.29.0

- **The advisor's review is now mandatory where it matters.** A Stop hook (`dam goldfish gate`, Claude Code; Codex and Copilot get the same entry) reads the turn that is ending: when it changed three or more code files, or any file whose path names auth, security, crypto, passwords, secrets, tokens, payments, billing, migrations, permissions or SQL, and no `advisor` or goldfish reviewer was called in that turn, the agent is not allowed to finish. It is told to call the `advisor` with the task, the changed paths and the test result, fix the Blockers and Majors, then finish. Asked once per turn, never when a stop hook already continued the turn, so it cannot loop. Documents (md, txt...) do not count; work done inside subagents does not either. `DAM_REVIEW=off` turns it off. Tested in a real session: three files written, stop refused, advisor (Opus 5.5) called, then finished.

## 0.28.0

- **The goldfish method is part of how every model works with the advisor and the workers.** Beaver's session rules (given to Claude Code, Codex and Copilot, and to subagents) have a new "Fresh eyes" section: broad reading and searching go to a `worker`; before calling a multi-file, security, money, data or migration change done, the `advisor` gets the task in one sentence, the changed paths and the test result, and nothing of the conversation, and its Blockers and Majors are fixed or answered; after two failed fixes it is asked before a third. The `advisor` agent now reviews as a goldfish (reads the code, not the summary, reports what is wrong first); the `worker` ends with what a reviewer should check first; the Codex `advisor` and `worker` files say the same.

## 0.27.0

- **The goldfish review and the documents' upkeep now happen by themselves, and cost less.** A hook after each file write (`dam goldfish hook`, every host) notices a finished design document (a Markdown file named design, spec, rfc, proposal or architecture, or with an `## Implementation Plan`) and tells the agent once per version to run `/the-dam:goldfish` without asking. The review is limited to the sections changed since the last one (`dam goldfish prepare` lists them, `dam goldfish mark` records the reviewed version), so a small edit is a small review and an unchanged document costs nothing. `DAM_GOLDFISH=ask` makes it offer first, `off` stops it. At session start the dam says once, in a line, when the README or a document under `docs/` names a file that is gone (or moved, with where the map finds it); no model runs for it. The map is used, never created, for that check.

## 0.26.0

- **The rest of the Elephant-Goldfish Model's skills.** `/the-dam:resolve` sorts review findings (contest, escalate, fix, decline) and applies the ones you accept; `/the-dam:goldfish-loop` repeats review and resolve until two clean rounds in a row; `/the-dam:bootstrap` writes a contract README per folder from the map, then checks it with `drift`.
- **TokenEconomizer, ported up to `0aa2aca`** (Hermes, Grok, Cursor and Pi are left out: the dam has three hosts). Micro-skills: a new `absent_tool` fact ("`ruff …` fails here with command not found, and no other way to run it has worked") learned only when the error names that tool, retired when it runs or a working form appears; `uvx` / `pipx run` / `python -m mypy` read as the tool; a build is repaired only by the same tool; `pre-commit` and `shellcheck` count as checks; test output text blocks keep their line breaks. Loop guard: putting back a file with `git checkout --` or `git restore` counts as an edit only if the turn edited it; Playwright results and `dotenv -e … --` prefixes are read. Not ported: the history reader, hook queue and the `held` decision.
- **Upstream is followed by itself.** A weekly scheduled task runs `dam upstream`, ports what applies, tests, pins and publishes.

## 0.25.0

- **Goldfish review, from Google's Elephant-Goldfish Model.** `/the-dam:design` turns an idea into a concepts brief and a specification, step by step. `/the-dam:goldfish DOCUMENT` has the document reviewed by reviewers that get the document and its source files but none of the conversation: four plugin agents (`goldfish-simplicity`, `goldfish-reader` on Haiku 5.5, `goldfish-critic`, `goldfish-ready`), five stages, findings settled with you and the `advisor`. The Rust part, `dam goldfish prepare`, finds the specification/plan split, the linked files inside the folder and, from the map, the files that define the code the document names. `/the-dam:drift` (`dam goldfish drift`) checks the documents' code spans against the map: a file that is gone (with where it is now), a name that no code file has (with its current spelling).
- **Follow the projects the dam builds on.** `dam upstream` reads `UPSTREAM`, asks each repository where it is now, lists the commits and the changed files under the ported paths (with `gh`), and `dam upstream pin NAME` records that you ported them. A weekly workflow opens an issue when one has moved. Elephant-Goldfish Model is now in `UPSTREAM` and `NOTICE`.

## 0.24.1

- **Gaps closed.** The map is also rebuilt before the answers the Claude Code module asks for (names in your prompt, search hits), not only for `graph query` and the Codex and Copilot hooks. Codex gets its models even when Beaver is off. The module names the host when it sets the Claude Code models. The README no longer gives 24% as the compression figure for a whole session (about 2.6%).

## 0.24.0

- **The same model ladder in Codex.** Balanced model for the session, most capable as advisor, smallest for the workers: `gpt-6.1-sol`, `gpt-6-astra`, `gpt-6-luna` (the ids on learn.chatgpt.com/docs/models; availability depends on your plan). The first Codex session after the plugin is installed writes `model = "gpt-6.1-sol"` and `agents.default_subagent_model = "gpt-6-luna"` into `~/.codex/config.toml` and adds two agent files in `~/.codex/agents/`, `advisor` (`gpt-6-astra`, read-only instructions) and `worker` (`gpt-6-luna`). Once only; it remembers the lines it replaced; `/the-dam:models uninstall` (`$the-dam:models` in Codex) puts them back, leaves a model you changed since and removes only the agent files it wrote. A `config.toml` with multi-line strings or a dotted `agents.` key is left untouched. A `-m` on the command line still wins. Copilot CLI is not covered: its settings key for the model is not documented well enough to edit safely.

## 0.23.0

- **Which Claude model does what (Claude Code).** Sonnet 5.5 for the session, Opus 5.5 as advisor, Haiku 5.5 for the workers. A plugin cannot set the session model or the advisor by itself (Claude Code only honours `agent` and `subagentStatusLine` in a plugin's settings), so the installation does it: the first session after the plugin is installed runs `dam te models install`, which writes `model` and `advisorModel` into your own `settings.json`, remembering what was there (once only; `/the-dam:models uninstall` puts it back and it stays undone, `/the-dam:models install` does it again) (a model you changed since is left alone; `--model` on the command line still wins). `/the-dam:models show` prints where things stand. The workers need no install: the module pins Claude Code's built-in subagents (general-purpose, Explore, Plan...) to Haiku 5.5 when the caller named no model, and leaves forks, named models and other agents as they are. The plugin also ships two agents, `worker` (Haiku 5.5) and `advisor` (Opus 5.5, read-only), for the main model to hand work to.

## 0.22.6

- **Temporary folders no longer collide.** Their names came from the clock alone, so two threads starting at the same instant could get the same folder and one would delete the other's: tests failed now and then (git or `sh` "not found" on macOS). A counter now tells them apart and an existing folder is never taken.

## 0.22.5

- **Correction: code and search output is never shortened.** 0.22.0 to 0.22.4 also cut the middle of what `cat`, `sed`, `head`, `tail`, `grep`, `rg`, `git diff` and `git show` printed, which hid code the model had asked to read. That output is now left whole, as is any command chaining one of them. The figures of 0.22.1 (13.9%) counted those outputs; measured with the real commands, shortening build, test and install output saves **0.8%** and the whole compression **2.6%** of tool output in long sessions. Most large output is code and search results read on purpose, and file reads (32% of tokens) are never touched. `dam squeeze commands FILE.jsonl...` shows which commands print the tokens.
- **Micro-skills note a repeated repair sooner.** A fact (for example "`pytest` is not found here, `python -m pytest` works") is learned after 3 sessions and was only told to the model after it had applied 4 more times, so five sessions in the same repository. It is now told after it has applied twice. A test runs three sessions and checks the note. Facts are kept per repository: sessions in a new folder each time never accumulate.

## 0.22.4

- **The map follows your edits.** It was only rebuilt by hand (`graph update`), so after a session of edits it answered with the old code: a function added an hour ago was "not found". Every `graph query`, and every hook that reads the map (Codex, Copilot), now rebuilds it first when a file changed after it was written. It still never creates a map by itself.

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
