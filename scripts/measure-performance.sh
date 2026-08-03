#!/usr/bin/env bash
set -euo pipefail

root="$(cd "${1:-$(pwd)}" && pwd)"
criv_bin="${CRIV_BIN:-target/debug/criv}"
samples="${CRIV_PERF_SAMPLES:-5}"
if [[ "$criv_bin" != /* ]]; then
  criv_bin="$(pwd)/$criv_bin"
fi

if [[ ! "$samples" =~ ^[1-9][0-9]*$ ]]; then
  echo "CRIV_PERF_SAMPLES must be a positive integer, got: $samples" >&2
  exit 2
fi

if [[ ! -x "$criv_bin" ]]; then
  echo "missing criv binary: $criv_bin" >&2
  echo "run: cargo build" >&2
  exit 127
fi

if [[ ! -f "$root/criv.toml" ]]; then
  echo "not a criv vault: $root" >&2
  exit 2
fi

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

run_sample() {
  local label="$1"
  local sample="$2"
  shift
  shift
  local stdout="$tmp_dir/$label.$sample.out"
  local stderr="$tmp_dir/$label.$sample.err"

  printf '%s\tsample=%s/%s\t' "$label" "$sample" "$samples"
  if /usr/bin/time -p "$@" >"$stdout" 2>"$stderr"; then
    awk '
      /^real / { real = $2 }
      /^user / { user = $2 }
      /^sys / { sys = $2 }
      END {
        printf "ok\treal=%s\tuser=%s\tsys=%s\n", real, user, sys
      }
    ' "$stderr"
    awk '/^real / { print $2 }' "$stderr" >>"$tmp_dir/$label.real"
  else
    local status=$?
    awk -v status="$status" '
      /^real / { real = $2 }
      /^user / { user = $2 }
      /^sys / { sys = $2 }
      END {
        printf "failed:%s\treal=%s\tuser=%s\tsys=%s\n", status, real, user, sys
      }
    ' "$stderr"
    sed 's/^/  /' "$stderr" >&2
    return "$status"
  fi
}

summarize() {
  local label="$1"
  local values="$tmp_dir/$label.real"
  local sorted="$tmp_dir/$label.sorted"
  sort -n "$values" >"$sorted"
  awk -v label="$label" '
    { values[NR] = $1 }
    END {
      if (NR % 2 == 1) {
        median = values[(NR + 1) / 2]
      } else {
        median = (values[NR / 2] + values[NR / 2 + 1]) / 2
      }
      printf "%s\tsummary\tsamples=%d\treal_min=%.3f\treal_median=%.3f\treal_max=%.3f\n",
        label, NR, values[1], median, values[NR]
    }
  ' "$sorted"
}

run_samples() {
  local label="$1"
  shift
  local sample
  for ((sample = 1; sample <= samples; sample++)); do
    run_sample "$label" "$sample" "$@"
  done
  summarize "$label"
}

revision="$(git -C "$root" rev-parse --verify HEAD 2>/dev/null || printf 'unavailable')"
echo "criv_perf	root=$root	binary=$criv_bin	revision=$revision	samples=$samples	date=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
(
  cd "$root"
  run_samples "watch_once_cold" "$criv_bin" watch --once
  run_samples "watch_once_warm" "$criv_bin" watch --once
  run_samples "source_index_files" "$criv_bin" search --files src
  run_samples "check" "$criv_bin" check
  run_samples "enforce_ci" "$criv_bin" enforce --stage ci
  run_samples "query_next_adr_id" "$criv_bin" query next-adr-id
  run_samples "query_orphan_docs" "$criv_bin" query orphan-docs
  run_samples "query_nodes_docs" "$criv_bin" query nodes --kind doc
  run_samples "diff_latest" "$criv_bin" query diff latest latest
)
