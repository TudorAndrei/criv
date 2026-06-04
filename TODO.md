# TODOs

## Obsidian Plugin

- Done: source previews use explicit syntax-highlighted token rendering in hover
  cards and the source panel.
- Done: frontmatter pattern targets render inspectable match lists from
  `.criv/state.json`, including file, range, and captures.
- Done: source autocomplete ranks by match quality and frecency, including
  partial-path and basename matches.
- Done: editor-mode drift decorations mark unresolved source and `match:` links.
- Done: TypeScript fixture tests reuse
  `.obsidian/plugins/criv/fixtures/link-resolution.json`.

## Release And Hardening

- Done: local release gates run cleanly with `cargo test --workspace`,
  `cargo fmt --check`, `target/debug/criv check`, and
  `target/debug/criv enforce --stage ci`.
- Done: the full mise hook suite runs cleanly with `mise run check`, including
  cargo fmt, clippy, workspace tests, actionlint, zizmor, `criv check`, and
  CI-stage enforcement.
- Done: the next release policy is git-tag-only; crates.io publishing remains
  deferred until CLI API, state schema, and installer policy are stable.
- Done: `mise run perf` measures watch, source-index, check, enforcement, and
  diff timings through `scripts/measure-performance.sh`.
- Done: the release workflow has been hardened after the failed `v0.1.1` and
  `v0.2.0` tag-triggered workflow conclusions by removing the Intel macOS job,
  pinning Actions to full SHAs, and uploading only archive/checksum assets.
- Done: the hardened GitHub Actions binary workflow was verified by the
  `v0.3.0` tag-triggered run, which completed successfully and published
  `criv-aarch64-apple-darwin.tar.gz`,
  `criv-aarch64-unknown-linux-gnu.tar.gz`,
  `criv-x86_64-unknown-linux-gnu.tar.gz`,
  `criv-x86_64-pc-windows-msvc.zip`, and `SHA256SUMS.txt`.
