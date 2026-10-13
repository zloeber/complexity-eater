#!/bin/zsh
# Write a CycloneDX SBOM of the locked workspace graph to ce.cdx.json.
# Usage: scripts/generate-sbom.zsh [output]
set -euo pipefail

root="${0:A:h}/.."
cd "$root"
output="${1:-ce.cdx.json}"

if ! command -v cargo-cyclonedx >/dev/null 2>&1; then
  print -u2 "cargo-cyclonedx is not installed. Run scripts/install-supply-chain-tools.zsh"
  exit 1
fi

# cargo-cyclonedx 0.5 emits CycloneDX 1.5. One workspace SBOM covers the
# locked graph for every release target. The tool picks the output name.
cargo cyclonedx --format json --spec-version 1.5 --target all
generated=$(find . -maxdepth 3 -type f \( -name '*.cdx.json' -o -name 'cyclonedx.json' \) ! -path './target/*' -print -quit)
if [[ -z "$generated" ]]; then
  print -u2 "cargo-cyclonedx did not write an SBOM"
  exit 1
fi
mkdir -p "$(dirname "$output")"
mv "$generated" "$output"
print "wrote ${output}"
