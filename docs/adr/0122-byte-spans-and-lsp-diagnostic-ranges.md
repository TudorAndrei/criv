---
id: ADR-0122
kind: decision
title: Use byte spans and LSP diagnostic ranges
status: accepted
date: 2026-08-20
governs:
  - src/check.rs
  - src/c4.rs
  - src/likec4.rs
  - src/policy_scan.rs
  - src/structural.rs
  - src/vault.rs
  - assets/likec4-bridge.mjs
  - extensions/vscode-criv/src/diagnostics/model.ts
  - extensions/vscode-criv/src/diagnostics/publisher.ts
---

# Use Byte Spans and LSP Diagnostic Ranges

## Context

`criv check` diagnostics identify a repository-relative path and an optional
one-based line. This is sufficient for text output, but the VS Code adapter
must mark the complete line. GitHub annotations cannot select the exact source
text either. The JSON format is an array of these diagnostics, and existing
consumers must continue to accept it.

Several producers already have better locations. `rumdl` reports start and end
lines and character columns. Structural policy matches have syntax-tree
ranges. The LikeC4 bridge returns a range, but the Rust bridge drops it. These
different producers do not use one common coordinate system.

The consumers also require different coordinates. Rust source renderers such
as `miette` use UTF-8 byte offsets and lengths. The VS Code API and Language
Server Protocol use zero-based UTF-16 positions. GitHub workflow annotations
use one-based lines and columns. One coordinate representation cannot be sent
to all three consumers without an explicit conversion boundary.

Diagnostic locations are command output. They are not State graph identity or
State rows. Source excerpts also duplicate repository content and can become
stale between the check and the consumer.

## Decision

The core diagnostic model uses an optional UTF-8 byte span. The start byte is
zero-based and inclusive. The end byte is zero-based and exclusive. Both
offsets are relative to the complete file contents. An empty span can identify
an insertion point. A producer must omit the span when it cannot prove exact
UTF-8 character boundaries. It must not guess.

Keep the existing optional `line` field as a one-based, file-relative
compatibility field. For a diagnostic with a span, this line identifies the
span start. Existing text output keeps its current path and line form.

Keep the JSON output as a top-level array and keep all existing fields. Add an
optional `range` field. Its `start` and `end` positions use zero-based lines and
zero-based UTF-16 code-unit characters, as defined by the Language Server
Protocol and used by VS Code. The start is inclusive and the end is exclusive.
This range can cross lines and can be empty. Old consumers can ignore the new
field. The VS Code adapter prefers it and keeps the current complete-line
fallback for a line-only diagnostic.

Do not publish byte offsets, excerpts, or source contents in the JSON range.
The JSON adapter derives the range from the validated byte span and the same
file contents that produced the diagnostic. Conversion between UTF-8 byte
offsets and UTF-16 positions has one tested implementation. It must cover
non-ASCII text, supplementary Unicode characters, CRLF input, multi-line
ranges, and empty ranges.

The GitHub adapter derives its one-based `line`, `col`, `endLine`, and
`endColumn` fields from the same validated span. It applies GitHub's endpoint
rules at this adapter boundary. A line-only diagnostic keeps the current
annotation form.

Preserve exact locations from `rumdl`, structural policy matches, LikeC4, and
vault parsing when the producer has enough source information. A producer that
has only a line continues to create a line-only diagnostic. Location support
can therefore migrate by producer without a flag day.

Do not add diagnostic spans to generated State and do not change the State
schema. Do not serialize excerpts. A future human renderer can read the
current file through the confined repository file interface and derive an
excerpt from the byte span.

Do not add `miette` as part of the span migration. Re-evaluate it only after
the main diagnostic producers preserve exact spans and a separate renderer
change shows that source excerpts improve the command output enough to justify
the dependency.

## Consequences

Existing scripts and editor versions keep the JSON array, current fields, and
one-based line behavior. New editor versions can select the exact source range.
GitHub annotations can point to exact columns when the producer supplies a
span. State readers and snapshots do not change.

The implementation must keep source text available long enough to validate
and convert producer coordinates. It must reject invalid or inconsistent
locations and use the line-only fallback. This adds conversion code, but it
keeps encoding rules out of individual producers and renderers.

Tests must cover old JSON without `range`, JSON with `range`, unknown additive
fields, Unicode before and inside a range, CRLF files, multi-line and empty
ranges, GitHub annotation conversion, VS Code publication, and unchanged State
serialization.

## Alternatives Considered

### Use UTF-16 positions in the core

Rejected. UTF-16 is the editor wire convention. Rust parsers and source
renderers work naturally with UTF-8 bytes, so a UTF-16 core would move editor
policy into every producer.

### Publish only byte offsets in JSON

Rejected. Every editor consumer would need to read and decode the file before
it could create a range. LSP positions are the established editor boundary.

### Store excerpts with each diagnostic

Rejected. Excerpts duplicate untrusted repository content, increase machine
output, and can be stale. A renderer can derive them from a validated span and
the current file.

### Add `miette` with the first span implementation

Rejected. The exact-range contract gives value to GitHub and VS Code without a
new renderer dependency. Renderer selection remains a separate decision.
