#!/usr/bin/env bash
set -euo pipefail

root="$(cd "${1:-$(pwd)}" && pwd)"
criv_bin="${CRIV_BIN:-target/debug/criv}"
if [[ "$criv_bin" != /* ]]; then
  criv_bin="$(pwd)/$criv_bin"
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

run_measure() {
  local label="$1"
  shift
  local stdout="$tmp_dir/$label.out"
  local stderr="$tmp_dir/$label.err"

  printf '%s\t' "$label"
  if /usr/bin/time -p "$@" >"$stdout" 2>"$stderr"; then
    awk '
      /^real / { real = $2 }
      /^user / { user = $2 }
      /^sys / { sys = $2 }
      END {
        printf "ok\treal=%s\tuser=%s\tsys=%s\n", real, user, sys
      }
    ' "$stderr"
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

echo "criv_perf	root=$root	binary=$criv_bin	date=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
(
  cd "$root"
  run_measure "watch_once_cold" "$criv_bin" watch --once
  run_measure "watch_once_warm" "$criv_bin" watch --once
  run_measure "source_index_files" "$criv_bin" search --files src
  run_measure "check" "$criv_bin" check
  run_measure "enforce_ci" "$criv_bin" enforce --stage ci
  run_measure "diff_latest" "$criv_bin" query diff latest latest
)
