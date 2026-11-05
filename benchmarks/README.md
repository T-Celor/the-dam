# Benchmarks

The comparison scripts of earlier versions (Python, against Headroom, graphify and the Python TokenEconomizer) were removed in 0.21, which is Rust only; they stay in the Git history. They need those projects checked out and, for Headroom, its proxy.

What remains runs on this machine and sends nothing anywhere:

| Command | Measures |
|---|---|
| `dam squeeze bench ~/.claude/projects/*/*.jsonl` | What the compression saves on the tool results of your own Claude Code sessions, by output size |
