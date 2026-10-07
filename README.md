# cabinet

A Rust library for reading and writing Microsoft Cabinet archives. It has no
Windows API or `windows-uup` dependency. Builds require Rust 1.99 and the
sibling `../ms-compress` checkout.

```rust
use cabinet::{Cabinet, CabinetBuilder, WriteCompression};
use std::io::Cursor;

let mut builder = CabinetBuilder::new(WriteCompression::Quantum {
    level: 7,
    window_order: 18,
});
builder.add_file("hello.txt", b"Hello, CAB!")?;
let mut output = Cursor::new(Vec::new());
builder.write(&mut output)?;
let mut cabinet = Cabinet::new(Cursor::new(output.into_inner()))?;
assert_eq!(cabinet.read_file_bytes("hello.txt", 1024)?, b"Hello, CAB!");
# Ok::<(), std::io::Error>(())
```

The reader lists and streams stored, MSZIP, LZX, and Quantum members. It validates
CAB tables, member ranges, names, block boundaries, and nonzero CFDATA checksums.
`Cabinet::open` discovers complete spanning sets; `Cabinet::from_parts` accepts
explicitly ordered seekable inputs. Checksums do not authenticate a publisher.
Inputs must remain stable during reading.

The writer supports all four compression methods. `CabinetBuilder` borrows
member names and byte slices and writes to any `Write + Seek` output. It creates
one unsigned cabinet with one solid folder, deterministic default timestamps,
and UTF-8 names. `add_file_with_metadata` accepts raw DOS timestamps and the
supported read-only, hidden, system and archive attribute bits. It rejects absolute/traversing or duplicate names and CAB size/count
limit violations. Codec buffers are bounded by the frame and dictionary sizes;
member contents are not copied into an archive-sized staging buffer. Errors can
leave partial output, which the caller must discard.

MSZIP uses independently compressed DEFLATE frames. LZX uses verbatim Huffman
blocks with greedy frame-local matches and persistent trees/recent offsets.
Quantum uses adaptive arithmetic models and greedy dictionary matches across
frames; its level controls the search bound. These are real compression
encoders, with no requirement to match another producer's byte stream or ratio.
The writer does not create spanning sets or signatures.

Run `cargo test -p ms-cabinet --locked`. Writer interoperability tests use 7-Zip when
available; set `CABINET_REQUIRE_7Z=1` to require it, or `CABINET_7Z` to select its path.
`cargo run -p ms-cabinet --example cab_decode -- ARCHIVE MEMBER` streams a decoded
member to stdout. Application package-inspection and CLI tests remain in
`windows-uup`, importing `cabinet` directly.

The crate's own code is MIT licensed. The separately licensed `ms-compress`
dependency supplies the LGPL LZX and Quantum implementations; see its notices
and licenses when redistributing. The checksum/sample reference's MIT notice is
retained in `LICENSE-cab-MIT.txt`.

## Benchmark

From the repository root, run:

```sh
cargo bench -p ms-cabinet --bench cab --locked -- --iterations 5 --size 1048576
cargo bench -p ms-cabinet --bench cab --locked -- --input "$PWD/path/to/payload" --csv "$PWD/results.csv"
```

The benchmark reports writer and
streaming-reader throughput in MiB/s, CAB bytes, and CAB/input size ratio for
stored, MSZIP, two LZX windows, and two Quantum levels. The default deterministic
corpora contain repetitive, mixed, and pseudorandom bytes. Each case validates a
round trip before timing; codec initialization is included, while input generation
and equality checks are excluded. The destination buffer is reused between
iterations, and decoded bytes stream to a sink. This measures in-memory codec
performance, without filesystem throughput. Cargo runs the executable in the
crate directory; use absolute paths for inputs and CSV output.

Historical results are in [the original benchmark report](docs/benchmark.md).
The [release review comparison](docs/release-review-20261007/benchmark.md) records
matching before/after codec and adversarial-parser measurements. Run the latter
with `cargo bench --bench parser --locked`.

## Command-line examples

Create a cabinet from files or a directory tree:

```sh
cargo run -p ms-cabinet --features cli --example makecab -- --output payload.cab --compression mszip file.txt assets/
cargo run -p ms-cabinet --features cli --example makecab -- --output payload-lzx.cab --compression lzx --window 21 assets/
cargo run -p ms-cabinet --features cli --example makecab -- --output payload-quantum.cab --compression quantum --window 18 --level 7 assets/
```

`--compression` accepts `none`, `mszip` (default), `lzx`, or `quantum`. Directory
inputs retain their root basename, recurse in sorted order, and omit empty
directories. Inputs must be regular files/directories with portable UTF-8 names;
symlinks and special files are rejected. This example loads input contents into
memory before building the archive. Existing output files are preserved, and
failed writes do not publish partial archives. The output parent must exist.

List, verify, or extract a cabinet:

```sh
cargo run -p ms-cabinet --features cli --example cabextract -- payload.cab --list
cargo run -p ms-cabinet --features cli --example cabextract -- payload.cab --test
cargo run -p ms-cabinet --features cli --example cabextract -- payload.cab --output extracted
cargo run -p ms-cabinet --features cli --example cabextract -- payload.cab --output selected --member assets/icon.png
```

Extraction requires a new destination directory with an existing parent. Members
are decoded into a temporary directory before publishing. Unsafe paths and
file/directory conflicts are rejected; existing destinations are preserved.
`--max-bytes` bounds the total declared member size (default 4 GiB), and repeated
`--member` options select exact normalized names. Extraction writes regular files
and does not restore CAB timestamps or attributes. Have exclusive access to the
destination parent while extracting. These are library examples, not replacements
for Microsoft `makecab` directive files or the original `cabextract` option syntax.

Build standalone example executables with:

```sh
cargo build -p ms-cabinet --features cli --release --examples --locked
```

They are emitted under `target/release/examples/`, or the corresponding path under
`CARGO_TARGET_DIR` when configured.

## Install the commands

From the repository root:

```sh
cargo install --path . --features cli --locked
makecab --output files.cab --compression lzx assets/
cabextract files.cab --list
cabextract files.cab --output extracted
```

Cargo installs both binaries into `$CARGO_HOME/bin` (normally `~/.cargo/bin`);
include that directory in `PATH`. The `cli` feature is optional, so library-only
consumers do not need Clap or the commands' tempfile dependency. Examples and
installed binaries share their command implementations.

## Fuzzing

See [the fuzzing guide](fuzz/README.md) for all-fixture regression replay,
bounded reader/spanning/round-trip targets, and reproducible campaigns.
[Release status](docs/RELEASE-STATUS.md) records the resolved review findings and
the remaining platform and publishing gates.

## Publication

The crates.io package is `ms-cabinet`; the Rust library name remains `cabinet`.
Use `cabinet = { package = "ms-cabinet", version = "0.1.0" }`.
The repository is https://github.com/CaddyGlow/ms-cabinet.
Version tags run validation, build CLI artifacts, publish the crate, and create the GitHub Release.
