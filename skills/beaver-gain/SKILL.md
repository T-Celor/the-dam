---
name: beaver-gain
description: >
  Show the measured savings of Beaver's rules (code, cost, speed) from
  Ponytail's published benchmark. One-shot display. Use for /beaver-gain,
  "what does beaver save", "beaver impact".
---

Display this card when invoked. One-shot: change no level and write nothing.

Beaver's rules are Ponytail's. The figures are Ponytail 5's published agentic benchmark: headless Claude Code (Opus 5.5, default effort) on 39 tasks (feature tickets in a FastAPI and React repo, bug fixes, security and privacy cases, small apps), 5 runs each, against the same agent without the rules. 18 tasks have hidden checks for correctness and safety. Each figure is the geometric mean of the per-task medians, measured, not computed from this repo.

```
  beaver gain             Ponytail benchmark · 39 tasks × 5 runs · Opus 5.5

  no rules        ████████████████████  100%
  Lines of code   █████████···········   47%   ▼ 53%
  Output tokens   ███████████·········   55%   ▼ 45%
  Cost            ███████████████·····   74%   ▼ 26%
  Time            ████████████········   59%   ▼ 41%
  Hidden checks passed   97%  (no rules 96%)
  Tests where the logic needs one   98%  (no rules 68%)

  This session:  /the-dam              (tokens the dam saved, measured)
  This repo:     /beaver-debt          (shortcuts deferred)
                 /beaver-audit         (what is still cuttable)
```

Never print a per-repo figure for the rules ("you saved X lines here"): the longer version was never written, so there is nothing to subtract from. The dam's own savings (compression, pruned tools) are measured, and `/the-dam` shows them.
