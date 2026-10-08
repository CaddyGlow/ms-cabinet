# CAB streaming writer measurements

Measured UTC: 2026-10-08T21:48:07+00:00

Host: 13th Gen Intel(R) Core(TM) i7-13700K; Linux-6.18.53-x86_64-with-glibc2.44; 24 logical CPUs.
Compiler: `rustc 1.99.0 (b940084d7 2026-09-28)`.
Package: `ms-cabinet 0.1.3`; release benchmark profile with thin LTO.
Lockfile SHA-256: `97f55f277e724ab604aae81c02d7dc395b3b9685d522888ee5b06c81784df55f`.
Binary SHA-256: `3d4f35b24918bb7a00f11e54e6bffd8dd3a9626ad1e8de0aa20c448e3ce07f3c`.

Each row summarizes independent fresh processes; throughput and peak RSS are medians.
Runs use normal host scheduling without CPU affinity or frequency controls.
Timings exclude process startup; these measurements do not establish a universal speedup.
All paired CAB files were compared byte-for-byte and have matching SHA-256 digests.
Timing includes deterministic input generation, API registration, codec initialization,
compression, and writes to a temporary file. It excludes file creation and final hashing;
writes are not fsynced, so throughput includes the OS page cache rather than durable storage.
GNU time peak RSS covers the whole child process, including startup, input generation,
encoding, and fixed-buffer hashing. It is not an exact codec-workspace measurement.
The byte API allocates and fills a complete input Vec inside the measured process.
The reader API generates input in chunks up to 32 KiB; neither API retains output in RAM.
LZX uses a 2 MiB window; Quantum uses level 6 and a 2 MiB window. Codec allocations,
allocator reuse, and process baseline can dominate small-input peaks.
Repetitive input repeats a fixed ASCII phrase; random input is deterministic xorshift32
with seed 0x12345678. No source archives or historical benchmark evidence were modified.

| Method | Corpus | Input MiB | CAB bytes | Byte MiB/s | Reader MiB/s | Byte peak MiB | Reader peak MiB |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| stored | repetitive | 16 | 16781384 | 875.28 | 1214.68 | 17.90 | 2.07 |
| stored | random | 16 | 16781384 | 447.65 | 543.71 | 18.09 | 2.15 |
| mszip | repetitive | 16 | 92038 | 376.29 | 415.59 | 18.46 | 2.73 |
| mszip | random | 16 | 16784968 | 48.90 | 50.63 | 18.46 | 2.53 |
| lzx | repetitive | 16 | 290258 | 286.28 | 295.43 | 19.96 | 4.03 |
| lzx | random | 16 | 17035818 | 51.87 | 55.15 | 19.96 | 4.03 |
| quantum | repetitive | 16 | 39366 | 8.85 | 9.66 | 38.88 | 23.38 |
| quantum | random | 16 | 16946142 | 0.58 | 0.47 | 40.02 | 24.46 |

Raw per-process samples: [streaming-large-benchmark-20261008.csv](streaming-large-benchmark-20261008.csv).

Reproduce from the cabinet checkout using its development shell:

```sh
cargo bench --bench streaming --no-run --locked
# Pass the executable path printed by Cargo (target directory may be shared).
python3 scripts/benchmark-streaming.py --binary /path/to/streaming-executable \
  --csv docs/streaming-large-benchmark-20261008.csv --report docs/streaming-large-benchmark-20261008.md \
  --sizes 16777216 --repetitions 3 --large-copy 0 \
  --methods stored mszip lzx quantum
```

This optional larger run took 299.26 seconds of cumulative timed encoding;
it is intentionally separate from the short default benchmark. The final
Quantum/random pair took 80.11/90.69 seconds (byte/reader), compared with
19.83/32.27 and 27.43/33.98 seconds for the earlier pairs. The cause of this
variation was not diagnosed; all samples remain in the CSV and medians above.

For 16 MiB inputs, the reader API avoids approximately 16 MiB of input retention
across all four codecs. Quantum still peaks at 23.38–24.46 MiB: its 2 MiB history
window is only one component of encoder workspace, not a total memory ceiling.

The [short default benchmark](streaming-benchmark-20261008.md) covers 64 KiB
and 1 MiB inputs across all four codecs, plus a 32 MiB stored-only memory case.
