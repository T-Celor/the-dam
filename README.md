# The dam

Runs Headroom, graphify, TokenEconomizer and Ponytail together in Claude Code. Nothing graphical: one status line, and `/the-dam` for the detail.

```
🦫 12.3k saved · context 31%
```

```
🦫 The dam
  Context 31% (310k of 1M tokens)
  ✓ Headroom: 12.3k tokens saved
  ✓ TokenEconomizer: 2 tools pruned, 5 facts learned
  … graphify: no graph in this folder, run /graphify
  ✓ Ponytail: active
```

✓ works, … is waiting for something, ✗ is off or missing.

Tokens saved are counted across all projects. Pruned tools, facts and graph nodes belong to the current folder. TokenEconomizer and graphify report counts, not tokens, and Ponytail reports no figure at all, so none of them is turned into a token saving.

## Getting started

```
/plugin marketplace add tcelor/the-dam
/plugin install the-dam@the-dam
```

Restart Claude Code. The rest starts by itself at each session: whatever is missing among the four tools is installed (this needs `uv` and `git`), and the Headroom proxy starts.

## Commands

- `/the-dam` shows what each tool saved.
- `/the-dam-setup` installs missing tools now.
- `/the-dam-compress` turns compression on or off.
- `/the-dam-doctor` checks that the plugin can find each tool.
- `/the-dam-route` runs `headroom init -g claude`. This works in a terminal only, because Claude Desktop overwrites `ANTHROPIC_BASE_URL`.

## Compression

At session start the plugin checks the Headroom proxy on port 8787 and starts it if needed. Starting takes about a minute, and the proxy uses about 1 GB of memory until the session that started it ends.

`Bash` and `Grep` outputs over 6000 characters then go through `/v1/compress` before the model reads them. This drops lines, so the full output is saved in `~/.claude/the-dam-outputs/` (the last 20) and the model gets the path. `Read` and `Edit` are never compressed. If the proxy fails or takes over 3 seconds, the output passes through unchanged.

graphify only counts where a project already has a graph: the plugin never creates `graphify-out/` by itself.

## Hooks

- Session start: installs missing tools and starts the proxy.
- Tool call: condenses large outputs, then schedules a gain check within 15 seconds. An error here never blocks the tool.
- Turn end: updates the graph if code changed and a graph exists (no LLM), then runs a gain check.
- Gain check: reads every tool's figures again, updates the status line and shows a toast with what is new, such as `+1.5k tokens saved`.
- Every 15 seconds: reads the context again, so the status line follows a `/compact`.
- Prompt: tells the model about the graph, only where one exists.

TokenEconomizer and Ponytail run their own hooks.
