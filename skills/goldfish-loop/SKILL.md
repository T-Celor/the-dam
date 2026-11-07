---
name: goldfish-loop
description: "Repeat goldfish review and resolve on one document, or one section, until two clean rounds in a row; for any document (spec, README, plan), with the reviewers and standard you pick. Use for /the-dam:goldfish-loop DOCUMENT."
argument-hint: "<document> [section] [reviewers]"
---

Method after the Elephant-Goldfish Model (Google, Apache 2.0). Needs a subagent tool; a review in this context is not a goldfish review.

The user's arguments: "$ARGUMENTS".

## Setup

Ask only for what is missing, one thing at a time: the document, the section (a heading, or the whole file), the reviewers, the standard they judge by.

Reviewers are the plugin's agents: `goldfish-simplicity`, `goldfish-reader`, `goldfish-critic`, `goldfish-ready`. A spec: critic and simplicity. A plan: ready and reader. The standard is a file; a short list of the questions the document must answer is enough. Run `dam goldfish prepare <document>` (see `/the-dam:goldfish`) for the context paths; the map adds the code the document names. Show the plan, get a yes.

## Each round

1. Find the section's current lines again: the file changed last round.
2. Run the chosen reviewers together, each with the document, the lines, the standard and the context paths, nothing of the conversation. Give each finding an id `<reviewer>-<round>-<n>`.
3. A reviewer that fails or returns nothing is retried once, then recorded as **not run**, never as zero findings.
4. Hand the findings to `/the-dam:resolve` and wait for the user's choice there.

## When to stop

- A finding the user contested or accepted as a risk is **settled**: if it comes back at the same place, do not reopen it.
- An applied fix does not settle a finding: if it comes back, reopen it.
- A round is **clean** when no unsettled Blocker or Major was raised and every reviewer ran. Nits do not count. Judge it before applying that round's fixes.
- Two clean rounds in a row: done. Five rounds, or a new finding at the place of an applied fix that reverting would solve: list the open findings by severity and ask the user once whether to accept, go on, or return to drafting.

Only start another round when the user says so.
