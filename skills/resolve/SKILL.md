---
name: resolve
description: "Settle the findings of a goldfish review: contest what is wrong, escalate what only the user knows, propose fixes, apply the ones the user accepts. Use for /the-dam:resolve, after /the-dam:goldfish or /the-dam:goldfish-loop."
argument-hint: "<document> [findings file]"
---

Method after the Elephant-Goldfish Model (Google, Apache 2.0). Run it in the session that wrote the document: you remember why it says what it says.

The user's arguments: "$ARGUMENTS" (the document, and the findings; none given: the last report next to the document).

## 1. Sort

Give each finding one label:

- **CONTEST**: wrong, or based on a misreading of the code. Cite what the reviewer could read: a file and name (check it with the map), or a decision written in the document. "It was intentional" is not evidence.
- **ESCALATE**: wrong, but the proof is outside every file the reviewer had (a team habit, a spoken decision). Say so; do not fix; give it to the user.
- **PROPOSE FIX**: right. Write the exact replacement text.
- **DECLINE**: a Nit not worth a fix. Record it.

Rules: you wrote the document, so wanting to contest is not evidence. A reader who could not follow it means the document lacks context: never contest that. Do not add text only to please a reviewer with no context; add it when a human reader needs it, else ESCALATE. Never change a severity: contest it with a reason.

## 2. Report

Write `goldfish_review_report.md` next to the document, findings by severity: reviewer, severity, scope (lines), description, your label, why, and the proposed edit. Tell the user the path and list the ESCALATE items first in chat.

## 3. Decide

Ask once, as plain text, and stop: `auto accept` (apply every PROPOSE FIX), `continue` (apply nothing), `interactive` (one finding at a time: accept, skip, discuss), or their own instruction. Apply only what they accepted, in one batch, then check the file still reads as one document. Put `Status: resolved` at the top of the report and give the counts: accepted, contested, escalated, declined, still open. Then record the reviewed version, so only sections edited from now on are reviewed again: `dam goldfish mark <document>` (`DAM_RUN_OR_FAIL=1 "<plugin folder>/scripts/dam-run" goldfish mark <document>` in a POSIX shell, `& "<plugin folder>\bin\windows-x64\dam.exe" goldfish mark <document>` in PowerShell).
