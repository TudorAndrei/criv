#!/usr/bin/env bash
set -euo pipefail

# mise-action archives MISE_DATA_DIR, not the runner's default ~/.rustup.
# Keep Cargo downloads separate so they are not duplicated in the tools cache.
data_dir="${RUNNER_TEMP:?}/criv-mise"
cargo_home="${CARGO_HOME:-${HOME:?}/.cargo}"
if [ "${RUNNER_OS:?}" = Windows ]; then
  data_dir="$(cygpath -m "$data_dir")"
  cargo_home="$(cygpath -m "$cargo_home")"
fi

{
  printf 'MISE_DATA_DIR=%s\n' "$data_dir"
  printf 'MISE_RUSTUP_HOME=%s/rustup\n' "$data_dir"
  printf 'RUSTUP_HOME=%s/rustup\n' "$data_dir"
  printf 'MISE_CARGO_HOME=%s\n' "$cargo_home"
  printf 'CARGO_HOME=%s\n' "$cargo_home"
} >> "${GITHUB_ENV:?}"
