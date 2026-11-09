#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "$0")/.." && pwd)"
test_root="$(mktemp -d)"
trap 'rm -rf "$test_root"' EXIT
commit="0123456789abcdef0123456789abcdef01234567"

bundle="$test_root/bundle"
targets=(
  aarch64-apple-darwin
  aarch64-unknown-linux-gnu
  x86_64-pc-windows-msvc
  x86_64-unknown-linux-gnu
)
for target in "${targets[@]}"; do
  mkdir -p "$bundle/$target"
  suffix=""
  if [[ "$target" == x86_64-pc-windows-msvc ]]; then
    suffix=".exe"
  fi
  printf 'candidate-%s\n' "$target" >"$bundle/$target/criv$suffix"
done
printf 'viewer\n' >"$test_root/vscode-criv.vsix"

(
  cd "$test_root"
  "$repository_root/scripts/package-release-assets.sh" \
    "$bundle" "$test_root/vscode-criv.vsix" 0.10.1 "$commit" dist
)

jq -e --arg commit "$commit" '
  .schema == "criv.release-manifest.v1" and
  .commit == $commit and
  .version == "0.10.1" and
  (.artifacts | length) == 4 and
  ([.artifacts[].target] | sort) == [
    "aarch64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu"
  ] and
  ([.artifacts[] | select((.sha256 | length) == 64 and .bytes > 0)] | length) == 4
' "$test_root/dist/release-manifest.json" >/dev/null

(
  cd "$test_root/dist"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum --check SHA256SUMS.txt
  else
    shasum -a 256 --check SHA256SUMS.txt
  fi
)
test "$(tar -tzf "$test_root/dist/criv-aarch64-apple-darwin.tar.gz" | sort | tr '\n' ' ')" = \
  "criv vscode-criv.vsix "
zip_entries="$(unzip -Z1 "$test_root/dist/criv-x86_64-pc-windows-msvc.zip")"
grep -Fx 'criv.exe' <<<"$zip_entries" >/dev/null
grep -Fx 'vscode-criv.vsix' <<<"$zip_entries" >/dev/null

history="$test_root/history"
mkdir "$history"
cp "$repository_root/cog.toml" "$history/"
export GIT_AUTHOR_NAME=criv GIT_AUTHOR_EMAIL=criv@example.invalid
export GIT_COMMITTER_NAME=criv GIT_COMMITTER_EMAIL=criv@example.invalid
git -C "$history" init -q -b main
git -C "$history" add cog.toml
git -C "$history" commit -q --no-verify -m "chore: start history"

expect_next_version() {
  local tag="$1" message="$2" expected="$3" actual
  git -C "$history" tag "$tag"
  git -C "$history" commit -q --no-verify --allow-empty -m "$message"
  actual="$(cd "$history" && "$repository_root/scripts/release-version.sh" 2>/dev/null)"
  if [[ "$actual" != "$expected" ]]; then
    printf 'after %s and "%s": expected "%s", got "%s"\n' \
      "$tag" "$message" "$expected" "$actual" >&2
    exit 1
  fi
}

expect_next_version v0.14.0 "refactor: tidy" ""
expect_next_version v0.14.1 "fix: repair" "0.14.2"
expect_next_version v0.14.2 "feat: add" "0.15.0"
expect_next_version v0.15.0 "refactor!: remove" "1.0.0"
expect_next_version v0.15.1 $'perf(x): drop\n\nBREAKING CHANGE: gone' "1.0.0"
expect_next_version v1.0.0 "fix(cli)!: rename" "2.0.0"

echo "hosted release scripts: ok"
