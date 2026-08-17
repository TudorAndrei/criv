#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 1 || $# -gt 2 ]]; then
  echo "usage: accept-release-gates.sh BUNDLE_PATH [REMOTE]" >&2
  exit 2
fi

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 127
  fi
}

require_command cargo
require_command git
require_command jq

bundle_path="$(cd "$1" && pwd)"
remote="${2:-origin}"
input="$bundle_path/gate-input.json"

if [[ ! -f "$input" ]]; then
  echo "missing gate input: $input" >&2
  exit 1
fi
if [[ "$(git rev-parse --abbrev-ref HEAD)" != "main" ]]; then
  echo "release acceptance must run from main" >&2
  exit 1
fi
if [[ -n "$(git status --porcelain)" ]]; then
  echo "release acceptance requires a clean working tree" >&2
  git status --short >&2
  exit 1
fi

commit="$(git rev-parse HEAD)"
if [[ "$(jq -r .commit "$input")" != "$commit" ]]; then
  echo "gate input does not identify HEAD $commit" >&2
  exit 1
fi

gate_root="$(pwd)/.criv/release-gates/$commit"
mkdir -p "$gate_root/binaries"
temporary_receipt="$(mktemp)"
trap 'rm -f "$temporary_receipt"' EXIT

cargo build --locked --release -p criv-perf-harness --bin criv-discovery-gate
target/release/criv-discovery-gate \
  --input "$input" \
  --output "$temporary_receipt"

while IFS=$'\t' read -r target path file_name; do
  destination="$gate_root/binaries/$target"
  mkdir -p "$destination"
  cp -p "$path" "$destination/$file_name"
done < <(jq -r '.artifacts[] | [.target, .path, .file_name] | @tsv' "$temporary_receipt")

jq '
  .artifacts |= map(.path = ("binaries/" + .target + "/" + .file_name))
' "$temporary_receipt" >"$gate_root/release-gate.json"
cp -p "$input" "$gate_root/gate-input.json"

scripts/performance/publish-release-gate-note.sh \
  "$gate_root/release-gate.json" \
  "$commit" \
  "$remote"

echo "accepted release gates for $commit"
echo "local accepted assets: $gate_root"
