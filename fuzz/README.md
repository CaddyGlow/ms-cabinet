# CAB fuzzing

Run regression vectors with `cargo test --manifest-path fuzz/Cargo.toml --locked`.
The regression suite replays every retained CAB fixture (including all Quantum
windows, CVE samples, mixed codecs and spanning parts). The original reader and
writer integration suites remain in `tests/`, including malformed headers,
checksums, independent 7-Zip interoperability and complete spanning sets.

Install honggfuzz (`cargo install honggfuzz --version 0.5.62 --locked`) and its
Linux build dependencies (binutils-dev, libunwind-dev, liblzma-dev), then run
`bash scripts/fuzz.sh`. It seeds from every retained CAB vector and runs three
bounded targets: `cab` parses and streams members, `spanning` parses two explicit
parts, and `roundtrip` tests stored, MSZIP, LZX and Quantum writer/reader agreement.
Reader inputs and cumulative decoded output are limited to 1 MiB; writer payloads
to 8 KiB. Each iteration has a five-second timeout in the campaign runner.

Set `CABINET_FUZZ_ITERATIONS` (default 10000 per target) or absolute
`CABINET_FUZZ_OUTPUT`. Runs preserve corpus hashes, toolchain, arguments, logs,
and crash evidence under a unique directory in `target/fuzz`. Replay a finding
with `cargo run --manifest-path fuzz/Cargo.toml --locked --bin replay -- TARGET FILE`.
Never delete a finding before retaining it with a regression and provenance.
Campaigns are a separate gate from deterministic tests; these bounds do not
prove safety for arbitrarily large archives.
