# The dam

Runs Headroom, graphify, TokenEconomizer and Ponytail together in Claude Code. Nothing graphical: one status line, and `/the-dam` for the detail.

```
🦫 12.3k saved · context 31%
```

```
🦫 The dam
  Context 31% (310k of 1M tokens)
  ✓ Headroom: 12.3k tokens saved
  ✓ TokenEconomizer: 2 tools pruned (4 pending), 5 facts learned, 1 loops stopped, 10.5k tokens spent
  … graphify: no graph in this folder, run /graphify
  ✓ Ponytail: active
```

✓ works, … is waiting for something, ✗ is off or missing.

Tokens saved are counted across all projects. Pruned tools, facts and graph nodes belong to the current folder. TokenEconomizer measures no token saving as it goes, so the plugin shows what it did: tools pruned (and those it will prune once the project has been idle for an hour), facts learned, loops stopped, and the tokens its own work cost. graphify reports nodes, and Ponytail no figure at all.

## Getting started

```
/plugin marketplace add tcelor/the-dam
/plugin install the-dam@the-dam
```

Restart Claude Code. The rest starts by itself at each session: whatever is missing among the four tools is installed (this needs `uv` and `git`), and the Headroom proxy starts.

## Commands

- `/the-dam` shows what each tool saved.
- `/the-dam-compress` turns compression on or off.

## Compression

At session start the plugin checks the Headroom proxy on port 8787 and starts it if needed. Starting takes about a minute, and the proxy uses about 1 GB of memory until the session that started it ends.

`Bash` and `Grep` outputs over 6000 characters then go through `/v1/compress` before the model reads them. This drops lines, so the full output is saved in `~/.claude/the-dam-outputs/` (the last 20) and the model gets the path. `Read` and `Edit` are never compressed. If the proxy fails or takes over 3 seconds, the output passes through unchanged.

## Graph

At session start, in a git repository with at least 20 tracked code files and no graph yet, the plugin builds one in the background with `graphify update .`, from the code's structure alone: no LLM, no tokens. `graphify-out/` goes in `.git/info/exclude`, so your `.gitignore` stays as it is. For a graph that also understands docs and meaning, run `/graphify`, which uses the model.

## Hooks

- Session start: scans for the four tools and installs what is missing, starts the proxy if it is down, and builds a first graph if the project deserves one. Whatever is already in place is skipped.
- Tool call: condenses large outputs, then schedules a gain check within 15 seconds. An error here never blocks the tool.
- Turn end: updates the graph if code changed and a graph exists (no LLM), then runs a gain check.
- Gain check: reads every tool's figures again, updates the status line and shows a toast with what is new, such as `+1.5k tokens saved`.
- Every 15 seconds: reads the context again, so the status line follows a `/compact`.
- Prompt: tells the model about the graph, only where one exists.

TokenEconomizer and Ponytail run their own hooks.
