---
id: ADR-0081
kind: decision
title: Require Material State Store Performance Gains
status: accepted
date: 2026-08-06
governs:
  - scripts/performance/**
  - src/state.rs
  - src/snapshots.rs
  - src/query.rs
  - crates/criv-wasm/src/lib.rs
---

# Require Material State Store Performance Gains

## Context

[[0007-content-addressed-state-and-diffing|ADR-0007]] and
[[0068-bounded-local-snapshot-lifecycle|ADR-0068]] make JSON the current
machine State and snapshot representation. Replacing it changes content
identity, publication, recovery, native queries, and the packaged editor path.
A smaller file alone does not justify that migration risk.

[[0069-repeatable-two-tier-performance-evidence|ADR-0069]] and
[[0072-keep-performance-observation-outside-core|ADR-0072]] define the two
approved workloads and the measurement method. The storage baseline measured
`barrs-small` and `criv-medium`. It found that one semantic source change still
publishes complete State payloads. It also found that the packaged Wasm path
parses the complete State for each projection, lookup, and selector operation.

No product telemetry gives operation counts. Code paths show three useful
frequency classes:

- complete and changed publication, editor cold load, and first projections
  are common;
- node lookup and selector search repeat during editor use;
- State diff and snapshot list are rare explicit commands.

The approved workloads contain no published architecture payload. They cannot
set an architecture-payload performance limit, and the medium workload must
not be treated as a large workload.

## Decision

Use matched comparisons against the JSON baseline to select a machine State
store. A matched comparison has the same workload, operation, cache state,
release profile, sample count, harness revision, and machine identity. Use the
median of at least five successful samples. Preserve the raw samples and median
absolute deviation. The ratios below are the contract. The absolute values are
reference values for the issue 89 machine, not portable machine limits.

### Hard selection gates

A candidate must meet every hard gate on both approved workloads:

1. Its complete authoritative stored State, including required indexes, is at
   most 60% of the pretty JSON State bytes.
2. Its complete retained twenty-snapshot store, including shared partitions,
   manifests, and indexes, is at most 60% of the JSON snapshot bytes.
3. One semantic source change creates or replaces at most 20% of the two JSON
   State payloads written for the latest State and new snapshot. Count every
   byte created or replaced in the candidate State store. Exclude the separate
   source-graph cache from both sides.
4. The medium first-projection median after module and State load is at most
   40% of the JSON median.
5. Every other required elapsed-time median is at most 110% of its matched JSON
   median. One fast operation cannot compensate for another slow operation.
6. Native and Wasm peak memory are each at most 110% of the matched JSON peak.
   Use one reliable external method for both sides. A missing native peak-memory
   value is not a pass.

The stored-byte, changed-publication-byte, and medium first-projection gates are
the minimum gains that justify a format and migration change. If no candidate
passes every gate, keep JSON and do not select a replacement.

Correctness remains a gate before performance. A candidate cannot pass if it
fails deterministic identity, schema-version, truncated-data, corrupt-data,
interrupted-publication, offline-package, native, or Wasm requirements.

### Native reference limits

Times are median milliseconds. Goals guide comparisons but do not select a
candidate by themselves.

| Operation | Small JSON | Small hard maximum | Small goal | Medium JSON | Medium hard maximum | Medium goal |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Complete publication | 64.067 | 70.474 | 48.050 | 223.880 | 246.268 | 167.910 |
| One-source changed publication | 53.220 | 58.542 | 26.610 | 213.256 | 234.582 | 106.628 |
| Latest-State load and validation | 0.605 | 0.666 | 0.303 | 5.962 | 6.558 | 2.981 |
| Two-State diff with one added symbol | 8.234 | 9.057 | 4.117 | 35.658 | 39.224 | 17.829 |
| List twenty snapshots | 22.727 | 25.000 | 11.364 | 156.007 | 171.608 | 78.004 |

Complete-publication goals are 75% of JSON. Changed publication, load, diff,
and list goals are 50% of JSON.

### Packaged Wasm reference limits

Times are median milliseconds. Each operation uses the packaged Wasm path.

| Operation | Small JSON | Small hard maximum | Small goal | Medium JSON | Medium hard maximum | Medium goal |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Cold module, State, and four projections | 16.603 | 18.263 | 9.962 | 70.710 | 77.781 | 42.426 |
| Four projections after load | 9.252 | 10.177 | 3.701 | 66.443 | 26.577 | 26.577 |
| Existing node lookup | 1.487 | 1.636 | 0.372 | 6.613 | 7.274 | 1.653 |
| Missing node lookup | 1.707 | 1.878 | 0.427 | 6.650 | 7.315 | 1.663 |
| Empty selector query | 2.231 | 2.454 | 1.115 | 7.218 | 7.940 | 3.609 |
| Exact selector query | 2.519 | 2.771 | 1.260 | 7.659 | 8.425 | 3.829 |
| Suffix selector query | 2.099 | 2.309 | 1.050 | 8.236 | 9.060 | 4.118 |
| Missing selector query | 1.945 | 2.140 | 0.973 | 8.216 | 9.038 | 4.108 |

Cold-load goals are 60% of JSON. First-projection goals are 40%. Lookup goals
are 25% because the selected layout must supply a node index and reuse prepared
data. Selector goals are 50% because the selected layout must reuse a prepared
selector index.

### Storage and memory reference limits

Bytes are exact except where the table labels process memory in megabytes. One
changed JSON publication has two logical State payloads: the latest State and
the new snapshot. This comparison gives JSON no charge for its pointer and
snapshot-index metadata, but counts all candidate store metadata.

| Metric | Small JSON | Small hard maximum | Small goal | Medium JSON | Medium hard maximum | Medium goal |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| One stored State, bytes | 296,039 | 177,623 | 148,019 | 2,435,275 | 1,461,165 | 1,217,637 |
| Twenty snapshots, bytes | 6,028,898 | 3,617,338 | 1,507,224 | 48,813,618 | 29,288,170 | 12,203,404 |
| One-source changed publication, bytes | 592,078 | 118,415 | 59,207 | 4,870,550 | 974,110 | 487,055 |
| Wasm cold process peak, MB | 74.0 | 81.4 | 59.2 | 98.0 | 107.8 | 78.4 |
| Wasm projection process peak, MB | 74.6 | 82.0 | 59.7 | 98.8 | 108.7 | 79.0 |

Stored-State goals are 50% of JSON. The retained-snapshot goal is 25% because
immutable partitions should be shared between close revisions. The changed
publication goal is 10%. Peak-memory goals are 80%.

Issue 89 has no reliable native peak-memory number. The candidate benchmark
must collect a new matched JSON value and candidate value. Its hard limit is
110% and its comparison goal is 80% of that matched JSON value.

### Result use

Report every hard result separately for each workload. Do not average
workloads or operations into one score. Report goals after hard-gate results so
a useful comparison does not hide a failed gate.

The benchmark may use absolute values from the tables only when its evidence
identity matches issue 89. On another machine, calculate the hard maximum and
goal from a new matched JSON run with the ratios above.

The benchmark must measure the architecture partition for validity, version,
and recovery, but it must mark its performance result as unsupported by the
approved workloads. A future observed workload with an architecture payload
requires a new decision before it adds a selection limit.

## Consequences

The format benchmark has numeric pass and fail rules. A compact codec cannot
win while it keeps complete changed writes or repeated Wasm parsing. A fast
point lookup cannot hide slow publication, excess memory, or a large retained
store.

The contract is strict. A candidate can show useful comparison goals and still
fail selection. In that case criv keeps JSON until a design meets the material
gains or a later ADR changes the contract with new evidence.

Absolute times remain machine-specific. Each selection run must pay the cost
of a matched JSON baseline, but this keeps hardware and toolchain changes from
changing the product decision.
