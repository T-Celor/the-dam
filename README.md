<p align="center">
  <img src="assets/icon.svg" width="128" height="128" alt="The dam's beaver">
</p>

<h1 align="center">The dam</h1>

<p align="center">
  <strong>Spend fewer tokens in Claude Code without losing a line.</strong>
</p>

<p align="center">
  <a href="https://github.com/T-Celor/the-dam/actions/workflows/ci.yml"><img src="https://github.com/T-Celor/the-dam/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-PolyForm%20Strict%201.0.0-blue.svg" alt="PolyForm Strict 1.0.0"></a>
  <img src="https://img.shields.io/badge/built%20with-Rust-b7410e.svg" alt="Built with Rust">
  <img src="https://img.shields.io/badge/runs-100%25%20local-2e7d32.svg" alt="Runs locally">
</p>

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark) and (prefers-reduced-motion: reduce)" srcset="assets/intro-dark.png">
    <source media="(prefers-reduced-motion: reduce)" srcset="assets/intro.png">
    <source media="(prefers-color-scheme: dark)" srcset="assets/intro-dark.gif">
    <img src="assets/intro.gif" width="100%" alt="The dam in a few seconds: a beaver on its log dam, then seven cards. Compression, 24% fewer tokens with every line kept. Web pages, 99% less of a page, HTML to Markdown. Map, 2,112 files mapped in 1.5 s. Tool pruning, fewer tools on every request. Loop guard, one nudge out of a loop. Learned facts, remembered across sessions. Beaver, 45% less output. Then the install command.">
  </picture>
</p>

## Contents

[Install](#install) · [Commands](#commands) · [Status line](#status-line) · [How it works](#how-it-works) · [Gains](#gains) · [Built on](#built-on) · [Maintenance](#maintenance) · [Limits](#limits) · [Licence](#licence)

## Install

```
/plugin marketplace add T-Celor/the-dam
/plugin install the-dam@the-dam
```

Restart Claude Code. The programs come prebuilt for Windows x64, Linux x64 and arm64, and macOS x64 and arm64. On any other system the plugin offers to build them with `cargo`.

If the separate TokenEconomizer or Ponytail plugins are installed, turn them off so nothing runs twice:

```
claude plugin disable tokeneconomizer@tokeneconomizer
claude plugin disable ponytail@ponytail
```

## Commands

| Command | What it does |
|---|---|
| `/the-dam` | The session's journal beside the chat, newest first, with its totals |
| `/the-dam-compress` | Compression on or off |
| `/the-dam:status` | What TokenEconomizer learned for this repository |
| `/the-dam:toolprune` | The tools left out here; give one back or apply a waiting list |
| `/the-dam:config` | Each TokenEconomizer mechanism on, off or tuned |
| `/the-dam:microskills` | The learned facts given at session start |
| `/the-dam:repos` | Every repository with learned state |
| `/beaver lite\|full\|ultra\|off` | Beaver's level for this session (`full` by default) |
| `/the-dam:beaver-review`, `:beaver-audit` | Review of a change, or of the whole repository |
| `/the-dam:beaver-debt` | Every `shortcut:` comment, as a ledger |
| `/the-dam:beaver-gain`, `:beaver-help` | Beaver's benchmark and reference card |

## Status line

```
🦫 full · 💧 412k/12.4M (3.2%) · 🪵 27 · 🌀 1 · 🧠 5 · 🗺️ 2.2k · 🌊 31%
```

| Icon | Figure |
|---|---|
| 🦫 | Beaver's level |
| 💧 | Tokens the session's requests did not carry, out of all they counted. A token taken out of a tool output counts once for each later request, since each would have re-read it, until a compaction. A pruned MCP tool's schema counts once per request. |
| 🪵 | Tools left out, `+n` waiting |
| 🌀 | Loops stopped |
| 🧠 | Facts given to the model |
| 🗺️ | Places on the project's map |
| 🌊 | Context window in use |

A `0`, `off` or `–` means the tool has had nothing to do yet.

## How it works

### Compression

<picture><source media="(prefers-color-scheme: dark)" srcset="assets/part-compression-dark.gif"><img src="assets/part-compression.gif" width="600" alt="Five retry lines fold into one template with a table of values: 24% fewer tokens"></picture>

`dam-squeeze` takes Bash, PowerShell, Grep and MCP outputs of 2,000 characters or more. Lines that differ only by numbers become a template and a table of values, repeated lines a count, search hits a list per file, JSON records a table. Errors and outliers stay as they are, and each result is decoded and checked against the source before the model sees it. The original stays in `~/.claude/the-dam-outputs/`.

### Web pages

<picture><source media="(prefers-color-scheme: dark)" srcset="assets/part-web-dark.gif"><img src="assets/part-web.gif" width="600" alt="Menus, ads and scripts drop out of a fetched page and the rest becomes Markdown: 99% smaller"></picture>

When a command fetches a page (`curl`, `wget`, `Invoke-WebRequest`), `dam-squeeze --web` keeps its main content, as Readability picks it, and writes it as Markdown with html-to-markdown's rules for escaping, tables, code and links. The full HTML is kept in a file.

### Map

<picture><source media="(prefers-color-scheme: dark)" srcset="assets/part-map-dark.gif"><img src="assets/part-map.gif" width="600" alt="A question walks the local map to where a name is defined: crusher.rs line 118"></picture>

`dam-graph` records who defines, calls, extends and imports what in 10 languages, plus document headings and links and the keys of YAML, TOML and JSON. It is rebuilt at each session start. When a prompt or a `Grep` names something, where it is defined and what calls it are added for the model.

### Tool pruning

<picture><source media="(prefers-color-scheme: dark)" srcset="assets/part-pruning-dark.gif"><img src="assets/part-pruning.gif" width="600" alt="Twelve tools, the six never used here left out"></picture>

A tool unused over 10 prompts in 2 sessions, or over 5 sessions, is left out of later sessions once the repository has been idle for an hour. Every request is then shorter. `/the-dam:toolprune` gives any tool back.

### Loop guard

<picture><source media="(prefers-color-scheme: dark)" srcset="assets/part-loops-dark.gif"><img src="assets/part-loops.gif" width="600" alt="The same error three times, then one nudge to try another way"></picture>

When a turn keeps hitting the same failure, a judge model checks it and the model gets one note to try another approach.

### Learned facts

<picture><source media="(prefers-color-scheme: dark)" srcset="assets/part-facts-dark.gif"><img src="assets/part-facts.gif" width="600" alt="A failure seen in three sessions becomes a fact given at the next start"></picture>

Failures repeated across sessions, and a model review every 3 sessions, become facts once 3 sessions confirm them: a build step, or the command spelling that works here. Each new session starts with them.

### Beaver

<picture><source media="(prefers-color-scheme: dark)" srcset="assets/part-beaver-dark.gif"><img src="assets/part-beaver.gif" width="600" alt="Fifty lines of change become one"></picture>

Ponytail's rules, rewritten: make the smallest change that fully solves the task, and ask the map before searching. They are given at session start and to each subagent.

Several sessions can run at once. Each keeps its own level, originals and counts.

## Gains

<picture><source media="(prefers-color-scheme: dark) and (prefers-reduced-motion: reduce)" srcset="assets/gains-dark.png"><source media="(prefers-reduced-motion: reduce)" srcset="assets/gains.png"><source media="(prefers-color-scheme: dark)" srcset="assets/gains-dark.gif"><img src="assets/gains.gif" width="100%" alt="The measured gains. Web pages: 99% of a GitHub page, 84% of Wikipedia, 81% of the Rust book, 73% of Hacker News and 58% of the Rust blog no longer read. Compression: 100% of lines kept against Headroom's 69.9%. Ponytail's benchmark: 53% fewer lines, 45% fewer output tokens, 26% lower cost, 41% less time. Map: 2,112 files in 1.5 s against graphify's 816 s, the right place named for 15 of 15 questions against 12. TokenEconomizer: 18.4 to 30.9% lower cost."></picture>

**Compression**, against [Headroom](https://github.com/headroomlabs-ai/headroom) 0.42 in token mode on 43 real outputs of 6,000 characters or more:

| | Headroom | the dam |
|---|---|---|
| Tokens saved | **27.3%** | 24.1% |
| Lines recoverable | 69.9% | **100%** |
| Median time per output | 45 ms warm, about 8 s cold | **15 ms** |
| Memory | about 1 GB | **11 MB** |

Headroom saves a little more because it drops what goes past its caps.

**Web pages**, the HTML as fetched against the Markdown the model reads:

| Page | HTML | Markdown | Saved | Time |
|---|---|---|---|---|
| GitHub repository page | 261 KB | 3.7 KB | **99%** | 0.11 s |
| Wikipedia, "Beaver" | 778 KB | 128 KB | **84%** | 0.18 s |
| The Rust book, installation | 31 KB | 6.1 KB | **81%** | 0.15 s |
| Hacker News front page | 34 KB | 9.4 KB | **73%** | 0.13 s |
| The Rust blog | 99 KB | 42 KB | **58%** | 0.14 s |

On html-to-markdown's own test files, the conversion matches 75% of lines, setting aside HTML comments (left out on purpose) and column padding.

**Map**, against [graphify](https://graphify.com)'s structural graph on four repositories of 18 to 2,112 source files:

| | graphify | the dam |
|---|---|---|
| Build time, 2,112 files | 816 s | **1.5 s** |
| Definitions / calls / imports found | reference | 96–99.9% / 96–100% / 80–100% |
| Right place named, 15 questions | 12 | **15** |
| Mean answer size | 1,656 tokens | **772 tokens** |

**Published by the projects it builds on:** TokenEconomizer lowers mean cost by 18.4 to 30.9% on SWE-Bench tasks across five agent and model pairs ([paper](https://arxiv.org/abs/2610.22732)). Ponytail's rules give 53% fewer lines of code, 45% fewer output tokens, 26% lower cost and 41% less time on 39 tasks run 5 times each.

**The Rust rewrite of TokenEconomizer**, against Microsoft's Python on the same inputs:

| Compared | Cases | Differences |
|---|---|---|
| Hooks through a turn, real and made-up sessions | 7,664 events | 0 |
| Whole sessions with tool pruning and facts | 240 hooks | 0 |
| Test-run detection | 25 commands | 0 |
| Session start, fold, pricing, settings, judge | 820 | 1 (a checkout count in `repos --json`) |

## Built on

| Project | Licence | Used for | Followed at |
|---|---|---|---|
| [TokenEconomizer](https://github.com/microsoft/TokenEconomizer) (Microsoft) | Apache 2.0 | Loop guard, tool pruning, learned facts, rewritten in Rust | `d58755d` |
| [Ponytail](https://github.com/DietrichGebert/ponytail) (Dietrich Gebert) | MIT | Beaver's rules and hooks | `v5.3.3` |
| [Headroom](https://github.com/headroomlabs-ai/headroom) | Apache 2.0 | The folding rules of its lossless compaction | `fc1ba89` |
| [markdowner](https://github.com/supermemoryai/markdowner) | MIT | Web pages for a model as Markdown | `f887834` |
| [html-to-markdown](https://github.com/JohannesKaufmann/html-to-markdown) | MIT | Escaping, table and layout rules, and its tests | `v2.5.2` |
| [Readability](https://github.com/mozilla/readability), [Turndown](https://github.com/mixmark-io/turndown) | Apache 2.0, MIT | Main-content choice and Markdown rules | – |
| [graphify](https://graphify.com) | – | The map's reference measurements; no code taken | `v0.9.99` |

[NOTICE](NOTICE) credits each with its licence. None of their authors endorse this project.

## Maintenance

| What | How |
|---|---|
| Upstream | [`UPSTREAM`](UPSTREAM) records the version followed for each project. `scripts/upstream.sh` lists what changed since, and `scripts/upstream.sh <tool>` shows the diff. Changes are ported by hand. |
| Tests | `cd dam && cargo test`, `claude plugin test .`, `claude plugin validate .` |
| Benchmarks | `benchmarks/` holds the scripts behind the figures above |
| CI | On every push: tests on Windows, Linux and macOS, then each program run end to end without a model (`scripts/smoke.sh`). CI then builds the programs for five platforms and commits them. |
| Pictures | `uv run --with pillow python assets/draw.py` draws every picture in this README |
| Changes | [CHANGELOG.md](CHANGELOG.md) |

```
hooks/register.tsx    status line, /the-dam pane, compression and map calls
dam/src/bin/          dam-squeeze, dam-graph, dam-beaver, dam-economizer
dam/src/              the library they share
skills/, commands/    Beaver's and TokenEconomizer's
scripts/              dam-run (picks the right program), te, upstream.sh, smoke.sh
```

## Limits

- Token figures are estimates at four characters per token.
- Real Claude Code sessions have only been run on Windows. CI covers the other systems without a model.
- The Windows programs are not signed, so Windows may warn about them.
- The map is a scanner, not a parser, so unusual syntax can be missed. Documents are read for headings, links and quoted names only.
- Web conversion drops `data:` images, HTML comments, and empty or in-page links.
- The TokenEconomizer rewrite works with Claude Code only.

## Licence

[PolyForm Strict 1.0.0](LICENSE): free for personal and other noncommercial use, with no right to redistribute, modify or use it commercially. It is source-available, not open source. For a commercial licence, contact the author through GitHub. Versions up to 0.15.0 were published under Apache 2.0 and stay under it. The projects it builds on keep their own licences, listed in [NOTICE](NOTICE). The dam is not affiliated with Microsoft, Anthropic or those projects' authors.
