# ms-cabinet MSI media integration plan

Status: cabinet-side audit completed for the caddy-msi media ownership refactor.
See [the integration audit](msi-media-integration-audit.md) and
[compatibility evidence](msi-media-compatibility-20261009.md). No new primitive
was needed; the user-requested 0.1.4 patch release adds contracts and regressions.
The proposed caddy-msi optional media feature remains a consumer integration gate.

## Scope

Keep cabinet serialization, extraction and compression here. MSI Media/File
relationships, sequence partitioning, installer flags, GUID policy and package
publication belong to other crates. Do not add a dependency on caddy-msi or
ms-package. Existing stored, nonspanning cabinet output is the migration baseline.

A code change or cabinet release is needed only if the new backend audit finds
an actual missing primitive. Existing APIs may already satisfy the integration.

## Work batches

1. Audit the writer/reader APIs used by ms-package against caddy-msi's proposed
   optional media feature. Confirm borrowed input, stable member ordering,
   explicit writer completion, actual I/O error propagation and bounded decoding.
2. Document framing/codec workspace requirements so caddy-msi can budget its
   retained buffers. Distinguish logical limits from allocator/process heap caps.
3. Test independent cabinets whose member names are MSI file identifiers, empty
   and boundary-size payloads, multiple cabinets and deterministic output.
   Keep grouping and MSI table semantics outside this crate.
4. If a missing capability is confirmed, add the smallest compatible primitive
   with direct regressions. Do not combine this refactor with new compression,
   cabinet spanning or CLI behavior. Preserve default and CLI dependency graphs.
5. Publish a cabinet update first only if caddy-msi requires new APIs. Otherwise
   retain the compatible published version and record the audit result.

## Gates and acceptance

Preserve codec licenses, fixtures and historical benchmark/oracle evidence.
Run rustfmt, cargo test --all-features --locked and cargo clippy --all-targets
--all-features --locked -- -D warnings. Exercise partial writes and finalization
failures and bounded cabinet extraction. Verify the library still builds for
WASM through the consumer's optional media feature.

Compare cabinet bytes to the qualified ms-package 0.2.1 artifacts. Any intentional
output difference requires fresh independent extraction and Windows lifecycle
qualification before consumers publish. Completion means the backend has the
needed cabinet primitives with documented I/O/workspace contracts and no MSI
policy has migrated into this crate. ms-compress requires no codec changes for
this stored-cabinet ownership refactor.
