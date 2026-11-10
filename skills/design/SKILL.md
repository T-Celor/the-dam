---
name: design
description: "Turn an idea into a design document in steps the user approves: test the idea, write a concepts brief, then the specification. Use proactively before implementing a feature that spans several files or needs a choice, and for /the-dam:design IDEA, 'design this', 'write a spec for'. Then run /the-dam:goldfish on it."
argument-hint: "<idea or feature>"
---

Method after the Elephant-Goldfish Model (Google, Apache 2.0): you hold the context and draft; the user decides.

The user's idea: "$ARGUMENTS".

## 1. Explore

Ask the map (`query "<name or question>"`) what the project already has before proposing anything: reuse beats new code. Then discuss requirements and trade-offs, one question at a time, from the weakest point of the idea first. Do not agree because the user said it. Name what they may not be seeing and the strongest case against.

Write the agreed direction to `concepts_brief.md` in plain words, no promotional adjectives. Tell the user the path.

## 2. Draft

Write `design_draft.md` from the brief, one section at a time, and stop after each for the user's feedback. Sections: problem and who has it, goals and non-goals, the design (interfaces and data flow, not helper functions), risks, then `## Implementation Plan` (files to change, in order, with tests). Name real files and names; check them with the map.

## 3. Hand over

Tell the user to run `/the-dam:goldfish design_draft.md`. Save the approved design under `docs/designs/` only when they say so.
