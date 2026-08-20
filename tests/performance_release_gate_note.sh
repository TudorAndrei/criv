#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "$0")/.." && pwd)"
test_root="$(mktemp -d)"
trap 'rm -rf "$test_root"' EXIT
source_repository="$test_root/source"
remote_repository="$test_root/remote.git"

git init -q -b main "$source_repository"
git -C "$source_repository" config user.email performance@criv.invalid
git -C "$source_repository" config user.name "criv release gate test"
printf 'fixture\n' >"$source_repository/fixture.txt"
git -C "$source_repository" add fixture.txt
git -C "$source_repository" commit -q -m fixture
commit="$(git -C "$source_repository" rev-parse HEAD)"
git clone -q --bare "$source_repository" "$remote_repository"

receipt="$test_root/receipt.json"
jq -n --arg commit "$commit" '{
  schema: "criv.discovery-release-gate.v2",
  commit: $commit,
  passed: true,
  artifacts: [
    {target:"aarch64-apple-darwin"},
    {target:"aarch64-unknown-linux-gnu"},
    {target:"x86_64-pc-windows-msvc"},
    {target:"x86_64-unknown-linux-gnu"}
  ]
}' >"$receipt"

(
  cd "$source_repository"
  "$repository_root/scripts/performance/publish-release-gate-note.sh" \
    "$receipt" "$commit" "$remote_repository"
)

git -C "$source_repository" fetch -q --force "$remote_repository" \
  refs/notes/criv-release-gates:refs/notes/criv-release-gates
test "$(git -C "$source_repository" notes --ref=criv-release-gates show "$commit" | jq -r .commit)" = "$commit"

echo "release-gate git note publication: ok"
