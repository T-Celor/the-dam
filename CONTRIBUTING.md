# Contributing

Thanks for helping. Small, focused pull requests are the easiest to review.

## Set up

You need Rust (stable), Claude Code, and Python 3.10 or later for TokenEconomizer's bundled parts.

```bash
cd dam && cargo test
claude plugin validate .
claude plugin test .
```

## Rules of the house

- **No lost lines.** `dam-squeeze` must give the original back. Every new fold has an inverse, and its result is decoded and compared before use.
- **Hooks never fail the agent.** On any error a hook answers `{}` and the session goes on.
- **Nothing leaves the machine.** No telemetry and no network calls in the Rust programs. The one model call, TokenEconomizer's judge, goes through the user's own Claude Code.
- **Numbers come from a run.** A figure in the README needs a script in `benchmarks/`, and estimates say so.
- **Same behaviour when porting.** A Rust port of TokenEconomizer must match the Python on the same inputs: see `benchmarks/hookdiff.py` and `benchmarks/lgdiff.py`.
- **Keep third-party credit.** Update `NOTICE` when you take from another project.

## Report a bug

Open an issue with your system, Claude Code version, what you ran and what you expected. For a compression bug, a small output that reproduces it is ideal. Remove anything private first. For a security problem, see [SECURITY.md](SECURITY.md).
