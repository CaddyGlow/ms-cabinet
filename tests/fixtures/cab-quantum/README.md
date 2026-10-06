# CAB Quantum fixtures

The unmodified `mszip_lzx_qtm.cab` is a real mixed-compression sample from the
[libmspack corpus](https://github.com/kyz/libmspack/tree/55d501976171397ccd5d5a7a1ca7da065b1d9a06/libmspack/test/test_files/cabd),
pinned at `55d501976171397ccd5d5a7a1ca7da065b1d9a06`. Its Quantum member is
`qtm.txt`, 59 bytes, with upstream MD5 `98fcfa4962a0f169a3c7fdbcb445cf17`.
The same corpus supplies `cve-2014-9556-qtm-infinite-loop.cab` and
`cve-2018-18584-qtm-max-size-block.cab`. The malformed
`cve-2010-2801-qtm-flush.cab` comes from that commit's `cabextract/test/bugs`.
These CVE fixtures are negative cases, never evidence of successful decoding.

Libmspack corpus/decoder: (C) 2003–2023 Stuart Caie, GNU LGPL 2.1; full text
`COPYING.LIB`. The cabextract corpus is under that program's GPL-2.0-or-later,
(C) 2000–2023 Stuart Caie; full GPL v2 text `COPYING`. Neither corpus supplies a
separate per-fixture copyright/license notice.

The 14 `quantum-*.cab` files are generated locally by the retained independent
Python arithmetic encoder `generate.py` (LGPL-2.1-only, model-format reference:
libmspack's `qtmd.c`). They are not outputs from the Rust decoder. Every generated
member was independently extracted with p7zip 17.05 and checked against the
encoder's original bytes before retention. `generated-expected.json` records those
source and member SHA-256 hashes.

- Twelve two-frame folders cover dictionary orders 10 through 21, all seven
  selectors, all 27 length slots, direct offset/length bits, overlapping matches,
  small dictionary wraparound, model rescaling/reordering, and history reuse.
- `quantum-high-history.cab` contains 128 frames and 4 MiB of decoded data. Its
  later frames exercise all position models through repeated reorder cycles,
  the maximum 19 direct offset bits, and 2 MiB match distances.
- `quantum-package.cab` contains a valid root `update.mum` for default inventory
  integration. Its package is synthetic and makes no servicing claim.

The native spanning test rewraps a validated two-frame payload into two cabinet
fragments and reads the later member, requiring both dictionary and model history.
The codec tests also cover every truncation of the real sample, zero through four
padding bytes, invalid padding, failed-reader state, and deterministic malformed
and mutated inputs.

Reproduce generation into a **new** directory and independent extraction:

```sh
python3 tests/fixtures/cab-quantum/generate.py --output /tmp/new-quantum-fixtures
cargo build --locked --bin windows-uup
python3 ../windows-uup/scripts/validate-cab-quantum.py --binary target/debug/windows-uup \
  --output /tmp/quantum-comparison.json
```

Use Cargo's configured target directory when it differs. The comparison script
checks every member of all 15 valid cabinets (30 members, 28 Quantum) against 7z
and verifies that all source cabinet hashes are unchanged. Runtime decoding does
not launch Python or an external decoder.

Retained CAB SHA-256:

```json
{
  "cve-2010-2801-qtm-flush.cab": "78bb4c9540e36d3d4228e22cdff0a5677f9c2dddc4239c322277d0b9125d8364",
  "cve-2014-9556-qtm-infinite-loop.cab": "2e6f57d26e12f3f1c0f1f968d7ebb92e77ea7720ea6f9e18f11eb00ef0e845ad",
  "cve-2018-18584-qtm-max-size-block.cab": "3ba0c719d8fb12dc531e2a7790c2d15c12d8840de8e4ddc60572c4145181fbf4",
  "mszip_lzx_qtm.cab": "0ce0b55fe705b744d41bb361170c0467db30da0c7f9bdd386d5dade71a78e171",
  "quantum-10.cab": "34a9602a54462780fddcfc5ff0a1e9ca92dfe4fb373592eaf78bbedc4e7aa951",
  "quantum-11.cab": "87ff1d033ad872a8b2b24d038d33e87b2ec72a975be4218f6ce85041e4af43e9",
  "quantum-12.cab": "794c612cc3ed7d51c06c49d941d76031d704badb5ff1626736c793397abf2aea",
  "quantum-13.cab": "b075eb58b2ee954822719657104073f58f750c12ae46485fd867c00a395d77be",
  "quantum-14.cab": "8e57fbcb7f8ced0542947fac5eaba966bb5deffb24b8f7d8e49545d26e9faca1",
  "quantum-15.cab": "8a800bd60dd0fe51a7a4bec7b642fd673e3dca9176f2b318eb7a3eb4cc654445",
  "quantum-16.cab": "4ca100ce7be009c4ebdeaeffab5e553473337a8a02ca1a9564fd3c8760e52d26",
  "quantum-17.cab": "7ef3bbc01d1de4c3831bbf4bbec7d3abd825d42fb701d985f03dd50daf9fbf3a",
  "quantum-18.cab": "f4471b6a6a136ee3b53944ac1817655c6993de8f300a7244a42c58d4398d13d6",
  "quantum-19.cab": "66c5ec39ba9b386b50f1b9a6a927552ae721ebfbafebdd0d0b182a4cffb28f7f",
  "quantum-20.cab": "024aece49fe1217b412a028dc3516bf0fd205f28bc23c8bcd624dff76317342a",
  "quantum-21.cab": "a70d1494d1b535ff64419e7cf3c27d5dc3440805feca5db2c4c6d78729efbbdc",
  "quantum-high-history.cab": "f429293172e636603df511d17d88c0bc5d5e5c97a445f81349c21341e11b58cc",
  "quantum-package.cab": "b3a360cf09ff168d02deeee69a1e494b0c75a8487550f599510818cbe98b5596"
}
```
