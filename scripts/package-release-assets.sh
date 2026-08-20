#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 5 ]]; then
  echo "usage: package-release-assets.sh BUNDLE VSIX VERSION COMMIT DIST" >&2
  exit 2
fi

bundle="$(cd "$1" && pwd)"
vsix="$(cd "$(dirname "$2")" && pwd)/$(basename "$2")"
version="$3"
commit="$4"
dist="$5"

if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$ ]]; then
  echo "invalid release version: $version" >&2
  exit 1
fi
if [[ ! "$commit" =~ ^[0-9a-fA-F]{40}$ ]]; then
  echo "commit must be one full hexadecimal Git commit ID" >&2
  exit 1
fi
if [[ ! -f "$vsix" ]]; then
  echo "missing VS Code package: $vsix" >&2
  exit 1
fi

mkdir -p "$dist"
dist="$(cd "$dist" && pwd)"
stage_root="$(mktemp -d)"
artifacts_json="$(mktemp)"
trap 'rm -rf "$stage_root"; rm -f "$artifacts_json"' EXIT

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

file_bytes() {
  wc -c <"$1" | awk '{print $1}'
}

targets=(
  aarch64-apple-darwin
  aarch64-unknown-linux-gnu
  x86_64-pc-windows-msvc
  x86_64-unknown-linux-gnu
)

for target in "${targets[@]}"; do
  suffix=""
  archive="criv-$target.tar.gz"
  if [[ "$target" == "x86_64-pc-windows-msvc" ]]; then
    suffix=".exe"
    archive="criv-$target.zip"
  fi
  relative_path="$target/criv$suffix"
  binary="$bundle/$relative_path"
  if [[ ! -f "$binary" ]]; then
    echo "missing release binary: $binary" >&2
    exit 1
  fi

  sha256="$(sha256_file "$binary")"
  bytes="$(file_bytes "$binary")"
  jq -nc \
    --arg target "$target" \
    --arg path "$relative_path" \
    --arg archive "$archive" \
    --arg sha256 "$sha256" \
    --argjson bytes "$bytes" \
    '{target:$target,path:$path,archive:$archive,sha256:$sha256,bytes:$bytes}' \
    >>"$artifacts_json"

  stage="$stage_root/$target"
  mkdir -p "$stage"
  cp -p "$vsix" "$stage/vscode-criv.vsix"
  if [[ -n "$suffix" ]]; then
    cp -p "$binary" "$stage/criv.exe"
    (
      cd "$stage"
      zip -q "$dist/$archive" criv.exe vscode-criv.vsix
    )
  else
    cp -p "$binary" "$stage/criv"
    chmod +x "$stage/criv"
    tar -C "$stage" -czf "$dist/$archive" criv vscode-criv.vsix
  fi
done

jq -n \
  --arg commit "$commit" \
  --arg version "$version" \
  --arg vscode_vsix_sha256 "$(sha256_file "$vsix")" \
  --argjson artifacts "$(jq -s . "$artifacts_json")" \
  '{
    schema:"criv.release-manifest.v1",
    commit:$commit,
    version:$version,
    vscode_vsix_sha256:$vscode_vsix_sha256,
    artifacts:$artifacts
  }' >"$dist/release-manifest.json"

(
  cd "$dist"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum criv-*.tar.gz criv-*.zip release-manifest.json >SHA256SUMS.txt
  else
    shasum -a 256 criv-*.tar.gz criv-*.zip release-manifest.json >SHA256SUMS.txt
  fi
)

echo "$dist"
