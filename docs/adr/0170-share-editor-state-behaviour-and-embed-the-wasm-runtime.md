---
id: ADR-0170
kind: decision
title: Share editor State behaviour and embed the Wasm runtime
status: accepted
date: 2026-11-06
supersedes:
  - ADR-0099
governs:
  - packages/criv-editor-state/src/*.ts
  - packages/criv-editor-state/esbuild.mjs
  - crates/criv-wasm/src/decode.rs
  - extensions/vscode-criv/src/**/*.ts
  - extensions/vscode-criv/esbuild.config.mjs
  - .obsidian/plugins/criv/src/**/*.ts
  - .obsidian/plugins/criv/esbuild.config.mjs
  - fixtures/link-resolution.json
---

# Share Editor State Behaviour and Embed the Wasm Runtime

## Context

[[0099-enforce-shared-likec4-and-wasm-adapter-boundary|ADR-0099]] kept State
row types, the status stream, and the generated Wasm import out of
`@criv/editor-state`. Each editor package therefore had its own copy of the
row types, the status machine, the Wasm adapter, the safe-path check, the
line-fragment parser, the target-prefix rule, and the ambiguous-target
message. The copies did not agree. VS Code accepted `#L4-8` and stripped
`code:`, `symbol:`, and `note:`; Obsidian accepted `#l4` and rejected
`match:` targets. Neither rule was the CLI rule.

Both companions also loaded the Wasm runtime by a relative import,
`../../pkg/criv_wasm.js`, that the bundler kept unchanged. From
`dist/extension.js` that path names `extensions/pkg`, and from the Obsidian
`main.js` it names `.obsidian/pkg`. Neither directory exists, so the packaged
companions reported an unavailable runtime. Obsidian built the `bundler`
Wasm target, which needs a bundler that understands Wasm imports; the
Obsidian app does not supply one.

The Wasm crate signalled State errors with a text prefix such as
`criv-state-json-invalid:` that the TypeScript side parsed out of the message.

## Decision

`@criv/editor-state` is the single owner of the editor-neutral State
behaviour:

- the Wasm contract row types and the stable error codes;
- the State module factory, `createCrivStateModule`, which loads the Wasm
  module once, caches a load failure, and adapts one loaded revision;
- the status stream, `CrivStateStream`, which owns the loaded revision, the
  monotonic generation, the listeners, and the mapping from an error to the
  `missing`, `invalid`, or `unavailable` status;
- the source-reference rules: `safeVaultPath`, `parseLineFragment`,
  `sourceTargetParts`, `patternTargetId`, `resolveSourceTarget`, and
  `ambiguousSourceTargetMessage`, with `formatBytes`, `errorMessage`, and
  `isRecord`.

The source-reference rules follow the CLI link grammar in `src/vault.rs`. A
line fragment is `L<n>` or `L<n>-L<m>` with `n > 0`. Any other fragment is a
symbol selector. `source:` is the only prefix that a source target drops.
`match:<id>` and `<note>#match:<id>` are pattern links, never source targets.

Each editor package owns only its host: how it finds and reads the State
file, how it watches it, its commands, views, settings, and messages. It
implements `CrivStateHost` and passes it to `CrivStateStream`. Hosts do not
keep copies of the package rules.

Build the Wasm crate once, with the `web` target, into
`packages/criv-editor-state/pkg`. The `build:wasm` script calls `wasm-pack`
through `mise exec`, so `mise.toml` is the only version pin. Both companions
bundle `@criv/editor-state/packaged` with `crivWasmBytesPlugin`, which embeds
the Wasm bytes in the bundle. A bundle has no file dependency on its install
directory. A direct package test bundles the packaged loader outside the
package and loads the State fixture through it.

`LoadedState` rejects a State document with a JavaScript `Error` that has a
`code` property. The message does not repeat the code.

The Obsidian `ports.ts` keeps the narrow `StatePort` query interface. The
State status value is the package type. The Obsidian lifecycle-owner layout and
import direction from
[[0107-group-obsidian-host-files-by-owner-scope|ADR-0107]] are unchanged.

`fixtures/link-resolution.json` is the one link-resolution fixture. Its
`state` is the State that `criv watch --once` writes for the vault that the
Rust fixture test builds. The Obsidian tests resolve its source and pattern
cases through the embedded Wasm runtime.

All three TypeScript packages extend `tsconfig.base.json`. Both companions
use the same script names: `build:wasm`, `build`, `check-types`, `dev`,
`test`, `lint`, `format`, and `format:check`. Each builds its tests with one
`scripts/build-tests.mjs` harness and runs them with `node --test`.

From ADR-0099, retain the stable codes for invalid JSON, unsupported schema,
unavailable Wasm, and use after disposal; State validation and projection in
`criv-wasm`; exact revision disposal; and the `check:editor-contracts` gate.
The LikeC4 rules of ADR-0099 are retired by
[[0155-remove-likec4-from-criv|ADR-0155]].

## Consequences

The packaged VS Code extension and the Obsidian plugin load the Wasm runtime
from their own bundle. Both editors apply the same target rules as the CLI.
A rule change is made once in the package and is visible in both editors.

Targets that the editors used to accept, such as `#l4`, `#L4-8`, and
`code:src/lib.rs`, are now symbol selectors or plain paths. Such a target
usually does not resolve.

The shared fixture shows that the Wasm lookup does not yet resolve a basename
with a symbol fragment, such as `helper.rs#help`, which the CLI resolves. The
Obsidian test reports those cases as known gaps until the Wasm lookup
changes.

The bundles grow by the size of the Wasm module. The VSIX and the plugin no
longer ship a separate `pkg/` directory.
