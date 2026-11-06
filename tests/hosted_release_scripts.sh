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

echo "hosted release scripts: ok"
