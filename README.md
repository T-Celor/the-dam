# The dam

Runs a small Rust compressor, graphify, TokenEconomizer and Ponytail together in Claude Code. Nothing graphical: one status line, and `/the-dam` for the detail.

```
🦫 12.3k saved · context 31%
```

```
🦫 The dam
  Context 31% (310k of 1M tokens)
  ✓ Compression: 12.3k tokens saved
  ✓ TokenEconomizer: 2 tools pruned (4 pending), 5 facts learned, 1 loops stopped, 10.5k tokens spent
  … graphify: no graph in this folder, run /graphify
  ✓ Ponytail: active
```

✓ works, … is waiting for something, ✗ is off or missing.

Tokens saved are an estimate (four characters per token), counted across all projects. Pruned tools, facts and graph nodes belong to the current folder. TokenEconomizer measures no token saving as it goes, so the plugin shows what it did: tools pruned (and those it will prune once the project has been idle for an hour), facts learned, loops stopped, and the tokens its own work cost. graphify reports nodes, and Ponytail no figure at all.

## Getting started

```
/plugin marketplace add tcelor/the-dam
/plugin install the-dam@the-dam
```

Restart Claude Code. The rest starts by itself at each session: whatever is missing among the tools is installed (this needs `uv`, and `cargo` outside Windows).

## Commands

- `/the-dam` shows what each tool saved.
- `/the-dam-compress` turns compression on or off.

## Compression

`Bash` outputs between 6000 and 29000 characters go through `dam-squeeze` (source in `squeeze/`, Windows build in `bin/`) before the model reads them. It drops no line: runs of lines that differ only by numbers become a template and a table of what varies, identical lines a count, shared prefixes are written once, and a JSON array of records becomes a table. Errors, warnings and values that stand out stay plain lines, and code keeps its shape. Every result is decoded and compared with the source before it is used, trailing spaces aside; `dam-squeeze --decode` gives the original back. Even so, so the full output is saved in `~/.claude/the-dam-outputs/` (the last 20) and the model gets the path. `Read` and `Edit` are never compressed. Longer outputs stay as they are, since Claude Code already saves those and shows a preview. If the compressor fails, takes over 3 seconds or saves under 200 tokens, the output passes through unchanged.

## Graph

At session start, in a git repository with at least 20 tracked code files and no graph yet, the plugin builds one in the background with `graphify update .`, from the code's structure alone: no LLM, no tokens. `graphify-out/` goes in `.git/info/exclude`, so your `.gitignore` stays as it is. For a graph that also understands docs and meaning, run `/graphify`, which uses the model.

## Hooks

- Session start: scans for the four tools and installs what is missing, and builds a first graph if the project deserves one. Whatever is already in place is skipped.
- Tool call: condenses large outputs, then schedules a gain check within 15 seconds. An error here never blocks the tool.
- Turn end: updates the graph if code changed and a graph exists (no LLM), then runs a gain check.
- Gain check: reads every tool's figures again, updates the status line and shows a toast with what is new, such as `+1.5k tokens saved`.
- Every 15 seconds: reads the context again, so the status line follows a `/compact`.
- Prompt: tells the model about the graph, only where one exists.

Ponytail runs its own hooks.

## TokenEconomizer

[TokenEconomizer](https://github.com/microsoft/TokenEconomizer) by Microsoft ships inside this plugin (`lib/`, `scripts/`, `commands/`), unchanged, under the Apache 2.0 licence in `lib/LICENSE`, and needs Python 3.10 or later. Its commands read `/the-dam:status`, `/the-dam:toolprune` and so on. If you also have the separate plugin, disable it so that it does not run twice: `claude plugin disable tokeneconomizer@tokeneconomizer`.
