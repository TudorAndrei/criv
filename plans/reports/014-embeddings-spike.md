# Plan 014 Embeddings Feature Spike

Date: 2026-07-06

## Scope

This spike measured the hidden `embeddings` Cargo feature without changing
production code. The question was whether criv should ship an
embeddings-enabled release artifact, keep the feature as a source-only opt-in,
or retire the semantic note search path.

ADR-0008 says semantic note search is optional because embedding dependencies
are heavy and should not be forced into every default build. The live
implementation still follows that design: a user must have all of these in
place to reach semantic note search today:

- build criv with `--features embeddings`
- set `[index] embeddings = true` in `criv.toml`
- run `criv search --notes "<query>" --semantic`

Default builds fail before the feature path with
`semantic note search requires building criv with --features embeddings` when
the runtime flag is enabled. Builds where `index.embeddings = false` fail first
with `semantic note search requires index.embeddings = true in criv.toml`.

## Measurements

Commands were run on this workspace at the current dirty working tree after
plan 010 removed criv's direct `git2` dependency. The release workflow still
uses `cargo build --locked --release --target "${{ matrix.target }}" --bin criv`
with no feature flag.

| Measurement | Default build | `--features embeddings` | Delta / note |
|-------------|---------------|-------------------------|--------------|
| Release build command | `cargo build --release --bin criv` | `cargo build --release --features embeddings --bin criv` | Embeddings build first failed in the sandbox because `ort-sys` needed to download ONNX Runtime; rerun with network approval succeeded. |
| Warm release build wall time | 29.35s | 38.70s | +9.35s, about +31.9%. |
| Release binary size | 12,332,560 bytes | 30,482,144 bytes | +18,149,584 bytes, about +147.2%; the embeddings binary is about 2.47x the default binary. |
| Feature tests | n/a | `cargo test --workspace --features embeddings` passed | 140 lib tests, 26 CLI workflow tests, 10 wasm tests, doc tests all passed. |
| Duplicate dependency report | `cargo tree --features embeddings -d` | 523 lines | New material stack includes `fastembed`, `hf-hub`, `ort`, `ort-sys`, `tokenizers`, `ndarray`, and `ureq`; duplicate versions include `base64`, `dirs`, `getrandom`, `hashbrown`, `notify`, `phf`, `rand`, `rand_core`, and `winnow`. |
| Native build footprint | n/a | `target/release/build/ort-sys-*` totals about 8.6 MB plus metadata | This is build output, not distributed artifact size. |
| Runtime model cache | n/a | `.criv/embeddings` was 97 MB after first successful query | Cache path was `models--Qdrant--all-MiniLM-L6-v2-onnx`. |
| Cold semantic search | n/a | 4.64s on a 20-note scratch vault | Included first model retrieval after network approval. |
| Warm semantic search | n/a | 0.13s on the same 20-note scratch vault | Re-embeds all notes on each invocation; acceptable in the tiny corpus, but this was not a large-vault benchmark. |
| Warm lexical search | effectively 0.00s | n/a | Same query on the same corpus. |
| Offline / blocked-network behavior | n/a | immediate failure: `failed to initialize fastembed: Failed to retrieve model.onnx` | This happened before network approval with an empty cache. |

Quality probes on the scratch corpus were mixed. Semantic search beat lexical
search for paraphrases where exact terms were absent, such as `pipeline points
at line numbers`, which correctly ranked the CI diagnostics note first while
lexical returned no rows. It also produced relevant results for `no internet on
airplane` and `native transformer dependency`. Some broad queries misranked
results, for example `small command line binary` ranked the query-reference
note above the release-artifacts/build-profile notes. This is enough signal to
say the feature has user value, but not enough to justify a release expansion
without larger-vault benchmarks.

## Options

### A: Ship Embeddings-Enabled Release Artifacts

Add a second release variant to the existing four-target matrix, probably named
with an `-embeddings` suffix for `x86_64-unknown-linux-gnu`,
`aarch64-unknown-linux-gnu`, `aarch64-apple-darwin`, and
`x86_64-pc-windows-msvc`. The workflow change is mechanically small: add a
feature dimension or a second build/package step that runs
`cargo build --locked --release --features embeddings --target ... --bin criv`
and publishes separate archives.

The cost is disproportionate today. The measured macOS arm64 binary grew by
18.1 MB and 147.2%, and first use still needs a 97 MB model cache download. The
release matrix would also inherit ONNX Runtime/native-download risk on every
platform, which is exactly the kind of release complexity ADR-0008 tried to keep
out of the default path. Shipping the variant would make the feature reachable,
but it would not make semantic search self-contained or offline-friendly.

### B: Keep Source-Only Opt-In and Document It Honestly

Leave the default feature set and release workflow unchanged. Document the real
activation sequence in the README/search docs: install/build with
`cargo install --git ... --features embeddings` or `cargo build --release
--features embeddings`, set `index.embeddings = true`, then run
`criv search --notes ... --semantic`. Also document first-run model download,
the `.criv/embeddings` cache, and the offline failure mode. A small follow-up
could improve the CLI hint so users who pass `--semantic` see both required
gates clearly.

This keeps ADR-0008's intent intact: default builds remain small and
deterministic, while users who deliberately want local semantic retrieval can
enable it. It accepts that the feature remains niche and source-built for now,
but stops making it look like a normal release capability.

### C: Retire the Feature

Delete `semantic_notes`, remove the optional `fastembed` dependency and feature,
and write a new ADR superseding ADR-0008. Keep accepting the
`[index] embeddings` config key for at least one compatibility cycle or emit a
targeted deprecation warning, because existing vaults may already contain it.

This would reduce maintenance surface and avoid ONNX Runtime release concerns
entirely. The downside is that the feature does compile, tests pass, and the
scratch corpus showed useful paraphrase retrieval. Retiring it now would throw
away a working optional capability before the project has measured real vault
usage or tried clear opt-in documentation.

## Recommendation

Choose option B for the next release cycle: keep semantic note search as a
source-only opt-in, do not add release artifacts yet, and document the feature's
build flag, runtime flag, first-run model cache, and offline behavior.

The numbers are the reason. A 147.2% binary-size increase conflicts with the
size-optimized release profile and ADR-0008's explicit concern about heavy
embedding dependencies. The 97 MB runtime model cache means an
embeddings-enabled artifact would still surprise users with a network-dependent
first run. At the same time, the feature is not dead code: it compiles, passes
the feature test suite, and produces useful paraphrase matches on a small
corpus. Source-only opt-in preserves that value while keeping the default
release boring.

## Follow-Up ADR Outline

Title: Keep Semantic Note Search as Source-Only Opt-In

Status: proposed

Supersedes: ADR-0008 only if the project wants to refine the release stance;
otherwise it can reference ADR-0008 as reaffirmed by measurement.

Context:

- ADR-0008 made semantic search optional to avoid forcing heavy embedding
  dependencies into default builds.
- Current release artifacts do not enable the feature.
- Measurement found a 12,332,560 byte default binary and a 30,482,144 byte
  embeddings binary, plus a 97 MB first-run model cache.
- The feature works, but offline empty-cache behavior is a hard failure to
  retrieve `model.onnx`.

Decision:

- Keep `default = []` and do not add embeddings-enabled release artifacts.
- Document source-build activation and cache/offline behavior.
- Revisit release artifacts only after large-vault benchmarks and a deliberate
  distribution plan for the model/runtime dependency.

Consequences:

- Default releases stay compact and deterministic.
- Semantic note search remains available to users willing to build from source.
- The project carries the optional dependency tree deliberately, with clear
  documentation instead of accidental darkness.
