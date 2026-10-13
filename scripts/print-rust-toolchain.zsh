#!/bin/zsh
# Print the toolchain channel pinned in rust-toolchain.toml.
set -euo pipefail

root="${0:A:h}/.."
python3 - "$root/rust-toolchain.toml" <<'PY'
import re
import sys
from pathlib import Path

text = Path(sys.argv[1]).read_text()
match = re.search(r'(?m)^channel\s*=\s*"([^"]+)"', text)
if not match:
    raise SystemExit("rust-toolchain.toml has no channel")
print(match.group(1))
PY
