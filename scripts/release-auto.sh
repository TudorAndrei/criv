#!/usr/bin/env bash
set -euo pipefail

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    echo "run: mise install" >&2
    exit 127
  fi
}

require_command cog
require_command cargo
require_command git

if ! cargo release --version >/dev/null 2>&1; then
  echo "missing required cargo subcommand: cargo-release" >&2
  echo "run: mise install" >&2
  exit 127
fi

branch="$(git rev-parse --abbrev-ref HEAD)"
if [[ "$branch" != "main" ]]; then
  echo "release must be cut from main; current branch is $branch" >&2
  exit 1
fi

if [[ -n "$(git status --porcelain)" ]]; then
  echo "release requires a clean working tree" >&2
  git status --short >&2
  exit 1
fi

version="$(cog bump --dry-run --auto)"
version="${version#v}"

if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$ ]]; then
  echo "cog did not return a SemVer version: $version" >&2
  exit 1
fi

tag="v$version"
wasm_tag="criv-wasm-v$version"

if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
  echo "tag already exists: $tag" >&2
  exit 1
fi

if git rev-parse -q --verify "refs/tags/$wasm_tag" >/dev/null; then
  echo "tag already exists: $wasm_tag" >&2
  exit 1
fi

echo "cutting release $tag"

cargo release version "$version" --workspace --execute --no-confirm

cargo test --locked --workspace
cargo fmt --check
cargo run --locked --quiet -- check
cargo run --locked --quiet -- enforce --stage ci
cargo run --locked --quiet -- watch --once
cargo run --locked --quiet -- query diff latest latest

git add :/Cargo.toml :/Cargo.lock ':(glob)**/Cargo.toml'
git commit -m "chore(release): $tag"
git tag "$tag"
git tag "$wasm_tag"

git push origin main
git push origin "$tag" "$wasm_tag"
