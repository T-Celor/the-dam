#!/bin/sh
# Runs each program as Claude Code would, through dam-run, on a throwaway repository and state.
# No model and no network, so it says nothing about a real session, only that the programs work
# on this system. CI runs it on Linux, macOS and Windows after `cargo build --release`.
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
export TOKENECONOMIZER_HOME="$work/state" CLAUDECODE=1 CLAUDE_PLUGIN_ROOT="$root"

case "$(uname -s)-$(uname -m)" in
  MINGW* | MSYS* | CYGWIN*) folder=windows-x64 ext=.exe ;;
  Linux-x86_64) folder=linux-x64 ext= ;;
  Linux-aarch64) folder=linux-arm64 ext= ;;
  Darwin-arm64) folder=macos-arm64 ext= ;;
  Darwin-x86_64) folder=macos-x64 ext= ;;
  *) echo "unknown system" >&2; exit 1 ;;
esac
mkdir -p "$root/bin/$folder"
for name in dam-squeeze dam-graph dam-ponytail dam-economizer; do
  cp "$root/dam/target/release/$name$ext" "$root/bin/$folder/$name$ext"
done
run="$root/scripts/dam-run"

check() {
  case "$2" in
    *"$3"*) echo "ok: $1" ;;
    *) echo "FAILED: $1, wanted '$3' in: $2" >&2; exit 1 ;;
  esac
}

# Lossless compression, through a round trip.
i=0
while [ "$i" -lt 400 ]; do
  echo "2026-01-01 12:00:$i INFO worker $i finished job $i in $i ms"
  i=$((i + 1))
done >"$work/log.txt"
"$run" dam-squeeze <"$work/log.txt" >"$work/small.txt"
"$run" dam-squeeze --decode <"$work/small.txt" >"$work/back.txt"
cmp "$work/log.txt" "$work/back.txt"
[ "$(wc -c <"$work/small.txt")" -lt "$(wc -c <"$work/log.txt")" ]
echo "ok: compression round trip"

# The graph, on the plugin's own source.
repo="$work/repo"
mkdir -p "$repo"
cp -R "$root/dam/src" "$repo/src"
git -C "$repo" init -q
git -C "$repo" add -A
(cd "$repo" && "$run" dam-graph update >/dev/null)
check "graph query" "$(cd "$repo" && "$run" dam-graph query "where is the lossless decode")" "squeeze"

# Status, then a whole turn of hooks on it.
check "status" "$("$run" dam-economizer --cwd "$repo" status)" "TokenEconomizer -- repo"
check "status as json" "$("$run" dam-economizer --cwd "$repo" status --json)" '"identified_by": "path"'

key=$("$run" dam-economizer --cwd "$repo" status --json | sed -n 's/.*"repo_key": "\(.*\)",/\1/p')
mkdir -p "$TOKENECONOMIZER_HOME/sessions/claude" "$TOKENECONOMIZER_HOME/repos/$key/claude"
printf '{"key":"%s","root":"%s","label":"repo","remote":"","how":"path","at":1}' "$key" "$repo" >"$TOKENECONOMIZER_HOME/sessions/claude/smoke.json"
hook() {
  printf '{"session_id":"smoke","cwd":"%s","hook_event_name":%s}' "$repo" "$1" | TE_JUDGE_NO_SPAWN=1 "$run" dam-economizer hook
}
hook '"UserPromptSubmit","prompt_id":"p1","prompt":"go"' >/dev/null
hook '"PostToolUse","prompt_id":"p1","tool_name":"Bash","tool_input":{"command":"ls"},"tool_response":{"stdout":"a"}' >/dev/null
hook '"Stop","prompt_id":"p1"' >/dev/null
hook '"SessionEnd","reason":"exit"' >/dev/null
check "session recorded" "$(cat "$TOKENECONOMIZER_HOME/repos/$key/claude/sessions/smoke.json")" '"end_reason": "exit"'

check "ponytail" "$(printf '{"session_id":"smoke","cwd":"%s"}' "$repo" | "$run" dam-ponytail activate)" "PONYTAIL"
check "te command" "$("$root/scripts/te" --cwd "$repo" config show)" "loopguard"
echo "all good on $folder"
