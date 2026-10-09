# MSI media backend audit, 2026-10-09

The existing cabinet API supplies the primitives required by the stored,
nonspanning MSI media ownership refactor. No new encoder, spanning support,
MSI policy, dependency or public API is required. `ms-cabinet` 0.1.3 remains a
compatible backend for that refactor; the separately requested cabinet patch
release records contracts and regressions without requiring consumers to adopt
a new API.

## Consumer and ownership boundary

The audited sibling `ms-package/src/authoring/installer_media.rs` registers MSI
File identifiers as cabinet names with `CabinetBuilder::add_file` and
`WriteCompression::None`, writes to a bounded seekable cursor, then explicitly
flushes it. `src/installer.rs` resolves embedded/external bytes through explicit
caller I/O and delegates cabinet extraction through its archive abstraction.
The proposed sibling `docs/msi-media-refactoring-plan.md` moves sequence,
Media/File relationships, layout validation, retained-media accounting and
sink completion into an optional `caddy-msi` media backend. GUID policy,
canonical package admission and aggregate publication remain consumer concerns.

The inspected `caddy-msi` checkout has no media feature or cabinet dependency
yet. Therefore a build of its proposed optional feature is a future consumer
gate, not evidence this cabinet-only change can provide. The cabinet library
can be checked directly for `wasm32-unknown-unknown`; CLI features are unnecessary
for the browser backend. Default features remain empty and compression
dependencies resolve from crates.io.

## I/O contract

`CabinetBuilder` borrows original names and byte slices. Its duplicate-name
index owns normalized strings; it does not copy payloads. Registration order
determines both member-table order and solid-folder payload order. Default DOS
metadata is fixed, so identical inputs, order and compression configuration
produce deterministic output. Empty members are supported.

`write` and `write_from_readers` accept `Write + Seek` and serialize at the
current position. Successful return includes final frame serialization, header
patching and a seek back to the cabinet end. Existing trailing bytes are not
truncated. There is no deferred builder completion on drop. `write_all` handles
short destination writes; actual write and seek errors propagate. On failure,
discard the partial output. No generic `Write` interface can determine durable
publication or downstream finalization: the caller must flush, finalize and
check errors before counting an artifact as complete.

Reader-based creation opens one source at a time by zero-based registration
index, including empty members, and drops it before opening the next. Readers
can borrow caller storage. Short declared input returns `UnexpectedEof`; an
extra byte returns `InvalidData`. Interrupted source reads are retried; other
source/open errors propagate. These APIs allow the consumer to enforce actual
read and output budgets using its own reader/writer adapters.

## Workspace and logical limits

For stored output, serialization uses one 32,768-byte stack frame and no
codec-specific payload workspace. Frames continue across file boundaries.
Borrowed payload storage, output cursor storage, builder file descriptors,
normalized-name index allocations and caller source buffers are separate.
The consumer's existing subtraction of 32,768 from its scratch allowance
accounts for the frame's logical size; it is not an allocator or process heap
cap. Builder metadata grows with member count and names. A stored cabinet's
serialized size is exactly `44 + sum(17 + name.len()) + payload_bytes +
8 * ceil(payload_bytes / 32768)`; zero total payload uses no data frames.

MSZIP writing additionally creates a DEFLATE encoder and compressed temporary
vectors per frame. LZX and Quantum retain codec state and dictionary/search
workspace determined by their window parameters (up to dictionary order 21).
Dictionary size alone is not a total codec allocation estimate. This refactor
uses stored output and does not require changing those codecs or claiming
an exact heap cap for them.

`Cabinet::new` parses directory and block metadata without decoding payloads.
It retains names, entries, folders and block descriptors; metadata counts and
ranges are checked against physical cabinet data, but there is no caller-set
metadata heap limit. Bound incoming cabinet bytes separately.
`read_file_bytes(name, limit)` rejects an oversized declared member before
decoding and returns exactly the declared number of bytes or an error. Its
limit bounds returned payload bytes, not total retained memory. `read_file`
streams without a member-sized result allocation. Either path may decode
earlier solid-folder frames to establish history.

A nonspanning decoder retains one compressed frame (its on-disk length is a
16-bit field) and one decoded frame (at most 32,768 payload bytes), plus codec
state. MSZIP adds one byte to its output buffer to detect oversized expansion.
Stored decoding needs no dictionary; MSZIP keeps up to 32,768 bytes of history
between frames, temporarily appends another frame before trimming it, and uses
temporary history-seeding buffers; LZX and Quantum retain dictionaries
and their additional codec structures. Spanning sets may join split compressed
fragments across cabinets, so the single physical frame bound must not be
misrepresented as a universal aggregate compressed-buffer cap. Vector capacity,
allocation rounding and allocator bookkeeping are outside these logical sizes.

## Qualification baseline

Preserve the sibling qualification receipt
`ms-package/docs/evidence/package-authoring-20261009/msi-matrix/core021-equivalence.json`:
it records equality of 63 artifacts with `ms-cabinet` 0.1.3, `caddy-msi` 0.10.1
and `caddy-archive-core` 0.2.1. Qualified original artifacts are retained at
`/data/cache/ms-package-msi-matrix-artifacts`; the registry dependency comparison
is at `/data/cache/ms-package-msi-matrix-artifacts-core021`. The sibling matrix
README, producer source/locks, hashes and Windows lifecycle logs record the
qualification scope. These historical files must remain unchanged.

Cabinet comparison should reconstruct stored cabinets in recorded member order
and compare bytes, including embedded MSI streams and external sidecars.
An intentional byte change requires fresh independent extraction and Windows
lifecycle qualification before consumers publish. This documentation and test
change introduces no intentional serialization difference. Fresh validation
results belong in the release record, separate from historical qualification.
