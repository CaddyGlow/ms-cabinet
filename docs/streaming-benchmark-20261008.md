# CAB streaming writer measurements

Measured UTC: 2026-10-08T21:42:30+00:00

Host: 13th Gen Intel(R) Core(TM) i7-13700K; Linux-6.18.53-x86_64-with-glibc2.44; 24 logical CPUs.
Compiler: `rustc 1.99.0 (b940084d7 2026-09-28)`.
Package: `ms-cabinet 0.1.3`; release benchmark profile with thin LTO.
Lockfile SHA-256: `97f55f277e724ab604aae81c02d7dc395b3b9685d522888ee5b06c81784df55f`.
Binary SHA-256: `3d4f35b24918bb7a00f11e54e6bffd8dd3a9626ad1e8de0aa20c448e3ce07f3c`.

Each row summarizes independent fresh processes; throughput and peak RSS are medians.
Runs use normal host scheduling without CPU affinity or frequency controls.
Small-case throughput differences can include first-use and scheduling noise.
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
| stored | repetitive | 0.0625 | 65624 | 543.94 | 679.04 | 2.36 | 2.36 |
| stored | repetitive | 1 | 1048904 | 347.23 | 505.87 | 3.09 | 2.36 |
| stored | random | 0.0625 | 65624 | 207.88 | 219.84 | 2.36 | 2.36 |
| stored | random | 1 | 1048904 | 225.89 | 272.41 | 2.90 | 2.36 |
| mszip | repetitive | 0.0625 | 433 | 80.23 | 88.54 | 2.73 | 2.73 |
| mszip | repetitive | 1 | 5820 | 151.97 | 165.76 | 3.65 | 2.73 |
| mszip | random | 0.0625 | 65638 | 24.29 | 21.71 | 2.73 | 2.53 |
| mszip | random | 1 | 1049128 | 41.42 | 38.24 | 3.65 | 2.53 |
| lzx | repetitive | 0.0625 | 1208 | 53.09 | 57.11 | 4.21 | 4.03 |
| lzx | repetitive | 1 | 18210 | 125.24 | 106.08 | 5.15 | 4.03 |
| lzx | random | 0.0625 | 66618 | 16.88 | 17.64 | 4.02 | 3.84 |
| lzx | random | 1 | 1064810 | 52.69 | 51.76 | 4.96 | 4.03 |
| quantum | repetitive | 0.0625 | 305 | 68.05 | 57.65 | 3.44 | 3.45 |
| quantum | repetitive | 1 | 2614 | 15.89 | 16.59 | 13.86 | 12.83 |
| quantum | random | 0.0625 | 66344 | 4.46 | 3.99 | 3.64 | 3.56 |
| quantum | random | 1 | 1059316 | 2.86 | 2.87 | 14.19 | 13.26 |
| stored | repetitive | 32 | 33562696 | 862.68 | 1194.01 | 34.00 | 2.36 |
| stored | random | 32 | 33562696 | 443.96 | 539.72 | 33.75 | 2.36 |

Raw per-process samples: [streaming-benchmark-20261008.csv](streaming-benchmark-20261008.csv).

Reproduce from the cabinet checkout using its development shell:

```sh
cargo bench --bench streaming --no-run --locked
# Pass the executable path printed by Cargo (target directory may be shared).
python3 scripts/benchmark-streaming.py --binary /path/to/streaming-executable \
  --csv docs/streaming-benchmark-20261008.csv --report docs/streaming-benchmark-20261008.md \
  --sizes 65536 1048576 --repetitions 3 --large-copy 33554432 \
  --methods stored mszip lzx quantum
```

At 32 MiB, the reader API used 2.36 MiB median peak RSS for both corpora,
versus 33.75–34.00 MiB for the byte API. The CAB bytes were identical.
Compression throughput varied by codec and corpus; the benefit demonstrated
here is avoiding the complete input allocation, rather than a universal speedup.

The 108 default-case processes totalled 3.58 seconds of timed encoding (excluding
runner startup, hashing, file comparison, and process startup). The optional
[16 MiB results](streaming-large-benchmark-20261008.md) extend every codec beyond
its configured history window; the Quantum/random cases take substantially longer.
