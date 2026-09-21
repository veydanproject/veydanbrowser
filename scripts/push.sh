#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

CHANNEL="${1:-}"
PLATFORMS="${2:-}"
VERSION_OVERRIDE="${3:-}"
MSG="${4:-}"

usage() {
  echo ">> usage: make push alpha|beta|rc|release [linux] [windows] [macos] [android] [ios]"
  exit 1
}

next_channel_n() {
  local prefix="$1"
  local max=0 t rest n
  for t in $(git tag -l "${prefix}*"); do
    rest="${t#"$prefix"}"
    n="${rest%%-*}"
    case "$n" in
      ''|*[!0-9]*) ;;
      *) if [ "$n" -gt "$max" ]; then max=$n; fi ;;
    esac
  done
  local remote
  remote="$(git ls-remote --tags origin "refs/tags/${prefix}*" 2>/dev/null | cut -f2 | sed -e 's#^refs/tags/##' -e 's#\^{}$##')"
  for t in $remote; do
    rest="${t#"$prefix"}"
    n="${rest%%-*}"
    case "$n" in
      ''|*[!0-9]*) ;;
      *) if [ "$n" -gt "$max" ]; then max=$n; fi ;;
    esac
  done
  echo $((max + 1))
}

tag_exists() {
  local t="$1"
  git rev-parse "$t" >/dev/null 2>&1 && return 0
  git ls-remote --tags origin "refs/tags/$t" 2>/dev/null | grep -q . && return 0
  return 1
}

push_release() {
  local current rel major minor patch next
  current="$(cat VERSION)"
  if [ -n "$VERSION_OVERRIDE" ]; then rel="$VERSION_OVERRIDE"; else rel="$current"; fi
  if tag_exists "v$rel"; then
    echo ">> tag v$rel already exists"
    exit 1
  fi
  bash "$ROOT/scripts/set-version.sh" "$rel"
  if [ -n "$(git status --porcelain)" ]; then
    git add -A
    git commit -m "${MSG:-release $rel}"
  fi
  git tag "v$rel"
  IFS=. read -r major minor patch <<<"$rel"
  next="$major.$minor.$((patch + 1))"
  bash "$ROOT/scripts/set-version.sh" "$next"
  git add VERSION package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml src-tauri/tauri.android.conf.json
  git commit -m "chore: start $next"
  git push && git push origin "v$rel"
  echo ">> Released v$rel (next $next)"
}

push_channel() {
  local version prefix n tag p buildable=""
  for p in $PLATFORMS; do
    case "$p" in linux|windows|macos|android) buildable=1 ;; esac
  done
  if [ -n "$PLATFORMS" ] && [ -z "$buildable" ]; then
    echo ">> ios CI is not set up yet; add android or a desktop platform"
    exit 1
  fi
  version="$(cat VERSION)"
  prefix="v${version}-${CHANNEL}."
  n="$(next_channel_n "$prefix")"
  tag="${prefix}${n}"
  for p in $PLATFORMS; do
    tag="$tag-$p"
  done
  if [ -n "$(git status --porcelain)" ]; then
    git add -A
    git commit -m "${MSG:-$CHANNEL snapshot $version}"
  fi
  git tag "$tag"
  git push && git push origin "$tag"
  echo ">> Released $tag"
}

case "$CHANNEL" in
  release) push_release ;;
  alpha|beta|rc) push_channel ;;
  *) usage ;;
esac
