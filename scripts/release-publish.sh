#!/usr/bin/env bash
set -euo pipefail

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 127
  fi
}

require_command cargo
require_command git
require_command jq

branch="$(git rev-parse --abbrev-ref HEAD)"
if [[ "$branch" != "main" ]]; then
  echo "release must be published from main; current branch is $branch" >&2
  exit 1
fi
if [[ -n "$(git status --porcelain)" ]]; then
  echo "release publication requires a clean working tree" >&2
  git status --short >&2
  exit 1
fi

commit="$(git rev-parse HEAD)"
version="$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name == "criv") | .version')"
tag="v$version"
wasm_tag="criv-wasm-v$version"

if [[ "$(git log -1 --format=%s)" != "chore(release): $tag" ]]; then
  echo "HEAD is not the prepared $tag release commit" >&2
  exit 1
fi
for candidate in "$tag" "$wasm_tag"; do
  if git rev-parse -q --verify "refs/tags/$candidate" >/dev/null; then
    echo "tag already exists: $candidate" >&2
    exit 1
  fi
done

git fetch --force origin \
  refs/notes/criv-release-gates:refs/notes/criv-release-gates
receipt="$(mktemp)"
trap 'rm -f "$receipt"' EXIT
git notes --ref=criv-release-gates show "$commit" >"$receipt"
now="$(date +%s)"
jq -e --arg commit "$commit" --argjson now "$now" '
  .schema == "criv.discovery-release-gate.v1" and
  .passed == true and
  .commit == $commit and
  .valid_until_unix >= $now and
  (.artifact_run_id | type == "number" and . > 0) and
  (.artifacts | length == 4)
' "$receipt" >/dev/null

git tag "$tag"
git tag "$wasm_tag"
git push origin "$tag" "$wasm_tag"
