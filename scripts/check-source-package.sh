#!/usr/bin/env bash
set -euo pipefail
# Verify a source release by compiling and testing the extracted sibling layout.
archive=$1
destination=$(mktemp -d)
trap 'rm -rf "$destination"' EXIT
tar -xzf "$archive" -C "$destination"
mapfile -t manifests < <(find "$destination" -path '*/cabinet/Cargo.toml')
[[ ${#manifests[@]} == 1 ]]
source_root=$(dirname "$(dirname "${manifests[0]}")")
test -f "$source_root/ms-compress/Cargo.toml"
test -f "$source_root/COMMITS"
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$destination/target}" cargo test --manifest-path "${manifests[0]}" --all-features --locked
