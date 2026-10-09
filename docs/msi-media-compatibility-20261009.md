# MSI media compatibility audit, 2026-10-09

The ms-cabinet 0.1.4 documentation/test release preserves stored cabinet bytes
and the existing dependency graph. No new cabinet API or compression change is
required for the proposed caddy-msi media ownership refactor.

## Qualified artifact comparison

The baseline is the ms-package 0.2.1 qualification matrix, recorded in
`/data/cache/ms-package-release021-clean-sources/ms-package/docs/evidence/package-authoring-20261009/msi-matrix/core021-equivalence.json`.
Its SHA-256 is
`a06978ca154abefd2afd5b8a8cf05611da6bf33db39db5e6021cee42ead84b10`.
The receipt identifies ms-cabinet 0.1.3, caddy-msi 0.10.1 and
caddy-archive-core 0.2.1. All 63 files under its artifact root,
`/data/cache/ms-package-msi-matrix-artifacts-core021`, were checked against
the receipt's qualified SHA-256 values and matched.

An isolated Rust probe opened every external cabinet and every MSI CFB stream
starting with `MSCF`. It extracted members with a 64 MiB per-member bound,
retained entry ordering, and rebuilt each cabinet with borrowed inputs and
`CabinetBuilder::new(WriteCompression::None)`. All 22 cabinets were byte-identical:
15 external cabinets and seven embedded streams, including edited packages,
x86/x64, user/machine and independent split/mixed media cases. These are
independent cabinets, not spanning cabinet sets.

The probe source, lockfile and complete per-cabinet SHA-256 results are retained
at `/data/cache/cabinet-msi-media-audit-20261009/`. Reproduce with:

```sh
nix develop --no-write-lock-file path:/home/rick/projects-caddy/nix-dev-shells/default -c cargo run --manifest-path /data/cache/cabinet-msi-media-audit-20261009/cabinet-media-compat/Cargo.toml --locked --features media
```

This verifies serialization equality with already-qualified artifacts; it does
not claim a new Windows lifecycle run or an independent decoder qualification.
There was no output difference requiring those additional gates.

## WASM consumer checks

Both isolated consumers built for `wasm32-unknown-unknown` using the shared web
profile. The first exposes an optional `media` feature activating the local
ms-cabinet dependency and compiles borrowed cabinet creation. The second uses
the preserved ms-package 0.2.1 qualification source with an optional `media` feature forwarding
to `ms-package/write`, and patches crates.io ms-cabinet to this working tree.
Neither consumer changes a sibling repository.

```sh
nix develop --no-write-lock-file path:/home/rick/projects-caddy/nix-dev-shells/web -c cargo check --manifest-path /data/cache/cabinet-msi-media-audit-20261009/cabinet-media-compat/Cargo.toml --locked --lib --features media --target wasm32-unknown-unknown
nix develop --no-write-lock-file path:/home/rick/projects-caddy/nix-dev-shells/web -c cargo check --manifest-path /data/cache/cabinet-msi-media-audit-20261009/cabinet-ms-package-wasm/Cargo.toml --locked --lib --features media --target wasm32-unknown-unknown
```

The sibling caddy-msi 0.10.1 has no optional media feature yet. Therefore the
literal future caddy-msi feature cannot be tested in this release; the direct
optional consumer and existing ms-package authoring consumer establish cabinet
backend target compatibility. These are compile checks, not WASM runtime tests.
The shared default profile lacks the WASM standard library; the web profile
provides it.

## Dependency preservation

Inspection of the manifest and lockfile changes confirms that dependency
requirements, features and resolved versions are unchanged; only this package's
version changes. `cargo tree --locked -e normal --no-default-features` retains
flate2, ms-compress and serde as direct dependencies. Enabling `cli` adds only
the existing clap and tempfile dependencies. Neither caddy-msi nor ms-package
is a library dependency, and compression dependencies still resolve from
crates.io. The isolated probes' consumer dependencies are not added here.
