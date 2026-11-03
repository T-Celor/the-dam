---
name: beaver-review
description: >
  Quality review of a change: is the logic right, is it safe, does it hold
  under real load, is risky code tested, is it fast enough, and is every line
  needed. Reads the connected code, not only the diff. Each finding is
  explained in plain English. Use for "review this", "code review", "review
  the last commit", "review my PR", "is this over-engineered", /beaver-review.
---

Review a change like the senior developer who will be paged when it breaks. Order: correct, safe, holds under load, tested, fast, lean. Lean matters: every extra line must be read, tested and fixed later. The user asked for this report, so give it in full.

## 1. Understand first

- Review what the user names: uncommitted or staged changes, a branch, a PR, or files. Nothing named: the uncommitted changes, else the last commit.
- Read the diff, then the code it touches: callers of every changed function, what it calls, the tests, the README. When the session has a map, ask it for the callers and definitions (`query "<name>"`) before grepping.
- Trace the real flow: what comes in, what is stored, what goes out.
- A change can break code it does not touch. When a signature, return value or behavior changes, find every caller.
- Find the expected load (README, deploy config): one person running a script, or many users and processes at once. Say which you assumed.

## 2. Look for

1. **Bug:** wrong result, crash, missed edge case (empty, zero, last item, rounding, time zones), a caller broken by the change, a fix in one caller while the shared function stays broken.
2. **Risk:** security holes (injection, weak randomness, secrets, unchecked input from users), data loss (swallowed errors, writes in the wrong order, no transaction).
3. **Scale:** fine for one user, wrong for many: check-then-write races, the same work in every process, lists or memory that only grow, a query per item, O(n^2) on big input, per-process state that must be shared.
4. **Missing test:** risky new logic (a branch, a parser, money, security, data writes, a bug fix) with no test that fails when it breaks. One good test, not coverage.
5. **Speed:** big slowdowns are problems; small wins are suggestions.
6. **Lean:** delete (dead code, unused options, speculative features); reuse (the repo has this helper: name the path); stdlib or platform; yagni (one-implementation abstraction, config nobody sets); merge (near-copies that change together); split (one function doing unrelated jobs; split by job, never by line count).

## 3. Check before you report

- Every finding needs a concrete case: "this input leads to this wrong result". No case, no finding.
- Re-read the lines: the caller exists, the value can really be empty, the code really is unused (search tests, fixtures, config and dynamic references too).
- A `shortcut:` (or older `ponytail:`) comment naming its limit is a decision, not a finding, unless the expected load crosses it.
- Propose the smallest fix; prefer fixes that delete code. No new layers, frameworks or config.
- No style taste, no "consider", no vague worries.

## 4. Output

Very simple English, short sentences. Explain a technical term the first time. The reader may never have seen this code.

Start with `What this change does:` in two or three sentences. Then the findings, skipping empty groups:

- **Must fix:** bug, security, data loss, breaks at the expected load.
- **Should fix:** risky code without a test, real slowness, duplication, mixed jobs, code that should not exist.
- **Nice to have:** small speed-ups, shorter forms.

Number findings across groups so the user can say "fix 2 and 5". Each has four parts, one or two short sentences each:

2. **Orders land on the wrong day** (`billing/close_day.py:L40-52`)
   - **What this is:** At midnight this job bills all orders of the day.
   - **Problem:** It takes "today" from the server clock in UTC, so an order at 00:30 in Berlin is billed the day before.
   - **Fix:** `datetime.now(ZoneInfo("Europe/Berlin")).date()`. One line.
   - **If we skip it:** Late orders show the wrong date; accounting fixes them by hand.

End with:
- `Verdict: Ship.` or `Verdict: fix 1 and 3 first.`
- `Lean: -<N> lines possible.` when lean findings exist.
- `Not checked:` one line, if something mattered and you could not check it.

Nothing found: `What this change does:`, then `Looks good. Ship.` and one line on what you checked.

Lists findings, changes no code.
