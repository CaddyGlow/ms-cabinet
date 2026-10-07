# Release review: before and after — 2026-10-07

Both revisions use Rust 1.99.0 on the same x86_64 Linux Intel Core i7-13700K
host, identical lockfiles and a frozen copy of the same ms-compress working tree.
The baseline is the reviewed working tree, including its existing Rust 1.99
configuration; it is not just the initial extraction commit. Source SHA-256
manifests and environment details are retained alongside this report.

Three alternating before/after rounds ran sequentially, with no concurrent test
or fuzz jobs during the timed comparison. Each codec round uses 1 MiB deterministic
repetitive, mixed and random corpora, one verified warm-up and ten timed writes
and ten timed reads. Tables report the median of three runs. Thin LTO, allocation,
codec initialization and header finalization are included; corpus creation,
equality checks and disk throughput are excluded. This is a local microbenchmark
on a shared machine, not a statistical performance guarantee.

```sh
cargo bench --bench cab --locked -- --iterations 10 --size 1048576 --csv /absolute/path/results.csv
cargo bench --bench parser --locked
```

## Malformed-input rejection

The parser benchmark creates valid stored folders and repeated physical block
ranges. Counts and input generation are identical in both revisions. Input
construction/cloning and result destruction are excluded from parse timing.
All valid cases must open, and both overlapping cases must be rejected.
`read_calls` counts source Read calls, providing a deterministic measure of the
work eliminated before rejection, independent of timing noise.

| Case | Input bytes | Read calls before → after | Parse µs before → after | Speedup |
| --- | ---: | ---: | ---: | ---: |
| valid-1x1000 | 9044 | 1002 → 1002 | 13.498 → 12.162 | 1.1× |
| valid-100x10 | 9836 | 1101 → 1101 | 19.898 → 17.941 | 1.1× |
| overlap-100x100 | 1736 | 10101 → 101 | 168.068 → 1.605 | 104.7× |
| overlap-1000x1000 | 17036 | 1001001 → 1001 | 18280.275 → 18.215 | 1003.6× |

The 17,036-byte attack previously expanded one physical block table into a
million descriptors. It now fails after reading the 1,000 folder headers,
before any block descriptor is allocated. A separate regression uses a
850,036-byte input declaring 50,000 reused folders and 2.5 billion descriptors;
it is checked only after the fix because running the baseline could exhaust
memory. Aggregate counts are bounded by physical input bytes; sorting folder
locations also rejects overlap during the first conflicting folder parse.

## Codec throughput

All 18 CAB output lengths are unchanged. The codec writer was not modified.
Most median throughput changes are within a few percent. Repetitive Quantum
reads show a larger improvement in this local run; these short measurements
alone do not establish its cause or guarantee the same gain on another host.

| Corpus | Method | CAB bytes | Write MiB/s before → after | Change | Read MiB/s before → after | Change |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| repetitive | stored | 1048904 | 16623.11 → 16694.32 | +0.43% | 15285.89 → 15633.43 | +2.27% |
| repetitive | mszip | 5800 | 605.49 → 622.68 | +2.84% | 2339.75 → 2323.80 | -0.68% |
| repetitive | lzx-15 | 15626 | 423.57 → 430.42 | +1.62% | 2439.20 → 2455.27 | +0.66% |
| repetitive | lzx-21 | 18186 | 411.44 → 419.79 | +2.03% | 2008.68 → 1996.52 | -0.61% |
| repetitive | quantum-1-18 | 2614 | 91.56 → 90.67 | -0.98% | 799.14 → 996.38 | +24.68% |
| repetitive | quantum-7-18 | 2614 | 93.33 → 92.47 | -0.92% | 798.86 → 997.25 | +24.83% |
| mixed | stored | 1048904 | 17139.31 → 17193.59 | +0.32% | 16020.45 → 16009.40 | -0.07% |
| mixed | mszip | 532255 | 135.84 → 134.42 | -1.05% | 593.06 → 593.68 | +0.10% |
| mixed | lzx-15 | 542752 | 121.03 → 118.64 | -1.97% | 510.70 → 506.58 | -0.81% |
| mixed | lzx-21 | 545312 | 117.67 → 118.43 | +0.65% | 486.37 → 489.45 | +0.63% |
| mixed | quantum-1-18 | 533770 | 11.00 → 11.02 | +0.15% | 21.15 → 21.00 | -0.71% |
| mixed | quantum-7-18 | 533770 | 10.74 → 10.77 | +0.21% | 21.16 → 21.12 | -0.17% |
| random | stored | 1048904 | 16571.30 → 17156.66 | +3.53% | 16007.86 → 16055.13 | +0.30% |
| random | mszip | 1049128 | 66.35 → 66.70 | +0.53% | 4356.31 → 4365.82 | +0.22% |
| random | lzx-15 | 1062250 | 64.85 → 64.95 | +0.15% | 290.23 → 290.12 | -0.04% |
| random | lzx-21 | 1064810 | 64.61 → 64.66 | +0.08% | 282.67 → 282.98 | +0.11% |
| random | quantum-1-18 | 1059316 | 5.09 → 5.10 | +0.22% | 10.68 → 10.65 | -0.24% |
| random | quantum-7-18 | 1059316 | 5.09 → 5.10 | +0.31% | 10.68 → 10.66 | -0.18% |

Machine-readable medians: [codec comparison](comparison.csv) and
[parser comparison](parser-comparison.csv). All twelve raw CSV files are retained.
Full source snapshots and raw build logs remain under
`/data/cache/cabinet-release-review-20261007`.

## Fuzz coverage before and after

| Gate | Before | After |
| --- | --- | --- |
| Complete spanning seed decoding | 0/21 generated pairs accepted | Complete five-part seed must assemble and decode all six members |
| Quantum writer dictionaries | Only 13, 17, 21 reachable | All orders 10–21 independently selectable at all seven levels |
| Writer frame transitions | 8 KiB cap; no 32 KiB boundary | 131073-byte cap; multiple frames and two members |
| Corpus acceptance checks | Rejected seeds silently counted | Positive set and generated writer seeds verified before campaigns |
| Allocation attack seeds | Absent | 1,000 and 50,000 reused-folder cases retained |

The historical 1,000-iteration receipt remains preserved, with its earlier
coverage limitations explained in the updated fuzz guide. The current campaign
and full validation results are recorded in [release status](../RELEASE-STATUS.md).
