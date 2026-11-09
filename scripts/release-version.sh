#!/usr/bin/env bash
# Print the next release version without the tag prefix, or nothing when no
# commit since the latest release tag requires a release.
set -euo pipefail

for command in cog git; do
  if ! command -v "$command" >/dev/null 2>&1; then
    echo "missing required command: $command" >&2
    echo "run: mise install" >&2
    exit 127
  fi
done

latest_tag="$(git describe --tags --abbrev=0 --match 'v[0-9]*' 2>/dev/null || true)"
range="HEAD"
if [[ -n "$latest_tag" ]]; then
  range="$latest_tag..HEAD"
fi

subjects="$(git log --no-merges --format='%s' "$range")"
bodies="$(git log --no-merges --format='%b' "$range")"
level="--auto"
if grep -qE '^[[:alnum:]-]+(\([^)]*\))?!:' <<<"$subjects" ||
  grep -qE '^BREAKING[ -]CHANGE:' <<<"$bodies"; then
  level="--major"
fi

status=0
output="$(cog bump --dry-run "$level" 2>&1)" || status=$?
if grep -qiE 'no commits|no conventional|no bump|nothing to bump' <<<"$output"; then
  echo "no conventional commits require a release" >&2
  exit 0
fi
if ((status != 0)); then
  printf '%s\n' "$output" >&2
  exit 1
fi

version="$(grep -E '^v?[0-9]+\.[0-9]+\.[0-9]+' <<<"$output" | tail -n 1 || true)"
version="${version#v}"
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$ ]]; then
  echo "Cocogitto returned an invalid SemVer version: $version" >&2
  exit 1
fi
printf '%s\n' "$version"
