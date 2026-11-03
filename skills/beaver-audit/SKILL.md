---
name: beaver-audit
description: >
  Quality audit of a whole repo: bugs, security holes, what breaks under real
  load, risky code without tests, slow paths, and what to delete, merge or
  split. Ranked, each finding explained in plain English. One-shot report,
  changes nothing. Use for "audit this codebase", "review the whole repo",
  "find bloat", "what can I delete", /beaver-audit.
---

Audit the repo like the senior developer who just inherited it and will be paged when it breaks. Order: correct, safe, holds under load, tested, fast, lean. The user asked for this report, so give it in full.

## 1. Map first

- Audit what the user names: a folder, a package, or the whole repo (the default).
- When the session has a map, start from it: `query "entry points"`, `query "<module>"` show what is defined where and what calls what, and which documents describe it. Read files for the details the map does not hold.
- Read the README, build and deploy config, dependency list, entry points (main, routes, handlers, jobs, CLI commands) and tests.
- Find the expected load: one person running a script, or many users and processes. Say which you assumed.
- Trace the main flows end to end and read them fully: input from users, money, auth, data writes, background jobs, anything shared between processes.
- Big repo: go deep where a mistake costs most. Say which parts you did not read.

## 2. Look for

1. **Bug:** wrong result, crash, missed edge case (empty, zero, last item, rounding, time zones), callers that disagree with what a function returns, one rule applied two ways.
2. **Risk:** security holes (injection, weak randomness, secrets in code, unchecked input from users), data loss (swallowed errors, writes in the wrong order, no transaction).
3. **Scale:** check-then-write races, the same work in every process, lists or memory that only grow, a query per item, O(n^2) on big input, per-process state that must be shared.
4. **Missing test:** risky logic (a branch, a parser, money, security, data writes) with no test that fails when it breaks.
5. **Speed:** big slowdowns are problems; small wins are suggestions.
6. **Lean:** delete (dead code, unused options, flags and config, speculative features); reuse (two helpers doing one thing: keep one, name the path); stdlib or platform (a dependency doing what a few lines can); yagni (one-implementation interface, one-product factory, pass-through wrapper); merge (near-copies); split (one function or class doing unrelated jobs; by job, never by line count).

## 3. Check before you report

- Every finding needs a concrete case. No case, no finding.
- Before calling code unused, search the whole tree, including tests, fixtures, config and string or dynamic references. The map's "called by" helps; a search confirms.
- A `shortcut:` (or older `ponytail:`) comment naming its limit is a decision, not a finding, unless the expected load crosses it.
- Propose the smallest fix; prefer deleting. No new layers, frameworks or config.
- No style taste, no "consider", no vague worries.

## 4. Output

Very simple English. Explain a technical term the first time.

Start with `What this repo does:` in two or three sentences, and the load you assumed. Then the findings, skipping empty groups:

- **Must fix:** bug, security, data loss, breaks at the expected load.
- **Should fix:** risky code without a test, real slowness, duplication, mixed jobs, code that should not exist.
- **Nice to have:** small speed-ups, shorter forms.

Number findings across groups. At most 20; say how many smaller ones you left out. Each has four parts, one or two short sentences each:

2. **Orders land on the wrong day** (`billing/close_day.py:L40-52`)
   - **What this is:** At midnight this job bills all orders of the day.
   - **Problem:** It takes "today" from the server clock in UTC, so an order at 00:30 in Berlin is billed the day before.
   - **Fix:** `datetime.now(ZoneInfo("Europe/Berlin")).date()`. One line.
   - **If we skip it:** Late orders show the wrong date; accounting fixes them by hand.

End with:
- `Verdict:` one line: healthy, or what to fix first.
- `Lean: -<N> lines, -<M> dependencies possible.` when lean findings exist.
- `Not checked:` the parts you did not read or could not run.

Nothing found: `What this repo does:`, then `Healthy. Nothing to fix.` and one line on what you checked.

One-shot report, changes no code.
