#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "$0")/.." && pwd)"
test_root="$(mktemp -d)"
workflow="$(mktemp "$repository_root/.github/workflows/.cache-contract-XXXXXX.yml")"
trap 'rm -rf "$test_root"; rm -f "$workflow"' EXIT
setup="$repository_root/.github/actions/setup-build-tools/configure-env.sh"

for platform in Linux macOS; do
  for cargo_mode in default custom; do
    environment="$test_root/environment"
    expected="$test_root/expected"
    home="$test_root/home with spaces"
    runner_temp="$test_root/runner temp"
    cargo_home="$home/.cargo"
    : > "$environment"
    if [ "$cargo_mode" = custom ]; then
      cargo_home="$test_root/cargo downloads"
      env HOME="$home" RUNNER_TEMP="$runner_temp" RUNNER_OS="$platform" \
        CARGO_HOME="$cargo_home" GITHUB_ENV="$environment" bash "$setup"
    else
      env -u CARGO_HOME HOME="$home" RUNNER_TEMP="$runner_temp" RUNNER_OS="$platform" \
        GITHUB_ENV="$environment" bash "$setup"
    fi
    {
      printf 'MISE_DATA_DIR=%s/criv-mise\n' "$runner_temp"
      printf 'MISE_RUSTUP_HOME=%s/criv-mise/rustup\n' "$runner_temp"
      printf 'RUSTUP_HOME=%s/criv-mise/rustup\n' "$runner_temp"
      printf 'MISE_CARGO_HOME=%s\n' "$cargo_home"
      printf 'CARGO_HOME=%s\n' "$cargo_home"
    } > "$expected"
    diff -u "$expected" "$environment"
  done
done

# The lint compatibility adapter must still validate the shared action's inputs.
printf '%s\n' \
  'name: Cache contract' \
  'on: push' \
  'jobs:' \
  '  test:' \
  '    runs-on: ubuntu-latest' \
  '    steps:' \
  '      - name: Set up tools' \
  '        uses: $/.github/actions/setup-build-tools' \
  '        with:' \
  '          compiler-cache: "false"' > "$workflow"
bash "$repository_root/scripts/lint-workflows.sh" "$workflow"
printf '          unknown-cache-input: "true"\n' >> "$workflow"
if bash "$repository_root/scripts/lint-workflows.sh" "$workflow" > "$test_root/lint-error" 2>&1; then
  echo 'workflow lint accepted an unknown setup input' >&2
  exit 1
fi
grep -F 'input "unknown-cache-input" is not defined' "$test_root/lint-error" >/dev/null

printf 'CI cache environment and workflow input checks: ok\n'
