#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "$0")/.." && pwd)"
test_root="$(mktemp -d)"
trap 'rm -rf "$test_root"' EXIT

cargo build --locked --quiet --manifest-path "$repository_root/Cargo.toml" --bin criv
cargo run --locked --quiet --manifest-path "$repository_root/Cargo.toml" \
  -p criv-perf-harness --bin criv-perf-harness -- \
  --repository-root "$repository_root" \
  --binary "$repository_root/target/debug/criv" \
  --profile test \
  --allow-non-release \
  --samples 1 \
  --allow-low-samples \
  --manifest "$repository_root/fixtures/performance/elixir-mixed.toml" \
  --manifest "$repository_root/fixtures/performance/elixir-parse-heavy.toml" \
  --manifest "$repository_root/fixtures/performance/elixir-relationships.toml" \
  --case watch-once-cold \
  --results-root "$test_root/results" >/dev/null

result_dir="$(find "$test_root/results" -mindepth 1 -maxdepth 1 -type d -print -quit)"
test -n "$result_dir"
test "$(wc -l <"$result_dir/samples.jsonl" | tr -d ' ')" = "3"

jq -e '
  if .workload == "elixir-mixed" then
    .selected_source_files == 24
    and .selected_source_bytes == 524288
    and .selected_elixir_files == 12
  elif .workload == "elixir-parse-heavy" then
    .selected_source_files == 128
    and .selected_source_bytes == 4194304
    and .selected_elixir_files == 128
    and .expected_relationships == 0
  elif .workload == "elixir-relationships" then
    .selected_source_files == 128
    and .selected_source_bytes == 4194304
    and .selected_elixir_files == 128
    and .expected_relationships == 8192
  else false end
  and .selected_elixir_files == .parsed_elixir_files
  and .selected_elixir_bytes == .parsed_elixir_bytes
  and .expected_relationships == .parsed_relationships
  and .expected_relationships == .published_relationships
  and (.elixir_path_digest | length) == 64
  and .peak_rss_bytes > 0
  and .real_seconds > 0
  and (.stdout_digest | length) == 64
  and (.stderr_digest | length) == 64
  and (.state_digest | length) == 64
  and (.source_graph_digest | length) == 64
' "$result_dir/samples.jsonl" >/dev/null

jq -e '
  .schema == "criv.performance-summary.v2"
  and (.cases | length) == 3
  and all(.cases[];
    .successful_samples == 1
    and .failed_samples == 0
    and .selected_elixir_files == .parsed_elixir_files
    and .selected_elixir_bytes == .parsed_elixir_bytes
    and .expected_relationships == .parsed_relationships
    and .expected_relationships == .published_relationships
    and (.elixir_path_digest | length) == 64
    and .peak_rss_bytes.median > 0)
' "$result_dir/summary.json" >/dev/null

echo "Elixir performance workloads: ok"
