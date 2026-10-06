# CAB fuzz smoke — 2026-10-07

Three honggfuzz 0.5.62 instrumented targets completed at least 1,000 iterations
each with zero crashes and timeouts. This is a smoke campaign, not a sustained
security qualification. All 23 retained CAB fixtures seeded reader fuzzing;
spanning pairs and all 256 writer selectors seeded their respective targets.

Command: `CABINET_FUZZ_ITERATIONS=1000 bash scripts/fuzz.sh`, inside the
archive-rs Nix development environment. Raw logs and corpus remain at
`/home/rick/projects-caddy/cabinet/target/fuzz/run-20261006T221413Z-1368705`. Seed hashes, arguments, toolchain and summaries are retained here.

Standalone all-feature tests, Clippy with warnings denied, fuzz regression tests,
fuzz Clippy, rustfmt, actionlint and shellcheck passed. Archive workspace
all-feature tests, Clippy and fuzz regression tests also passed. Windows-native
and sustained fuzz campaigns were not run.
