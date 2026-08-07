#!/usr/bin/env bash
# Start the worktree workspace for the shell that invoked Worktrunk.

set -euo pipefail

branch=${1:?branch required}
worktree_path=${2:?worktree_path required}
repo=${3:-}
script_dir=$(cd "$(dirname "$0")" && pwd)

# Herdr panes can inherit TMUX from the Herdr server process. Check Herdr first.
if [ "${HERDR_ENV:-}" = 1 ]; then
  if ! command -v herdr >/dev/null 2>&1; then
    echo "wt-session: herdr not installed; skipping workspace for $branch" >&2
    exit 0
  fi
  if [ -z "${HERDR_WORKSPACE_ID:-}" ]; then
    echo "wt-session: HERDR_WORKSPACE_ID is missing; skipping workspace for $branch" >&2
    exit 0
  fi
  if ! herdr worktree open \
    --workspace "$HERDR_WORKSPACE_ID" \
    --path "$worktree_path" \
    --label "$branch" \
    --no-focus
  then
    echo "wt-session: herdr could not open $worktree_path; continuing without a workspace" >&2
  fi
  exit 0
fi

if [ -n "${TMUX:-}" ]; then
  exec bash "$script_dir/wt-tmux.sh" start "$branch" "$worktree_path" "$repo"
fi

echo "wt-session: no tmux or herdr shell detected; skipping workspace for $branch"
