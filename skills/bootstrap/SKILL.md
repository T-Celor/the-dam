---
name: bootstrap
description: "Write or repair a README.md for each folder of a codebase, as a contract (what it exposes, what it assumes, what it touches), built from the map, then checked against the code. Use proactively when a folder has no README or a stale one, when documenting a codebase, and for /the-dam:bootstrap [folder]."
argument-hint: "[folder]"
---

Method after the Elephant-Goldfish Model (Google, Apache 2.0): leaf folders get a contract README, parent folders an index of their children's contracts.

The user's folder: "$ARGUMENTS" (none: the repository root).

## 1. Scope, before writing

List the folders under it that hold code (`git ls-files`, not generated or vendored folders). Show the list with, for each, whether a README exists, and ask once which to write. Never overwrite a README the user wrote without showing the change first.

## 2. Contract per folder

For each folder, get its facts from the map, not by reading every file: `query "<folder>"` gives what it defines, what calls it, what it calls and imports. A folder big enough to need it goes to an `worker` agent (Haiku) with that answer and the folder path; the agent reads only the files whose names the answer gives.

A leaf README says, in plain words: what the folder is for (one sentence); for each public file or name, what it takes, returns and changes (files, network, state), and what must hold when it is called; what it assumes of its callers; how to test it. Names and paths are real: copy them from the map.

A parent README is an index: one line per child, with a link to the child's README. No contract is repeated.

## 3. Check

Run `dam goldfish drift` on the READMEs written (`DAM_RUN_OR_FAIL=1 "<plugin folder>/scripts/dam-run" goldfish drift <files>`; PowerShell: `& "<plugin folder>\bin\windows-x64\dam.exe" goldfish drift <files>`). Fix every missing file or name before reporting. Then offer `/the-dam:goldfish` on the parent README.

Report: folders written, folders skipped, and any hit the check left that you judged intended.
