#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: assemble-hosted-release-gates.sh BUNDLE COMMIT OUTPUT" >&2
  exit 2
fi

bundle="$(cd "$1" && pwd)"
commit="$2"
output="$3"
primary_target="aarch64-apple-darwin"
targets=(
  aarch64-apple-darwin
  aarch64-unknown-linux-gnu
  x86_64-pc-windows-msvc
  x86_64-unknown-linux-gnu
)

if [[ ! "$commit" =~ ^[0-9a-fA-F]{40}$ ]]; then
  echo "commit must be one full hexadecimal Git commit ID" >&2
  exit 1
fi

result_directory() {
  local parent="$1"
  local directories=()
  while IFS= read -r directory; do
    directories+=("$directory")
  done < <(find "$parent" -mindepth 1 -maxdepth 1 -type d -print | sort)
  if [[ ${#directories[@]} -ne 1 ]]; then
    echo "expected one result directory under $parent, found ${#directories[@]}" >&2
    exit 1
  fi
  printf '%s\n' "${directories[0]#"$bundle/"}"
}

pair_file="$(mktemp)"
target_file="$(mktemp)"
trap 'rm -f "$pair_file" "$target_file"' EXIT

add_pair() {
  local target="$1"
  local workload="$2"
  local profile="$3"
  local selected="$4"
  local root="$bundle/$target/scaling/$workload"
  jq -nc \
    --arg target "$target" \
    --arg profile "$profile" \
    --argjson selected "$selected" \
    --arg baseline "$(result_directory "$root/baseline")" \
    --arg candidate "$(result_directory "$root/candidate")" \
    '{target:$target,profile:$profile,selected_files:$selected,baseline:$baseline,candidate:$candidate}' \
    >>"$pair_file"
}

for target in "${targets[@]}"; do
  manifest="$bundle/$target/artifacts/artifact.json"
  jq -e --arg commit "$commit" --arg target "$target" '
    .schema == "criv.discovery-remote-artifact.v3" and
    .commit == $commit and
    .target == $target
  ' "$manifest" >/dev/null
  add_pair "$target" source-100000 source 90000
  add_pair "$target" source-100000 source_candidates 90000
  jq -c \
    --arg base "${target}/artifacts/" \
    '. + {candidate_binary: ($base + .candidate_binary)} |
      {target,commit,candidate_binary,baseline_binary_digest,baseline_binary_bytes,
       baseline_build_seconds,candidate_build_seconds,clean_builds,
       compiler_cache_disabled,registry_inputs_present,elixir_coverage}' \
    "$manifest" >>"$target_file"
done

for workload_profile in \
  "vault-250000 vault 225000" \
  "markdown-250000 markdown 225000"; do
  read -r workload profile selected <<<"$workload_profile"
  add_pair "$primary_target" "$workload" "$profile" "$selected"
done

baseline_dependencies="$(jq -r .baseline_normal_dependencies "$bundle/$primary_target/artifacts/artifact.json")"
candidate_dependencies="$(jq -r .candidate_normal_dependencies "$bundle/$primary_target/artifacts/artifact.json")"
baseline_revision="$(jq -r .baseline_revision "$bundle/$primary_target/artifacts/artifact.json")"
baseline_contract="$(jq -r .baseline_evidence_contract "$bundle/$primary_target/artifacts/artifact.json")"
candidate_contract="$(jq -r .candidate_evidence_contract "$bundle/$primary_target/artifacts/artifact.json")"
baseline_package_names="$(jq -c .baseline_normal_package_names "$bundle/$primary_target/artifacts/artifact.json")"
candidate_package_names="$(jq -c .candidate_normal_package_names "$bundle/$primary_target/artifacts/artifact.json")"
for target in "${targets[@]}"; do
  manifest="$bundle/$target/artifacts/artifact.json"
  test "$(jq -r .baseline_normal_dependencies "$manifest")" = "$baseline_dependencies"
  test "$(jq -r .candidate_normal_dependencies "$manifest")" = "$candidate_dependencies"
  test "$(jq -r .baseline_revision "$manifest")" = "$baseline_revision"
  test "$(jq -r .baseline_evidence_contract "$manifest")" = "$baseline_contract"
  test "$(jq -r .candidate_evidence_contract "$manifest")" = "$candidate_contract"
  test "$(jq -c .baseline_normal_package_names "$manifest")" = "$baseline_package_names"
  test "$(jq -c .candidate_normal_package_names "$manifest")" = "$candidate_package_names"
done

if [[ "$baseline_contract" == "criv.release-evidence.pre-elixir.v1" &&
  "$candidate_contract" == "criv.release-evidence.elixir.v1" ]]; then
  evidence_transition="elixir-baseline-reset"
elif [[ "$baseline_contract" == "criv.release-evidence.elixir.v1" &&
  "$candidate_contract" == "criv.release-evidence.elixir.v1" ]]; then
  evidence_transition="compatible-baseline"
else
  echo "unsupported release evidence transition from $baseline_contract to $candidate_contract" >&2
  exit 1
fi

mkdir -p "$(dirname "$output")"
valid_until="$(( $(date +%s) + 7 * 24 * 60 * 60 ))"
jq -n \
  --arg commit "$commit" \
  --arg evidence_transition "$evidence_transition" \
  --argjson valid_until "$valid_until" \
  --arg primary_target "$primary_target" \
  --arg live_commands "$(result_directory "$bundle/$primary_target/live/candidate")" \
  --argjson scaling "$(jq -s . "$pair_file")" \
  --argjson targets "$(jq -s . "$target_file")" \
  --argjson before "$baseline_dependencies" \
  --argjson after "$candidate_dependencies" \
  --arg baseline_revision "$baseline_revision" \
  --arg baseline_contract "$baseline_contract" \
  --arg candidate_contract "$candidate_contract" \
  --argjson before_names "$baseline_package_names" \
  --argjson after_names "$candidate_package_names" \
  '{
    schema:"criv.discovery-gate-input.v2",
    commit:$commit,
    toolchain:"1.97.1",
    evidence_transition:$evidence_transition,
    valid_until_unix:$valid_until,
    primary_target:$primary_target,
    live_commands:$live_commands,
    scaling:$scaling,
    artifacts:{
      baseline_revision:$baseline_revision,
      baseline_contract:$baseline_contract,
      candidate_contract:$candidate_contract,
      normal_dependencies_before:$before,
      normal_dependencies_after:$after,
      normal_package_names_before:$before_names,
      normal_package_names_after:$after_names,
      native_compiler_or_library_added:false,
      targets:$targets
    }
  }' >"$output"

echo "$output"
