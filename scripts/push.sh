#!/usr/bin/env bash
# The last 4.0.x, from the branch release/4.0 only.
#
#   make push release          tag v$(VERSION), push the branch as main and the
#                              tag to the remote `veydanbrowser`
#   DRY=1 make push release    run every check, print the commands, change nothing
#
# Installed 4.0.x take their updates from veydanbrowser's releases, so the tag
# goes there and its release.yml builds and publishes the release. Nothing
# goes to origin from here: a v* tag there would start a build in a private
# repository. Channels (alpha, beta, rc) are not released from this
# branch: 4.0 gets one last release.
#
# Before anything is pushed: the branch is release/4.0, the tree is clean,
# the version is 4.0.x, the pushed commit does not descend from 2b08618 (the
# commit that started tracking VHub/, deploy/ and docs/) and its tree holds no
# VHub/, VLink/, deploy/ or docs/, and Veydan Space 5 is released (the link the
# app shows leads to a release).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

CHANNEL="${1:-}"
PLATFORMS="${2:-}"
VERSION_OVERRIDE="${3:-}"
DRY="${DRY:-}"

REMOTE=veydanbrowser
BRANCH=release/4.0
# The first commit that tracks private folders; no ancestor of a push.
PRIVATE_ROOT=2b086186bd4fe9cecde10dbb7c348f4c4f5f1492
PRIVATE_PATHS="VHub VLink deploy docs"
SPACE5_LATEST="${SPACE5_LATEST:-https://github.com/veydanproject/Veydan-Space/releases/latest}"

fail() {
  echo ">> $*" >&2
  exit 1
}

run() {
  if [ -n "$DRY" ]; then
    echo "   would run: $*"
  else
    "$@"
  fi
}

case "$CHANNEL" in
  release) ;;
  alpha|beta|rc) fail "release/4.0 makes one last release: use \`make push release\`" ;;
  *) fail "usage: [DRY=1] make push release" ;;
esac
[ -z "$PLATFORMS" ] || fail "the last 4.0.x is built for every platform; drop: $PLATFORMS"

[ "$(git rev-parse --abbrev-ref HEAD)" = "$BRANCH" ] || fail "push.sh runs on $BRANCH only (here: $(git rev-parse --abbrev-ref HEAD))"
git remote get-url "$REMOTE" >/dev/null 2>&1 || fail "no remote '$REMOTE' (git remote add $REMOTE https://github.com/veydanproject/veydanbrowser.git)"
[ -z "$(git status --porcelain)" ] || fail "the tree is not clean: commit the release first"

rel="$(cat VERSION)"
[ -z "$VERSION_OVERRIDE" ] || [ "$VERSION_OVERRIDE" = "$rel" ] || fail "VERSION is $rel; set another one with scripts/set-version.sh and commit it"
case "$rel" in
  4.0.[0-9]*) ;;
  *) fail "VERSION is $rel: $REMOTE releases 4.0.x only, its latest.json never points at 5.x" ;;
esac
tag="v$rel"

# The pushed commit carries nothing private.
if git cat-file -e "$PRIVATE_ROOT^{commit}" 2>/dev/null && git merge-base --is-ancestor "$PRIVATE_ROOT" HEAD; then
  fail "HEAD descends from ${PRIVATE_ROOT:0:7}: it holds private folders"
fi
for p in $PRIVATE_PATHS; do
  if [ -n "$(git ls-tree --name-only HEAD -- "$p")" ]; then
    fail "HEAD has $p/ in its tree: it must not reach $REMOTE"
  fi
done
echo ">> private parts: none (${PRIVATE_ROOT:0:7} is no ancestor; no $PRIVATE_PATHS)"

if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
  fail "tag $tag already exists here"
fi
if git ls-remote --tags "$REMOTE" "refs/tags/$tag" | grep -q .; then
  fail "tag $tag already exists in $REMOTE"
fi

# The message in the app sends people to this link; it must lead to a release.
landed="$(curl -sS -o /dev/null -L --max-time 30 -w '%{url_effective}' "$SPACE5_LATEST" || true)"
case "$landed" in
  */releases/tag/v5.*) echo ">> Veydan Space 5: ${landed##*/}" ;;
  *) fail "Veydan Space 5 is not released yet ($SPACE5_LATEST leads to '${landed:-nothing}'): release space-v5.0.0 first" ;;
esac

head="$(git rev-parse --short HEAD)"
echo ">> $tag at $head → $REMOTE (main and the tag)"
run git tag "$tag"
run git push "$REMOTE" "HEAD:refs/heads/main"
run git push "$REMOTE" "refs/tags/$tag"
if [ -n "$DRY" ]; then
  echo ">> dry run: nothing tagged or pushed"
else
  echo ">> Released $tag in $REMOTE; release.yml there builds it and publishes latest.json"
fi
