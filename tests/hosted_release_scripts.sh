#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "$0")/.." && pwd)"
test_root="$(mktemp -d)"
trap 'rm -rf "$test_root"' EXIT
workflow="$repository_root/.github/workflows/release.yml"
commit="0123456789abcdef0123456789abcdef01234567"

prepare_step="$test_root/prepare-step.yml"
sed -n \
  '/      - name: Prepare automatic release/,/      - name: Export release selection/p' \
  "$workflow" >"$prepare_step"
grep -F 'GH_TOKEN: ${{ github.token }}' "$prepare_step" >/dev/null
grep -F 'git push --no-verify origin HEAD:main' \
  "$repository_root/scripts/release-auto.sh" >/dev/null
grep -F '&& !found { print; found=1 }' \
  "$repository_root/scripts/release-auto.sh" >/dev/null

quality_job="$test_root/quality-job.yml"
sed -n '/  quality:/,/  build:/p' "$workflow" >"$quality_job"
grep -F 'rustup toolchain install 1.97.1 --profile minimal --no-self-update' \
  "$quality_job" >/dev/null
grep -F 'rustup component add clippy rustfmt --toolchain 1.97.1' \
  "$quality_job" >/dev/null
grep -F 'npm --prefix extensions/vscode-criv run package' "$quality_job" >/dev/null
grep -F 'name: release-vsix-${{ needs.prepare.outputs.commit }}' "$quality_job" >/dev/null

build_job="$test_root/build-job.yml"
sed -n '/  build:/,/  package:/p' "$workflow" >"$build_job"
grep -F 'name: Build ${{ matrix.name }}' "$build_job" >/dev/null
grep -F 'cargo build --locked --release --target "$RELEASE_TARGET" --package criv --bin criv' \
  "$build_job" >/dev/null
grep -F 'name: release-binary-${{ matrix.target }}-${{ needs.prepare.outputs.commit }}' \
  "$build_job" >/dev/null
for target in \
  aarch64-apple-darwin \
  aarch64-unknown-linux-gnu \
  x86_64-pc-windows-msvc \
  x86_64-unknown-linux-gnu; do
  grep -F "target: $target" "$build_job" >/dev/null
done

package_job="$test_root/package-job.yml"
sed -n '/  package:/,/  verify:/p' "$workflow" >"$package_job"
grep -F '      - quality' "$package_job" >/dev/null
grep -F '      - build' "$package_job" >/dev/null
grep -F 'ref: ${{ github.sha }}' "$package_job" >/dev/null
grep -F 'pattern: release-binary-*-${{ needs.prepare.outputs.commit }}' \
  "$package_job" >/dev/null
grep -F 'name: release-vsix-${{ needs.prepare.outputs.commit }}' "$package_job" >/dev/null
grep -F 'path: release-vsix' "$package_job" >/dev/null
if grep -Eq 'npm ci|npm .* run package|path: release-source' "$package_job"; then
  echo "package job rebuilds the exact VS Code package" >&2
  exit 1
fi
grep -F 'scripts/package-release-assets.sh' "$package_job" >/dev/null
grep -F 'release-vsix/vscode-criv.vsix' "$package_job" >/dev/null
grep -F '"$VERSION" "$COMMIT" dist' "$package_job" >/dev/null

if grep -Eq 'Measure 100k|Measure primary|Measure clean|release-evidence|criv-discovery-gate|publish-release-gate-note' \
  "$workflow"; then
  echo "automatic release still contains performance measurement" >&2
  exit 1
fi

grep -F 'dist/release-manifest.json' "$workflow" >/dev/null
grep -F '"release-manifest.json"' "$workflow" >/dev/null

tag_step="$test_root/tag-step.yml"
sed -n \
  '/      - name: Create or validate release tags/,/      - name: Create or update draft release/p' \
  "$workflow" >"$tag_step"
grep -F 'GH_TOKEN: ${{ github.token }}' "$tag_step" >/dev/null
grep -F 'git push --atomic origin' "$tag_step" >/dev/null

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
