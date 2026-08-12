#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "$0")/.." && pwd)"
test_root="$(mktemp -d)"
trap 'rm -rf "$test_root"' EXIT
fake_binary="$repository_root/tests/fixtures/performance/fake-criv.sh"
manifest="$repository_root/fixtures/performance/barrs-small.toml"

if cargo run --quiet -p criv-perf-harness --bin criv-perf-harness -- \
  --profile test 2>"$test_root/missing.err"; then
  echo "missing binary unexpectedly succeeded" >&2
  exit 1
fi
grep -q -- "--binary" "$test_root/missing.err"

if cargo run --quiet -p criv-perf-harness --bin criv-perf-harness -- \
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
  cargo run --quiet -p criv-perf-harness --bin criv-perf-harness -- \
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
run_revision="$(jq -r .revision "$first_result/run.json")"
jq -e '.schema == "criv.performance-summary.v2" and (.cases | length) == 2' \
  "$first_result/summary.json" >/dev/null
test "$(wc -l <"$first_result/samples.jsonl" | tr -d ' ')" = "2"
jq -e '.schema == "criv.performance-sample.v2" and has("measurement") == false' \
  "$first_result/samples.jsonl" >/dev/null
test "$(cut -f1 "$test_root/fake.log" | sort -u | wc -l | tr -d ' ')" -ge "8"

note_fixture="$test_root/note-fixture"
mkdir -p "$note_fixture"
jq '.samples = 3 | .dirty = false | .profile = "release"' \
  "$first_result/run.json" >"$note_fixture/run.json"
jq '.run.samples = 3 | .cases |= map(.successful_samples = 3)' \
  "$first_result/summary.json" >"$note_fixture/summary.json"
cp "$first_result/samples.jsonl" "$note_fixture/samples.jsonl"
for sample in 2 3; do
  sed -e "s/\"sample\":1/\"sample\":$sample/" "$first_result/samples.jsonl" \
    >>"$note_fixture/samples.jsonl"
done

"$repository_root/scripts/performance/render-git-note.sh" \
  "$note_fixture" \
  "$test_root/performance-note.json" \
  "$run_revision" \
  "refs/heads/main" \
  "https://github.invalid/actions/runs/123" \
  "123" \
  "1" \
  "performance-fixture"
jq -e '
  .schema == "criv.performance-git-note.v2"
  and .evidence.observation == "external-subprocess"
  and .report == {"artifact_path":"report.html","format":"html"}
  and (.timings | length) == 2
  and has("work") == false
' "$test_root/performance-note.json" >/dev/null

cargo run --quiet -p criv-perf-harness --bin criv-perf-report -- \
  --result-dir "$note_fixture" \
  --note "$test_root/performance-note.json" \
  --output "$test_root/report.html" \
  --github-summary "$test_root/report-summary.md"
grep -q '<!doctype html>' "$test_root/report.html"
grep -q 'role="img"' "$test_root/report.html"
grep -q 'JSON evidence remains canonical' "$test_root/report.html"
grep -q '^## Performance report' "$test_root/report-summary.md"

foreign_sample="$test_root/foreign-sample"
mkdir -p "$foreign_sample"
cp "$note_fixture/run.json" "$note_fixture/summary.json" "$note_fixture/samples.jsonl" \
  "$foreign_sample"
jq -c '.run_id = "another-run"' \
  "$note_fixture/samples.jsonl" | head -1 >"$foreign_sample/samples.jsonl"
if "$repository_root/scripts/performance/render-git-note.sh" \
  "$foreign_sample" \
  "$test_root/foreign-sample-note.json" \
  "$run_revision" \
  "refs/heads/main" \
  "https://github.invalid/actions/runs/123" \
  "123" \
  "1" \
  "performance-fixture" \
  2>/dev/null; then
  echo "foreign sample unexpectedly rendered" >&2
  exit 1
fi

export CRIV_FAKE_FAIL_CASE="watch_once_cold"
if run_harness >"$test_root/failure.out" 2>"$test_root/failure.err"; then
  echo "failed sample unexpectedly succeeded" >&2
  exit 1
fi
failed_result="$(find "$test_root/results" -mindepth 1 -maxdepth 1 -type d | sort | tail -1)"
jq -e 'select(.case == "watch_once_cold") | .exit_status == 7' \
  "$failed_result/samples.jsonl" >/dev/null

echo "performance harness smoke: ok"
