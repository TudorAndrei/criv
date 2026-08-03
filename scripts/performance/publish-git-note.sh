#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: publish-git-note.sh NOTE_FILE COMMIT REMOTE" >&2
  exit 2
fi

note_file="$1"
commit="$2"
remote="$3"
notes_ref="refs/notes/criv-performance"
max_attempts=5

jq -e '.schema == "criv.performance-git-note.v1"' "$note_file" >/dev/null
git cat-file -e "$commit^{commit}"

refresh_notes_ref() {
  local status
  set +e
  git ls-remote --exit-code --refs "$remote" "$notes_ref" >/dev/null
  status=$?
  set -e

  case "$status" in
    0)
      git fetch --force "$remote" "$notes_ref:$notes_ref"
      ;;
    2)
      git update-ref -d "$notes_ref"
      ;;
    *)
      echo "failed to inspect $notes_ref on $remote" >&2
      return "$status"
      ;;
  esac
}

for attempt in $(seq 1 "$max_attempts"); do
  refresh_notes_ref
  git notes --ref="$notes_ref" add --force --file "$note_file" "$commit"
  if git push "$remote" "$notes_ref:$notes_ref"; then
    echo "published performance note for $commit to $notes_ref"
    exit 0
  fi
  if [[ "$attempt" -lt "$max_attempts" ]]; then
    echo "notes ref changed concurrently; retrying ($attempt/$max_attempts)" >&2
    sleep "$attempt"
  fi
done

echo "failed to publish performance note after $max_attempts attempts" >&2
exit 1
