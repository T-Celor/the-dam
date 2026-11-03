<p align="center">
  <img src="assets/icon.svg" width="128" height="128" alt="The dam's beaver">
</p>

<h1 align="center">The dam</h1>

<p align="center">
  <strong>Spend fewer tokens in Claude Code, without losing a line.</strong><br>
  Lossless output compression, a local code graph, a loop guard and lazy-senior-dev rules, in one plugin.
</p>

<p align="center">
  <a href="https://github.com/T-Celor/the-dam/actions/workflows/ci.yml"><img src="https://github.com/T-Celor/the-dam/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-PolyForm%20Strict%201.0.0-blue.svg" alt="PolyForm Strict 1.0.0"></a>
  <img src="https://img.shields.io/badge/built%20with-Rust-b7410e.svg" alt="Built with Rust">
  <img src="https://img.shields.io/badge/runs-100%25%20local%20by%20default-2e7d32.svg" alt="Local by default">
</p>

---

Coding agents burn context on repeated log lines, broad searches and failing loops. The dam sits between the tools and the model and cuts that waste, then shows what each tool did in one status line, one icon per tool:

```
🦫 full · 💧 45.2k · 🪵 3+2 · 🌀 1 · 🧠 5 · 🗺️ 2.2k · 🌊 31%
```

| Icon | Tool | Figure |
|---|---|---|
| 🦫 | Ponytail | its level in this session |
| 💧 | Compression | tokens saved, every session together |
| 🪵 | Tool pruning | tools removed, plus those waiting |
| 🌀 | Loop guard | loops stopped |
| 🧠 | Learned facts | facts in use |
| 🗺️ | Code graph | nodes |
| 🌊 | Context | share of the window used |

A tool with nothing to show yet stays out of the line. Type `/the-dam` to open the session's journal beside the chat: what each tool did and when, then the totals. Where no pane can be drawn (`claude -p`), it answers with the totals.

## Install

In Claude Code:

```
/plugin marketplace add T-Celor/the-dam
/plugin install the-dam@the-dam
```

Restart Claude Code. Everything starts by itself at each session.

| You need | For |
|---|---|
| Nothing | Everything, on supported systems |
| `cargo` | Only on a system with no prebuilt program |

If you already run the separate TokenEconomizer or Ponytail plugins, disable them so that they do not run twice:

```
claude plugin disable tokeneconomizer@tokeneconomizer
claude plugin disable ponytail@ponytail
```

## What it does

### Lossless compression: `dam-squeeze`

`Bash` outputs and `Grep`'s matching lines of 6,000 characters or more are condensed before the model reads them, and no line is dropped:

- runs of lines that differ only by numbers become a template and a table of what varies;
- identical lines become a count, and repeated blocks a reference;
- search hits and path listings are grouped under their file or directory;
- a JSON array of records becomes a table.

Errors, warnings and values that stand out (a 503 among 200s) stay plain lines, and code keeps its shape. Every result is decoded and compared with the source before it is used; only trailing spaces and terminal colours may differ. If anything fails, takes over 3 seconds or saves under 200 tokens, the output passes through unchanged. The original also stays in `~/.claude/the-dam-outputs/` (the newest 200, across sessions), and the model is told where.

An output too long for Claude Code to show, which it saves to a file and previews, is condensed from that file. If it then fits, the model gets all of it. If not, the saved file holds the condensed form, and in place of the first 2 KB the model sees the lines that stand out (errors, failures, warnings), the first lines of the condensed form and where both files are.

Compared with [Headroom](https://github.com/headroomlabs-ai/headroom) 0.42 in its token mode, on 43 real outputs of the size this plugin compresses:

| | Headroom | `dam-squeeze` |
|---|---|---|
| Tokens saved (estimated, 4 characters per token) | **27.3%** | 24.1% |
| Lines recoverable from the result | 69.9% | **100%** |
| Median time per output | about 8 s on a freshly started proxy, 45 ms warm in an earlier run | **15 ms** |
| Memory | about 1 GB, always on | **11 MB peak on a 1.9 MB log** |

Headroom saves more because it drops content past its caps, and it offers a retrieval tool to get that content back. The dam keeps every line. On code and listings with little repetition it saves little or nothing.

### A local code graph: `dam-graph`

At each session start, in a git repository with at least 20 tracked code files, it rebuilds in the background who defines, calls, extends and imports what, in Python, JavaScript, TypeScript, Rust, Java, C#, Go, C, C++ and Kotlin. The model is told it can ask:

```
dam-graph query "how does the log compressor keep error lines"
```

The model is told from the first prompt; a query made before the first build ends builds the graph itself. It does not have to ask: when a prompt names code (`parseConfig`, `crush_rows()`, `SmartCrusher`), or a `Grep` looks for one name, what the graph knows of that name (where it is defined, what it calls, what calls it) is added for the model. A sentence with no such name gets nothing added, as the graph's answer to plain words is too loose to be worth its tokens. No model and no network. `.dam-graph/` goes in `.git/info/exclude`, so your `.gitignore` stays as it is.

Measured against [graphify](https://graphify.com)'s structural graph as the reference, on four repositories of 18 to 2,112 source files:

| | graphify | `dam-graph` |
|---|---|---|
| Time to build, 2,112 files | 816 s | **1.5 s** |
| Definitions found | reference | 96 to 99.9% |
| Calls found | reference | 96 to 100% |
| Imports found | reference | 80 to 100% |
| Right place named, 15 questions | 12 | **15** |
| Mean answer size | 1,656 tokens | **772 tokens** |

It also reports links graphify does not, and not all of them are verified. It is a scanner rather than a parser, so unusual syntax can be missed. It does not read documents or images, and does not cluster or draw the graph.

### A loop guard and tool pruning: TokenEconomizer

[TokenEconomizer](https://github.com/microsoft/TokenEconomizer) by Microsoft notices a turn repeating the same failure and redirects it, leaves out tools a repository never uses, and can learn repository facts. Its commands are here as `/the-dam:status`, `/the-dam:toolprune`, `/the-dam:config` and so on.

It is rewritten in Rust (`dam-economizer`), for Claude Code only, and reads the same state files, so existing state keeps working. Against the Python on the same inputs:

| Compared | Cases | Differences |
|---|---|---|
| Hooks through a turn, on real and made-up sessions | 7,664 events | 0 |
| Whole sessions with tool pruning and facts on, real logs | 240 hooks | 0 |
| Session start, fold, pricing, settings, judge | 820 | 1, a checkout count in `repos --json` |
| Tool pruning, including odd settings files | 587 comparisons | 12, where the Python crashes on a malformed `permissions` and this refuses to edit |
| Learned facts, including the model's answers | 300 steps | 0, against a copy with one order made stable (the Python's ties vary with its hash seed) |

The judge and the proposer call a model through your own Claude Code; those calls were compared with a stand-in for `claude`. They are the only network use the plugin causes itself, they appear in the `tokens spent` figure, and `/the-dam:config` turns them off. Fact learning is off by default.

Left out: other agents (Codex, Copilot, Pi, OpenCode), the repository map (`dam-graph` fills that role), `probe`, and merging or migrating state between repository keys.

### Lazy senior dev rules: Ponytail

The rules of [Ponytail](https://github.com/DietrichGebert/ponytail) by Dietrich Gebert: the smallest change that fully solves the task. `dam-ponytail` rewrites its hooks for Claude Code, with its levels (`/ponytail lite|full|ultra`, `stop ponytail`) and its map of what already exists in the codebase. Its task board, webhook and status badge are left out.

## Several sessions at once

Sessions may run side by side, in one project or several:

- each keeps its own Ponytail level, its own saved originals and its own count of tokens saved;
- TokenEconomizer's state files are written under the same locks as Microsoft's version, and a session is counted once in its ledger whichever session folds it;
- a session that ends headless (`claude -p`) is counted at the next session start rather than at its end, as Claude Code allows a session-end hook only a moment.

## Commands

| Command | Does |
|---|---|
| `/the-dam` | Opens the session's journal and the totals beside the chat |
| `/the-dam-compress` | Turns compression on or off |
| `/the-dam:status`, `:toolprune`, `:config`, `:microskills`, `:repos` | TokenEconomizer's own |
| `/the-dam:ponytail`, `:ponytail-review`, `:ponytail-audit`, `:ponytail-debt` | Ponytail's own |

## How it fits together

One Rust application with a small program per job, so that each process holds only the code of its own job, and a thin TypeScript module for the parts Claude Code runs itself.

```
hooks/register.tsx    status line, /the-dam, calls dam-squeeze and dam-graph
scripts/dam-run       picks the program built for this machine
dam/src/bin/          dam-squeeze, dam-graph, dam-ponytail, dam-economizer
dam/src/              the library they share
scripts/te            TokenEconomizer's command line
skills/, commands/    Ponytail's and TokenEconomizer's
```

Peak memory on Windows: `dam-ponytail` 3.5 to 5.9 MB, `dam-graph` 10 MB on 145 files and 67 MB on 2,112.

## Limits

- Token savings are estimates, not measured by a tokenizer. The counter covers compression only.
- Prebuilt programs come from CI for Windows x64, Linux x64 and macOS. CI runs every program end to end on each (`scripts/smoke.sh`), with no model. Real Claude Code sessions have only been run on Windows.
- The Windows programs are not signed, so Windows may warn about them.
- The model has to read the condensed format, which may cost it some effort on heavily folded outputs.
- Rules a model writes for learned facts run on Rust's regular expressions, which differ from Python's in rare cases.
- The rewrite of TokenEconomizer and the bundled rules of Ponytail do not follow their own releases.

## Development

```bash
cd dam && cargo test
claude plugin validate .
claude plugin test .
```

`benchmarks/` holds the comparison scripts behind the numbers above. See [CONTRIBUTING.md](CONTRIBUTING.md).

## Credits and licence

PolyForm Strict License 1.0.0, see [LICENSE](LICENSE): free for personal and other noncommercial use, with no right to redistribute it, change it or use it commercially. This is source-available, not open source. For a commercial licence, contact the author through GitHub. Versions up to 0.15.0 were published under Apache 2.0, and stay under it. The dam builds on the work of others, listed with their licences in [NOTICE](NOTICE). It is an independent project, not affiliated with or endorsed by Microsoft, Anthropic, or the authors of Headroom, graphify and Ponytail.
