#!/bin/zsh
# Delete one GHCR container tag. Used when a release image fails its scan
# before the version tag is published. Missing packages are not an error.
# Usage: scripts/delete-ghcr-tag.zsh <owner> <package> <tag>
set -euo pipefail

if [[ $# -ne 3 ]]; then
  print -u2 "usage: scripts/delete-ghcr-tag.zsh <owner> <package> <tag>"
  exit 2
fi

owner="$1"
package="$2"
tag="$3"

if ! command -v gh >/dev/null 2>&1; then
  print -u2 "gh is not installed; leaving ${owner}/${package}:${tag} in place"
  exit 1
fi

python3 - "$owner" "$package" "$tag" <<'PY'
import json
import subprocess
import sys

owner, package, tag = sys.argv[1:]
paths = [
    f"/users/{owner}/packages/container/{package}/versions",
    f"/orgs/{owner}/packages/container/{package}/versions",
]
found = False
for path in paths:
    result = subprocess.run(
        ["gh", "api", "--paginate", path],
        text=True,
        capture_output=True,
    )
    if result.returncode != 0:
        continue
    text = result.stdout.strip()
    if not text:
        continue
    try:
        versions = json.loads(text)
    except json.JSONDecodeError:
        # gh --paginate concatenates JSON arrays. Accept one array.
        sys.stderr.write(result.stderr)
        raise SystemExit(1)
    if isinstance(versions, dict):
        versions = [versions]
    for version in versions:
        tags = ((version.get("metadata") or {}).get("container") or {}).get("tags") or []
        if tag not in tags:
            continue
        found = True
        version_id = version["id"]
        delete = subprocess.run(
            ["gh", "api", "--method", "DELETE", f"{path}/{version_id}"],
            text=True,
        )
        if delete.returncode != 0:
            raise SystemExit(delete.returncode)
        print(f"deleted {owner}/{package}:{tag}")
if not found:
    print(f"{owner}/{package}:{tag} was not present")
PY
