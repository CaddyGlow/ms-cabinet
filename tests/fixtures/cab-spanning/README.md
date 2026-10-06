# Microsoft makecab spanning fixture

Five unmodified cabinets from Stuart Caie’s cabextract test corpus:
[upstream directory](https://github.com/kyz/libmspack/tree/55d501976171397ccd5d5a7a1ca7da065b1d9a06/cabextract/test/cabs).
Pinned upstream commit: `55d501976171397ccd5d5a7a1ca7da065b1d9a06`.

The retained upstream `split-cabs.sh` documents generation with Microsoft
`makecab.exe` under Wine using random payloads, MSZIP compression, 30,000-byte
volumes, and reserved header/folder/data areas. Its correction to a makecab
previous-volume link is already present in the upstream fixture; the downloaded
CAB bytes here were not changed. Embedded names are `Split-N.CAB`, whereas the
corpus filenames are lowercase. Six files total 140,128 uncompressed bytes.
The five volumes total 142,150 bytes and exercise all three continued-member
markers, multiple folders, solid compression, and split CFDATA blocks.

Provenance/license: cabextract (C) 2000–2023 Stuart Caie,
GPL-2.0-or-later, as declared in upstream `cabextract/src/cabextract.c`.
The corpus has no separate per-fixture license notice; these retained test assets
are distributed under that upstream project license. Full GPL v2 text is in
`COPYING`. These assets and the generator are test-only; no cabextract/libmspack
implementation code was copied into the Rust CAB reader.

SHA-256 of the unchanged upstream cabinets:

```json
{
  "split-1.cab": "802de38233dbdeb5ff006c14e62c504740320385f16f1db63bcef4811c768413",
  "split-2.cab": "e364408c0549b8845d929e1d5b85cd8ae53dc7216382fd935f8adad9a1e5f55f",
  "split-3.cab": "567fff7f7029129643072f62b9ef1256dbfd1e45f8707936cd30f8b735571a71",
  "split-4.cab": "a3589cdf11e420c39b3e32162d7b59135bb0863dab54eb19a2abb987a7c1f266",
  "split-5.cab": "fc9ed6b84b2d766019cc8b314ce0199d06f907225fe3604fbfbfafafdb449447"
}
```

The independent 7z extraction hashes are asserted in `tests/reader.rs`.
Reproduce the byte comparison from the repository root:

```sh
cargo build --locked --bin windows-uup
python3 ../windows-uup/scripts/validate-cab-spanning.py --binary target/debug/windows-uup \
  --output /tmp/cab-spanning-comparison.json
```

Use the binary path from Cargo’s configured target directory if it differs.
The oracle uses temporary uppercase copies for p7zip’s case-sensitive volume
lookup; native extraction uses the original lowercase fixtures and supports
starting from any volume. Original cabinet hashes are checked before and after.
