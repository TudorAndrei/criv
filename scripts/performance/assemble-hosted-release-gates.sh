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
    .schema == "criv.discovery-remote-artifact.v2" and
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
       compiler_cache_disabled,registry_inputs_present}' \
    "$manifest" >>"$target_file"
done

for workload_profile in \
  "source-250000 source 225000" \
  "source-250000 source_candidates 225000" \
  "vault-250000 vault 225000" \
  "markdown-250000 markdown 225000"; do
  read -r workload profile selected <<<"$workload_profile"
  add_pair "$primary_target" "$workload" "$profile" "$selected"
done

baseline_dependencies="$(jq -r .baseline_normal_dependencies "$bundle/$primary_target/artifacts/artifact.json")"
candidate_dependencies="$(jq -r .candidate_normal_dependencies "$bundle/$primary_target/artifacts/artifact.json")"
for target in "${targets[@]}"; do
  manifest="$bundle/$target/artifacts/artifact.json"
  test "$(jq -r .baseline_normal_dependencies "$manifest")" = "$baseline_dependencies"
  test "$(jq -r .candidate_normal_dependencies "$manifest")" = "$candidate_dependencies"
done

mkdir -p "$(dirname "$output")"
valid_until="$(( $(date +%s) + 7 * 24 * 60 * 60 ))"
jq -n \
  --arg commit "$commit" \
  --argjson valid_until "$valid_until" \
  --arg primary_target "$primary_target" \
  --arg live_commands "$(result_directory "$bundle/$primary_target/live/candidate")" \
  --argjson scaling "$(jq -s . "$pair_file")" \
  --argjson targets "$(jq -s . "$target_file")" \
  --argjson before "$baseline_dependencies" \
  --argjson after "$candidate_dependencies" \
  '{
    schema:"criv.discovery-gate-input.v1",
    commit:$commit,
    toolchain:"1.97.1",
    valid_until_unix:$valid_until,
    primary_target:$primary_target,
    live_commands:$live_commands,
    scaling:$scaling,
    artifacts:{
      normal_dependencies_before:$before,
      normal_dependencies_after:$after,
      native_compiler_or_library_added:false,
      targets:$targets
    }
  }' >"$output"

echo "$output"
