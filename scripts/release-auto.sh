#!/usr/bin/env bash
set -euo pipefail

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    echo "run: mise install" >&2
    exit 127
  fi
}

write_output() {
  local name="$1"
  local value="$2"
  if [[ -n "${GITHUB_OUTPUT:-}" ]]; then
    printf '%s=%s\n' "$name" "$value" >>"$GITHUB_OUTPUT"
  fi
}

release_output() {
  local commit="$1"
  local tag="$2"
  write_output release true
  write_output commit "$commit"
  write_output tag "$tag"
  write_output version "${tag#v}"
  echo "prepared $tag at $commit"
}

no_release() {
  write_output release false
  write_output commit ""
  write_output tag ""
  write_output version ""
  echo "no conventional commits require a release"
}

remote_tag_commit() {
  local tag="$1"
  git ls-remote --refs origin "refs/tags/$tag" | awk 'NR == 1 { print $1 }'
}

require_command cargo
require_command cog
require_command git

if ! cargo release --version >/dev/null 2>&1; then
  echo "missing required cargo subcommand: cargo-release" >&2
  echo "run: mise install" >&2
  exit 127
fi

branch="$(git branch --show-current)"
if [[ "$branch" != "main" ]]; then
  echo "release must be prepared from main; current branch is ${branch:-detached HEAD}" >&2
  exit 1
fi
if [[ -n "$(git status --porcelain)" ]]; then
  echo "release requires a clean working tree" >&2
  git status --short >&2
  exit 1
fi

expected_head="${1:-}"
head_commit="$(git rev-parse HEAD)"
if [[ -n "$expected_head" && "$head_commit" != "$expected_head" ]]; then
  echo "main moved from the successful CI commit $expected_head to $head_commit; this release run is stale"
  no_release
  exit 0
fi

git fetch --force --tags origin
latest_tag="$(git describe --tags --abbrev=0 --match 'v[0-9]*' 2>/dev/null || true)"
range="HEAD"
if [[ -n "$latest_tag" ]]; then
  range="$latest_tag..HEAD"
fi

prepared_line="$(git log "$range" --format='%H%x09%s' | awk -F '\t' '$2 ~ /^chore\(release\): v[0-9]+\.[0-9]+\.[0-9]+/ && !found { print; found=1 }')"
if [[ -n "$prepared_line" ]]; then
  prepared_commit="${prepared_line%%$'\t'*}"
  prepared_subject="${prepared_line#*$'\t'}"
  prepared_tag="${prepared_subject#chore(release): }"
  prepared_wasm_tag="criv-wasm-${prepared_tag}"
  root_tag_commit="$(remote_tag_commit "$prepared_tag")"
  wasm_tag_commit="$(remote_tag_commit "$prepared_wasm_tag")"
  for tagged_commit in "$root_tag_commit" "$wasm_tag_commit"; do
    if [[ -n "$tagged_commit" && "$tagged_commit" != "$prepared_commit" ]]; then
      echo "prepared release tag points to another commit" >&2
      exit 1
    fi
  done
  release_output "$prepared_commit" "$prepared_tag"
  exit 0
fi

bump_output=""
if ! bump_output="$(cog bump --dry-run --auto 2>&1)"; then
  if grep -qiE 'no commits|no conventional|no bump|nothing to bump' <<<"$bump_output"; then
    no_release
    exit 0
  fi
  printf '%s\n' "$bump_output" >&2
  exit 1
fi
version="$(printf '%s\n' "$bump_output" | tail -n 1)"
version="${version#v}"
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$ ]]; then
  echo "Cocogitto returned an invalid SemVer version: $version" >&2
  exit 1
fi

tag="v$version"
wasm_tag="criv-wasm-v$version"
for candidate in "$tag" "$wasm_tag"; do
  if git rev-parse -q --verify "refs/tags/$candidate" >/dev/null ||
    [[ -n "$(remote_tag_commit "$candidate")" ]]; then
    echo "tag already exists: $candidate" >&2
    exit 1
  fi
done

cargo release version "$version" --workspace --execute --no-confirm
git add :/Cargo.toml :/Cargo.lock ':(glob)**/Cargo.toml'
git commit --no-verify -m "chore(release): $tag"
git push --no-verify origin HEAD:main

release_output "$(git rev-parse HEAD)" "$tag"
