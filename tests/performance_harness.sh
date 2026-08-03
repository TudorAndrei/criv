#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "$0")/.." && pwd)"
test_root="$(mktemp -d)"
trap 'rm -rf "$test_root"' EXIT
fake_binary="$repository_root/tests/fixtures/performance/fake-criv.sh"
manifest="$repository_root/fixtures/performance/barrs-small.toml"

if cargo run --quiet -p criv-perf-harness -- --profile test 2>"$test_root/missing.err"; then
  echo "missing binary unexpectedly succeeded" >&2
  exit 1
fi
grep -q -- "--binary" "$test_root/missing.err"

if cargo run --quiet -p criv-perf-harness -- \
  --binary "$fake_binary" \
  --binary "$fake_binary" \
  --profile test \
  --allow-non-release \
  2>"$test_root/duplicate.err"; then
  echo "duplicate binary unexpectedly succeeded" >&2
  exit 1
fi
grep -q "cannot be used multiple times" "$test_root/duplicate.err"

run_harness() {
  cargo run --quiet -p criv-perf-harness -- \
    --binary "$fake_binary" \
    --profile test \
    --allow-non-release \
    --samples 1 \
    --allow-low-samples \
    --manifest "$manifest" \
    --case watch-once-cold \
    --case watch-once-warm \
    --results-root "$test_root/results" \
    --repository-root "$repository_root"
}

export CRIV_FAKE_LOG="$test_root/fake.log"
run_harness >"$test_root/first.out"
run_harness >"$test_root/second.out"

test "$(find "$test_root/results" -mindepth 1 -maxdepth 1 -type d | wc -l | tr -d ' ')" = "2"
first_result="$(find "$test_root/results" -mindepth 1 -maxdepth 1 -type d | sort | head -1)"
jq -e '.schema == "criv.performance-summary.v1" and (.cases | length) == 2' \
  "$first_result/summary.json" >/dev/null
test "$(wc -l <"$first_result/samples.jsonl" | tr -d ' ')" = "2"
jq -e '.measurement.schema == "criv.performance-measurement.v1"' \
  "$first_result/samples.jsonl" >/dev/null
test "$(cut -f1 "$test_root/fake.log" | sort -u | wc -l | tr -d ' ')" -ge "8"

export CRIV_FAKE_FAIL_CASE="watch_once_cold"
if run_harness >"$test_root/failure.out" 2>"$test_root/failure.err"; then
  echo "failed sample unexpectedly succeeded" >&2
  exit 1
fi
failed_result="$(find "$test_root/results" -mindepth 1 -maxdepth 1 -type d | sort | tail -1)"
jq -e 'select(.case == "watch_once_cold") | .exit_status == 7' \
  "$failed_result/samples.jsonl" >/dev/null

echo "performance harness smoke: ok"
