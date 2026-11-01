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
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache%202.0-blue.svg" alt="Apache 2.0"></a>
  <img src="https://img.shields.io/badge/built%20with-Rust-b7410e.svg" alt="Built with Rust">
  <img src="https://img.shields.io/badge/runs-100%25%20local%20by%20default-2e7d32.svg" alt="Local by default">
</p>

---

Coding agents burn context on repeated log lines, broad searches and failing loops. The dam sits between the tools and the model and cuts that waste, then tells you what it did in one status line:

```
🦫 12.3k saved · context 31%
```

Type `/the-dam` for the detail:

```
🦫 The dam
  Context 31% (310k of 1M tokens)
  ✓ Compression: 12.3k tokens saved
  ✓ TokenEconomizer: 2 tools pruned (4 pending), 5 facts learned, 1 loops stopped, 10.5k tokens spent
  ✓ Code graph: 2.2k nodes
  ✓ Ponytail: active
```

## Install

In Claude Code:

```
/plugin marketplace add T-Celor/the-dam
/plugin install the-dam@the-dam
```

Restart Claude Code. Everything starts by itself at each session.

| You need | For |
|---|---|
| Nothing | Compression, the code graph, Ponytail's rules, on supported systems |
| Python 3.10+ | TokenEconomizer's parts not yet ported to Rust. `uv` installs it for you if present |
| `cargo` | Only on a system with no prebuilt program |

If you already run the separate TokenEconomizer or Ponytail plugins, disable them so that they do not run twice:

```
claude plugin disable tokeneconomizer@tokeneconomizer
claude plugin disable ponytail@ponytail
```

## What it does

### Lossless compression: `dam-squeeze`

`Bash` outputs of 6,000 to 29,000 characters are condensed before the model reads them, and no line is dropped:

- runs of lines that differ only by numbers become a template and a table of what varies;
- identical lines become a count, and repeated blocks a reference;
- search hits and path listings are grouped under their file or directory;
- a JSON array of records becomes a table.

Errors, warnings and values that stand out (a 503 among 200s) stay plain lines, and code keeps its shape. Every result is decoded and compared with the source before it is used; only trailing spaces and terminal colours may differ. If anything fails, takes over 3 seconds or saves under 200 tokens, the output passes through unchanged. The original also stays in `~/.claude/the-dam-outputs/` (the last 20), and the model is told where.

Compared with [Headroom](https://github.com/headroomlabs-ai/headroom) 0.42 in its default mode, on 43 real outputs of the size this plugin compresses:

| | Headroom | `dam-squeeze` |
|---|---|---|
| Tokens saved (estimated, 4 characters per token) | **27.5%** | 24.1% |
| Lines recoverable from the result | 69.7% | **100%** |
| Median time per output | 45 ms warm, over 9 s cold | **15 ms** |
| Memory | about 1 GB, always on | **11 MB peak on a 1.9 MB log** |

Headroom saves more because it drops content past its caps, and it offers a retrieval tool to get that content back. The dam keeps every line. On code and listings with little repetition it saves little or nothing.

### A local code graph: `dam-graph`

At each session start, in a git repository with at least 20 tracked code files, it rebuilds who defines, calls, extends and imports what, in Python, JavaScript, TypeScript, Rust, Java, C#, Go, C, C++ and Kotlin. The model is told it can ask:

```
dam-graph query "how does the log compressor keep error lines"
```

No model and no network. `.dam-graph/` goes in `.git/info/exclude`, so your `.gitignore` stays as it is.

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

The hooks that run on every prompt and tool call are ported to Rust (`dam-economizer`) and read the same state files. Replaying 7,519 hook events from real and made-up sessions through both gave identical answers and state. Session start and end, its commands and other agents still run on the bundled Python, unchanged.

Its loop guard can call a model through your own Claude Code to judge a stuck turn, and its fact learning is off by default. Those calls are the only network use the plugin causes itself, and they appear in the `tokens spent` figure. `/the-dam:config` turns them off.

### Lazy senior dev rules: Ponytail

The rules of [Ponytail](https://github.com/DietrichGebert/ponytail) by Dietrich Gebert: the smallest change that fully solves the task. `dam-ponytail` rewrites its hooks for Claude Code, with its levels (`/ponytail lite|full|ultra`, `stop ponytail`) and its map of what already exists in the codebase. Its task board, webhook and status badge are left out.

## Commands

| Command | Does |
|---|---|
| `/the-dam` | Shows what each tool did |
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
lib/, scripts/te*     TokenEconomizer, bundled
skills/, commands/    Ponytail's and TokenEconomizer's
```

Peak memory on Windows: `dam-ponytail` 3.5 to 5.9 MB, `dam-graph` 10 MB on 145 files and 67 MB on 2,112.

## Limits

- Token savings are estimates, not measured by a tokenizer. The counter covers compression only.
- Prebuilt programs come from CI for Windows x64, Linux x64 and macOS. Only Windows has been tested by hand so far.
- The Windows programs are not signed, so Windows may warn about them.
- The model has to read the condensed format, which may cost it some effort on heavily folded outputs.
- The bundled copies of TokenEconomizer and Ponytail do not follow their own releases.

## Development

```bash
cd dam && cargo test
claude plugin validate .
claude plugin test .
```

`benchmarks/` holds the comparison scripts behind the numbers above. See [CONTRIBUTING.md](CONTRIBUTING.md).

## Credits and licence

Apache License 2.0, see [LICENSE](LICENSE). The dam builds on the work of others, listed with their licences in [NOTICE](NOTICE). It is an independent project, not affiliated with or endorsed by Microsoft, Anthropic, or the authors of Headroom, graphify and Ponytail.
