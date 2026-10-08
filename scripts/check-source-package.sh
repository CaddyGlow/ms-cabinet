#!/usr/bin/env bash
set -euo pipefail
# Verify standalone sources against the committed crates.io dependency lockfile.
archive=${1:?usage: check-source-package.sh SOURCE_ARCHIVE}
destination=$(mktemp -d)
trap 'rm -rf "$destination"' EXIT
tar -xzf "$archive" -C "$destination"
mapfile -t manifests < <(find "$destination" -path '*/cabinet/Cargo.toml')
[[ ${#manifests[@]} == 1 ]]
source_root=$(dirname "$(dirname "${manifests[0]}")")
test -f "$source_root/cabinet/Cargo.lock"
test -f "$source_root/COMMITS"
cargo test --manifest-path "${manifests[0]}" --all-features --locked
