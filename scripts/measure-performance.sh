#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "$0")/.." && pwd)"
exec cargo run \
  --quiet \
  --manifest-path "$repository_root/Cargo.toml" \
  -p criv-perf-harness \
  -- \
  --repository-root "$repository_root" \
  "$@"
