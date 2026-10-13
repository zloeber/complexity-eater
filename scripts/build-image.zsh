#!/bin/zsh
# Build the pinned production image and tag it ce:supply-chain.
# Usage: scripts/build-image.zsh
set -euo pipefail

root="${0:A:h}/.."
cd "$root"
export DOCKER_BUILDKIT=1

version=$(
  python3 - <<'PY'
import re
from pathlib import Path
text = Path("Cargo.toml").read_text()
match = re.search(r'(?ms)^\[workspace\.package\][^\[]*?^version = "([^"]+)"', text)
if not match:
    raise SystemExit("workspace version was not found")
print(match.group(1))
PY
)
revision=$(git rev-parse HEAD 2>/dev/null || print unknown)
created=$(date -u +%Y-%m-%dT%H:%M:%SZ)

docker build \
  --progress=plain \
  --build-arg "VERSION=${version}" \
  --build-arg "REVISION=${revision}" \
  --build-arg "CREATED=${created}" \
  --tag ce:supply-chain \
  "$root"

print "built ce:supply-chain version=${version} revision=${revision}"
