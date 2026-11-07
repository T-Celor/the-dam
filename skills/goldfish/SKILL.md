---
name: goldfish
description: "Review a design document with fresh reviewers that have none of the conversation (goldfish), then settle the findings with the user. Use for /the-dam:goldfish DOCUMENT, 'review this design', 'have it challenged'. Needs a subagent tool."
argument-hint: "<design document> [feature | plan]"
---

Method after the Elephant-Goldfish Model (Google, Apache 2.0). You are the elephant: you hold the context. The reviewers are goldfish: they get the document and its source files, never the conversation. A review done in this context is not a goldfish review. With no subagent tool, say so and stop.

The user's arguments: "$ARGUMENTS" (the document; `feature` reviews the specification only, `plan` the implementation plan only).

## 1. Prepare (the dam does it)

Run `dam goldfish prepare <document>`: `DAM_RUN_OR_FAIL=1 "<plugin folder>/scripts/dam-run" goldfish prepare <document>` in a POSIX shell, `& "<plugin folder>\bin\windows-x64\dam.exe" goldfish prepare <document>` in PowerShell. It prints the line ranges (specification, plan), the linked files inside the folder, and the files that define the code the document names, found by the map. Links it lists as left out: ask the user once, as plain text, which to include.

## 2. Review, in order

Each stage is one subagent of the plugin, given the document path, its line range, the context paths and nothing else. Pass your prompt without comment. Tell it the map is available: its Grep answers carry it.

| Stage | Agent | Range |
|---|---|---|
| 1 Simplicity | `goldfish-simplicity` | specification |
| 2 Cold read | `goldfish-reader` | specification |
| 3 Critique | `goldfish-critic` | specification |
| 4 Simplicity again | `goldfish-simplicity` | specification, as revised |
| 5 Readiness | `goldfish-ready` | plan |

Skip the plan stages when the document has no plan, or the specification stages for `plan`. Stages 1 to 3 do not depend on each other: run them together. After each stage, offer the findings to the user and fix what they accept before the next one, unless they asked for one consolidated report.

## 3. Settle

Merge duplicate findings, rank them Blocker, Major, Nit, one line each. Two findings that disagree, or a Blocker you doubt: ask the `advisor`. Write the report to a file next to the document and tell the user its path. The user decides every trade-off; do not change the design on a finding they have not accepted.

A review does not replace tests or the user's judgement.
