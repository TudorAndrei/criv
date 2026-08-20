#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "$0")/.." && pwd)"
test_root="$(mktemp -d)"
trap 'rm -rf "$test_root"' EXIT
bundle="$test_root/bundle"
commit="0123456789abcdef0123456789abcdef01234567"

prepare_step="$test_root/prepare-step.yml"
sed -n \
  '/      - name: Prepare automatic release/,/      - name: Export release selection/p' \
  "$repository_root/.github/workflows/release.yml" >"$prepare_step"
grep -F 'GH_TOKEN: ${{ github.token }}' "$prepare_step" >/dev/null
grep -F 'git push --no-verify origin HEAD:main' \
  "$repository_root/scripts/release-auto.sh" >/dev/null

quality_job="$test_root/quality-job.yml"
sed -n \
  '/  quality:/,/  measure:/p' \
  "$repository_root/.github/workflows/release.yml" >"$quality_job"
grep -F 'rustup toolchain install 1.97.1 --profile minimal --no-self-update' \
  "$quality_job" >/dev/null
grep -F 'rustup component add clippy rustfmt --toolchain 1.97.1' \
  "$quality_job" >/dev/null

record_evidence_step="$test_root/record-evidence-step.yml"
sed -n \
  '/      - name: Record hosted evidence identity/,/        shell: pwsh/p' \
  "$repository_root/.github/workflows/release.yml" >"$record_evidence_step"
grep -F '        if: always()' "$record_evidence_step" >/dev/null

upload_evidence_step="$test_root/upload-evidence-step.yml"
sed -n \
  '/      - name: Upload raw evidence and measured binary/,/        uses:/p' \
  "$repository_root/.github/workflows/release.yml" >"$upload_evidence_step"
grep -F '        if: always()' "$upload_evidence_step" >/dev/null

baseline_step="$test_root/baseline-step.yml"
sed -n \
  '/      - name: Select compatible release baseline/,/      - name: Measure 100k Source workload/p' \
  "$repository_root/.github/workflows/release.yml" >"$baseline_step"
grep -F 'git describe --tags --abbrev=0 --match' "$baseline_step" >/dev/null
grep -F '"$EXPECTED_COMMIT^"' "$baseline_step" >/dev/null
grep -F 'git worktree add --detach "$baseline_root" "$baseline_revision"' "$baseline_step" >/dev/null
grep -F '"$baseline_revision" = v0.9.0' "$baseline_step" >/dev/null
test "$(tr -d '\r\n' < "$repository_root/scripts/performance/release-evidence-contract.txt")" = \
  "criv.release-evidence.elixir.v1"

coverage_step="$test_root/coverage-step.yml"
sed -n \
  '/      - name: Prove complete Elixir file coverage/,/      - name: Measure live-watch convergence/p' \
  "$repository_root/.github/workflows/release.yml" >"$coverage_step"
grep -F 'lib/coverage.ex' "$coverage_step" >/dev/null
grep -F 'src/coverage.exs' "$coverage_step" >/dev/null
grep -F 'module:Coverage.Ex/fn:last/1' "$coverage_step" >/dev/null
grep -F 'module:Coverage.Exs/fn:last/1' "$coverage_step" >/dev/null

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
      schema:"criv.discovery-remote-artifact.v3",
      commit:$commit,
      target:$target,
      baseline_revision:"v0.9.0",
      baseline_evidence_contract:"criv.release-evidence.pre-elixir.v1",
      candidate_evidence_contract:"criv.release-evidence.elixir.v1",
      baseline_normal_dependencies:100,
      candidate_normal_dependencies:99,
      baseline_normal_package_names:["criv","fff-search","tree-sitter"],
      candidate_normal_package_names:["criv","tree-sitter","tree-sitter-elixir"],
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
      registry_inputs_present:true,
      elixir_coverage:{
        selected_paths:["lib/coverage.ex","src/coverage.exs"],
        parsed_paths:["lib/coverage.ex","src/coverage.exs"],
        selected_bytes:200,
        parsed_bytes:200,
        state_sha256:"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
      }
    }' >"$artifact_root/artifact.json"
done

primary="$bundle/aarch64-apple-darwin"
for workload in vault-250000 markdown-250000; do
  mkdir -p "$primary/scaling/$workload/baseline/run" "$primary/scaling/$workload/candidate/run"
done
mkdir -p "$primary/live/candidate/run"

"$repository_root/scripts/performance/assemble-hosted-release-gates.sh" \
  "$bundle" "$commit" "$bundle/gate-input.json"
jq -e --arg commit "$commit" '
  .commit == $commit and
  .primary_target == "aarch64-apple-darwin" and
  .schema == "criv.discovery-gate-input.v2" and
  .evidence_transition == "elixir-baseline-reset" and
  (.scaling | length) == 10 and
  ([.scaling[] | select(.selected_files == 90000)] | length) == 8 and
  ([.scaling[] | select(.selected_files == 225000)] | length) == 2 and
  (.artifacts.targets | length) == 4 and
  .artifacts.normal_dependencies_before == 100 and
  .artifacts.normal_dependencies_after == 99 and
  .artifacts.normal_package_names_after == ["criv","tree-sitter","tree-sitter-elixir"] and
  ([.artifacts.targets[].elixir_coverage.parsed_paths] | length) == 4
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
  '{schema:"criv.discovery-release-gate.v2",commit:$commit,passed:true,artifacts:$artifacts}' \
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
