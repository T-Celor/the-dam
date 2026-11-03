---
name: beaver-help
description: >
  Quick reference for Beaver's levels, skills and settings. One-shot display.
  Use for /beaver-help, "beaver help", "how do I use beaver".
---

Display this card when invoked. One-shot: change no level and write nothing.

## Levels

| Level | Command | What changes |
|-------|---------|--------------|
| Lite | `/beaver lite` | Builds what was asked, names the smaller option in one line. |
| Full | `/beaver` | The smallest complete change, a test where the logic needs one, a reply that names what was skipped and any risk. Default. |
| Ultra | `/beaver ultra` | Also questions the request and pushes back before building. |

The level holds for the session. `/beaver off`, "stop beaver" or "normal mode" turns it off. `/beaver default lite` sets the level new sessions start in. The 🦫 in the dam's status line shows the level.

## Skills

| Skill | What it does |
|-------|--------------|
| `/the-dam:beaver` | The rules themselves: least new code, replies that name skipped work and risks. |
| `/the-dam:beaver-review` | Review of a change: bugs, security, load, missing tests, speed, what to cut. |
| `/the-dam:beaver-audit` | The same review for the whole repo, ranked. |
| `/the-dam:beaver-debt` | Every `shortcut:` comment, as a ledger. |
| `/the-dam:beaver-gain` | The benchmark the rules come from. |
| `/the-dam:beaver-help` | This card. |

## With the rest of the dam

Beaver tells the model to ask the project's map before searching, to follow the facts TokenEconomizer learned, and to open a condensed output's original only when needed.

## Settings

Default level, highest priority first:

1. `BEAVER_DEFAULT_MODE=ultra` in the environment.
2. `~/.config/beaver/config.json` (Windows: `%APPDATA%\beaver\config.json`): `{ "defaultMode": "lite" }`. `"off"` starts sessions without Beaver.
3. `full`.

`BEAVER_MAP=0` leaves out the list of existing code given at session start. `BEAVER_SUBAGENT_MATCHER=Explore|Plan` gives the rules only to those subagents. Ponytail's names (`PONYTAIL_*`, `~/.config/ponytail`, `/ponytail`) still work.

Beaver is built on Ponytail by Dietrich Gebert (MIT): https://github.com/DietrichGebert/ponytail
