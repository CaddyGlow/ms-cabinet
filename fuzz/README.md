# CAB fuzzing

Run deterministic regressions with `cargo test --manifest-path fuzz/Cargo.toml --locked`.
They replay all 23 retained CAB fixtures, assert successful decoding of the
complete five-volume set, reject malformed set framing, check reachability of all
93 writer configurations (stored, MSZIP, seven LZX windows, and all 12 Quantum
windows at seven levels), and exercise empty, exact-frame and multiframe payloads.
The original reader/writer integration suites retain independent 7-Zip checks,
malformed inputs, CVE fixtures, complete sets and split codec-frame regressions.

Install honggfuzz (`cargo install honggfuzz --version 0.5.62 --locked`) and its
Linux build dependencies (binutils-dev, libunwind-dev, liblzma-dev), then run
`bash scripts/fuzz.sh`. Rust 1.99 is required. The runner verifies positive seeds
before starting, records seed hashes and runs three instrumented targets:

- `cab`: retained and generated CAB inputs, bounded member decoding, and two
  repeated-folder-range allocation attack regressions.
- `spanning`: up to 64 explicitly framed inputs, seeded by a complete five-volume
  set that must assemble and decode and an incomplete negative set.
- `roundtrip`: independently selected codec, dictionary and level bytes followed
  by a payload up to 131073 bytes, split into two members. The later member is
  read first to exercise solid-folder history, skips and cross-frame decoding.

CAB inputs and cumulative emitted member bytes are limited to 1 MiB. Decoding
may also process earlier solid-folder bytes needed to reach selected members;
these byte limits do not bound that work. The runner applies a five-second
per-iteration timeout. Corpus generation uses every writer configuration and
24 frame-boundary cases, including four full frames plus a final partial frame.

Spanning corpus framing is v2: one u8 count, then a little-endian u32 length and
raw CAB bytes for each part. Zero counts, counts above 64, truncation and trailing
bytes are rejected. Round-trip framing uses three independent configuration
bytes before payload bytes. Older campaign receipts describe the earlier framing
and coverage; they do not establish current spanning or full-window coverage.

Set `CABINET_FUZZ_ITERATIONS` (default 10000 per target) or an absolute
`CABINET_FUZZ_OUTPUT`. Unique run directories preserve seed hashes, seeding logs,
toolchain, arguments, raw logs and findings. Replay current-format findings with
`cargo run --manifest-path fuzz/Cargo.toml --locked --bin replay -- TARGET FILE`.
Never delete a finding before retaining it with a regression and provenance.
Campaigns remain a separate gate; short smoke runs are not security qualification.
