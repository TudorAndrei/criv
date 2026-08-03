#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "$0")/.." && pwd)"
test_root="$(mktemp -d)"
trap 'rm -rf "$test_root"' EXIT
source_repository="$test_root/source"
remote_repository="$test_root/remote.git"

git init -q -b main "$source_repository"
git -C "$source_repository" config user.email performance@criv.invalid
git -C "$source_repository" config user.name "criv performance test"
printf 'fixture\n' >"$source_repository/fixture.txt"
git -C "$source_repository" add fixture.txt
git -C "$source_repository" commit -q -m fixture
commit="$(git -C "$source_repository" rev-parse HEAD)"
git clone -q --bare "$source_repository" "$remote_repository"

note_file="$test_root/note.json"
printf '{"schema":"criv.performance-git-note.v1","generation":1}\n' >"$note_file"
(
  cd "$source_repository"
  "$repository_root/scripts/performance/publish-git-note.sh" \
    "$note_file" "$commit" "$remote_repository"
)

git -C "$source_repository" fetch -q --force "$remote_repository" \
  refs/notes/criv-performance:refs/notes/criv-performance
test "$(git -C "$source_repository" notes --ref=criv-performance show "$commit" | jq -r .generation)" = "1"

printf '{"schema":"criv.performance-git-note.v1","generation":2}\n' >"$note_file"
(
  cd "$source_repository"
  "$repository_root/scripts/performance/publish-git-note.sh" \
    "$note_file" "$commit" "$remote_repository"
)
git -C "$source_repository" fetch -q --force "$remote_repository" \
  refs/notes/criv-performance:refs/notes/criv-performance
test "$(git -C "$source_repository" notes --ref=criv-performance show "$commit" | jq -r .generation)" = "2"

echo "performance git note publication: ok"
