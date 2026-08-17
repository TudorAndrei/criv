#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "$0")/.." && pwd)"
test_root="$(mktemp -d)"
trap 'rm -rf "$test_root"' EXIT
bundle="$test_root/bundle"
commit="0123456789abcdef0123456789abcdef01234567"
targets=(
  aarch64-apple-darwin
  aarch64-unknown-linux-gnu
  x86_64-pc-windows-msvc
  x86_64-unknown-linux-gnu
)

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

for target in "${targets[@]}"; do
  artifact_root="$bundle/$target/artifacts"
  mkdir -p \
    "$bundle/$target/scaling/source-100000/baseline/run" \
    "$bundle/$target/scaling/source-100000/candidate/run" \
    "$artifact_root"
  suffix=""
  if [[ "$target" == x86_64-pc-windows-msvc ]]; then
    suffix=".exe"
  fi
  printf 'candidate-%s\n' "$target" >"$artifact_root/candidate-criv$suffix"
  jq -n \
    --arg commit "$commit" \
    --arg target "$target" \
    --arg binary "candidate-criv$suffix" \
    '{
      schema:"criv.discovery-remote-artifact.v2",
      commit:$commit,
      target:$target,
      baseline_revision:"v0.9.0",
      baseline_normal_dependencies:100,
      candidate_normal_dependencies:99,
      baseline_binary:"baseline-criv",
      baseline_binary_bytes:1000,
      baseline_binary_digest:"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      baseline_build_seconds:[10,11,12],
      candidate_binary:$binary,
      candidate_binary_bytes:900,
      candidate_binary_sha256:"unused",
      candidate_build_seconds:[9,10,11],
      clean_builds:true,
      compiler_cache_disabled:true,
      registry_inputs_present:true
    }' >"$artifact_root/artifact.json"
done

primary="$bundle/aarch64-apple-darwin"
for workload in source-250000 vault-250000 markdown-250000; do
  mkdir -p "$primary/scaling/$workload/baseline/run" "$primary/scaling/$workload/candidate/run"
done
mkdir -p "$primary/live/candidate/run"

"$repository_root/scripts/performance/assemble-hosted-release-gates.sh" \
  "$bundle" "$commit" "$bundle/gate-input.json"
jq -e --arg commit "$commit" '
  .commit == $commit and
  .primary_target == "aarch64-apple-darwin" and
  (.scaling | length) == 12 and
  ([.scaling[] | select(.selected_files == 90000)] | length) == 8 and
  ([.scaling[] | select(.selected_files == 225000)] | length) == 4 and
  (.artifacts.targets | length) == 4 and
  .artifacts.normal_dependencies_before == 100 and
  .artifacts.normal_dependencies_after == 99
' "$bundle/gate-input.json" >/dev/null

receipt="$bundle/release-gate.json"
artifacts_json="$test_root/artifacts.jsonl"
for target in "${targets[@]}"; do
  suffix=""
  if [[ "$target" == x86_64-pc-windows-msvc ]]; then
    suffix=".exe"
  fi
  path="$target/artifacts/candidate-criv$suffix"
  binary="$bundle/$path"
  jq -nc \
    --arg target "$target" \
    --arg path "$path" \
    --arg sha256 "$(sha256_file "$binary")" \
    '{target:$target,path:$path,sha256:$sha256}' >>"$artifacts_json"
done
jq -n \
  --arg commit "$commit" \
  --argjson artifacts "$(jq -s . "$artifacts_json")" \
  '{schema:"criv.discovery-release-gate.v1",commit:$commit,passed:true,artifacts:$artifacts}' \
  >"$receipt"
printf 'viewer\n' >"$test_root/vscode-criv.vsix"

"$repository_root/scripts/package-release-assets.sh" \
  "$bundle" "$receipt" "$test_root/vscode-criv.vsix" 0.10.0 "$test_root/dist"
(
  cd "$test_root/dist"
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum --check SHA256SUMS.txt
  else
    shasum -a 256 --check SHA256SUMS.txt
  fi
)
test "$(tar -tzf "$test_root/dist/criv-aarch64-apple-darwin.tar.gz" | sort | tr '\n' ' ')" = "criv vscode-criv.vsix "
zip_entries="$(unzip -Z1 "$test_root/dist/criv-x86_64-pc-windows-msvc.zip")"
grep -Fx 'criv.exe' <<<"$zip_entries" >/dev/null
grep -Fx 'vscode-criv.vsix' <<<"$zip_entries" >/dev/null

echo "hosted release scripts: ok"
