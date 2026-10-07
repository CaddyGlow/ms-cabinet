# Release status — 2026-10-07

All four findings from the pre-release code review are fixed. Linux host tests,
Clippy, deterministic fuzz regressions, source-layout packaging rehearsal and
an instrumented 10,000-iteration-per-target fuzz smoke campaign pass. This is a
validated local release candidate; remote publishing and platform gates remain.

## Review findings

| Finding | Status | Evidence |
| --- | --- | --- |
| Reused folder ranges expand descriptors before rejection | Fixed | Aggregate block preflight tied to physical bytes, disjoint range validation; 1,000 and 50,000 folder regressions |
| Spanning corpus contains only rejected partial sets | Fixed | Explicit framing accepts up to 64 parts; complete five-volume seed must assemble and decode all six members |
| Writer fuzzing misses Quantum windows and frame boundaries | Fixed | Independent configuration bytes reach 93 codec/window/level combinations; payloads up to 131073 bytes and two members |
| Dependency ref can drift between release jobs | Fixed | One resolver SHA feeds Linux/Windows validation, source archive and all binary jobs; each artifact records its commits |

Linux all-feature reader/writer/command/doc tests pass, including required
independent 7-Zip interoperability. The reader suite now includes three new
resource/range regressions. The fuzz regression suite has five tests covering
all 23 retained fixture files, positive complete-set decoding, malformed framing,
all writer configurations and frame/member boundaries. Both projects pass
Clippy with warnings denied, rustfmt, workflow lint and shell lint.

## Benchmark comparison

[Matching before/after results](release-review-20261007/benchmark.md) retain
three paired runs, all raw CSVs and source hashes. The 17,036-byte repeated-range
case drops from 1,001,001 reads and about 18 ms to 1,001 reads and about 18 µs.
All 18 codec archive sizes are unchanged. The local codec rates show no material
regression; short timings and shared-host noise limit performance conclusions.

## Current fuzz campaign

Each of `cab`, `spanning` and `roundtrip` completed 10,001 reported iterations
(requested 10,000) with zero crashes and zero timeouts using honggfuzz 0.5.62 and
Rust 1.99. [Summaries and seed hashes](release-review-20261007/fuzz/summaries.txt)
are retained with toolchain, seeding receipts and arguments. Full raw logs,
mutated corpora and workspaces remain at
`/data/cache/cabinet-release-review-20261007/fuzz-after-10000`.
This smoke run does not replace sustained fuzz campaigns.

## Release and remaining gates

Release CI resolves ms-compress once, validates Linux and Windows at that SHA,
checks the version tag, packages sources, tests the extracted source layout,
and builds/tests Linux x86_64, Windows x86_64 and macOS ARM64 outputs. Source and
binary artifacts retain commit provenance and license notices.

The local source-layout rehearsal uses working-tree content hashes because the
local ms-compress repository has no commits. It is not a published Git source
release. Before an actual release, the dependency must have an accessible
committed revision and the hosted tag workflow must pass. That workflow, native
Windows/macOS execution, Windows FDI interoperability and sustained fuzz
campaigns were not run here. No tag or release was published.

Historical fixtures, license notices, original benchmark data and the earlier
fuzz receipt remain preserved. Their claims apply to their original runs; the
updated fuzz guide explains the old corpus coverage limitations.
