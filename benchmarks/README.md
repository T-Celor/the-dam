# Benchmarks

The scripts behind the README's numbers. They run on this machine and send nothing anywhere. Several read your own Claude Code session logs under `~/.claude/projects`, and Headroom's need its proxy on port 8787. They need local clones of the repositories they compare, passed as arguments.

| Script | Compares |
|---|---|
| `midcmp.py`, `rules.py` | `dam-squeeze` and Headroom |
| `gaps.py`, `questions.py` | `dam-graph` and graphify |
| `lgdiff.py`, `hookdiff.py` | The Rust and Python TokenEconomizer, on the same inputs |

They were written to answer one question each, and are kept for that rather than polished.
