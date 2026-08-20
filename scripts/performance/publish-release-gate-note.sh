#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "usage: publish-release-gate-note.sh RECEIPT COMMIT REMOTE" >&2
  exit 2
fi

receipt="$1"
commit="$2"
remote="$3"
notes_ref="refs/notes/criv-release-gates"
max_attempts=5

jq -e --arg commit "$commit" '
  .schema == "criv.discovery-release-gate.v2" and
  .passed == true and
  .commit == $commit and
  (.artifacts | length == 4)
' "$receipt" >/dev/null
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
  git notes --ref="$notes_ref" add --force --file "$receipt" "$commit"
  if git push "$remote" "$notes_ref:$notes_ref"; then
    echo "published release-gate receipt for $commit to $notes_ref"
    exit 0
  fi
  if [[ "$attempt" -lt "$max_attempts" ]]; then
    echo "notes ref changed concurrently; retrying ($attempt/$max_attempts)" >&2
    sleep "$attempt"
  fi
done

echo "failed to publish release-gate receipt after $max_attempts attempts" >&2
exit 1
