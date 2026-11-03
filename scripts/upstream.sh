#!/usr/bin/env bash
# What changed in each project the dam builds on since the point it follows (UPSTREAM): Ponytail (Beaver),
# TokenEconomizer, Headroom (the folding rules) and graphify (the map's reference measurements).
# A tag is compared with the latest tag, a commit with the default branch. Port what applies, then write
# the new point into UPSTREAM. Reads only; needs git and network access to GitHub.
#   scripts/upstream.sh            every tool, a summary each
#   scripts/upstream.sh ponytail   one tool, with its diff
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
only="${1:-}"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
found=0
while read -r tool repo base paths; do
  case "$tool" in ''|'#'*) continue ;; esac
  [ -n "$only" ] && [ "$tool" != "$only" ] && continue
  found=1
  git clone -q --filter=blob:none "https://github.com/$repo.git" "$work/$tool"
  cd "$work/$tool"
  if git rev-parse -q --verify "refs/tags/$base" >/dev/null; then
    latest="$(git tag --sort=-v:refname | head -1)"
  else
    latest="$(git rev-parse --short HEAD)"
    base="$(git rev-parse --short "$base")"
  fi
  if [ "$(git rev-parse "$base^{commit}")" = "$(git rev-parse "$latest^{commit}")" ]; then
    echo "== $tool: in line with $repo $base"
    cd "$here"
    continue
  fi
  scope=()
  [ "$paths" != "-" ] && read -r -a scope <<< "$paths"
  count="$(git rev-list --count "$base..$latest" -- "${scope[@]}")"
  if [ "$count" = 0 ]; then
    echo "== $tool: in line with $repo $base for ${scope[*]} ($latest changed nothing there)"
    cd "$here"
    continue
  fi
  echo "== $tool: $repo $base -> $latest, $count commits${scope[*]:+ touching ${scope[*]}}"
  if [ -z "$only" ]; then
    git log --oneline "$base..$latest" -- "${scope[@]}" | head -15 || true
  else
    git log --oneline "$base..$latest" -- "${scope[@]}"
    if [ "$paths" != "-" ]; then
      echo
      git diff --stat "$base..$latest" -- "${scope[@]}"
      echo
      git diff "$base..$latest" -- "${scope[@]}"
    fi
  fi
  echo
  cd "$here"
done < "$here/UPSTREAM"
if [ "$found" = 0 ]; then
  echo "No tool named $only in UPSTREAM." >&2
  exit 2
fi
