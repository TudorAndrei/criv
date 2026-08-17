#!/usr/bin/env bash
set -euo pipefail

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 127
  fi
}

require_command cargo
require_command gh
require_command git
require_command jq
require_command npm
require_command shasum
require_command tar
require_command zip

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
    if [[ "$(git rev-list -n 1 "$candidate")" != "$commit" ]]; then
      echo "tag points to a different commit: $candidate" >&2
      exit 1
    fi
  fi
done

git fetch --force origin \
  refs/notes/criv-release-gates:refs/notes/criv-release-gates
receipt="$(mktemp)"
release_root="$(mktemp -d)"
trap 'rm -f "$receipt"; rm -rf "$release_root"' EXIT
git notes --ref=criv-release-gates show "$commit" >"$receipt"
now="$(date +%s)"
jq -e --arg commit "$commit" --argjson now "$now" '
  .schema == "criv.discovery-release-gate.v1" and
  .passed == true and
  .commit == $commit and
  .valid_until_unix >= $now and
  (.artifacts | length == 4) and
  ([.artifacts[].target] | sort) == [
    "aarch64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu"
  ]
' "$receipt" >/dev/null

gate_root="$(pwd)/.criv/release-gates/$commit"
local_receipt="$gate_root/release-gate.json"
if [[ ! -f "$local_receipt" ]] || ! cmp -s "$receipt" "$local_receipt"; then
  echo "the local accepted receipt does not match the published Git note" >&2
  echo "run the local discovery acceptance command on this computer" >&2
  exit 1
fi

while IFS=$'\t' read -r target path sha256; do
  expected_path="binaries/$target/$(basename "$path")"
  if [[ "$path" != "$expected_path" ]]; then
    echo "invalid accepted artifact path: $path" >&2
    exit 1
  fi
  artifact="$gate_root/$path"
  if [[ ! -f "$artifact" ]]; then
    echo "missing local accepted artifact: $artifact" >&2
    exit 1
  fi
  actual="$(shasum -a 256 "$artifact" | awk '{print $1}')"
  if [[ "$actual" != "$sha256" ]]; then
    echo "accepted artifact digest mismatch: $artifact" >&2
    exit 1
  fi
done < <(jq -r '.artifacts[] | [.target, .path, .sha256] | @tsv' "$receipt")

npm ci
npm --prefix extensions/vscode-criv run package
viewer="$(pwd)/extensions/vscode-criv/vscode-criv.vsix"
dist="$release_root/dist"
mkdir -p "$dist"

while IFS=$'\t' read -r target path; do
  stage="$release_root/stage/$target"
  mkdir -p "$stage"
  if [[ "$target" == "x86_64-pc-windows-msvc" ]]; then
    cp -p "$gate_root/$path" "$stage/criv.exe"
    cp -p "$viewer" "$stage/vscode-criv.vsix"
    (
      cd "$stage"
      zip -q "$dist/criv-$target.zip" criv.exe vscode-criv.vsix
    )
  else
    cp -p "$gate_root/$path" "$stage/criv"
    cp -p "$viewer" "$stage/vscode-criv.vsix"
    chmod +x "$stage/criv"
    tar -C "$stage" -czf "$dist/criv-$target.tar.gz" criv vscode-criv.vsix
  fi
done < <(jq -r '.artifacts[] | [.target, .path] | @tsv' "$receipt")

(
  cd "$dist"
  shasum -a 256 criv-*.tar.gz criv-*.zip >SHA256SUMS.txt
)

git rev-parse -q --verify "refs/tags/$tag" >/dev/null || git tag "$tag"
git rev-parse -q --verify "refs/tags/$wasm_tag" >/dev/null || git tag "$wasm_tag"
git push origin "$tag" "$wasm_tag"

if gh release view "$tag" --json isDraft --jq .isDraft >/dev/null 2>&1; then
  if [[ "$(gh release view "$tag" --json isDraft --jq .isDraft)" != "true" ]]; then
    echo "release is already published: $tag" >&2
    exit 1
  fi
else
  gh release create "$tag" \
    --draft \
    --verify-tag \
    --title "$tag" \
    --notes "criv $tag"
fi
gh release upload "$tag" \
  "$dist"/*.tar.gz \
  "$dist"/*.zip \
  "$dist/SHA256SUMS.txt" \
  --clobber
gh release edit "$tag" --draft=false --verify-tag
