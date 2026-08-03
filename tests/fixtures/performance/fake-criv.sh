#!/usr/bin/env bash
set -euo pipefail

if [[ -n "${CRIV_FAKE_LOG:-}" ]]; then
  printf '%s\t%s\t%s\t%s\n' \
    "$PWD" \
    "${CRIV_PERF_SAMPLE_ID:-missing}" \
    "${CRIV_PERF_CASE:-missing}" \
    "${CRIV_PERF_CACHE_STATE:-missing}" >>"$CRIV_FAKE_LOG"
fi

requested_failure=false
if [[ "${CRIV_FAKE_FAIL_CASE:-}" == "${CRIV_PERF_CASE:-}" \
  && "${CRIV_PERF_SAMPLE_ID:-}" != "warmup" \
  && "${CRIV_PERF_SAMPLE_ID:-}" != "seed" ]]; then
  requested_failure=true
fi

if [[ "${1:-}" == "watch" ]]; then
  mkdir -p .criv/snapshots
  snapshot="aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  printf '{"schema":"criv.state.v0","graph":{"root":"","nodes":[],"edges":[]},"registered-patterns":[],"patterns":{},"source-index":[]}\n' >.criv/state.json
  cp .criv/state.json ".criv/snapshots/$snapshot.json"
  printf '%s\n' "$snapshot" >.criv/latest
  printf '{"schema":"criv.source-graph/2"}\n' >.criv/source-graph.json
fi

if [[ "${1:-}" == "query" && "${2:-}" == "diff" && ! -f .criv/latest ]]; then
  echo "latest does not resolve" >&2
  exit 1
fi

if [[ "$requested_failure" == true ]]; then
  echo "requested fake failure" >&2
  exit 7
fi

printf 'fake criv ok\n'
