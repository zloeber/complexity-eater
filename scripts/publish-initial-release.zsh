#!/bin/zsh
# Tag the workspace version when main has never been released.
# Usage: scripts/publish-initial-release.zsh <commit-sha>
set -euo pipefail

if [[ $# -ne 1 ]]; then
  print -u2 "usage: scripts/publish-initial-release.zsh <commit-sha>"
  exit 2
fi

sha="$1"
if ! git cat-file -e "${sha}^{commit}" 2>/dev/null; then
  print -u2 "commit not found: ${sha}"
  exit 2
fi

if git ls-remote --tags origin | grep -Eq 'refs/tags/v[0-9]+\.[0-9]+\.[0-9]+$'; then
  print "A version tag already exists."
  exit 0
fi

version="$(
  python3 - <<'PY'
import re
from pathlib import Path

text = Path("Cargo.toml").read_text()
match = re.search(
    r'(?ms)^\[workspace\.package\][^\[]*?^version = "([^"]+)"',
    text,
)
if not match or not re.fullmatch(r"\d+\.\d+\.\d+", match.group(1)):
    raise SystemExit("workspace version was not found")
print(match.group(1))
PY
)"

tag="v${version}"
gh release create "$tag" \
  --target "$sha" \
  --title "$tag" \
  --notes "Complexity Eater ${version}."
gh workflow run release-binaries.yml --ref main -f "tag=${tag}"
print "Created ${tag} and started binary builds."
