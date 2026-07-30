#!/usr/bin/env bash
# wt-tmux.sh — bridge worktrunk worktrees to tmux sessions.
#
# One worktree, one detached tmux session, one window, cwd = the worktree.
# Called from worktrunk hooks in .config/wt.toml:
#
#   [pre-start]
#   tmux = "bash .config/wt-tmux.sh start {{ branch }} {{ worktree_path }} {{ repo }}"
#
#   [pre-remove]
#   tmux = "bash .config/wt-tmux.sh stop {{ branch }} {{ repo }}"
#
# Usage: wt-tmux.sh <start|stop|attach|name|list> [branch] [args...]
#
# Exits 0 whenever tmux is simply unavailable or the session is already in the
# desired state: a pre-* hook that fails aborts the worktree operation, and
# losing a worktree because tmux is missing is a worse outcome than losing the
# session.

set -euo pipefail

cmd=${1:-}
[ -n "$cmd" ] || { echo "usage: wt-tmux.sh <start|stop|attach|name|list> [branch] [worktree_path] [repo]" >&2; exit 2; }
shift || true

# tmux reads "." and ":" as target separators, and a branch's own "/" would blur
# the wt/<repo>/<branch> shape, so flatten all of them inside the branch part.
session_name() {
  local branch=$1 repo=${2:-}
  branch=${branch//[.:\/ ]/-}
  repo=${repo//[.:\/ ]/-}
  printf '%s\n' "wt/${repo:+$repo/}$branch"
}

have_tmux() { command -v tmux >/dev/null 2>&1; }

# `=name` forces an exact match instead of a prefix match.
has_session() { tmux has-session -t "=$1" 2>/dev/null; }

case $cmd in
  name)
    session_name "${1:?branch required}" "${2:-}"
    ;;

  start)
    branch=${1:?branch required}
    path=${2:?worktree_path required}
    name=$(session_name "$branch" "${3:-}")
    have_tmux || { echo "wt-tmux: tmux not installed, skipping session for $branch" >&2; exit 0; }
    if has_session "$name"; then
      echo "wt-tmux: session $name already exists"
      exit 0
    fi
    # new-session daemonizes the tmux server, so the session outlives this hook.
    tmux new-session -d -s "$name" -c "$path"
    tmux set-environment -t "=$name" WT_BRANCH "$branch"
    tmux set-environment -t "=$name" WT_WORKTREE_PATH "$path"
    echo "wt-tmux: created session $name at $path"
    ;;

  attach)
    branch=${1:?branch required}
    name=$(session_name "$branch" "${3:-}")
    have_tmux || { echo "wt-tmux: tmux not installed" >&2; exit 1; }
    if ! has_session "$name"; then
      if [ -n "${2:-}" ]; then
        "$0" start "$branch" "$2" "${3:-}"
      else
        echo "wt-tmux: no session $name; create the worktree with wt switch first" >&2
        exit 1
      fi
    fi
    if [ -n "${TMUX:-}" ]; then
      tmux switch-client -t "=$name"
    else
      tmux attach-session -t "=$name"
    fi
    ;;

  stop)
    branch=${1:?branch required}
    name=$(session_name "$branch" "${2:-}")
    have_tmux || exit 0
    has_session "$name" || { echo "wt-tmux: no session $name"; exit 0; }
    # Killing the session the caller is sitting in would kill the caller — and
    # with it the `wt remove` that asked for the cleanup. Move the client first.
    if [ -n "${TMUX:-}" ] && [ "$(tmux display-message -p '#S')" = "$name" ]; then
      other=$(tmux list-sessions -F '#S' | grep -vxF "$name" | head -1 || true)
      if [ -z "$other" ]; then
        echo "wt-tmux: $name is the only session and you are in it; kill it yourself once you leave" >&2
        exit 0
      fi
      tmux switch-client -t "=$other"
    fi
    tmux kill-session -t "=$name"
    echo "wt-tmux: killed session $name"
    ;;

  list)
    have_tmux || exit 0
    tmux list-sessions -F '#S' 2>/dev/null | grep '^wt/' || true
    ;;

  *)
    echo "wt-tmux: unknown command '$cmd'" >&2
    exit 2
    ;;
esac
