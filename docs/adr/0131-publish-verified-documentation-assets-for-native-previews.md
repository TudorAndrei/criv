---
id: ADR-0131
kind: decision
title: Publish Verified Documentation Assets for Native Previews
status: accepted
date: 2026-08-21
governs:
  - Cargo.toml
  - src/discovery/mod.rs
  - src/vault.rs
  - src/state.rs
  - crates/criv-state-wire/src/lib.rs
  - crates/criv-wasm/src/**/*.rs
  - .obsidian/plugins/criv/src/**
  - extensions/vscode-criv/src/**
---

# Publish Verified Documentation Assets for Native Previews

## Context

The editor companions can preview Source text and validated LikeC4 models.
They cannot use published State to find non-source documentation assets.
[[dependency-evaluations]] defers magic-number detection until this preview
contract exists.

Source discovery deliberately rejects binary files. Adding images or documents
to Source would mix two different identities and would make parser and query
rules depend on editor display needs.

An asset preview also has a different trust boundary from a Source preview.
File extensions alone do not verify content. SVG, HTML, and other active
formats can run code in a browser surface. Large assets can use too much memory
or make a State refresh slow. State must identify a local file without
embedding its bytes.

## Decision

Add one documentation asset inventory that is separate from Source. Discover
assets only below the configured docs directory. Keep the same no-link,
repository-relative path rules that vault discovery and `RepositoryFiles`
already enforce.

### Supported files

Support these passive preview formats:

- PNG as `image/png`;
- JPEG as `image/jpeg`;
- GIF as `image/gif`;
- WebP as `image/webp`; and
- PDF as `application/pdf`.

Use `infer` to verify the file signature. The detected MIME type must agree
with the file extension. Treat `.jpg` and `.jpeg` as the same JPEG class.

Do not inventory SVG, HTML, XML, audio, video, fonts, archives, office files,
or unknown binary data. SVG and web documents are active content. This
decision does not add a sanitizer or an active-content sandbox.

### Bounds and failures

An indexed asset must not be larger than 8 MiB. The sum of indexed asset sizes
must not be larger than 64 MiB for one State revision. Process candidate paths
in lexical order. Skip an unsupported file, a signature mismatch, or an
individual oversized file. Stop adding entries when the total bound would be
exceeded. These exclusions are not validation errors because an unsupported
documentation file remains a valid repository file.

A link, non-regular file, replacement race, or read error keeps the current
vault load and State refresh failure behavior. A failed refresh does not
replace the last good State. An editor that cannot open a published local file
shows an error and does not keep a valid-looking old preview.

### State contract

Add an optional `asset-index` collection to `criv.state.v1`. Omit it when it
is empty. Each entry contains:

- the normalized repository-relative path;
- the verified MIME type;
- the byte size; and
- a BLAKE3 content hash.

The path is the stable asset identity. The hash changes the State revision
when same-size content changes and gives editor surfaces a cache key. State
does not contain asset bytes, data URLs, excerpts, thumbnails, file-system
URLs, or editor-specific fields.

This is a backward-compatible addition. Existing consumers ignore the new
field. New consumers default a missing field to an empty inventory. Keep the
schema identity `criv.state.v1`. The empty repository wire shape stays
unchanged because an empty asset index is not serialized.

Wasm validates safe paths once and prepares the complete asset list with the
other initial projections. TypeScript adapters do not parse raw State or
rebuild MIME, size, hash, or path meaning.

### Editor behavior

Both companions show one Documentation Assets section from the active loaded
State revision.

Obsidian uses its vault resource API for lazy image thumbnails and its native
workspace file view for a selected image or PDF. VS Code uses its native
`vscode.open` path for a selected asset. The hosts resolve only an asset path
that exists in the active prepared inventory. They normalize the path again
before local file access and keep it below the open vault or workspace root.

The companions do not copy, upload, decode, transform, or persist asset bytes.
They do not add a webview, remote URL, file watcher, or preview cache. State
replacement supplies the new hash and inventory.

### Tests

The CLI tests verify supported signatures, extension agreement, path order,
same-size content hashes, individual and total bounds, and no Source entries
for assets. Shared wire and Wasm tests verify missing and populated
`asset-index` data, safe-path filtering, stable ordering, and initial
projection output.

Obsidian tests verify passive thumbnail creation, native open behavior, stale
preview removal, and visible read failures. VS Code tests verify the State tree
section, exact active-inventory authorization, path confinement, and native
open command.

## Consequences

Documentation images and PDFs become visible from the same validated State
revision as Source and architecture data. Source parsing and Source identity
remain unchanged. State size increases only by small metadata rows.

The CLI reads each supported, bounded asset to compute its content hash. The
64 MiB total bound limits this work. Passive formats have no custom renderer,
so host security and accessibility behavior stay with the editor.

Unsupported formats do not appear in the inventory. A later format needs a
new decision with its MIME verification, safety, size, and host display rules.
