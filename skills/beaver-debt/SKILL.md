---
name: beaver-debt
description: >
  List every `shortcut:` comment (and older `ponytail:` ones) as a debt ledger.
  One-shot report, changes nothing. Use for "beaver debt", "what did beaver
  defer", "list the shortcuts", /beaver-debt.
---

Every deliberate shortcut is marked with a `shortcut:` comment naming its limit and when to upgrade (older code may say `ponytail:`). This collects them into one ledger, so a deferral cannot quietly become permanent.

## Scan

Search the repo, skipping `.git` and what `.gitignore` ignores:

`git grep --no-index --exclude-standard -nE '(#|//|/[*]|--) ?(shortcut|ponytail):'`

If the user names their own marker (`/beaver-debt TODO`), search for that word instead. Each hit is one row. Skip hits that are not a deferral, such as a note about a keyboard shortcut.

## Output

One row per marker, grouped by file:

`<file>:<line>, <what was simplified>. limit: <the limit named>. upgrade: <when to revisit>.`

Take the limit and the trigger straight from the comment. For an owner per row, add `git blame -L<line>,<line>`.

Tag `no-trigger` on any marker that names no upgrade path: those are the ones that rot.

End with `<N> markers, <M> with no trigger.` Nothing found: `No shortcut debt. Clean ledger.`

Reads and reports only. Asked to keep it, write the ledger to a file (for example `BEAVER-DEBT.md`).
