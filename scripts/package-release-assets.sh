#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 5 ]]; then
  echo "usage: package-release-assets.sh BUNDLE RECEIPT VSIX VERSION DIST" >&2
  exit 2
fi

bundle="$(cd "$1" && pwd)"
receipt="$(cd "$(dirname "$2")" && pwd)/$(basename "$2")"
vsix="$(cd "$(dirname "$3")" && pwd)/$(basename "$3")"
version="$4"
dist="$5"

if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$ ]]; then
  echo "invalid release version: $version" >&2
  exit 1
fi
jq -e --arg version "$version" '
  .schema == "criv.discovery-release-gate.v1" and
  .passed == true and
  (.artifacts | length == 4)
' "$receipt" >/dev/null

mkdir -p "$dist"
stage_root="$(mktemp -d)"
trap 'rm -rf "$stage_root"' EXIT

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

while IFS=$'\t' read -r target path sha256; do
  binary="$bundle/$path"
  if [[ ! -f "$binary" ]]; then
    echo "missing measured binary: $binary" >&2
    exit 1
  fi
  actual="$(sha256_file "$binary")"
  if [[ "$actual" != "$sha256" ]]; then
    echo "measured binary digest mismatch: $binary" >&2
    exit 1
  fi

  stage="$stage_root/$target"
  mkdir -p "$stage"
  cp -p "$vsix" "$stage/vscode-criv.vsix"
  if [[ "$target" == "x86_64-pc-windows-msvc" ]]; then
    cp -p "$binary" "$stage/criv.exe"
    (
      cd "$stage"
      zip -q "$dist/criv-$target.zip" criv.exe vscode-criv.vsix
    )
  else
    cp -p "$binary" "$stage/criv"
    chmod +x "$stage/criv"
    tar -C "$stage" -czf "$dist/criv-$target.tar.gz" criv vscode-criv.vsix
  fi
done < <(jq -r '.artifacts[] | [.target, .path, .sha256] | @tsv' "$receipt")

cp -p "$receipt" "$dist/release-gate.json"
(
  cd "$dist"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum criv-*.tar.gz criv-*.zip release-gate.json >SHA256SUMS.txt
  else
    shasum -a 256 criv-*.tar.gz criv-*.zip release-gate.json >SHA256SUMS.txt
  fi
)

echo "$dist"
