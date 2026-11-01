# The dam

A Claude Code panel that shows one thing: a small river in low-poly isometric 3D, winding between trees, and a beaver dam built from what your tools really saved.

## What the picture measures

| On screen | Comes from | Rate |
|---|---|---|
| River | Context in use | Width and speed follow the percent; the counter gives tokens used and available |
| Base (3 pale poles, tied with a ribbon, at the foot) | Ponytail installed | Ponytail reports no token figure, so it is the base |
| Twigs | Headroom output compression, all projects together | 1 per 100 tokens |
| Logs | TokenEconomizer in this folder: tools pruned plus facts injected | 1 each |
| Trunks | graphify: nodes in this folder's `graphify-out/graph.json` | 1 per 50 nodes |

Logs and trunks are counts, not tokens: these tools do not report a measured token saving.

What the counter says is what stands on the dam: one log is one log, each in its own place, so a gain adds a piece and moves nothing. The picture stops adding at 90 twigs, 40 logs and 12 trunks, where the dam is full, and the counters go on.

## The math

All of it is in `hooks/model.ts`.

- Strength S runs from 0 to 1: 0.15 for Ponytail, up to 0.45 for twigs (full at 1M tokens, on a log scale), up to 0.20 each for logs and trunks. Each saturates, and the dam's size follows S^0.8.
- The dam leaks above a threshold of 60% + 25% x S of the context. Pressure p runs from 0 at that threshold to 1 at a full context.
- A leak washes away a share of the stock, climbing towards a ceiling of (40% twigs, 25% logs, 15% trunks) x p x (1 - 0.6 S). Below the threshold the wear is repaired, halving about every 7 minutes.
- At a full context a dam with S below 0.4 is swept away. A stronger dam holds and leaks.
- When you clean the context, the river drains by half the gap every 15 seconds.

Tokens saved are never lost.

graphify only counts where a project already has a graph: the plugin never creates `graphify-out/` by itself.

## The game

Bring the water down by at least 20 points from 70% or more, usually by compacting, and you win a pennant for this project. Reaching 100% loses half of them. Below 30% the water is calm and fish jump.

## Getting started

```
/plugin marketplace add tcelor/the-dam
/plugin install the-dam@the-dam
```

Restart Claude Code. The rest starts by itself at each session: the panel opens, whatever is missing among the four tools is installed (this needs `uv` and `git`), and the Headroom proxy starts.

## Commands

- `/the-dam` opens the panel again if you closed it.
- `/the-dam-setup` installs missing tools now.
- `/the-dam-pause` pauses or resumes the animation in the terminal.
- `/the-dam-compress` turns compression on or off.
- `/the-dam-doctor` checks that the plugin can find each tool.
- `/the-dam-route` runs `headroom init -g claude`. This works in a terminal only, because Claude Desktop overwrites `ANTHROPIC_BASE_URL`.

## Compression

At session start the plugin checks the Headroom proxy on port 8787 and starts it if needed. Starting takes about a minute, and the proxy uses about 1 GB of memory until the session that started it ends.

`Bash` and `Grep` outputs over 6000 characters then go through `/v1/compress` before the model reads them. This drops lines, so the full output is saved in `~/.claude/the-dam-outputs/` (the last 20) and the model gets the path. `Read` and `Edit` are never compressed. If the proxy fails or takes over 3 seconds, the output passes through unchanged.

## Hooks

- Session start: opens the panel, installs missing tools and starts the proxy.
- Tool call: wakes the matching beaver and condenses large outputs, then schedules a gain check within 15 seconds. An error here never blocks the tool.
- Turn end: updates the graph if code changed and a graph exists (no LLM), then runs a gain check.
- Gain check: reads every tool's figures again, redraws the dam and shows a toast with what is new, such as `+4 twigs`.
- Prompt: tells the model about the graph, only where one exists.

TokenEconomizer and Ponytail run their own hooks.

## Accessibility

The desktop picture has a text alternative with every figure. The reduced-motion setting stops its animation and hovering pauses it, though keyboard users have no pause there, so it does not fully meet RGAA criterion 13.8. In the terminal the counters are plain text on the bottom row, and `/the-dam-pause` stops the animation.

## Drawing

`hooks/iso.ts` builds the world once from your figures: real 3D solids, flat shaded and projected isometrically into polygons. Both renderers draw that one description, so the river and the dam follow the same numbers everywhere.

The panel draws SVG in a frame with no script or canvas, so the browser plays the movement back with CSS, the water following the river's course with a motion path. The source stays under the panel's 131072-character limit.

The terminal has no SVG, so it is pixel art: `hooks/raster.ts` fills the same polygons pixel by pixel, four pixels per cell with quadrant glyphs, in colours of the xterm 256 cube since that is what the terminal shows, ten frames a second, and repaints in place. It keeps the still layers until the figures change.
