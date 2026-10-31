#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repository_root"
if [ "$#" -eq 0 ]; then
  set -- .github/workflows/*.yml .github/workflows/*.yaml
fi
for workflow in "$@"; do
  [ -f "$workflow" ] || continue
  # actionlint 1.7.12 predates GitHub's $/ syntax. Translate only uses lines
  # for its parser; retain local action/input checks. zizmor audits originals.
  sed 's@^\([[:space:]]*uses: \)\$/@\1./@' "$workflow" |
    actionlint -stdin-filename "$workflow" -
done
