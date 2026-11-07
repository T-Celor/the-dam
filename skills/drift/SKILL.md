---
name: drift
description: "Find what documents say that the code no longer backs: files that are gone or moved, names that were renamed or removed. Use for /the-dam:drift DOCUMENT..., 'are the docs out of date', 'check the README against the code'."
argument-hint: "<document>..."
---

The user's documents: "$ARGUMENTS" (none: the README and the files under `docs/`).

Run `dam goldfish drift <documents>` (`DAM_RUN_OR_FAIL=1 "<plugin folder>/scripts/dam-run" goldfish drift ...` in a POSIX shell, `& "<plugin folder>\bin\windows-x64\dam.exe" goldfish drift ...` in PowerShell). It checks every `code span` of the documents against the project's map: a missing file (with where the map found it now), a name no code file has (with its current spelling when only the case or underscores changed). Nothing is changed.

Then, for each hit, read the text around it and the code, and say which is wrong: the document or the code. Report one line per hit, document first. Fix a document only when the user asks. The tool misses claims that are not in a code span, and flags a name the document invents on purpose (an example): say which.
