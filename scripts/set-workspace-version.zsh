#!/bin/zsh
# Set the workspace package version in Cargo.toml and Cargo.lock.
# Usage: scripts/set-workspace-version.zsh 1.2.3
set -euo pipefail

if [[ $# -ne 1 ]]; then
  print -u2 "usage: scripts/set-workspace-version.zsh <version>"
  exit 2
fi

version="$1"
python3 - "$version" <<'PY'
import re
import sys
from pathlib import Path

version = sys.argv[1]
if not re.fullmatch(r"\d+\.\d+\.\d+", version):
    raise SystemExit(f"refusing version {version!r}")

cargo = Path("Cargo.toml")
text = cargo.read_text()
updated, count = re.subn(
    r"(?ms)(\[workspace\.package\][^\[]*?^version = \")[^\"]+(\")",
    rf"\g<1>{version}\2",
    text,
    count=1,
)
if count != 1:
    raise SystemExit("workspace.package version was not updated")
path_deps = ("ce-protocol", "ce-core", "ce-transport", "ce-http")
for name in path_deps:
    updated, dep_count = re.subn(
        rf'(?m)^({name} = \{{ path = "crates/{name}", version = ")[^"]+(" \}})',
        rf"\g<1>{version}\2",
        updated,
        count=1,
    )
    if dep_count != 1:
        raise SystemExit(f"workspace dependency {name} version was not updated")
cargo.write_text(updated)

names = {"ce-cli", "ce-core", "ce-http", "ce-protocol", "ce-transport"}
lock_path = Path("Cargo.lock")
lines = lock_path.read_text().splitlines(keepends=True)
out = []
index = 0
seen = set()
while index < len(lines):
    if not lines[index].startswith("[[package]]"):
        out.append(lines[index])
        index += 1
        continue
    block = [lines[index]]
    index += 1
    while index < len(lines) and not lines[index].startswith("[[package]]"):
        block.append(lines[index])
        index += 1
    name = None
    has_source = False
    for line in block:
        if line.startswith("name = "):
            name = line.split("=", 1)[1].strip().strip('"')
        elif line.startswith("source = "):
            has_source = True
    if name in names and not has_source:
        replaced = False
        rewritten = []
        for line in block:
            if not replaced and line.startswith("version = "):
                rewritten.append(f'version = "{version}"\n')
                replaced = True
            else:
                rewritten.append(line)
        if not replaced:
            raise SystemExit(f"Cargo.lock package {name} has no version")
        seen.add(name)
        out.extend(rewritten)
    else:
        out.extend(block)

missing = names - seen
if missing:
    raise SystemExit(f"Cargo.lock is missing workspace packages: {sorted(missing)}")
lock_path.write_text("".join(out))
PY
