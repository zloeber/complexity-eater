#!/bin/zsh
# Build the release binary twice from a clean target directory and compare hashes.
# A mismatch is reported and does not by itself mean the toolchain pin failed.
# The command fails when a build fails.
# Usage: scripts/compare-release-builds.zsh
set -euo pipefail

root="${0:A:h}/.."
cd "$root"

if git rev-parse HEAD >/dev/null 2>&1; then
  export SOURCE_DATE_EPOCH="$(git log -1 --format=%ct)"
else
  export SOURCE_DATE_EPOCH=0
fi
export CARGO_INCREMENTAL=0

left="$root/target/repro-a"
right="$root/target/repro-b"
rm -rf "$left" "$right"

CARGO_TARGET_DIR="$left" cargo build --release --locked --bin ce
CARGO_TARGET_DIR="$right" cargo build --release --locked --bin ce

left_bin="$left/release/ce"
right_bin="$right/release/ce"
left_hash=$(shasum -a 256 "$left_bin" | awk '{print $1}')
right_hash=$(shasum -a 256 "$right_bin" | awk '{print $1}')
print "left  ${left_hash}  ${left_bin}"
print "right ${right_hash}  ${right_bin}"
if [[ "$left_hash" == "$right_hash" ]]; then
  print "REPRODUCIBLE=yes"
else
  print "REPRODUCIBLE=no"
  print "The two release binaries differ. The toolchain and Cargo.lock are pinned; the bytes are not bit-reproducible."
fi
