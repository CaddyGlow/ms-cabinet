#!/usr/bin/env bash
set -euo pipefail
root=$(cd "$(dirname "$0")/.." && pwd)
output=${CABINET_FUZZ_OUTPUT:-"$root/target/fuzz/run-$(date -u +%Y%m%dT%H%M%SZ)-$$"}
iterations=${CABINET_FUZZ_ITERATIONS:-10000}
[[ "$output" = /* && "$iterations" =~ ^[1-9][0-9]*$ ]] || exit 2
mkdir -p "$output/corpus/cab" "$output/corpus/spanning" "$output/corpus/roundtrip" "$output/logs"
cargo run --manifest-path "$root/fuzz/Cargo.toml" --locked --bin seed -- "$output/corpus" > "$output/seeding.log" 2>&1
python3 - "$output" <<'HASHES'
import pathlib, sys, hashlib
out = pathlib.Path(sys.argv[1])
with (out/'seeds.sha256').open('w') as f:
    for p in sorted((out/'corpus').rglob('*')):
        if p.is_file(): f.write(hashlib.sha256(p.read_bytes()).hexdigest()+'  '+str(p)+'\n')
HASHES
rustc -Vv > "$output/toolchain.txt"
cargo hfuzz version >> "$output/toolchain.txt" 2>&1
export HFUZZ_WORKSPACE="$output/workspace" HFUZZ_BUILD_ARGS=--locked
export CARGO_TARGET_DIR="$root/target/honggfuzz"
export RUSTC_WRAPPER='' CARGO_INCREMENTAL=0 CC=gcc NIX_HARDENING_ENABLE=''
cd "$root/fuzz"
for target in cab spanning roundtrip; do
    export HFUZZ_INPUT="$output/corpus/$target"
    export HFUZZ_RUN_ARGS="-n 1 -t 5 -N $iterations -F 1048576 --exit_upon_crash"
    printf '%s\n' "$HFUZZ_RUN_ARGS" > "$output/logs/$target.args"
    cargo hfuzz run "$target" 2>&1 | tee "$output/logs/$target.log"
    grep -Eq 'Summary iterations:[0-9]+ .*crashes_count:0 timeout_count:0 ' "$output/logs/$target.log"
    completed=$(sed -n 's/^Summary iterations:\([0-9]*\) .*/\1/p' "$output/logs/$target.log" | tail -n 1)
    [[ -n "$completed" ]] && (( completed >= iterations ))
done
